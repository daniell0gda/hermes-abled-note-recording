---
name: sketch-draw-model
description: Compares live sketch drawing models by drawing the same 20 spoken segments with each and reporting Jev and draw latency, parse errors and invented nodes. Use when choosing or changing sketch.draw_model, when a new xAI or other chat model appears, or when the user asks which draw model is fastest.
---

# Sketch draw model comparison

The script is `scripts/sketch_latency.py`. It runs the line-format drawing path with Grok, with Jev routing
each segment as the live app does. It writes output files to `.gen/` and prints a summary per model.

## Quick start

Run from the repo root with the project venv. Pass one model per run, or loop over several:

```powershell
$env:PYTHONPATH = (Get-Location).Path
foreach ($m in 'xai/grok-4-fast-non-reasoning', 'xai/grok-4-1-fast-non-reasoning') {
    .venv\Scripts\python.exe .claude\skills\sketch-draw-model\scripts\sketch_latency.py lines 1 --model $m
}
```

Each model takes about one minute. Run it in the background and wait for the notification.
The script needs the Grok sign-in (`grok_access_token`), the same one the app uses. It also makes live Grok and Jev calls.

## What to read

The summary lines per model:

- `draw median` and `draw max`: the drawing step. This is what decides the live speed.
- `total median`: Jev plus draw.
- `parse errors`: draws the line-format parser rejected, and draws that came back empty. Any non-zero count fails the model.
- `nodes=[]` in a run's summary: the draw parsed to no nodes. Treat that model as failed even if parse errors are 0.
- `invented`: nodes the speaker never said. Keep the list empty.

## Picking a model

1. Drop any model with parse errors above 0 of 16 draws.
2. Among the rest, take the lowest draw median. Use draw max as a tie-breaker.
3. A difference under about 0.1 s is noise with one run. Repeat the top two with `lines 3` before you decide.
4. Set `sketch.draw_model` in `config.yaml`, or change the default in `listening_app/config.py`.
5. Run `sketch-eval` before you keep the change. This script checks speed and format only, not diagram quality.

## Rules

- The output of each run stays in `.gen/`. Do not commit it.
- Do not change the segment list to suit one model. The same 20 segments must run for every model.
- Compare runs only when they used the same segments and the same run count.

## Files

- `scripts/sketch_latency.py` - the comparison script (copied from the Phase 0 prototype; `lines` format is the one in use).
