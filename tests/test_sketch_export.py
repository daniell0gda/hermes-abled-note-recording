import json
import re
from pathlib import Path
from typing import Any

from listening_app.sketch.export import DiagramFiles, diagram_page, file_name


def view(number: int = 1, title: str = "Request path", label: str = "Auth service") -> dict[str, Any]:
    return {"number": number, "title": title, "kind": "flow",
            "nodes": [{"id": "auth", "label": label, "shape": "service", "focal": False}],
            "edges": [], "groups": [], "selected": [], "pointed": None}


def embedded_view(page: str) -> dict[str, Any]:
    match = re.search(r"SketchRenderer\.render\(document\.getElementById\(\"sketch\"\), (.*)\);</script>", page)
    assert match is not None
    data: dict[str, Any] = json.loads(match[1].replace("<\\/", "</"))
    return data


def test_file_names_are_numbered_and_carry_a_title_slug() -> None:
    assert file_name(1, "Request path") == "01-request-path.html"
    assert file_name(12, "Zamówienie: cykl życia / v2") == "12-zamówienie-cykl-życia-v2.html"
    assert file_name(3, "") == "03-diagram.html"


def test_the_page_is_self_contained_and_renders_the_view() -> None:
    page = diagram_page(view())

    assert "<script src" not in page
    assert "SketchRenderer" in page
    assert "dagre" in page
    assert "--paper" in page
    assert "<title>Request path</title>" in page
    assert embedded_view(page) == view()


def test_text_from_the_speaker_cannot_break_out_of_the_page() -> None:
    page = diagram_page(view(title="<b>x</b>", label="</script><script>alert(1)</script>"))

    assert "<title>&lt;b&gt;x&lt;/b&gt;</title>" in page
    assert "</script><script>alert(1)" not in page
    assert embedded_view(page)["nodes"][0]["label"] == "</script><script>alert(1)</script>"


def test_every_update_rewrites_the_diagrams_file(tmp_path: Path) -> None:
    files = DiagramFiles(tmp_path / "diagrams" / "session")

    files.save(view(label="Auth"))
    path = files.save(view(label="Auth service"))

    assert path == tmp_path / "diagrams" / "session" / "01-request-path.html"
    assert embedded_view(path.read_text(encoding="utf-8"))["nodes"][0]["label"] == "Auth service"


def test_a_renamed_diagram_replaces_its_old_file(tmp_path: Path) -> None:
    files = DiagramFiles(tmp_path)
    files.save(view(title=""))
    files.save(view(number=2, title="Order lifecycle"))

    files.save(view(title="Request path"))

    assert sorted(path.name for path in tmp_path.iterdir()) == ["01-request-path.html", "02-order-lifecycle.html"]


def test_a_diagrams_path_follows_its_latest_title(tmp_path: Path) -> None:
    files = DiagramFiles(tmp_path)
    files.save(view(title=""))

    files.save(view(title="Request path"))

    assert files.path(1) == tmp_path / "01-request-path.html"
    assert files.path(2) is None
