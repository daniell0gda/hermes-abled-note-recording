# Live sketch — design

While you talk, what you describe is drawn as a diagram, so explaining something to someone is easier.

## Trigger

- Own global hotkey `ctrl+alt+d` (`sketch.hotkey`, `""` disables), independent of the recording hotkey.
- Works alone (sketch only: nothing sent to Hermes, no transcript file) or during a recording (same live transcript feeds both). Toggling sketch never affects the recording.

## Audio

- Shares the capture + STT pipeline with recording. Microphone only.
- Updates are driven by STT segments (end on 800 ms pause, max 30 s), not by Hermes `chunk_kb` batches.

## Two-model pipeline

**Jev** (TypeSafe, `jev-latest`) — fast typed judgments on every segment:

- Noul: does this segment add structure to a diagram?
- Choice: which diagram it belongs to — each existing diagram (number, title, node names) or "new". Handles "diagram 2", "back to the first one", "back to the login flow".
- Choice: diagram type (flow, sequence, tree, state, ER, …) for a new diagram.
- Questions/criteria in English; transcript passed as-is.
- Low confidence → the drawing model decides as part of its call.

- Separate Noul: does the speaker explicitly ask for a new diagram? (latency test missed "let's start a new diagram for …").

**Drawing model** (LiteLLM, default `xai/grok-4-fast-non-reasoning` via the app's Grok auth):

- Runs only when Jev reports structure; one request in flight, segments arriving meanwhile are batched into the next one.
- Returns the whole diagram as structure only, no styling, in our minimal line format (Mermaid as fallback if Phase 0 shows more errors):

  ```text
  auth "Auth service"
  redis "Redis"
  auth -> redis "caches tokens"
  group backend: auth, orders
  ```

  Prompt: draw only what was said. Edge order gives the message order for sequence diagrams.

**Jev styling** — after the drawing model, for new or changed elements only, all questions in one request:

| Element | Library choice |
|---|---|
| Node shape | service · database · queue/topic · cache · user/actor · external system · client/UI · state · decision · note |
| Edge style | sync call · async event · data read/write · transition · dependency |
| Emphasis | focal (Noul) |

State is the element plus the diagram's transcript. Choices are stored per element id, so styles never flicker.

## Per-segment steps

1. Jev routes (~0.3 s).
2. Drawing model writes the structure (~1.0 s).
3. Jev styles new/changed elements (~0.3 s).
4. App parses, lays out and renders with library SVG elements (<0.1 s).

## Rendering

- Own renderer: parses the structure format for the supported types (flow, sequence, state, tree, ER).
- Library of diagram-design SVG elements + style guide CSS ship with the app; layout by a JS layout engine (ELK or dagre) in the window.
- Element ids from the structure text (`auth`, `draft`) are the ids used for pointing and selection.
- Window opens instantly with the style loaded and an empty default diagram; first update only fills it.

## Context per call

- Each diagram keeps its own transcript (segments routed to it).
- Jev: new segment, previous 2–3 segments, diagram list (number, title, node names).
- Drawing model: current SVG, type reference, that diagram's transcript (recent lines + rolling summary), new lines, pointed element, selection.

## Window

- Built-in window (pywebview on Edge WebView2), one tab per diagram labelled number + title.
- Saying a diagram or clicking its tab switches the active diagram.

## Pointing and selection

- Pointing: keep the last `point_window_s` (2 s) of mouse positions; if all stay within `point_radius_px` (24 px) of their centroid, the element under the centroid (or the nearest within the radius) is pointed at. Lightly highlighted.
- Each segment uses the element pointed at while it was spoken (matched by timestamps) — resolves "this", "here", "that one".
- Click toggles an element in a multi-selection (solid outline); click on empty space clears. Selection survives updates for elements that still exist.

## Labels

- `label_language: auto | pl | en` (auto = spoken language). v1 targets English speech; other languages tested later.

## Saving

- One self-contained HTML file per diagram in `<output_dir>/diagrams/<session>/NN-<title>.html`, rewritten on every update.

## Failures

- Invalid SVG → keep last good diagram, show "update skipped", retry the skipped segments in the next call.
- Jev down or `TYPESAFE_API_KEY` missing → drawing model also routes; one tray notification.
- Sketch errors never stop or slow recording or Hermes upload.

## Config

```yaml
sketch:
  hotkey: "ctrl+alt+d"
  jev_model: jev-latest        # TYPESAFE_API_KEY overrides
  draw_model: xai/grok-4-fast-non-reasoning
  label_language: auto
  point_radius_px: 24
  point_window_s: 2
```

## Not in v1

- Full-quality diagram-design pass on stop.
- Sketch in headless MCP mode.
- Non-English speech.

## Latency measurements (2026-10-02, 20 English segments, `.gen/sketch_latency.py`)

| Drawing output | Draw median | Total median | Total max |
|---|---|---|---|
| Full SVG | 6.03 s | 6.31 s | 10.79 s |
| Mermaid | 0.99 s | 1.27 s | 1.87 s |

Jev: 0.26–0.28 s median, 0.40 s max for three questions.
