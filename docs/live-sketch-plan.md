# Live sketch — implementation plan

Design: [live-sketch-design.md](live-sketch-design.md). Each phase ends with a check; the next phase starts only when it passes.

## Phase 0 — Prototype the risky parts (in `.gen/`, throw-away)

0. **Structure format** — add our minimal line format (`id "label"`, `a -> b "label"`, `group name: a, b`) to `.gen/sketch_latency.py` and run it against Mermaid on the same 20 segments.
   → verify: draw latency and parse-error rate within ~10 % of Mermaid → keep the own format, else fall back to Mermaid.
1. **Jev styling** — extend `.gen/sketch_latency.py` with step 3 (shape / edge style / focal for new elements).
   → verify: diagram 1 + 2 from the test get sensible shapes (Postgres = database, Kafka = queue, Redis = cache, states = state); step 3 ≤ 0.4 s median.
2. **"New diagram" Noul + tighter drawing prompt** ("only what was said").
   → verify: segment 11 opens diagram 2; no invented nodes (`Request`, `Entry Point`, `Submitted`) in a rerun.
3. **Renderer spike** — static HTML: structure parser + ELK/dagre layout + library SVG elements + diagram-design CSS, fed with the saved structure + styles.
   → verify: flow and state diagrams render in diagram-design look; parse + layout < 100 ms.

**Gate:** total per-segment latency ≤ 2 s median, ≤ 3 s max. If not, revisit before Phase 1.

## Phase 1 — Core pipeline (no UI), TDD

New package `listening_app/sketch/`:

| Module | Responsibility |
|---|---|
| `jev_client.py` | TypeSafe HTTP calls; routing and styling questions; typed results |
| `drawer.py` | Drawing-model call via LiteLLM (Grok auth via `grok_access_token`), structure-format output |
| `structure.py` | Parse the structure format → nodes, edges, groups; reject invalid text |
| `diagrams.py` | Diagram set: numbers, titles, per-diagram transcript + summary, element styles by id |
| `pipeline.py` | Per-segment steps 1–4, single in-flight draw, batching, fallbacks |

1. Config: `sketch:` section in `config.py` + `config.example.yaml`; `TYPESAFE_API_KEY` override.
   → verify: `test_config.py` cases for defaults, env override, `""` hotkey.
2. `structure.py` → verify: unit tests for nodes, edges, groups, edge order (sequence); invalid lines rejected.
3. `diagrams.py` → verify: routing to existing / new diagram, styles kept per id, selection pruning of removed ids.
4. `jev_client.py`, `drawer.py` with fakes → verify: request payloads match the design; low confidence triggers fallback.
5. `pipeline.py` → verify: batching while a draw is in flight; invalid structure keeps last good diagram and retries segments; Jev failure → drawer routes.

## Phase 2 — App integration

1. Second `GlobalHotkey` for `sketch.hotkey` in `app.py`; `toggle_sketch`.
   → verify: `test_hotkey.py` / app tests — sketch toggles independently of recording.
2. Feed transcriber output (mic segments only) to the pipeline; start capture + STT when sketch runs without recording; nothing to Hermes in sketch-only mode.
   → verify: tests for the four on/off combinations; recording output unchanged.
3. Tray item "Live sketch" with state; notification on Jev fallback.
4. Save each diagram as self-contained HTML in `<output_dir>/diagrams/<session>/NN-<title>.html`.
   → verify: file written on every update; opens standalone.

## Phase 3 — Window

1. pywebview window (WebView2): tabs per diagram, renderer from Phase 0 productionised, instant empty template.
2. Python ↔ JS bridge: push diagram updates; receive pointing and selection events with timestamps.
3. Pointing: 2 s window, 24 px radius, nearest element; segment ↔ pointed element by timestamps.
4. Click multi-select, clear on empty space.
   → verify: pointing / selection logic unit-tested in Python (timestamp matching) and manual run with the test transcript.
5. PyInstaller: add pywebview + JS assets to `build.ps1`.
   → verify: built `ListeningApp.exe` opens the window and draws from live speech.

## Phase 4 — End-to-end check

- Live 5-minute English explanation with two diagrams, switching by voice, pointing and selecting.
  → verify: median latency ≤ 2 s, no broken diagram, recording + Hermes unaffected.

## Later

- Polish / auto language test for Jev.
- Full-quality diagram-design pass on stop.
- Sketch in MCP mode.
