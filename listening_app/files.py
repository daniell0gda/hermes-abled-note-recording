"""Small crash-safe file helpers."""

import os
import tempfile
from pathlib import Path


def atomic_write_text(path: Path, text: str) -> None:
    """Write to a temp file in the same folder, then rename over the target."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temp_name = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as temp_file:
            temp_file.write(text)
            temp_file.flush()
            os.fsync(temp_file.fileno())
        os.replace(temp_name, path)
    except BaseException:
        Path(temp_name).unlink(missing_ok=True)
        raise


def append_line(path: Path, line: str) -> None:
    """Append one line and flush it to disk before returning."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8", newline="\n") as file:
        file.write(line + "\n")
        file.flush()
        os.fsync(file.fileno())


def read_lines(path: Path) -> list[str]:
    """Non-empty lines of a text file; an empty list if the file does not exist."""
    if not path.exists():
        return []
    with path.open(encoding="utf-8") as file:
        return [line for line in file.read().splitlines() if line.strip()]
