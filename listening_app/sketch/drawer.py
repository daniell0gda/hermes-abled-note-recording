"""The drawing model (litellm): writes the whole diagram in the line format from what the speaker said."""

import re
from collections.abc import Callable
from dataclasses import dataclass, field

from listening_app.config import AppConfig, KeySource, Language
from listening_app.sketch.diagrams import Destination, Diagram, DiagramKind, DiagramSet
from listening_app.sketch.structure import Structure, StructureError
from listening_app.transcriber import grok_access_token, grok_api_base, import_litellm

DRAW_TIMEOUT_S = 15.0
MAX_TOKENS = 1500
MAX_NODES = 12
RETROSPECT_MAX_NODES = 9

KIND_HINTS = {
    DiagramKind.FLOW: "Nodes are components or steps; edges are calls or data flow. Prefer left-to-right unless the story is clearly top-down.",
    DiagramKind.SEQUENCE: "Nodes are participants; every edge is one message, listed in the order the messages happen.",
    DiagramKind.STATE: "Nodes are states; edges are transitions labelled with their trigger. Prefer top-to-bottom.",
    DiagramKind.TREE: "Edges go from parent to child. Prefer top-to-bottom.",
    DiagramKind.ER: "Nodes are entities; edges are relationships labelled with their meaning and cardinality.",
    DiagramKind.SWIMLANE: "Roles or teams are groups (lanes); steps move between them. Prefer top-to-bottom. Use group lines for each lane.",
    DiagramKind.NESTED: "Show scope and containment: outer groups hold inner members. Prefer top-to-bottom. No separate node with the same id as a group.",
    DiagramKind.LAYERS: "Stacked concerns (user → process → team → QA → done). Prefer top-to-bottom; groups mark layers.",
    DiagramKind.DEPENDENCY: "Unordered dependencies or ownership between components; edges mean depends-on, not sequence. Prefer left-to-right.",
}
LANGUAGE_RULES = {
    Language.AUTO: "Write labels in the language the speaker uses.",
    Language.POLISH: "Write all labels in Polish.",
    Language.ENGLISH: "Write all labels in English.",
}
FORMAT_RULES = (
    "Draw only what the speaker actually said: no nodes, steps, states or labels they did not mention, "
    "no invented start, entry or request nodes. Remove an element only when the speaker removes or corrects it.\n"
    "Format, one item per line:\n"
    'title "<short title naming the whole diagram>"\n'
    '<id> "<Label>"          declare every node like this\n'
    '<id> -> <id> "<label>"  an edge; the label is optional\n'
    'group <id> "<Label>": <id>, <id>\n'
    "Ids: short descriptive names in lowercase letters, digits and _, e.g. auth or order_db. "
    f"Keep the ids of existing elements. At most {MAX_NODES} nodes.\n"
    '"This", "here" or "that one" means the element the speaker pointed at while saying it.'
)
ANSWER_RULE = "Return ONLY the complete updated diagram, no fences, no prose."


CLEAN_RULES = (
    "Rewrite the speaker's transcript as clean chronological prose that a teammate can follow.\n"
    "- Keep the speaker's intent and every real component, role, team, step, skill, loop and decision they named.\n"
    "- Strip speech-to-text garbage, filler, repeated false starts and half-heard words.\n"
    "- Put events in the order they happen in the described workflow; later corrections replace earlier mistakes.\n"
    "- Merge aliases of the same thing into one name and use that name every time.\n"
    "- Say explicitly which steps happen inside a team or box, which step loops back to which when not satisfied, "
    "which steps are optional, and how the workflow ends - but only when the speaker said so.\n"
    "- One short sentence per step or fact.\n"
    "- Do not invent steps, roles or systems they never mentioned. No diagram, no headings, prose only."
)

RETROSPECT_RULES = (
    "Revise the whole diagram against everything the speaker said. Later statements win over earlier ones: "
    "when the speaker corrects themselves, drop the contradicted nodes, edges and labels.\n"
    "Editorial cleanup (do all of these):\n"
    "- Merge aliases and synonyms into one node (e.g. manual tester = manual tracker = QA).\n"
    "- Drop STT garbage: nodes whose labels were never clearly named as a real component, role or step.\n"
    "- One role or component = one node; do not split a single name across several nodes.\n"
    "- Never give a group the same id as a node — rename the group (e.g. team_group) or drop the duplicate node.\n"
    "- Prefer clear layers when the story has stages: entry → process → roles/team → QA → done.\n"
    "- Delete orphans with no edges unless the speaker named them; fix dangling edges.\n"
    "- Keep existing ids whenever the same element remains.\n"
    f"At most {RETROSPECT_MAX_NODES} nodes — drop the least important ones if needed.\n"
    "Do not invent elements the speaker never mentioned."
)

KIND_CHOICE_RULES = (
    "You may change the diagram type when a different type fits the whole explanation better. "
    "Start your answer with exactly one line `kind <type>` (then the diagram), choosing the best type:\n"
    "- flow: ordered steps / pipeline / happy path\n"
    "- sequence: messages between participants over time\n"
    "- state: lifecycle statuses and transitions\n"
    "- tree: hierarchy parent→child\n"
    "- er: entities and relationships\n"
    "- swimlane: handoffs between roles or teams (lanes as groups), prefer top-to-bottom\n"
    "- nested: scope / containment (groups hold members), prefer top-to-bottom\n"
    "- layers: stacked concerns top-to-bottom\n"
    "- dependency: unordered depends-on links\n"
    "If the current type is already right, omit the kind line and keep it."
)

_EXISTING_HEADER = re.compile(r"^diagram\s+(\d+)$")
_NEW_HEADER = re.compile(rf"^diagram\s+new\s+({'|'.join(kind.value for kind in DiagramKind)})$")
_KIND_HEADER = re.compile(rf"^kind\s+({'|'.join(kind.value for kind in DiagramKind)})$")
_NO_STRUCTURE = "none"
_FENCE = "```"


@dataclass(frozen=True)
class Credentials:
    api_key: str = field(repr=False)
    api_base: str | None = None


Completion = Callable[[str, list[dict[str, str]], Credentials], str]


@dataclass(frozen=True)
class SpokenLine:
    """A transcribed segment and the element id the speaker pointed at while saying it."""

    text: str
    pointed: str | None = None


@dataclass(frozen=True)
class RoutedDrawing:
    """The drawing model's choice of diagram (None: the lines add no structure) and that diagram's new text."""

    destination: Destination | None
    text: str


@dataclass(frozen=True)
class Revision:
    """A cleaned-up diagram text; `kind` is set when the model chose a better diagram type."""

    text: str
    kind: DiagramKind | None = None


def litellm_completion(model: str, messages: list[dict[str, str]], credentials: Credentials) -> str:
    litellm = import_litellm()
    options: dict[str, object] = {"timeout": DRAW_TIMEOUT_S, "max_retries": 0, "temperature": 0,
                                  "max_tokens": MAX_TOKENS}
    if credentials.api_key:
        options["api_key"] = credentials.api_key
    if credentials.api_base:
        options["api_base"] = credentials.api_base
    response = litellm.completion(model=model, messages=messages, **options)
    return str(response.choices[0].message.content or "")


def credentials_provider(config: AppConfig) -> Callable[[], Credentials]:
    """Credentials are fetched per call, so an expired Grok token is refreshed in between."""
    key = config.draw_key()
    if key.source is KeySource.GROK_AUTH:
        return lambda: Credentials(grok_access_token(), grok_api_base())
    return lambda: Credentials(key.value)


class Drawer:
    def __init__(self, model: str, credentials: Callable[[], Credentials], label_language: Language,
                 completion: Completion = litellm_completion) -> None:
        self._model = model
        self._credentials = credentials
        self._language_rule = LANGUAGE_RULES[label_language]
        self._completion = completion

    def draw(self, diagram: Diagram, lines: list[SpokenLine]) -> str:
        """The whole updated diagram in the line format, unparsed."""
        prompt = "\n".join([
            f"You keep a live {diagram.kind.value} diagram in sync with what a speaker explains out loud.",
            FORMAT_RULES, KIND_HINTS[diagram.kind], self._language_rule, ANSWER_RULE, "",
            "Current diagram:", diagram.structure.to_text() or "(empty)", "",
            *_transcript_section(diagram),
            *_selection_section(diagram),
            "New lines:", *_spoken(lines, diagram.structure),
        ])
        return self._complete(prompt)

    def draw_routed(self, diagrams: DiagramSet, previous: list[str], lines: list[SpokenLine]) -> RoutedDrawing:
        """Lets the drawing model also decide which diagram the lines belong to (Jev unsure or unavailable)."""
        active = diagrams.active
        prompt = "\n".join([
            "You keep live diagrams in sync with what a speaker explains out loud. "
            "Decide which diagram the new lines belong to, then update that diagram.",
            "The first line of your answer is exactly one of:",
            "diagram <number>      the new lines continue or return to that diagram",
            "diagram new <type>    the new lines themselves start a subject no existing diagram covers; "
            "<type> is one of " + ", ".join(kind.value for kind in DiagramKind),
            f"{_NO_STRUCTURE}                  the new lines add no structure (small talk, filler, questions)",
            "Then, unless none, the complete updated diagram.", "",
            FORMAT_RULES, *(f"{kind.value}: {hint}" for kind, hint in KIND_HINTS.items()), self._language_rule,
            "Answer with the first line and the diagram only, no fences, no prose.", "",
            "Diagrams:", *_diagram_listing(diagrams), "",
            "Recent transcript, already drawn, for context only:", *previous, "",
            "New lines:", *_spoken(lines, active.structure if active is not None else Structure()),
        ])
        return _split_routed(self._complete(prompt))

    def summarize(self, diagram: Diagram, lines: list[str]) -> str:
        """The diagram's rolling summary extended by `lines`."""
        prompt = "\n".join([
            "Summarize what the speaker explained for this diagram in at most five sentences. "
            "Keep every name of a component, step or state. Answer with the summary only.", "",
            "Summary so far:", diagram.summary or "(none)", "", "New transcript lines:", *lines,
        ])
        return self._complete(prompt).strip()

    def clean_transcript(self, lines: list[str], summary: str = "") -> str:
        """Clean chronological prose from the spoken transcript: Bucket B's source text."""
        parts = ["You prepare a clean transcript so a diagram can be redrawn from scratch.",
                 CLEAN_RULES, self._language_rule, ""]
        if summary:
            parts += ["Earlier, in short:", summary, ""]
        parts += ["Raw transcript:", *(lines or ["(nothing yet)"]), "", "Write the cleaned prose now."]
        return self._complete("\n".join(parts)).strip()

    def retrospect(self, diagram: Diagram) -> Revision:
        """A cleaned-up full diagram that reconciles corrections across the whole transcript."""
        prompt = "\n".join([
            f"You revise a live {diagram.kind.value} diagram so it matches everything the speaker said, "
            "not only the latest lines.",
            FORMAT_RULES, RETROSPECT_RULES, KIND_CHOICE_RULES,
            f"Current type hint: {KIND_HINTS[diagram.kind]}",
            *(f"{kind.value}: {hint}" for kind, hint in KIND_HINTS.items()),
            self._language_rule,
            "Return ONLY an optional `kind <type>` line, then the complete updated diagram — no fences, no prose.",
            "",
            "Current diagram:", diagram.structure.to_text() or "(empty)", "",
            *_transcript_section(diagram),
            *_selection_section(diagram),
            "Produce the complete revised diagram now.",
        ])
        return parse_revision(self._complete(prompt))

    def _complete(self, prompt: str) -> str:
        return self._completion(self._model, [{"role": "user", "content": prompt}], self._credentials())


def _transcript_section(diagram: Diagram) -> list[str]:
    summary = [f"Earlier, in short: {diagram.summary}", ""] if diagram.summary else []
    return [*summary, "Transcript so far:", *(diagram.recent_lines() or ["(nothing yet)"]), ""]


def _selection_section(diagram: Diagram) -> list[str]:
    if not diagram.selection:
        return []
    return ["Selected: " + ", ".join(_describe(element_id, diagram.structure) for element_id in sorted(diagram.selection)),
            ""]


def _spoken(lines: list[SpokenLine], structure: Structure) -> list[str]:
    return [line.text + (f" [pointing at {_describe(line.pointed, structure)}]" if line.pointed else "")
            for line in lines]


def _describe(element_id: str, structure: Structure) -> str:
    node = structure.node(element_id)
    return f"{node.label} ({element_id})" if node is not None else element_id


def _diagram_listing(diagrams: DiagramSet) -> list[str]:
    listing = []
    for diagram in diagrams.diagrams:
        current = ", current" if diagram is diagrams.active else ""
        listing += [f"Diagram {diagram.number} ({diagram.kind.value}{current}):", diagram.structure.to_text() or "(empty)"]
    return listing


def parse_revision(answer: str) -> Revision:
    """Optional leading `kind <type>` line, then the diagram body."""
    lines = answer.strip().splitlines()
    while lines and (not lines[0].strip() or lines[0].strip().startswith(_FENCE)):
        lines.pop(0)
    if not lines:
        raise StructureError("the answer is empty")
    header = lines[0].strip().lower()
    if match := _KIND_HEADER.match(header):
        body = "\n".join(lines[1:]).strip()
        if not body:
            raise StructureError("kind line without a diagram")
        return Revision(body, DiagramKind(match[1]))
    return Revision("\n".join(lines))


def _split_routed(answer: str) -> RoutedDrawing:
    lines = answer.strip().splitlines()
    while lines and (not lines[0].strip() or lines[0].strip().startswith(_FENCE)):
        lines.pop(0)
    if not lines:
        raise StructureError("the answer is empty")
    header, body = lines[0].strip().lower(), "\n".join(lines[1:])
    if header == _NO_STRUCTURE:
        return RoutedDrawing(None, "")
    if match := _EXISTING_HEADER.match(header):
        return RoutedDrawing(Destination.existing(int(match[1])), body)
    if match := _NEW_HEADER.match(header):
        return RoutedDrawing(Destination.new(DiagramKind(match[1])), body)
    raise StructureError(f"line 1 is not a diagram choice: {lines[0].strip()}")

