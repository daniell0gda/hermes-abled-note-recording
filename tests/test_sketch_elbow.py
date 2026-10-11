import subprocess
from pathlib import Path
from typing import Any

import pytest

from listening_app.sketch.elbow import ElbowRenderer, Theme, program_path
from listening_app.sketch.pipeline import SketchState

PROGRAM = program_path()
needs_elbow = pytest.mark.skipif(PROGRAM is None, reason="elbow is not built (cargo build --release in ../elbow) or installed")


def view(kind: str = "kanban", selected: list[str] | None = None) -> dict[str, Any]:
    return {"number": 1, "title": "Board", "kind": kind,
            "nodes": [{"id": "fix", "label": "Fix login (AVA-1 · sam)", "shape": "service", "focal": False}],
            "edges": [], "groups": [{"id": "doing", "label": "In progress (3)", "members": ["fix"]}],
            "selected": selected or [], "pointed": None}


@needs_elbow
def test_a_view_gets_elbows_svg_in_the_renderers_theme() -> None:
    drawn = ElbowRenderer(PROGRAM, Theme.DARK).with_svg(view())

    assert drawn["svg"].startswith("<svg")
    assert '<g class="sk-node" data-id="fix">' in drawn["svg"]
    assert 'fill="#2d3142"' in drawn["svg"]
    assert drawn["theme"] == "dark"


@needs_elbow
def test_every_view_of_a_state_is_drawn() -> None:
    state = SketchState((view(), {**view(), "number": 2}), 1, 1)

    drawn = ElbowRenderer(PROGRAM, Theme.LIGHT).drawn(state)

    assert all("svg" in drawn_view for drawn_view in drawn.views)
    assert (drawn.active, drawn.updated) == (1, 1)


def test_a_change_of_selection_or_pointer_does_not_draw_again(monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[str] = []

    def fake_run(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
        calls.append(kwargs["input"])
        return subprocess.CompletedProcess(command, 0, stdout="<svg/>", stderr="")

    monkeypatch.setattr(subprocess, "run", fake_run)
    renderer = ElbowRenderer(Path("elbow.exe"), Theme.LIGHT)

    renderer.with_svg(view())
    renderer.with_svg({**view(selected=["fix"]), "pointed": "fix"})

    assert len(calls) == 1
    assert '"selected"' not in calls[0] and '"pointed"' not in calls[0]


def test_a_view_without_nodes_is_left_to_the_window() -> None:
    empty = {**view(), "nodes": [], "groups": []}

    assert ElbowRenderer(PROGRAM, Theme.LIGHT).with_svg(empty) == empty


@pytest.mark.parametrize("program", [None, Path("missing-elbow.exe")])
def test_without_a_working_elbow_the_view_stays_as_it_was(program: Path | None) -> None:
    assert ElbowRenderer(program, Theme.LIGHT).with_svg(view()) == view()
