import time
from collections.abc import Callable
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


class ClosableSketch:
    instances: list["ClosableSketch"] = []

    def __init__(self, config: object, audio: object, notify: object, on_closed: Callable[[], None]) -> None:
        self.folder = Path("diagrams")
        self.on_closed = on_closed
        self.stopped = False
        ClosableSketch.instances.append(self)

    def start(self, recording: object, listen: bool = True) -> None:
        pass

    def stop(self) -> None:
        self.stopped = True


def controller_with_sketch(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[AppController, RecordingUi]:
    monkeypatch.setattr(app, "AudioSystem", lambda: None)
    monkeypatch.setattr(app, "LiveSketch", ClosableSketch)
    monkeypatch.setattr(ClosableSketch, "instances", [])
    path = tmp_path / "config.yaml"
    path.write_text("language: en\n", encoding="utf-8")
    configs = ConfigManager(path)
    configs.reload()
    controller, ui = AppController(configs, RedactingFormatter(), hotkeys=False), RecordingUi()
    controller.attach(ui)
    return controller, ui


def wait_until_idle(controller: AppController) -> None:
    deadline = time.monotonic() + 5
    while controller.is_sketch_busy and time.monotonic() < deadline:
        time.sleep(0.01)


def test_a_window_closed_while_live_sketch_starts_still_ends_it(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    controller, ui = controller_with_sketch(tmp_path, monkeypatch)
    monkeypatch.setattr(ClosableSketch, "start", lambda self, recording, listen=True: self.on_closed())

    controller.start_sketch(listen=True)
    wait_until_idle(controller)

    assert controller.is_sketching is False
    assert ClosableSketch.instances[-1].stopped is True


def test_closing_the_sketch_window_ends_live_sketch_like_the_tray(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    controller, ui = controller_with_sketch(tmp_path, monkeypatch)
    controller.start_sketch(listen=True)
    sketch = ClosableSketch.instances[-1]

    sketch.on_closed()
    wait_until_idle(controller)

    assert controller.is_sketching is False
    assert sketch.stopped is True
    assert ui.shown[-1] == "Live sketch off"
