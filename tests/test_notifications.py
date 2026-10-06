from pathlib import Path

import pytest

from listening_app import app
from listening_app.app import AppController
from listening_app.config import ConfigManager
from listening_app.logging_setup import RedactingFormatter


class RecordingUi:
    def __init__(self) -> None:
        self.shown: list[str] = []

    def notify(self, title: str, message: str) -> None:
        self.shown.append(title)

    def refresh(self) -> None:
        pass


def controller_with(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, config_text: str) -> tuple[AppController,
                                                                                                  RecordingUi]:
    monkeypatch.setattr(app, "AudioSystem", lambda: None)
    path = tmp_path / "config.yaml"
    path.write_text(config_text, encoding="utf-8")
    configs = ConfigManager(path)
    configs.reload()
    controller, ui = AppController(configs, RedactingFormatter()), RecordingUi()
    controller.attach(ui)
    return controller, ui


def test_notifications_are_shown_by_default(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    controller, ui = controller_with(tmp_path, monkeypatch, "language: pl\n")

    controller.notify("Recording started", "...")

    assert ui.shown == ["Recording started"]


def test_notifications_reach_every_attached_frontend(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    controller, tray = controller_with(tmp_path, monkeypatch, "language: pl\n")
    mcp = RecordingUi()
    controller.attach(mcp)

    controller.notify("Recording started", "...")

    assert tray.shown == ["Recording started"]
    assert mcp.shown == ["Recording started"]


def test_hidden_notifications_are_only_logged(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                              caplog: pytest.LogCaptureFixture) -> None:
    controller, ui = controller_with(tmp_path, monkeypatch, "notifications: false\n")

    with caplog.at_level("INFO"):
        controller.notify("Recording started", "...")

    assert ui.shown == []
    assert "Notification: Recording started" in caplog.text
