---
name: sketch-eval
description: Runs the live-sketch talk test set (six fluent spoken explanations, one per diagram kind) through the real pipeline with Grok and Jev, scores each diagram against its facts, compares runs and shows the renders next to hand-drawn show-me references. Use when changing live sketch prompts, the drawer, Jev questions, Bucket B, the parser or the renderer, when the user asks whether diagrams got better or worse, or wants a new test talk added.
---

# Sketch eval

Scripts are in `scripts/`; run them with the project venv from the repo root. Output goes to `.gen/long_eval/`.

## Quick start

```powershell
.venv\Scripts\python.exe .claude\skills\sketch-eval\scripts\run_talks.py --tag <name>
.venv\Scripts\python.exe .claude\skills\sketch-eval\scripts\compare.py <name> <older tag>
```

`run_talks.py` takes about 7 minutes for all six talks: run it in the background and wait for the notification.
`--talk <name>` runs one talk. Each run costs Grok and Jev calls.

## Workflow

1. Before a change, make sure a recent baseline tag exists in `.gen/long_eval/scores.tsv`; if not, run one.
2. Make the change, then run `pytest` (unit tests first, they are free).
3. Run the set with a new tag, then `compare.py <new> <baseline>`.
4. For each talk whose score dropped, read `.gen/long_eval/<tag>_<talk>_d1.png` and the reference
   `references/<talk>.png`, and read the diagram text in the run's log output.
5. Report the comparison table and what changed visually. One run varies by about one point per talk;
   repeat a talk before calling a one-point change real.

## Rules

- Fixes must stay generic. Never put words, names or steps from a test talk into prompts, hints or Jev criteria;
  the user's real talks can be about anything.
- Scores check meaning, not wording: when a sound diagram fails a check, widen the concept regex in `talks.py`
  rather than steering the model toward the test words.
- Renderer-only change: no need for a full run. Draw the references and a saved run with
  `scripts/render_structure.py <file.txt> <kind> <tag> [talk]` and look at the PNGs.

## Adding a talk

1. Write it the way a person explains to a colleague: fillers ("um", "you know"), a digression, one late
   correction ("actually, wait, ..."). Split it into segments the way STT cuts speech, one or two sentences each.
2. Add a `Talk` to `scripts/talks.py`: acceptable `kinds`, `concepts` (regex per thing that must appear),
   `edges` (direct), `paths` (reachable), `lanes` (swimlane only), `max_nodes`, `unique`.
3. Start one background agent to draw the show-me reference with the `diagram-design:diagram-design` skill
   (default style, no confirmation pause) into `references/<talk>.html` and `.png`, plus the same diagram in the
   app's line format as `references/<talk>.txt`.
4. Check the reference passes its own facts:
   `scripts/render_structure.py references/<talk>.txt <kind> ref_<talk> <talk>`. Fix the facts or the reference
   until it scores full marks, then run the new talk once.

## Files

- `scripts/talks.py` - talks, facts and `evaluate()`.
- `scripts/run_talks.py` - runs talks through `SketchPipeline`, saves JSON, PNG and a `scores.tsv` line.
- `scripts/compare.py` - table of two tags (default: the last two) with the failed checks.
- `scripts/render_structure.py` - draws a line-format file with the live renderer.
- `references/` - show-me reference per talk (`.html`, `.png`, line format `.txt`).
