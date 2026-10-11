"""Talk-set check: the real SketchPipeline (Jev + Grok, Bucket A and B) fed with the fluent test talks of talks.py.

usage: python run_talks.py --tag <name> [--talk issue|login|...|all] [--gap 4] [--no-b]
Prints each final diagram and its score, saves .gen/long_eval/<tag>_<talk>.json and _d<n>.png,
and appends one line per talk to .gen/long_eval/scores.tsv.
"""
import argparse
import json
import logging
import os
import subprocess
import sys
import threading
import time
import winreg
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
sys.path[:0] = [str(ROOT), str(Path(__file__).resolve().parent)]

from talks import TALKS, Talk, evaluate  # noqa: E402

from listening_app.config import load_config, resolve_config_path  # noqa: E402
from listening_app.sketch.drawer import Drawer, SpokenLine, credentials_provider  # noqa: E402
from listening_app.sketch.export import diagram_page  # noqa: E402
from listening_app.sketch.jev_client import JevClient  # noqa: E402
from listening_app.sketch.pipeline import SketchPipeline, SketchState  # noqa: E402

OUT = ROOT / ".gen" / "long_eval"
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
QUIET_S = 25.0


def typesafe_key() -> str:
    if os.environ.get("TYPESAFE_API_KEY"):
        return os.environ["TYPESAFE_API_KEY"]
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
        return winreg.QueryValueEx(key, "TYPESAFE_API_KEY")[0]


class Printer:
    def __init__(self) -> None:
        self.started = time.perf_counter()
        self.last_at = time.perf_counter()
        self.lock = threading.Lock()

    def stamp(self) -> str:
        return f"{time.perf_counter() - self.started:6.1f}s"

    def sketch_updated(self, state: SketchState) -> None:
        with self.lock:
            self.last_at = time.perf_counter()
        if state.updated is not None:
            view = next(v for v in state.views if v["number"] == state.updated)
            print(f"{self.stamp()} UPDATE d{state.updated} {view['kind']} {len(view['nodes'])}n/{len(view['edges'])}e")

    def update_skipped(self, number: int | None, reason: str) -> None:
        print(f"{self.stamp()} SKIPPED d{number}: {reason}")

    def jev_unavailable(self, reason: str) -> None:
        print(f"{self.stamp()} JEV DOWN: {reason}")


def structure_text(view: dict) -> str:
    lines = [f'title "{view["title"]}"  [{view["kind"]}]']
    lines += [f'  {n["id"]} "{n["label"]}" ({n["shape"]})' for n in view["nodes"]]
    lines += [f'  {e["source"]} -> {e["target"]} "{e["label"]}" ({e["style"]})' for e in view["edges"]]
    lines += [f'  group {g["id"]} "{g["label"]}": {", ".join(g["members"])}' for g in view["groups"]]
    return "\n".join(lines)


def render(view: dict, tag: str) -> Path:
    """The view drawn by the live renderer: .gen/long_eval/<tag>.html and .png (headless Edge)."""
    OUT.mkdir(parents=True, exist_ok=True)
    page = OUT / f"{tag}.html"
    page.write_text(diagram_page(view), encoding="utf-8")
    png = OUT / f"{tag}.png"
    subprocess.run([EDGE, "--headless=new", "--disable-gpu", "--hide-scrollbars", f"--screenshot={png}",
                    "--window-size=1600,1100", "--virtual-time-budget=4000", page.as_uri()],
                   check=False, capture_output=True, timeout=60)
    return png


def run_talk(talk: Talk, args: argparse.Namespace) -> None:
    config = load_config(resolve_config_path())
    if args.draw_model:
        config = config.model_copy(update={"sketch": config.sketch.model_copy(update={"draw_model": args.draw_model})})
    sketch = config.sketch
    printer = Printer()
    drawer = Drawer(sketch.draw_model, credentials_provider(config), sketch.label_language)
    judge = JevClient(typesafe_key(), sketch.jev_model)
    pipeline = SketchPipeline(drawer, judge, printer, retrospect_every_n=sketch.retrospect_every_n,
                              retrospect_max_nodes=sketch.retrospect_max_nodes,
                              bucket_b=sketch.bucket_b and not args.no_b,
                              bucket_b_min_new_lines=sketch.bucket_b_min_new_lines,
                              bucket_b_debounce_s=sketch.bucket_b_debounce_s,
                              bucket_b_max_wait_s=sketch.bucket_b_max_wait_s)
    pipeline.start()
    for text in talk.segments:
        print(f"{printer.stamp()} SAY {text}")
        pipeline.add(SpokenLine(text))
        time.sleep(args.gap)
    pipeline.wait_until_idle(60)
    while time.perf_counter() - printer.last_at < QUIET_S:
        time.sleep(1)
    pipeline.stop()
    judge.close()
    views = pipeline.snapshot().views
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"{args.tag}_{talk.name}.json").write_text(json.dumps(views, indent=1), encoding="utf-8")
    for view in views:
        points, total, notes = evaluate(talk, view)
        print(f"\n=== {talk.name} d{view['number']} score {points}/{total}")
        print(structure_text(view))
        for note in notes:
            print("  -", note)
        print("png:", render(view, f"{args.tag}_{talk.name}_d{view['number']}"))
    best = max(views, key=lambda view: evaluate(talk, view)[0], default=None)
    points, total, notes = evaluate(talk, best) if best is not None else (0, 1, ["no diagram"])
    with (OUT / "scores.tsv").open("a", encoding="utf-8") as scores:
        scores.write(f"{datetime.now():%Y-%m-%d %H:%M}\t{args.tag}\t{talk.name}\t{len(views)} diagrams\t"
                     f"{points}/{total}\t{'; '.join(notes)}\n")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag", required=True)
    parser.add_argument("--talk", default="all", choices=["all", *TALKS])
    parser.add_argument("--gap", type=float, default=4.0)
    parser.add_argument("--no-b", action="store_true")
    parser.add_argument("--draw-model", help="litellm draw model for this run, e.g. openrouter/google/gemini-2.5-flash-lite")
    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(relativeCreated)7.0f %(threadName)s %(message)s",
                        stream=sys.stdout)
    for noisy in ("httpx", "httpcore", "LiteLLM", "litellm"):
        logging.getLogger(noisy).setLevel(logging.WARNING)
    logging.getLogger("listening_app.sketch.bucket_b").setLevel(logging.DEBUG)
    for name in (TALKS if args.talk == "all" else [args.talk]):
        run_talk(TALKS[name], args)


if __name__ == "__main__":
    main()
