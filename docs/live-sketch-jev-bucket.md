# Live sketch - Bucket A / Bucket B (Jev clean rebuild)

Branch `feature/sketch-jev-bucket`, forked from `feature/sketch-retrospect`.

## Why

Bucket A (live draw + post-draw Grok retrospect) only ever patches the graph it already has. With messy STT
input the retrospect keeps the size and the junk (separate `workflow` / `team_box` nodes, stray
`implementer utilities`, wrong checker loop, no `done`). Bucket B rebuilds from a clean source instead.

## Two buckets

| | Bucket A (live) | Bucket B (clean rebuild) |
|---|---|---|
| Input | each STT segment as it arrives | the diagram's whole transcript (+ rolling summary) |
| Steps | Jev routes -> Grok draws the whole diagram -> Jev styles -> (retrospect) | Grok writes clean chronological prose -> split into chunks -> Jev judges each chunk one by one -> Grok draws all structural chunks **at once** on an **empty** diagram -> Jev styles |
| Thread | `sketch-route`, `sketch-draw` | `sketch-bucket-b` |
| Visible | immediately; the speaker keeps talking | soft-swapped in when a rebuild finishes |

Code: `listening_app/sketch/bucket_b.py` (`chunk_clean_text`, `rebuild_from_clean`), the prompts
`Drawer.clean_transcript` and `Drawer.rebuild` in `drawer.py`, scheduling and swap in `pipeline.py` (`run_bucket_b`,
`_swap_bucket_b`, `_bucket_b_loop`). Same draw model (`sketch.draw_model`) and Jev client as Bucket A.

### Jev per chunk

1. Kind: one Jev call on the whole clean text picks the diagram type (swimlane / nested / flow ...).
2. Each chunk: Jev `structural` Noul (context: previous 3 chunks + summary of the live diagram).
   Chunks without structure are skipped.
3. One Grok call (`Drawer.rebuild`) draws all structural chunks together, then one Jev call styles every element.

Drawing chunk by chunk (the first version) lost edges and relabelled nodes on longer talks: every call rewrote
the whole diagram from one sentence. On the issue-workflow talk (`.gen/sketch_long_eval.py`) the fact score went
from 9/21 to 20-21/21 with the single draw, the modelling rules and the worked example in `drawer.py`.

### Modelling rules

`MODEL_RULES` (all drawing prompts) and `MODEL_EXAMPLE` (rebuild only): labels are names, not actions; order words
are edges across sentences; loops and optional steps carry their condition; listed team members get a group; files
someone leaves are nodes; the stated ending is a node. The node limit is 16 (`MAX_NODES`); the retrospect no longer
drops nodes to fit 9.

## Trigger

- After every successful live draw, a diagram is armed for B when at least `sketch.bucket_b_min_new_lines`
  (default 2) transcript lines arrived since the last B run.
- B starts after a quiet spell of `bucket_b_debounce_s` (2 s) without new live draws, or at the latest
  `bucket_b_max_wait_s` (10 s) after it was armed, so continuous talk still gets rebuilds.
- One B run at a time; requests arriving meanwhile coalesce into the next run.

## Swap (no button)

- When a rebuild finishes and its generation is still current, the live diagram's structure, kind and styles
  are replaced (`BucketB swap` in the log). Transcript, summary, tab and selection (of surviving ids) stay.
- A rebuild that misses transcript lines the live view already has is not swapped in (`BucketB drop ... newer
  lines`): it would show less than the live view. B is requested again on the whole transcript, so while the
  speaker talks continuously, B swaps at the next pause.
- Lines Jev judges without structure are not drawn, but they join the current diagram's transcript. A fragment
  such as "and the issue is done." then still reaches the live draw and the clean rebuild.
- Stale results are dropped: `cancel_bucket_b()` and `stop()` bump the generation; an in-flight rebuild stops
  at the next chunk and never swaps.
- A live draw computed on the pre-swap diagram is not committed over the swap: its batch is re-queued and
  redrawn on top of the clean diagram. A retrospect that overlapped a swap is dropped.
- No apply button for now (Daniel, 2026-10-05).

## Failure

- No `TYPESAFE_API_KEY`: B is disabled (`BucketB disabled` warning), A works as before.
- Jev or Grok error during a run: the run ends (`BucketB ... fail`), the live view stays; new lines re-arm B.

## Config (`sketch:` section)

```yaml
sketch:
  bucket_b: true
  bucket_b_min_new_lines: 2
  bucket_b_debounce_s: 2.0
  bucket_b_max_wait_s: 10.0
  # retrospect_every_n: 0   # optional: turn off Bucket A's retrospect and rely on B
```

## Log lines

`BucketB start` -> `BucketB clean diagram N: C chars -> K chunks` -> `BucketB kind` ->
`BucketB Jev chunk i/K ok` (or `fail`) -> `BucketB rebuild diagram N: n nodes, e edges` -> `BucketB swap`
(or `BucketB drop ... outdated`, `BucketB drop ... newer lines`, `BucketB cancel`). With `log_level: DEBUG`,
`BucketB clean text` shows the cleaned prose.
