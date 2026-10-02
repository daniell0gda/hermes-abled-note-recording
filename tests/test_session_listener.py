import json
from datetime import datetime
from pathlib import Path

import numpy as np
import pytest

from listening_app.config import AppConfig
from listening_app.devices import AudioSystem
from listening_app.hermes_client import HermesClient, HermesSettings
from listening_app.models import CapturedSegment, SegmentStatus, Source
from listening_app.session import RecordingSession
from listening_app.transcriber import Transcription
from listening_app.transcript_store import Devices, SessionMeta, TranscriptStore


class NoAudio(AudioSystem):
    def __init__(self) -> None:
        pass


class QuietHermesListener:
    def hermes_problem(self, title: str, message: str) -> None:
        pass

    def hermes_recovered(self) -> None:
        pass

    def session_drained(self, journal: Path) -> None:
        pass


def transcription(seq: int, text: str) -> Transcription:
    segment = CapturedSegment(seq, Source.ME, seq, 1.0, 2.5, datetime.now().astimezone(), np.zeros(16, np.float32))
    return Transcription(segment, text, SegmentStatus.OK)


def recording(tmp_path: Path) -> RecordingSession:
    config = AppConfig(output_dir=str(tmp_path))
    hermes = HermesClient(HermesSettings.from_config(config), False, QuietHermesListener())
    session = RecordingSession(config, NoAudio(), hermes, lambda title, message: None, lambda: None)
    session._store = TranscriptStore(session.files, SessionMeta(
        session_id=session.session_id, started_at="2026-10-02T10:00:00+02:00", devices=Devices(mic="m", output="o"),
        stt_model="groq/whisper-large-v3-turbo", language="en"))
    return session


def stored_texts(session: RecordingSession) -> list[str]:
    lines = session.files.jsonl.read_text(encoding="utf-8").splitlines()
    return [str(json.loads(line)["text"]) for line in lines]


def test_a_listener_gets_every_transcription_after_it_is_stored(tmp_path: Path) -> None:
    session = recording(tmp_path)
    heard: list[tuple[str, list[str]]] = []
    session.listen(lambda item: heard.append((item.text, stored_texts(session))))

    session._on_transcribed(transcription(0, "Auth caches tokens."))

    assert heard == [("Auth caches tokens.", ["Auth caches tokens."])]


def test_without_a_listener_the_recording_output_is_unchanged(tmp_path: Path) -> None:
    with_listener, without = recording(tmp_path / "a"), recording(tmp_path / "b")
    with_listener.listen(lambda item: None)

    for session in (with_listener, without):
        session._on_transcribed(transcription(0, "One."))
        session._on_transcribed(transcription(1, "Two."))

    assert stored_texts(with_listener) == stored_texts(without) == ["One.", "Two."]


def test_a_failing_listener_never_affects_the_recording(tmp_path: Path) -> None:
    session = recording(tmp_path)

    def broken(_: Transcription) -> None:
        raise RuntimeError("sketch crashed")

    session.listen(broken)
    session._on_transcribed(transcription(0, "One."))
    session._on_transcribed(transcription(1, "Two."))

    assert stored_texts(session) == ["One.", "Two."]


def test_listening_stops_when_the_handler_is_removed(tmp_path: Path) -> None:
    session = recording(tmp_path)
    heard: list[str] = []
    session.listen(lambda item: heard.append(item.text))
    session._on_transcribed(transcription(0, "One."))

    session.listen(None)
    session._on_transcribed(transcription(1, "Two."))

    assert heard == ["One."]


@pytest.fixture(autouse=True)
def no_api_keys(monkeypatch: pytest.MonkeyPatch) -> None:
    for name in ("GROQ_API_KEY", "OPENAI_API_KEY", "HERMES_API_KEY"):
        monkeypatch.delenv(name, raising=False)
