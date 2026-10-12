"""Each diagram as one self-contained HTML file, rewritten on every update."""

import html
import json
import re
from functools import cache
from pathlib import Path
from typing import Any

from listening_app import paths
from listening_app.files import atomic_write_text

SKETCH_ASSETS = paths.ASSETS_DIR / "sketch"
FONTS_URL = ("https://fonts.googleapis.com/css2?family=Instrument+Serif&family=Geist:wght@400;500;600"
             "&family=Geist+Mono:wght@400;500&display=swap")
_SLUG_LENGTH = 48
_EMBEDDED_VIEW = re.compile(r'<script type="application/json" id="diagram-view">(.*?)</script>')
_LEGACY_VIEW = re.compile(r'SketchRenderer\.render\(document\.getElementById\("sketch"\), (.*)\);</script>')


@cache
def asset(name: str) -> str:
    """A bundled renderer file (sketch.css, sketch.js, dagre.min.js)."""
    return (SKETCH_ASSETS / name).read_text(encoding="utf-8")


def script_json(value: object) -> str:
    """JSON that is safe inside a <script> element."""
    return json.dumps(value, ensure_ascii=False).replace("</", "<\\/")


def file_name(number: int, title: str) -> str:
    slug = re.sub(r"\W+", "-", title.lower()).strip("-")[:_SLUG_LENGTH].strip("-") or "diagram"
    return f"{number:02d}-{slug}.html"


def drawing(view: dict[str, Any], title: str) -> str:
    """Elbow's SVG when the view carries it, else an empty SVG that the bundled renderer draws `view` into."""
    if "svg" in view:
        return f'<main class="sk-canvas">{view["svg"]}</main>'
    return f"""<main class="sk-canvas"><svg id="sketch" role="img" aria-label="{title}"></svg></main>
<script>{asset("dagre.min.js")}</script>
<script>{asset("sketch.js")}</script>
<script>SketchRenderer.render(document.getElementById("sketch"), {script_json(view)});</script>"""


def embedded_view(view: dict[str, Any]) -> str:
    """The view as JSON for the saved page, without elbow's SVG, so the page can be opened again."""
    data = {key: value for key, value in view.items() if key != "svg"}
    return f'<script type="application/json" id="diagram-view">{script_json(data)}</script>'


def read_diagram(path: Path) -> dict[str, Any]:
    """The view that `diagram_page` embedded in the saved page at `path`. Raises ValueError when there is none."""
    text = path.read_text(encoding="utf-8")
    match = _EMBEDDED_VIEW.search(text) or _LEGACY_VIEW.search(text)
    if match is None:
        raise ValueError(f"{path.name} holds no diagram")
    view: dict[str, Any] = json.loads(match[1].replace("<\\/", "</"))
    return view


def diagram_page(view: dict[str, Any]) -> str:
    """A standalone page that draws `view`. Only the web fonts are loaded from the internet, with local fallbacks."""
    title = html.escape(view["title"] or f"Diagram {view['number']}")
    theme = f' data-theme="{html.escape(view["theme"])}"' if "theme" in view else ""
    return f"""<!doctype html>
<html lang="en"{theme}>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link href="{FONTS_URL}" rel="stylesheet">
<style>{asset("sketch.css")}</style>
</head>
<body>
<div class="sk-page">
<header class="sk-header"><div class="sk-eyebrow-title">Diagram {view["number"]} · {html.escape(view["kind"])}</div>
<h1 class="sk-title">{title}</h1></header>
{drawing(view, title)}
{embedded_view(view)}
</div>
</body>
</html>
"""


class DiagramFiles:
    """`NN-<title>.html` per diagram in `folder`. When a diagram's title changes, its old file is removed."""

    def __init__(self, folder: Path) -> None:
        self.folder = folder
        self._paths: dict[int, Path] = {}

    def save(self, view: dict[str, Any]) -> Path:
        number = view["number"]
        path = self.folder / file_name(number, view["title"])
        atomic_write_text(path, diagram_page(view))
        previous = self._paths.get(number)
        if previous is not None and previous != path:
            previous.unlink(missing_ok=True)
        self._paths[number] = path
        return path

    def path(self, number: int) -> Path | None:
        """The file diagram `number` was last saved to, or None before its first save."""
        return self._paths.get(number)
