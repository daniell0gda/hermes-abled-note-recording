# Rust diagram renderer — plan

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

- `sketch_render/` — Rust crate (lib + `sketch-render` binary), no runtime besides `serde`/`serde_json`.
  - `spec` — input: the view JSON (`kind`, `nodes` with `shape`/`focal`, `edges` with `style`, `groups`).
  - `params` — the typed knobs a model may choose: direction, rank gap, node gap, label side.
  - `layout` — layered layout (cycle break, longest-path ranks, dummy nodes, barycenter ordering, 4px grid).
  - `route` — orthogonal edges with r=8 elbows, one channel track per edge, fanned attach points.
  - `label` — label candidates per edge; the first one that passes the checks wins.
  - `svg` — the `diagram-design` look: paper, type tags, sublabel from `(…)`, focal accent, legend strip.
  - `check` — the six connector rules plus label/label overlap, as a list of violations.
- Python side: `.claude/skills/sketch-eval/scripts/render_rust.py` draws a line-format file with the CLI and
  screenshots it like `render_structure.py`, so references, live renderer and Rust can be compared.

## Steps

1. Spike: crate, spec, layered layout, router, labels, SVG, checker, CLI → verify: `cargo test` passes,
   the `backend` reference renders with zero violations, and the PNG is close to `references/backend.png`.
2. Fine-tune flow: label placement, groups, edge styles, legend → verify: all six references render as `flow`
   with zero violations.
3. More kinds: tree (`org`), state (`order`), sequence (`login`) → verify: zero violations and side-by-side PNGs.
4. Variant search: render a few parameter sets, keep valid ones, pick the most compact → verify: report per talk.
5. Later (not in this branch): PyO3 binding, use in the app instead of `sketch.js`, Jev choosing `params`.
