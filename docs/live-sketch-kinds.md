# Live sketch diagram kinds

Supported kinds (renderer + routing):

| Kind | Layout | When to use |
|------|--------|-------------|
| flow | LR (default) | Ordered steps / pipeline |
| sequence | lifelines | Messages over time |
| state | TB | Lifecycle statuses |
| tree | TB | Hierarchy |
| er | LR | Entities and relationships |
| swimlane | TB | Role/team handoffs (groups as lanes) — stretch: same graph renderer as flow |
| nested | TB | Scope/containment via groups — stretch |
| layers | TB | Stacked concerns — stretch |
| dependency | LR | Unordered depends-on links |

Retrospect may switch `kind` via a leading `kind <type>` line. Prefer TB kinds when the story goes from user down to outcomes.
