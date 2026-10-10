# Rust diagram renderer — plan and status

## Goal

A Rust library and CLI that draws a live-sketch diagram (the `Diagram.view()` JSON) as an SVG/HTML page in the
`diagram-design` editorial style, with the six connector rules enforced in code. The AI (Grok, Jev, or Claude in
`show-me`) only chooses content and a few typed parameters; the library does all geometry.

## Why

- The current renderer (`assets/sketch/sketch.js` + dagre) breaks the connector rules: labels sit on their lines and
  fan-out stems share one path.
- Claude writing `diagram-design` SVG by hand is slow (minutes per diagram).
- Geometry checks in code make layouts testable and let a model pick among valid variants.

## Shape

- `sketch_render/` — Rust crate (lib + `sketch-render` binary), only `serde`/`serde_json`.
  - `spec` — input: the view JSON (`kind`, `nodes` with `shape`/`focal`, `edges` with `style`, `groups`).
  - `params` — the typed knobs a model may choose: direction, node gap, channel width, uniform rank size, theme.
  - `layout` — layered layout: cycle break, longest-path ranks, dummy nodes, barycenter ordering, port-aware
    placement (isotonic regression), straight long edges, optional lanes.
  - `route` — orthogonal edges with r=8 elbows, one channel track per bending hop, fanned attach points.
  - `sequence` — participants, lifelines, one message per row.
  - `label` — label spots along each line; the first spot clear of nodes, labels and other lines wins.
  - `svg` — the `diagram-design` look: paper, type tags, sublabel from `(…)`, focal accent, bridges, legend, dark theme.
  - `check` — the six connector rules as violations, plus crossings, bends, area and ambiguous labels.
  - `variants` — renders a parameter grid, scores each result and picks the best.
- `.claude/skills/sketch-eval/scripts/render_rust.py` draws a line-format file with the CLI (optional Jev-like
  `--styles`) and screenshots it like `render_structure.py`.

## Status (2026-10-11)

| Step | Result |
|---|---|
| 1. Spike: flow layout, router, labels, SVG, checker, CLI | Done. `backend` matches the hand-drawn reference, 0 violations |
| 2. Fine-tune flow: labels, groups as zones, edge styles, legend | Done |
| 3. More kinds: tree, state, sequence, swimlane | Done. All six reference talks: 0 violations |
| 4. Variant search | Done. `--best` / `--variants` |
| 5. PyO3 binding, use in the app, Jev choosing `params` | Not started |

Measured on this machine:

- One render: 0.05–1.5 ms. CLI with process start and `--best` (24 variants): 15–55 ms per diagram.
- Random diagrams within the diagram-design budget (`cargo run --release --example stress 500 --best`):
  12 of 500 still break a rule (flow 1/109, state 1/89, tree 4/106, swimlane 6/96, sequence 0/100).
  Most of these are a label that has no free spot in a crowded channel.

## Usage

```powershell
cd sketch_render
cargo test --release
cargo build --release
target\release\sketch-render.exe view.json --best --out diagram.html --report
target\release\sketch-render.exe view.json --variants            # every candidate with its score, for a model to choose
target\release\sketch-render.exe view.json --params '{"direction": "down", "theme": "dark"}' --out diagram.svg
```

On this machine `cargo` needs the VS 2022 Build Tools environment (`vcvars64.bat`) for the MSVC linker.

## Next

- Use the renderer in the app: a PyO3 module or the CLI from `export.py`, replacing `sketch.js` + dagre.
- Let Jev choose among `--variants` (or set `params` directly) instead of the fixed score.
- Let `show-me` write a view JSON and call the CLI instead of hand-writing SVG.
- Remaining layout gaps: crowded label channels in swimlanes, group zones that overlap other nodes are skipped,
  sequence fragments (`alt`, `loop`) and activation bars are not drawn.
