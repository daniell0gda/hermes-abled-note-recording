"""Microphone-only transcription for live sketch without a recording: nothing is stored or sent to Hermes."""

import itertools
import threading
import time
from collections.abc import Callable
from datetime import datetime

from listening_app.capture import AudioCapture
from listening_app.config import AppConfig
from listening_app.devices import AudioSystem
from listening_app.models import Audio, Source
from listening_app.segmenter import Segmenter, SegmenterSettings, SpeechSegment
from listening_app.session import to_captured_segment
from listening_app.transcriber import SttSettings, Transcriber, Transcription, grok_access_token
from listening_app.vad import SileroVad


class MicTranscription:
    """Captures the configured microphone, cuts it at pauses and transcribes each segment like a recording does."""

    def __init__(self, config: AppConfig, audio: AudioSystem, on_transcribed: Callable[[Transcription], None],
                 on_lost: Callable[[str], None]) -> None:
        self._config = config
        self._audio = audio
        self._on_transcribed = on_transcribed
        self._lost = on_lost
        self._started_at = datetime.now().astimezone()
        self._seq = itertools.count()
        self._segmenter_lock = threading.Lock()
        self._segmenter: Segmenter | None = None
        self._transcriber: Transcriber | None = None
        self._capture: AudioCapture | None = None

    def start(self) -> None:
        """Raises GrokAuthError, DeviceError or CaptureError when the microphone cannot be transcribed."""
        settings = SttSettings.from_config(self._config)
        if settings.grok_auth:
            grok_access_token()
        choice = self._audio.resolve_mic(self._config.devices.mic)
        self._started_at = datetime.now().astimezone()
        self._segmenter = Segmenter(SegmenterSettings.from_config(self._config.segmentation), SileroVad())
        self._transcriber = Transcriber(settings, self._on_transcribed)
        capture = AudioCapture(self._audio.pa, choice.capture, Source.ME, time.monotonic(), silent_when_idle=False,
                               on_audio=self._on_audio, on_lost=self._on_lost)
        capture.start()
        self._capture = capture

    def stop(self) -> None:
        """Stop capturing and wait until the last segment has been transcribed."""
        if self._capture is not None:
            self._capture.stop()
            self._capture = None
        self._flush()
        if self._transcriber is not None:
            self._transcriber.close()
            self._transcriber = None

    def _on_audio(self, samples: Audio) -> None:
        if self._segmenter is None:
            return
        with self._segmenter_lock:
            speech = self._segmenter.feed(samples)
        for segment in speech:
            self._submit(segment)

    def _on_lost(self, source: Source, reason: str) -> None:
        self._flush()
        self._lost(reason)

    def _flush(self) -> None:
        if self._segmenter is None:
            return
        with self._segmenter_lock:
            speech = self._segmenter.flush()
        if speech is not None:
            self._submit(speech)

    def _submit(self, speech: SpeechSegment) -> None:
        if self._transcriber is None:
            return
        seq = next(self._seq)
        self._transcriber.submit(to_captured_segment(seq, Source.ME, seq, speech, self._started_at))
