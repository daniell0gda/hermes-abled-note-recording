from pathlib import Path
from typing import Any

from listening_app.sketch.export import DiagramFiles
from listening_app.sketch.library import saved_diagram_path, saved_diagrams

FIRST_SESSION = "2026-10-09T18-05-00_ab12"
SECOND_SESSION = "2026-10-10T09-30-00_cd34"


def view(number: int, title: str) -> dict[str, Any]:
    return {"number": number, "title": title, "kind": "flow",
            "nodes": [], "edges": [], "groups": [], "selected": [], "pointed": None}


def save(diagrams: Path, session: str, *titles: str) -> None:
    files = DiagramFiles(diagrams / session)
    for number, title in enumerate(titles, start=1):
        files.save(view(number, title))


def test_saved_diagrams_list_the_newest_session_first_and_in_number_order(tmp_path: Path) -> None:
    save(tmp_path, FIRST_SESSION, "Old flow")
    save(tmp_path, SECOND_SESSION, "Request path", "Order saga")

    diagrams = saved_diagrams(tmp_path)

    assert [diagram.title for diagram in diagrams] == ["Request path", "Order saga", "Old flow"]
    assert diagrams[0].started.isoformat() == "2026-10-10T09:30:00"
    assert diagrams[0].path == tmp_path / SECOND_SESSION / "01-request-path.html"


def test_folders_and_files_that_are_not_sessions_are_skipped(tmp_path: Path) -> None:
    save(tmp_path, SECOND_SESSION, "Request path")
    (tmp_path / "notes").mkdir()
    (tmp_path / "index.html").write_text("<title>Stray</title>", encoding="utf-8")

    assert [diagram.title for diagram in saved_diagrams(tmp_path)] == ["Request path"]


def test_a_missing_diagrams_folder_has_no_diagrams(tmp_path: Path) -> None:
    assert saved_diagrams(tmp_path / "diagrams") == []


def test_a_saved_diagram_page_is_accepted_when_it_lies_one_session_deep(tmp_path: Path) -> None:
    save(tmp_path, SECOND_SESSION, "Request path")
    page = tmp_path / SECOND_SESSION / "01-request-path.html"

    assert saved_diagram_path(tmp_path, str(page)) == page.resolve()


def test_paths_that_are_not_saved_diagram_pages_are_refused(tmp_path: Path) -> None:
    diagrams = tmp_path / "diagrams"
    save(diagrams, SECOND_SESSION, "Request path")
    (diagrams / SECOND_SESSION / "notes.txt").write_text("", encoding="utf-8")
    outside = tmp_path / "elsewhere.html"
    outside.write_text("", encoding="utf-8")

    assert saved_diagram_path(diagrams, str(outside)) is None
    assert saved_diagram_path(diagrams, str(diagrams / SECOND_SESSION)) is None
    assert saved_diagram_path(diagrams, str(diagrams / SECOND_SESSION / "missing.html")) is None
    assert saved_diagram_path(diagrams, str(diagrams / SECOND_SESSION / "notes.txt")) is None
