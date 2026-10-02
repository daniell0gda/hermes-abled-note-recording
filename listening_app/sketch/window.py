"""The app's handle on the live sketch window process (see window_app)."""

import json
import logging
import subprocess
import sys
import threading
from collections.abc import Callable
from typing import Any

from listening_app import paths
from listening_app.sketch.pipeline import SketchState

log = logging.getLogger(__name__)

WINDOW_FLAG = "--sketch-window"
CLOSE_TIMEOUT_S = 3.0
CLOSED_EVENT: dict[str, Any] = {"type": "closed"}

WindowEvent = Callable[[dict[str, Any]], None]


def window_command() -> list[str]:
    if paths.is_frozen():
        return [sys.executable, WINDOW_FLAG]
    return [sys.executable, "-m", "listening_app", WINDOW_FLAG]


class SketchWindow:
    """Starts the window process and exchanges JSON lines with it. `on_event` runs on a reader thread; the
    window being closed by the user arrives as {"type": "closed"}."""

    def __init__(self, on_event: WindowEvent) -> None:
        self._on_event = on_event
        self._process: subprocess.Popen[str] | None = None
        self._lock = threading.Lock()

    def open(self) -> None:
        self._process = subprocess.Popen(
            window_command(), stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            text=True, encoding="utf-8", bufsize=1, creationflags=subprocess.CREATE_NO_WINDOW,
            cwd=paths.install_dir())
        threading.Thread(target=self._read_events, args=(self._process,), name="sketch-window", daemon=True).start()

    def show(self, state: SketchState) -> None:
        self._send({"type": "state", "views": list(state.views), "active": state.active, "updated": state.updated})

    def point(self, element: str | None) -> None:
        self._send({"type": "point", "element": element})

    def notice(self, text: str) -> None:
        self._send({"type": "notice", "text": text})

    def microphone(self, listening: bool) -> None:
        self._send({"type": "microphone", "on": listening})

    def close(self) -> None:
        process, self._process = self._process, None
        if process is None:
            return
        with self._lock:
            if process.stdin is not None:
                try:
                    process.stdin.close()
                except OSError:
                    pass
        try:
            process.wait(CLOSE_TIMEOUT_S)
        except subprocess.TimeoutExpired:
            process.kill()

    def _send(self, message: dict[str, Any]) -> None:
        process = self._process
        if process is None or process.stdin is None:
            return
        with self._lock:
            try:
                process.stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
                process.stdin.flush()
            except (OSError, ValueError) as exc:
                log.warning("The live sketch window is gone: %s", exc)

    def _read_events(self, process: subprocess.Popen[str]) -> None:
        assert process.stdout is not None
        for line in process.stdout:
            try:
                event = json.loads(line)
            except ValueError:
                log.warning("Unreadable message from the live sketch window: %r", line[:200])
                continue
            self._dispatch(event)
        if process is self._process:
            self._dispatch(CLOSED_EVENT)

    def _dispatch(self, event: dict[str, Any]) -> None:
        try:
            self._on_event(event)
        except Exception:
            log.exception("Handling a live sketch window event failed")
