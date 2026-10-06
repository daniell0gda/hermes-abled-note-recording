# Live sketch diagram kinds

Supported kinds (renderer + routing):

| Kind | Layout | When to use |
|------|--------|-------------|
| flow | LR (default) | Ordered steps / pipeline |
| sequence | lifelines | Messages over time |
| state | TB | Lifecycle statuses |
| tree | TB | Hierarchy |
| er | LR | Entities and relationships |
| swimlane | lanes | Role/team handoffs: each group is a horizontal lane, steps are ordered left to right by the flow |
| nested | TB | Scope/containment via groups — stretch |
| layers | lanes | Stacked concerns: each group is a band, same lane renderer as swimlane |
| dependency | LR | Unordered depends-on links |

Without any group, swimlane and layers fall back to the TB graph layout. Group lines with the same label are merged
into one lane by the parser.

Jev picks the kind (`KINDS` in `jev_client.py`). Flow and swimlane are told apart by whether it matters who does each
step; with the older wording Jev split about 50/50 between them on handoff talks.

Quality check: `.gen/sketch_long_eval.py --talk all` runs the six talks of `.gen/sketch_talks.py` (flow, sequence,
state, tree, architecture, swimlane) through the real pipeline and scores them; hand-drawn references are in
`.gen/long_eval/reference/`.

Retrospect may switch `kind` via a leading `kind <type>` line. Prefer TB kinds when the story goes from user down to outcomes.
