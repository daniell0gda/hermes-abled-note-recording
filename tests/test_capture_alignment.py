from collections.abc import Callable, Sequence
from typing import Any

import numpy as np
import pytest

from listening_app import capture
from listening_app.capture import AudioCapture, TimelineAligner
from listening_app.devices import AudioDevice
from listening_app.models import SAMPLE_RATE, Audio, Source

CHUNK = 320  # 20 ms


class FakeClock:
    def __init__(self) -> None:
        self.now = 0.0

    def monotonic(self) -> float:
        return self.now


class FakeStream:
    def is_active(self) -> bool:
        return True

    def stop_stream(self) -> None:
        pass

    def close(self) -> None:
        pass


class ScriptedPortAudio:
    """Opens a mono stream that delivers one 20 ms chunk at each scripted arrival time."""

    def __init__(self, clock: FakeClock, arrivals: Sequence[float]) -> None:
        self._clock = clock
        self._arrivals = arrivals

    def open(self, stream_callback: Callable[[bytes, int, Any, int], Any], **_: Any) -> FakeStream:
        for arrival in self._arrivals:
            self._clock.now = arrival
            stream_callback(np.ones(CHUNK, dtype=np.float32).tobytes(), CHUNK, {}, 0)
        return FakeStream()


def captured_microphone_audio(monkeypatch: pytest.MonkeyPatch, arrivals: Sequence[float]) -> list[Audio]:
    clock = FakeClock()
    monkeypatch.setattr(capture, "time", clock)
    received: list[Audio] = []
    device = AudioDevice(index=0, name="Fake mic", channels=1, sample_rate=SAMPLE_RATE)
    audio_capture = AudioCapture(ScriptedPortAudio(clock, arrivals), device, Source.ME, 0.0, False,
                                 received.append, lambda _source, _reason: None)
    audio_capture.start()
    audio_capture.stop()
    return received


def test_continuous_stream_gets_no_silence() -> None:
    aligner = TimelineAligner(start_time=0.0)

    inserted = [aligner.silence_before_chunk(CHUNK, (index + 1) * 0.02) for index in range(500)]

    assert sum(inserted) == 0


def test_jitter_below_the_threshold_is_ignored() -> None:
    aligner = TimelineAligner(start_time=0.0)

    assert aligner.silence_before_chunk(CHUNK, 0.3) == 0


def test_gap_in_a_loopback_stream_is_filled_with_silence() -> None:
    aligner = TimelineAligner(start_time=0.0)
    aligner.silence_before_chunk(CHUNK, 0.02)

    silence = aligner.silence_before_chunk(CHUNK, 5.02)

    assert silence == 5 * SAMPLE_RATE - CHUNK


def test_idle_stream_is_padded_up_to_the_clock_minus_a_margin() -> None:
    aligner = TimelineAligner(start_time=10.0, idle_margin_s=0.3)

    assert aligner.silence_while_idle(10.5) == 0
    assert aligner.silence_while_idle(13.3) == 3 * SAMPLE_RATE


def test_audio_resuming_after_idle_padding_is_not_padded_twice() -> None:
    aligner = TimelineAligner(start_time=0.0, idle_margin_s=0.3)
    aligner.silence_while_idle(4.0)

    assert aligner.silence_before_chunk(CHUNK, 4.05) == 0


def test_microphone_that_drops_frames_keeps_later_audio_at_its_real_time(monkeypatch: pytest.MonkeyPatch) -> None:
    before_drop = [(index + 1) * 0.02 for index in range(20)]  # 0.02 .. 0.40 s
    after_drop = [1.42 + index * 0.02 for index in range(30)]  # 1.42 .. 2.00 s, the second in between is lost

    received = captured_microphone_audio(monkeypatch, before_drop + after_drop)

    assert sum(len(samples) for samples in received) == 2 * SAMPLE_RATE
