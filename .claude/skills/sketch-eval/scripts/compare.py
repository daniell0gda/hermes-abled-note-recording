"""Compare two runs in .gen/long_eval/scores.tsv, talk by talk.

usage: python compare.py [new_tag] [old_tag]   (default: the last two tags in the file)
"""
import sys
from pathlib import Path

SCORES = Path(__file__).resolve().parents[4] / ".gen" / "long_eval" / "scores.tsv"


def load() -> tuple[list[str], dict[tuple[str, str], tuple[int, int, str]]]:
    tags: list[str] = []
    results: dict[tuple[str, str], tuple[int, int, str]] = {}
    for line in SCORES.read_text(encoding="utf-8").splitlines():
        parts = line.split("\t")
        if len(parts) < 5:
            continue
        tag, talk, score = parts[1], parts[2], parts[4]
        points, total = (int(value) for value in score.split("/"))
        results[(tag, talk)] = (points, total, parts[5] if len(parts) > 5 else "")
        if tag in tags:
            tags.remove(tag)
        tags.append(tag)
    return tags, results


def main() -> None:
    tags, results = load()
    new = sys.argv[1] if len(sys.argv) > 1 else tags[-1]
    old = sys.argv[2] if len(sys.argv) > 2 else next((tag for tag in reversed(tags) if tag != new), new)
    talks = [talk for tag, talk in results if tag == new]
    print(f"{'talk':10}{old:>12}{new:>12}{'change':>8}   failed checks in {new}")
    sums = [0, 0, 0]
    for talk in talks:
        before = results.get((old, talk))
        points, total, notes = results[(new, talk)]
        change = f"{points - before[0]:+d}" if before else "new"
        print(f"{talk:10}{(f'{before[0]}/{before[1]}' if before else '-'):>12}{f'{points}/{total}':>12}{change:>8}   {notes}")
        sums[0] += before[0] if before else 0
        sums[1] += points
        sums[2] += total
    print(f"{'total':10}{sums[0]:>12}{f'{sums[1]}/{sums[2]}':>12}{sums[1] - sums[0]:>+8d}")


if __name__ == "__main__":
    main()
