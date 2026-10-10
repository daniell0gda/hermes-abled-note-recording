from pathlib import Path
from typing import Any

from listening_app.sketch.export import DiagramFiles
from listening_app.sketch.library import PICKER_NAME, picker_page, saved_diagrams, write_picker

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
    (tmp_path / "2026-10-08T10-00-00_ee55").mkdir()
    (tmp_path / PICKER_NAME).write_text("old page", encoding="utf-8")

    assert [diagram.title for diagram in saved_diagrams(tmp_path)] == ["Request path"]


def test_a_missing_diagrams_folder_has_no_diagrams(tmp_path: Path) -> None:
    assert saved_diagrams(tmp_path / "diagrams") == []


def test_the_picker_links_each_diagram_relative_to_the_folder(tmp_path: Path) -> None:
    save(tmp_path, SECOND_SESSION, "Zamówienie: cykl & <życia>")
    diagrams = saved_diagrams(tmp_path)

    page = picker_page(diagrams, tmp_path)

    assert f'href="{SECOND_SESSION}/01-zam%C3%B3wienie-cykl-%C5%BCycia.html"' in page
    assert "Zamówienie: cykl &amp; &lt;życia&gt;" in page
    assert "2026-10-10 09:30" in page


def test_the_picker_says_so_when_nothing_is_saved_yet(tmp_path: Path) -> None:
    page = picker_page([], tmp_path)

    assert "No saved diagrams yet" in page
    assert "<ul>" not in page


def test_writing_the_picker_replaces_the_previous_page(tmp_path: Path) -> None:
    save(tmp_path, SECOND_SESSION, "Request path")
    write_picker(tmp_path)
    save(tmp_path, FIRST_SESSION, "Old flow")

    path = write_picker(tmp_path)

    assert path == tmp_path / PICKER_NAME
    page = path.read_text(encoding="utf-8")
    assert "Request path" in page
    assert "Old flow" in page
