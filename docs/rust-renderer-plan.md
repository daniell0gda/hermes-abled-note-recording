# elbow, the Rust diagram renderer — plan and status

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

- `elbow/` — Rust crate (lib + `elbow` binary), only `serde`/`serde_json`.
  - `spec` — input: the view JSON (`kind`, `nodes` with `shape`/`focal`, `edges` with `style`/`focal`, `groups`).
  - `params` — the typed knobs a model may choose: direction, node gap, channel width, uniform rank size, theme.
  - `layout` — layered layout: cycle break, longest-path ranks, dummy nodes, barycenter ordering, port-aware
    placement (isotonic regression), straight long edges, optional lanes.
  - `route` — orthogonal edges with r=8 elbows, one channel track per bending hop, fanned attach points.
  - `sequence` — participants, lifelines, one message per row.
  - `tiers` — layers: every group is a full-width band of whole ranks, top to bottom, numbered L1 from the bottom.
  - `rings` — nested: groups whose member sets hold each other become rings in rings; each group's nodes keep
    adjacent lanes with room for the ring borders.
  - `cycle` — dependency: back edges leave the ranked stack on its outer side (both nodes pinned to that end of
    their rank) and come back as the accent dashed `CYCLE` line.
  - `cardinality` — er: `1:N`, `0..1:1..*` or `one-to-many` in an edge label becomes a mark at each line end.
  - `label` — label spots along each line; the first spot clear of nodes, labels and other lines wins.
  - `svg` — the `diagram-design` look: paper, type tags, sublabel from `(…)`, focal accent on nodes and edges,
    bridges, legend, dark theme. Every node and edge is a `[data-id]` group (`sk-node` with halo, `sk-shape` and
    outline; `sk-edge` with a 12px `sk-hit` path, `sk-line`, label and marks), so the window's click handler and
    `SketchRenderer.mark()` / `boxes()` work on it unchanged. `is-selected` / `is-pointed` come from the view's
    `selected` / `pointed` and switch the looks through a `<style>` inside the SVG; shape and line colours are
    inline styles, so `sketch.css` cannot change them. Sections are selectable too: every zone, nested ring,
    swimlane and layer band is an `sk-group` with the group's id, drawn before edges and nodes (inner rings over
    outer ones). `boxes()` marks them `section`, and the pointer only picks a section when no node or edge is
    within its radius.
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
| 4b. The other kinds: er, nested, layers, dependency | Done. One fixture each in `elbow/tests/`: 0 violations |
| 5. PyO3 binding, use in the app, Jev choosing `params` | Not started |

What the new kinds draw (after the diagram-design type references):

- **er** — entity boxes: `ENTITY` tag and name in a tinted header, the label's bracket as a field list
  (`Order (id, customer_id, total)`); relationship lines without arrowheads; cardinality marks on each line end.
- **nested** — rings with depth-graded strokes and fills, labels on a paper mask over the top border, moved along
  the border off any connector; nodes outside every group sit before the rings.
- **layers** — a framed stack of bands, layer index and name in a left column, the member nodes inside each band.
- **dependency** — ranked top to bottom, an `N IN` fan-in badge on every node, leaves in the store look, the cycle
  routed outside the stack; it is the only accent (focal flags on other edges are ignored).

Measured on this machine:

- One render: 0.05–2.8 ms. CLI with process start and `--best` (24 variants): 15–55 ms per diagram.
- Random diagrams within the diagram-design budget (`cargo run --release --example stress 900 --best`):
  34 of 900 still break a rule (flow 6/102, state 6/103, tree 1/87, swimlane 2/102, sequence 0/108, er 2/106,
  nested 4/97, layers 13/92, dependency 0/103). Most of these are a label that has no free spot in a crowded
  channel. Random layers are hardest: the generator spreads nodes over layers at random, which gives many long
  and upward edges.

## Usage

```powershell
cd elbow
cargo test --release
cargo build --release
target\release\elbow.exe view.json --best --out diagram.html --report
target\release\elbow.exe view.json --variants            # every candidate with its score, for a model to choose
target\release\elbow.exe view.json --params '{"direction": "down", "theme": "dark"}' --out diagram.svg
```

On this machine `cargo` needs the VS 2022 Build Tools environment (`vcvars64.bat`) for the MSVC linker.

## Next

- Use the renderer in the app: a PyO3 module or the CLI from `export.py`, replacing `sketch.js` + dagre.
- Let Jev choose among `--variants` (or set `params` directly) instead of the fixed score.
- Let `show-me` write a view JSON and call the CLI instead of hand-writing SVG.
- Remaining layout gaps: crowded label channels in swimlanes and layers, group zones that overlap other nodes are
  skipped, sequence fragments (`alt`, `loop`) and activation bars are not drawn, groups in a nested view that
  overlap without one holding the other get only the nodes they hold alone.
- The drawer's ER hint does not ask for field lists in brackets yet, so live ER entities show only their names.
