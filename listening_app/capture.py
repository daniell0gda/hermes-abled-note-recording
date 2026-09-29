"""One capture per source: PortAudio callback -> 16 kHz mono float32 -> consumer callback."""

import logging
import queue
import threading
import time
from collections.abc import Callable
from typing import Any

import numpy as np
import pyaudiowpatch as pyaudio
import soxr

from listening_app.devices import AudioDevice, com_initialized
from listening_app.models import SAMPLE_RATE, Audio, Source

log = logging.getLogger(__name__)

_POLL_S = 0.1
_BUFFER_S = 0.02
_JOIN_TIMEOUT_S = 5.0
_STALL_LIMIT_S = 3.0
"""A microphone delivers frames even in silence, so this long without any means it is gone."""

AudioSink = Callable[[Audio], None]
LossHandler = Callable[[Source, str], None]


class CaptureError(Exception):
    pass


class TimelineAligner:
    """Keeps a stream's sample count in step with the clock by inserting silence for gaps.

    WASAPI loopback delivers no frames while nothing plays on the device. Without filling those gaps
    the segmenter would never see the pause that ends a segment, and later timestamps would drift.
    A microphone can lose frames too (a stalled stream overflows); without the silence every later
    timestamp of that track would come out early.
    """

    def __init__(self, start_time: float, gap_threshold_s: float = 0.5, idle_margin_s: float = 0.3) -> None:
        self._start = start_time
        self._threshold = round(gap_threshold_s * SAMPLE_RATE)
        self._idle_margin_s = idle_margin_s
        self._samples = 0

    def silence_before_chunk(self, chunk_samples: int, arrival_time: float) -> int:
        """Samples of silence to insert before a chunk that just arrived."""
        silence = self._gap_to(self._position(arrival_time) - chunk_samples)
        self._samples += silence + chunk_samples
        return silence

    def silence_while_idle(self, now: float) -> int:
        """Samples of silence to insert while no frames arrive, trailing the clock by a small margin."""
        silence = self._gap_to(self._position(now - self._idle_margin_s))
        self._samples += silence
        return silence

    def _position(self, moment: float) -> int:
        return round((moment - self._start) * SAMPLE_RATE)

    def _gap_to(self, position: int) -> int:
        gap = position - self._samples
        return gap if gap > self._threshold else 0


class AudioCapture:
    """Captures one WASAPI device on its own thread and hands 16 kHz mono audio to `on_audio`.

    `on_audio` and `on_lost` are always called from the capture thread, never concurrently.
    """

    def __init__(
        self,
        pa: Any,
        device: AudioDevice,
        source: Source,
        start_time: float,
        silent_when_idle: bool,
        on_audio: AudioSink,
        on_lost: LossHandler,
    ) -> None:
        self.device = device
        self.source = source
        self._pa = pa
        self._on_audio = on_audio
        self._on_lost = on_lost
        self._queue: queue.Queue[tuple[bytes, float]] = queue.Queue()
        self._resampler = soxr.ResampleStream(device.sample_rate, SAMPLE_RATE, 1, dtype="float32")
        self._aligner = TimelineAligner(start_time)
        self._silent_when_idle = silent_when_idle
        self._last_arrival = start_time
        self._stopping = threading.Event()
        self._stream: Any = None
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        try:
            self._stream = self._pa.open(
                format=pyaudio.paFloat32,
                channels=self.device.channels,
                rate=self.device.sample_rate,
                input=True,
                input_device_index=self.device.index,
                frames_per_buffer=int(self.device.sample_rate * _BUFFER_S),
                stream_callback=self._on_frames,
            )
        except OSError as exc:
            raise CaptureError(f"Cannot open '{self.device.name}': {exc}") from exc
        self._thread = threading.Thread(target=self._run, name=f"capture-{self.source}", daemon=True)
        self._thread.start()
        log.info("Capturing %s from '%s' (%d Hz, %d ch)", self.source, self.device.name,
                 self.device.sample_rate, self.device.channels)

    def stop(self) -> None:
        """Stop and deliver the remaining audio. Returns once the capture thread has finished."""
        self._stopping.set()
        if self._thread is not None:
            self._thread.join(timeout=_JOIN_TIMEOUT_S)

    def _on_frames(self, in_data: bytes, frame_count: int, time_info: Any, status: int) -> tuple[None, int]:
        self._queue.put((in_data, time.monotonic()))
        return None, pyaudio.paContinue

    def _run(self) -> None:
        with com_initialized():
            lost_reason = self._capture_and_close()
        if lost_reason:
            log.warning("Lost %s audio from '%s': %s", self.source, self.device.name, lost_reason)
            self._on_lost(self.source, lost_reason)

    def _capture_and_close(self) -> str | None:
        try:
            lost_reason = self._capture_until_stopped()
            self._close_stream()
            self._drain()
        except Exception as exc:
            log.exception("Capture of %s failed", self.source)
            self._close_stream()
            return f"capture error: {exc}"
        return lost_reason

    def _capture_until_stopped(self) -> str | None:
        while not self._stopping.is_set():
            self._process_next()
            reason = self._lost_reason(time.monotonic())
            if reason:
                return reason
        return None

    def _process_next(self) -> None:
        try:
            data, arrival = self._queue.get(timeout=_POLL_S)
        except queue.Empty:
            if self._silent_when_idle:
                self._emit(np.zeros(self._aligner.silence_while_idle(time.monotonic()), dtype=np.float32))
            return
        self._last_arrival = arrival
        self._process_chunk(data, arrival)

    def _process_chunk(self, data: bytes, arrival: float) -> None:
        samples = np.asarray(self._resampler.resample_chunk(self._downmix(data)), dtype=np.float32)
        silence = self._aligner.silence_before_chunk(len(samples), arrival)
        if silence and not self._silent_when_idle:
            log.warning("Lost %.1f s of %s audio; later audio keeps its real time", silence / SAMPLE_RATE, self.source)
        self._emit(np.zeros(silence, dtype=np.float32))
        self._emit(samples)

    def _downmix(self, data: bytes) -> Audio:
        frames = np.frombuffer(data, dtype=np.float32).reshape(-1, self.device.channels)
        return frames.mean(axis=1, dtype=np.float32)

    def _lost_reason(self, now: float) -> str | None:
        if not self._stream.is_active():
            return "the audio stream stopped (device unplugged?)"
        if not self._silent_when_idle and now - self._last_arrival > _STALL_LIMIT_S:
            return f"no audio for {_STALL_LIMIT_S:.0f} s (device unplugged?)"
        return None

    def _drain(self) -> None:
        while True:
            try:
                data, arrival = self._queue.get_nowait()
            except queue.Empty:
                break
            self._process_chunk(data, arrival)
        tail = self._resampler.resample_chunk(np.zeros(0, dtype=np.float32), last=True)
        self._emit(np.asarray(tail, dtype=np.float32))

    def _emit(self, samples: Audio) -> None:
        if len(samples):
            self._on_audio(samples)

    def _close_stream(self) -> None:
        stream, self._stream = self._stream, None
        if stream is None:
            return
        try:
            stream.stop_stream()
            stream.close()
        except OSError:
            log.debug("Closing the %s stream failed", self.source, exc_info=True)
