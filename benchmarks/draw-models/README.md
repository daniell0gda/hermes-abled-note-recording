# Live sketch draw models: latency and quality

Measured with `.claude/skills/sketch-draw-model/scripts/sketch_latency.py` (lines format, 20 spoken segments per run, Jev routing as in the app).
Raw outputs are in `raw/`. Quality checks come from `sketch-eval` (`raw/sketch-eval-gemini-vs-baseline.txt`).

## Latency

Draw = the drawing model's answer. Total = Jev plus draw. "Runs" is the number of 20-segment passes; each pass has 16 draws.

| Model | Route | Runs | Draw median | Draw max | Total median | Parse errors | Notes |
|---|---|---|---|---|---|---|---|
| `xai/grok-4-1-fast-non-reasoning` | xAI (Grok sign-in) | 1 | 0.79 s | 1.20 s | 1.08 s | 0/16 | **current default** |
| `xai/grok-4-fast-non-reasoning` | xAI (Grok sign-in) | 1 | 0.89 s | 2.54 s | 1.14 s | 0/16 | previous default |
| `xai/grok-4.20-non-reasoning` | xAI (Grok sign-in) | 1 | 0.93 s | 1.32 s | 1.21 s | 5/16 | line format broke |
| `google/gemini-2.5-flash-lite` | OpenRouter | 2 | 0.47 s | 9.11 s | 0.73 s | 0/32 | fastest; failed hiring eval |
| `anthropic/claude-haiku-5.5` | OpenRouter | 2 | 1.95 s | 11.76 s | 2.23 s | 0/32 | best node lists; too slow for ~1 s budget |
| `deepseek/deepseek-v4.1-flash` | OpenRouter | 2 | 7.06 s | 16.73 s | 7.34 s | empty in all 32 | reasoning uses the 1500-token limit; no content |
| `openai/gpt-4.1-nano` | OpenRouter | 2 | 1.01 s | 7.56 s | 1.26 s | 0/32 | 2 invented nodes in one run |
| `openai/gpt-4.1-mini` | OpenRouter | 2 | 1.04 s | 7.16 s | 1.31 s | 0/32 | node lists look complete |
| `mistralai/mistral-small-3.2-24b-instruct` | OpenRouter | 2 | 2.40 s | 13.34 s | 2.63 s | 12/32 | invalid lines in many draws |

Jev median was 0.24–0.28 s for every run.

## Quality (sketch-eval, six talks)

Baseline `base_6594_3` (earlier branch code, Grok default) against `or_gemini_lite` (Gemini 2.5 Flash Lite):

| Talk | Baseline | Gemini Flash Lite |
|---|---|---|
| issue | 18/23 | 23/23 |
| login | 13/13 | 13/13 |
| order | 17/17 | 12/17 |
| org | 23/24 | 24/24 |
| backend | 23/24 | 23/24 |
| hiring | 19/31 | 6/31 |
| **total** | **113/132** | **101/132** |

Gemini drew people as nodes and steps as edge labels in the hiring swimlane. That breaks the swimlane rule. In the order talk it dropped the `paid` node.

## Conclusions

- No tested model is faster than `grok-4-1-fast-non-reasoning` while also giving reliable diagrams.
- `gemini-2.5-flash-lite` is the fastest, but its diagrams are weaker on swimlanes.
- `deepseek-v4.1-flash` needs reasoning turned off before it can be used. That change is not tested.
- `mistral-small-3.2` fails on the line format.
- The OpenRouter and xAI runs took place on different days and through different routes. Treat the cross-route numbers as rough.

## Open items

- Intelligence was not measured directly. Node lists are a rough signal.
- No intelligence score was found for `grok-4-1-fast-non-reasoning`. Check the Artificial Analysis pages.
- A second hiring-style talk would test the swimlane rule on Grok and the candidates.
