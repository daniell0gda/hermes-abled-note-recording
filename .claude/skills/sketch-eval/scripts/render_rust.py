"""Draw a line-format file with the Rust renderer (elbow), and score it when a talk is given.

usage: python render_rust.py <file.txt> <kind> <tag> [talk] [--params <json>] [--styles <json>] [--theme dark]
Writes .gen/long_eval/<tag>.json (the view), .html and .png, and prints the renderer's rule report.
--styles is {"nodes": {id: {"shape": ...}}, "edges": {edge id: style}, "focus": [element id, ...]}, as Jev would
choose them.
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from run_talks import EDGE, OUT  # noqa: E402
from talks import TALKS, evaluate  # noqa: E402

from listening_app.sketch.diagrams import Diagram, DiagramKind, EdgeStyle, NodeStyle, Shape  # noqa: E402
from listening_app.sketch.elbow import program_path  # noqa: E402
from listening_app.sketch.structure import parse_structure  # noqa: E402

RENDERER = program_path()


def styled_view(source: Path, kind: DiagramKind, styles: dict) -> dict:
    diagram = Diagram(1, kind)
    diagram.update(parse_structure(source.read_text(encoding="utf-8")))
    nodes = {node_id: NodeStyle(Shape(style.get("shape", "service"))) for node_id, style in styles.get("nodes", {}).items()}
    edges = {edge_id: EdgeStyle(style) for edge_id, style in styles.get("edges", {}).items()}
    diagram.set_styles(nodes, edges, tuple(styles.get("focus", ())))
    return diagram.view()


def draw(view: dict, tag: str, params: str) -> tuple[Path, dict]:
    OUT.mkdir(parents=True, exist_ok=True)
    view_path = OUT / f"{tag}.json"
    view_path.write_text(json.dumps(view, ensure_ascii=False), encoding="utf-8")
    page = OUT / f"{tag}.html"
    result = subprocess.run([str(RENDERER), str(view_path), "--out", str(page), "--params", params, "--report"],
                            capture_output=True, text=True, check=True)
    png = OUT / f"{tag}.png"
    subprocess.run([EDGE, "--headless=new", "--disable-gpu", "--hide-scrollbars", f"--screenshot={png}",
                    "--window-size=1600,1100", "--virtual-time-budget=4000", page.as_uri()],
                   check=False, capture_output=True, timeout=60)
    return png, json.loads(result.stderr)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("kind", type=DiagramKind)
    parser.add_argument("tag")
    parser.add_argument("talk", nargs="?")
    parser.add_argument("--params", default="{}")
    parser.add_argument("--styles", default="{}")
    args = parser.parse_args()
    view = styled_view(args.source, args.kind, json.loads(args.styles))
    if args.talk:
        points, total, notes = evaluate(TALKS[args.talk], view)
        print(f"score {points}/{total} {notes}")
    png, report = draw(view, args.tag, args.params)
    print(f"{len(report['violations'])} violations, {report['measures']['crossings']} crossings, "
          f"{report['micros']} us")
    for violation in report["violations"]:
        print(f"  rule {violation['rule']}: {violation['detail']}")
    print("png:", png)


if __name__ == "__main__":
    main()
