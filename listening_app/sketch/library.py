"""The saved diagrams of every live sketch, listed on a picker page that the tray opens in the browser."""

import html
import re
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from urllib.parse import quote

from listening_app.files import atomic_write_text
from listening_app.sketch.export import FONTS_URL, asset

PICKER_NAME = "index.html"
PICKER_TITLE = "Saved diagrams"
_EMPTY_MESSAGE = "No saved diagrams yet. Turn on live sketch and explain what to draw."
_SESSION_STAMP_FORMAT = "%Y-%m-%dT%H-%M-%S"
_HEAD_CHARS = 4096
_TITLE = re.compile(r"<title>(.*?)</title>", re.DOTALL)
_PICKER_CSS = """
.sk-picker { flex: 1; min-height: 0; overflow: auto; padding: 8px 24px 24px; }
.sk-picker ul { list-style: none; margin: 0; padding: 0; max-width: 760px; }
.sk-picker li { display: flex; justify-content: space-between; align-items: baseline; gap: 16px;
  padding: 12px 0; border-bottom: 1px solid var(--rule); }
.sk-picker a { color: var(--link); font: 500 1rem/1.4 var(--sans); }
.sk-picker time { color: var(--muted); font: 400 12px/1.4 var(--mono); white-space: nowrap; }
.sk-picker p { color: var(--muted); font: italic 16px var(--serif); }
"""


@dataclass(frozen=True)
class SavedDiagram:
    title: str
    started: datetime
    path: Path


def saved_diagrams(folder: Path) -> list[SavedDiagram]:
    """Every diagram page in `folder`'s live sketch sessions, newest session first and in number order within one."""
    if not folder.is_dir():
        return []
    diagrams: list[SavedDiagram] = []
    for session in sorted(folder.iterdir(), reverse=True):
        started = _session_start(session)
        if started is not None:
            diagrams.extend(SavedDiagram(_page_title(page), started, page) for page in sorted(session.glob("*.html")))
    return diagrams


def picker_page(diagrams: list[SavedDiagram], folder: Path) -> str:
    """A page that lists `diagrams`. Each entry links to the diagram's own page, relative to `folder`."""
    items = "\n".join(_entry(diagram, folder) for diagram in diagrams)
    body = f"<ul>\n{items}\n</ul>" if diagrams else f"<p>{_EMPTY_MESSAGE}</p>"
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{PICKER_TITLE}</title>
<link href="{FONTS_URL}" rel="stylesheet">
<style>{asset("sketch.css")}{_PICKER_CSS}</style>
</head>
<body>
<div class="sk-page">
<header class="sk-header"><div class="sk-eyebrow-title">Live sketch</div><h1 class="sk-title">{PICKER_TITLE}</h1></header>
<main class="sk-picker">{body}</main>
</div>
</body>
</html>
"""


def write_picker(folder: Path) -> Path:
    """Write the picker of `folder`'s diagrams, replacing the previous one, and return its path."""
    path = folder / PICKER_NAME
    atomic_write_text(path, picker_page(saved_diagrams(folder), folder))
    return path


def _entry(diagram: SavedDiagram, folder: Path) -> str:
    href = quote(diagram.path.relative_to(folder).as_posix())
    return f'<li><a href="{href}">{html.escape(diagram.title)}</a><time>{diagram.started:%Y-%m-%d %H:%M}</time></li>'


def _session_start(session: Path) -> datetime | None:
    """When the live sketch in `session` started, read from its folder name; None for any other folder or file."""
    if not session.is_dir():
        return None
    try:
        return datetime.strptime(session.name.rpartition("_")[0], _SESSION_STAMP_FORMAT)
    except ValueError:
        return None


def _page_title(page: Path) -> str:
    with page.open(encoding="utf-8") as file:
        head = file.read(_HEAD_CHARS)
    match = _TITLE.search(head)
    return html.unescape(match[1]) if match else page.stem
