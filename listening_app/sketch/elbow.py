"""Diagram views drawn as SVG by elbow, the Rust renderer, through its command-line program."""

import json
import logging
import subprocess
import threading
import winreg
from collections import OrderedDict
from dataclasses import replace
from enum import StrEnum
from pathlib import Path
from typing import Any

from listening_app import paths
from listening_app.sketch.pipeline import SketchState

log = logging.getLogger(__name__)

PROGRAM_NAME = "elbow.exe"
BUNDLED_FOLDER = "elbow"
TIMEOUT_S = 5.0
CACHE_SIZE = 64
PERSONALIZE_KEY = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"
LIGHT_APPS_VALUE = "AppsUseLightTheme"
MARKING_FIELDS = ("selected", "pointed")


class Theme(StrEnum):
    LIGHT = "light"
    DARK = "dark"


def program_path() -> Path | None:
    """The bundled elbow program, or the crate's release build when run from source; None when neither exists."""
    candidates = (paths.bundle_dir() / BUNDLED_FOLDER / PROGRAM_NAME,
                  paths.install_dir() / "elbow" / "target" / "release" / PROGRAM_NAME)
    return next((candidate for candidate in candidates if candidate.is_file()), None)


def system_theme() -> Theme:
    """Dark when Windows apps use the dark theme, which the sketch window's web view follows too."""
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, PERSONALIZE_KEY) as key:
            light, _ = winreg.QueryValueEx(key, LIGHT_APPS_VALUE)
    except OSError:
        return Theme.LIGHT
    return Theme.LIGHT if light else Theme.DARK


class ElbowRenderer:
    """Adds elbow's SVG (`svg`) and its theme (`theme`) to every view with nodes. The selection and pointer are
    left out of the drawing, since the window marks them itself, so a click does not redraw. A view elbow
    cannot draw keeps no `svg`, and the window draws it itself."""

    def __init__(self, program: Path | None, theme: Theme) -> None:
        self._program = program
        self._theme = theme
        self._drawn: OrderedDict[str, str | None] = OrderedDict()
        self._lock = threading.Lock()

    def drawn(self, state: SketchState) -> SketchState:
        return replace(state, views=tuple(self.with_svg(view) for view in state.views))

    def with_svg(self, view: dict[str, Any]) -> dict[str, Any]:
        svg = self._svg(view) if view["nodes"] else None
        return view if svg is None else {**view, "svg": svg, "theme": self._theme.value}

    def _svg(self, view: dict[str, Any]) -> str | None:
        if self._program is None:
            return None
        key = json.dumps({name: value for name, value in view.items() if name not in MARKING_FIELDS},
                         ensure_ascii=False, sort_keys=True)
        with self._lock:
            if key in self._drawn:
                self._drawn.move_to_end(key)
                return self._drawn[key]
        svg = self._run(self._program, key)
        with self._lock:
            self._drawn[key] = svg
            while len(self._drawn) > CACHE_SIZE:
                self._drawn.popitem(last=False)
        return svg

    def _run(self, program: Path, view_json: str) -> str | None:
        command = [str(program), "-", "--best", "--params", json.dumps({"theme": self._theme.value})]
        try:
            result = subprocess.run(command, input=view_json, capture_output=True, text=True, encoding="utf-8",
                                    timeout=TIMEOUT_S, check=True, creationflags=subprocess.CREATE_NO_WINDOW)
        except (OSError, subprocess.SubprocessError) as exc:
            log.warning("elbow could not draw a diagram, the window draws it instead: %s", exc)
            return None
        return result.stdout.strip() or None
