"""Draw a line-format file with the live renderer, and score it when a talk is given.

usage: python render_structure.py <file.txt> <kind> <tag> [talk]
Writes .gen/long_eval/<tag>.html and .png. Use it to see a renderer change on a fixed structure,
e.g. a reference: python render_structure.py ../references/hiring.txt swimlane hiring_ref hiring
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from run_talks import render  # noqa: E402
from talks import TALKS, evaluate  # noqa: E402

from listening_app.sketch.diagrams import Diagram, DiagramKind  # noqa: E402
from listening_app.sketch.structure import parse_structure  # noqa: E402


def main() -> None:
    source, kind, tag = Path(sys.argv[1]), DiagramKind(sys.argv[2]), sys.argv[3]
    diagram = Diagram(1, kind)
    diagram.update(parse_structure(source.read_text(encoding="utf-8")))
    view = diagram.view()
    if len(sys.argv) > 4:
        points, total, notes = evaluate(TALKS[sys.argv[4]], view)
        print(f"score {points}/{total} {notes}")
    print("png:", render(view, tag))


if __name__ == "__main__":
    main()
