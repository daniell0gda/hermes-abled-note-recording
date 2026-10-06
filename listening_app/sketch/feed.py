"""Where live sketch hears the speaker: the running recording, or its own microphone-only transcription."""

import re
from collections.abc import Callable
from typing import Protocol

from listening_app.transcriber import Transcription

TranscriptionHandler = Callable[[Transcription], None]

_SENTENCE_END = re.compile(r"(?<=[.!?])\s+|\n+")


def typed_lines(text: str) -> list[str]:
    """Typed text cut into the sentence-sized lines a transcription would deliver."""
    return [line for part in _SENTENCE_END.split(text) if (line := " ".join(part.split()))]


class Microphone(Protocol):
    def start(self) -> None: ...

    def stop(self) -> None: ...


class Recording(Protocol):
    def listen(self, handler: TranscriptionHandler | None) -> None: ...


class SketchFeed:
    """Hands transcriptions to `handler`, switching source as recordings start and stop. Never touches a recording
    beyond attaching and detaching the handler. While muted it hears nothing, whatever the source."""

    def __init__(self, handler: TranscriptionHandler, open_microphone: Callable[[TranscriptionHandler], Microphone]) -> None:
        self._handler = handler
        self._open_microphone = open_microphone
        self._microphone: Microphone | None = None
        self._recording: Recording | None = None
        self._muted = False

    @property
    def listening(self) -> bool:
        return not self._muted

    def start(self, recording: Recording | None) -> None:
        self._recording = recording
        self._connect()

    def use_recording(self, recording: Recording) -> None:
        self._disconnect()
        self._recording = recording
        self._connect()

    def use_microphone(self) -> None:
        self._detach_recording()
        self._recording = None
        self._connect()

    def mute(self) -> None:
        self._disconnect()
        self._muted = True

    def unmute(self) -> None:
        """Raises like start when the microphone cannot be opened; the feed then stays muted."""
        if self._muted:
            self._attach()
            self._muted = False

    def stop(self) -> None:
        self._disconnect()
        self._recording = None

    def _connect(self) -> None:
        if not self._muted:
            self._attach()

    def _attach(self) -> None:
        if self._recording is not None:
            self._recording.listen(self._handler)
        else:
            self._start_microphone()

    def _disconnect(self) -> None:
        self._detach_recording()
        self._stop_microphone()

    def _detach_recording(self) -> None:
        if self._recording is not None:
            self._recording.listen(None)

    def _start_microphone(self) -> None:
        if self._microphone is not None:
            return
        microphone = self._open_microphone(self._handler)
        microphone.start()
        self._microphone = microphone

    def _stop_microphone(self) -> None:
        if self._microphone is not None:
            self._microphone.stop()
            self._microphone = None
