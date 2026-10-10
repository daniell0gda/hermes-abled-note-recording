"""The saved diagrams of every live sketch, listed in the live sketch window to pick one to continue."""

import html
import re
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

_SESSION_STAMP_FORMAT = "%Y-%m-%dT%H-%M-%S"
_HEAD_CHARS = 4096
_TITLE = re.compile(r"<title>(.*?)</title>", re.DOTALL)


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


def saved_diagram_path(folder: Path, raw: str) -> Path | None:
    """The diagram page at `raw` when it is one of `folder`'s saved diagrams, else None."""
    path = Path(raw).resolve()
    if path.suffix != ".html" or path.parent.parent != folder.resolve() or not path.is_file():
        return None
    return path


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
