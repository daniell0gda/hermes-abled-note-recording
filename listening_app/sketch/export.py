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


def diagram_page(view: dict[str, Any]) -> str:
    """A standalone page that draws `view`. Only the web fonts are loaded from the internet, with local fallbacks."""
    title = html.escape(view["title"] or f"Diagram {view['number']}")
    return f"""<!doctype html>
<html lang="en">
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
<main class="sk-canvas"><svg id="sketch" role="img" aria-label="{title}"></svg></main>
</div>
<script>{asset("dagre.min.js")}</script>
<script>{asset("sketch.js")}</script>
<script>SketchRenderer.render(document.getElementById("sketch"), {script_json(view)});</script>
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
