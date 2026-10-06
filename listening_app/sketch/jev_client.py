"""TypeSafe Jev: fast typed judgments that route each segment to a diagram and pick element styles."""

from dataclasses import dataclass
from typing import Any

import httpx

from listening_app.sketch.diagrams import (
    Changes,
    Destination,
    Diagram,
    DiagramKind,
    EdgeStyle,
    NodeStyle,
    Overview,
    Shape,
)

JEV_URL = "https://api.typesafe.ai/v1/systemone"
TIMEOUT_S = 5.0
YES = 0.5
ROUTE_CONFIDENCE = 0.4
UNREQUESTED_NEW_CONFIDENCE = 0.8
SWITCH_CONFIDENCE = 0.6
FOCAL_PROBABILITY = 0.7
NEW_TARGET = "new"

KINDS = {
    DiagramKind.FLOW: "How components, systems or steps connect: system architecture, data flow, pipelines, or a "
                      "process where it does not matter who does each step",
    DiagramKind.SEQUENCE: "The back-and-forth order of messages between a few participants, e.g. a protocol or handshake",
    DiagramKind.STATE: "The lifecycle or statuses of one thing and the transitions between them",
    DiagramKind.TREE: "Hierarchy, parent and child decomposition",
    DiagramKind.ER: "Entities and their relationships",
    DiagramKind.SWIMLANE: "A process where several named people, roles, teams or tools each do their own steps and "
                          "hand the work to each other, so who does each step matters",
    DiagramKind.NESTED: "Scope and containment: outer groups hold inner members",
    DiagramKind.LAYERS: "Stacked concerns from top to bottom (user, process, team, QA, done)",
    DiagramKind.DEPENDENCY: "Unordered dependencies or ownership between components",
}
SHAPES = {
    Shape.SERVICE: "An application service, server, API or backend component that runs code",
    Shape.DATABASE: "A database or persistent data store, e.g. Postgres, MySQL, MongoDB, S3",
    Shape.QUEUE: "A message queue, topic, stream or event bus, e.g. Kafka, RabbitMQ, SQS",
    Shape.CACHE: "An in-memory cache, e.g. Redis, Memcached",
    Shape.ACTOR: "A person or role, e.g. user, customer, admin, courier",
    Shape.EXTERNAL: "A third-party or external system outside the speaker's own system",
    Shape.CLIENT: "A client application or UI, e.g. browser, mobile app, web frontend",
    Shape.STATE: "A state or status of a thing, e.g. draft, paid, shipped",
    Shape.DECISION: "A decision or condition with alternative outcomes",
    Shape.NOTE: "A remark or annotation rather than a component",
}
EDGE_STYLES = {
    EdgeStyle.SYNC: "A synchronous call or request that expects a reply",
    EdgeStyle.ASYNC: "An asynchronous event or message, published or consumed, fire-and-forget",
    EdgeStyle.DATA: "Reading or writing data to a store",
    EdgeStyle.TRANSITION: "A transition from one state to another",
    EdgeStyle.DEPENDENCY: "A dependency, ownership or structural relationship",
}

_ROUTING_QUESTIONS: dict[str, dict[str, Any]] = {
    "structural": {
        "type": "noul",
        "instructions": "Does `new_segment` describe components, steps, states or relationships that belong in a diagram?",
        "criteria": {"true": "Adds or changes structure",
                     "false": "Small talk, filler, navigation or questions to the audience"},
    },
    "new_diagram": {
        "type": "noul",
        "instructions": "In `new_segment`, does the speaker explicitly ask to start a new, separate diagram?",
        "criteria": {"true": "Explicitly asks for a new or another diagram",
                     "false": "Continues or returns to an existing diagram, or does not mention diagrams"},
    },
    "kind": {
        "type": "choice",
        "instructions": "If `new_segment` starts a new diagram, which diagram type fits what the speaker explains "
                        "in `previous_segments` and `new_segment`?",
        "criteria": {kind.value: text for kind, text in KINDS.items()},
    },
}
_TARGET_INSTRUCTIONS = ("Which diagram is the speaker talking about in `new_segment`? They keep talking about "
                        "`current_diagram` unless they name another diagram, return to one, or clearly change the subject.")


class JevError(Exception):
    """Jev could not answer: network, key, rate limit, or an unexpected answer."""


@dataclass(frozen=True)
class Routing:
    structural: float
    new_requested: float
    target: int | None
    confidence: float
    kind: DiagramKind

    def destination(self, has_diagrams: bool) -> Destination | None:
        """None: the segment adds no structure. Low confidence leaves the choice to the drawing model."""
        if self.new_requested >= YES:
            return Destination.new(self.kind)
        if self.structural < YES:
            return None
        if not has_diagrams:
            return Destination.new(self.kind)
        if self.target is None:
            return Destination.new(self.kind) if self.confidence >= UNREQUESTED_NEW_CONFIDENCE else Destination.by_drawer()
        if self.confidence < ROUTE_CONFIDENCE:
            return Destination.by_drawer()
        return Destination.existing(self.target)

    def switches_to(self, active: int | None) -> int | None:
        """The diagram the speaker returns to by naming it ("back to diagram 1"), if it is not the current one."""
        if self.target is None or self.target == active or self.confidence < SWITCH_CONFIDENCE:
            return None
        return self.target


class JevClient:
    def __init__(self, api_key: str, model: str, transport: httpx.BaseTransport | None = None) -> None:
        self._model = model
        self._http = httpx.Client(headers={"Authorization": f"Bearer {api_key}"}, timeout=TIMEOUT_S,
                                  transport=transport)

    def close(self) -> None:
        self._http.close()

    def route(self, segment: str, previous: list[str], overview: Overview) -> Routing:
        state = {"new_segment": segment, "previous_segments": previous, "current_diagram": overview.active}
        targets = {f"d{number}": summary for number, summary in overview.summaries}
        targets[NEW_TARGET] = "A different subject that none of the existing diagrams covers"
        questions = {**_ROUTING_QUESTIONS,
                     "target": {"type": "choice", "instructions": _TARGET_INSTRUCTIONS, "criteria": targets}}
        answers = self._ask(state, questions)
        try:
            target = answers["target"]["choice"]
            return Routing(float(answers["structural"]["noul"]), float(answers["new_diagram"]["noul"]),
                           None if target == NEW_TARGET else int(target.removeprefix("d")),
                           float(answers["target"]["confidence"]), DiagramKind(answers["kind"]["choice"]))
        except (KeyError, TypeError, ValueError) as exc:
            raise JevError(f"unexpected routing answer: {exc!r}") from exc

    def style(self, diagram: Diagram, changes: Changes) -> tuple[dict[str, NodeStyle], dict[str, EdgeStyle]]:
        """Shape and emphasis of each new node, style of each new edge, in one request."""
        if not changes.nodes and not changes.edges:
            return {}, {}
        answers = self._ask(_style_state(diagram), _style_questions(diagram, changes))
        try:
            nodes = {node_id: NodeStyle(Shape(answers[f"shape:{node_id}"]["choice"]),
                                        float(answers[f"focal:{node_id}"]["noul"]) >= FOCAL_PROBABILITY)
                     for node_id in changes.nodes}
            edges = {edge_id: EdgeStyle(answers[f"edge:{edge_id}"]["choice"]) for edge_id in changes.edges}
        except (KeyError, TypeError, ValueError) as exc:
            raise JevError(f"unexpected style answer: {exc!r}") from exc
        return nodes, edges

    def _ask(self, state: dict[str, Any], questions: dict[str, Any]) -> dict[str, Any]:
        try:
            response = self._http.post(JEV_URL, json={"model": self._model, "state": state, "questions": questions})
            response.raise_for_status()
            answers: dict[str, Any] = response.json()["answers"]
        except (httpx.HTTPError, ValueError, KeyError) as exc:
            raise JevError(str(exc) or type(exc).__name__) from exc
        return answers


def _style_state(diagram: Diagram) -> dict[str, Any]:
    structure = diagram.structure
    return {"diagram_type": diagram.kind.value, "summary": diagram.summary, "transcript": diagram.recent_lines(),
            "nodes": [{"id": node.id, "label": node.label} for node in structure.nodes],
            "edges": [{"source": edge.source, "target": edge.target, "label": edge.label} for edge in structure.edges]}


def _style_questions(diagram: Diagram, changes: Changes) -> dict[str, Any]:
    structure = diagram.structure
    labels = {node.id: node.label for node in structure.nodes}
    questions: dict[str, Any] = {}
    for node_id in changes.nodes:
        label = labels[node_id]
        questions[f"shape:{node_id}"] = {
            "type": "choice", "criteria": {shape.value: text for shape, text in SHAPES.items()},
            "instructions": f'What kind of element is the node `{node_id}` ("{label}") in this diagram?'}
        questions[f"focal:{node_id}"] = {
            "type": "noul",
            "instructions": f'Does the speaker in `transcript` stress the node `{node_id}` ("{label}") '
                            "as the central or most important element of the diagram?",
            "criteria": {"true": "Explicitly stressed as central or most important",
                         "false": "One element among others"}}
    edge_ids = [edge.id for edge in structure.edges]
    for edge_id in changes.edges:
        index = edge_ids.index(edge_id)
        edge = structure.edges[index]
        questions[f"edge:{edge_id}"] = {
            "type": "choice", "criteria": {style.value: text for style, text in EDGE_STYLES.items()},
            "instructions": f"What kind of connection is `edges[{index}]` from `{edge.source}` to `{edge.target}`"
                            + (f' ("{edge.label}")' if edge.label else "") + "?"}
    return questions

