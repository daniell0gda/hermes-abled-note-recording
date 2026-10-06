"""Pause/resume of live transcription while a recording session is active."""

from pathlib import Path
from unittest.mock import MagicMock

import numpy as np
import pytest

from listening_app.config import AppConfig, ConfigError, load_config
from listening_app.devices import AudioSystem
from listening_app.hermes_client import HermesClient, HermesSettings
from listening_app.models import SAMPLE_RATE, Source
from listening_app.segmenter import Segmenter, SegmenterSettings
from listening_app.session import RecordingSession
from listening_app.vad import SileroVad


EXAMPLE = Path(__file__).resolve().parent.parent / "config.example.yaml"


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


@pytest.fixture(autouse=True)
def no_api_keys(monkeypatch: pytest.MonkeyPatch) -> None:
    for name in ("GROQ_API_KEY", "OPENAI_API_KEY", "HERMES_API_KEY"):
        monkeypatch.delenv(name, raising=False)


def session_with_segmenters(tmp_path: Path) -> RecordingSession:
    config = AppConfig(output_dir=str(tmp_path))
    hermes = HermesClient(HermesSettings.from_config(config), False, QuietHermesListener())
    session = RecordingSession(config, NoAudio(), hermes, lambda title, message: None, lambda: None)
    settings = SegmenterSettings.from_config(config.segmentation)
    session._segmenters = {Source.ME: Segmenter(settings, SileroVad())}
    session._transcriber = MagicMock()
    return session


def loud_chunk(seconds: float = 0.1) -> np.ndarray:
    n = int(SAMPLE_RATE * seconds)
    return np.full(n, 0.5, dtype=np.float32)


def test_example_config_includes_pause_hotkey() -> None:
    config = load_config(EXAMPLE)
    assert config.pause_hotkey == "ctrl+shift+p"


def test_empty_pause_hotkey_disables_it(tmp_path: Path) -> None:
    path = tmp_path / "config.yaml"
    path.write_text('pause_hotkey: ""\n', encoding="utf-8")
    assert load_config(path).pause_hotkey == ""


def test_invalid_pause_hotkey_is_rejected(tmp_path: Path) -> None:
    path = tmp_path / "config.yaml"
    path.write_text("pause_hotkey: p\n", encoding="utf-8")
    with pytest.raises(ConfigError, match="pause_hotkey"):
        load_config(path)


def test_pause_hotkey_must_differ_from_recording_and_sketch(tmp_path: Path) -> None:
    path = tmp_path / "config.yaml"
    path.write_text('hotkey: "ctrl+alt+r"\npause_hotkey: "Ctrl + Alt + R"\n', encoding="utf-8")
    with pytest.raises(ConfigError, match="pause_hotkey must differ from hotkey"):
        load_config(path)

    path.write_text('pause_hotkey: "ctrl+alt+d"\nsketch:\n  hotkey: "ctrl+alt+d"\n', encoding="utf-8")
    with pytest.raises(ConfigError, match="sketch.hotkey must differ from pause_hotkey"):
        load_config(path)


def test_pause_stops_new_segments_from_being_submitted(tmp_path: Path) -> None:
    session = session_with_segmenters(tmp_path)
    assert session.pause() is True
    assert session.is_paused

    for _ in range(20):
        session._on_audio(Source.ME, loud_chunk(0.2))

    session._transcriber.submit.assert_not_called()

    assert session.resume() is True
    assert not session.is_paused


def test_toggle_pause_round_trips(tmp_path: Path) -> None:
    session = session_with_segmenters(tmp_path)
    assert session.toggle_pause() is True
    assert session.is_paused
    assert session.toggle_pause() is False
    assert not session.is_paused


def test_pause_while_already_paused_is_a_no_op(tmp_path: Path) -> None:
    session = session_with_segmenters(tmp_path)
    assert session.pause() is True
    assert session.pause() is False


def test_resume_while_not_paused_is_a_no_op(tmp_path: Path) -> None:
    session = session_with_segmenters(tmp_path)
    assert session.resume() is False
