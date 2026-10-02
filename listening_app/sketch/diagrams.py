"""The diagrams of one sketch run: content, per-diagram transcript, element styles and selection."""

from dataclasses import dataclass, field, replace
from enum import StrEnum
from typing import Any

from listening_app.sketch.structure import Structure

MAX_FOCAL_NODES = 2


class DiagramKind(StrEnum):
    FLOW = "flow"
    SEQUENCE = "sequence"
    STATE = "state"
    TREE = "tree"
    ER = "er"


class Shape(StrEnum):
    SERVICE = "service"
    DATABASE = "database"
    QUEUE = "queue"
    CACHE = "cache"
    ACTOR = "actor"
    EXTERNAL = "external"
    CLIENT = "client"
    STATE = "state"
    DECISION = "decision"
    NOTE = "note"


class EdgeStyle(StrEnum):
    SYNC = "sync"
    ASYNC = "async"
    DATA = "data"
    TRANSITION = "transition"
    DEPENDENCY = "dependency"


@dataclass(frozen=True)
class NodeStyle:
    shape: Shape
    focal: bool = False


@dataclass(frozen=True)
class Destination:
    """Where a segment goes: an existing diagram, a new one of `kind` (number None), or the drawing model decides."""

    number: int | None
    kind: DiagramKind = DiagramKind.FLOW
    drawer_decides: bool = False

    @classmethod
    def new(cls, kind: DiagramKind) -> "Destination":
        return cls(None, kind)

    @classmethod
    def existing(cls, number: int) -> "Destination":
        return cls(number)

    @classmethod
    def by_drawer(cls) -> "Destination":
        return cls(None, drawer_decides=True)


@dataclass(frozen=True)
class Overview:
    """What routing needs to know about the diagrams: which one is current, and each one's summary line."""

    active: int | None
    summaries: tuple[tuple[int, str], ...]


@dataclass(frozen=True)
class Changes:
    """Ids of the elements an update added or relabelled: the ones that still need a style."""

    nodes: tuple[str, ...]
    edges: tuple[str, ...]


def _default_styles(kind: DiagramKind) -> tuple[NodeStyle, EdgeStyle]:
    if kind is DiagramKind.STATE:
        return NodeStyle(Shape.STATE), EdgeStyle.TRANSITION
    return NodeStyle(Shape.SERVICE), EdgeStyle.SYNC


@dataclass
class Diagram:
    number: int
    kind: DiagramKind
    structure: Structure = Structure()
    transcript: list[str] = field(default_factory=list)
    summary: str = ""
    summarized: int = 0
    selection: set[str] = field(default_factory=set)
    _node_styles: dict[str, NodeStyle] = field(default_factory=dict)
    _edge_styles: dict[str, EdgeStyle] = field(default_factory=dict)

    @property
    def title(self) -> str:
        return self.structure.title

    def changes(self, structure: Structure) -> Changes:
        """Elements of `structure` that are new or relabelled compared to the current content."""
        return Changes(tuple(node.id for node in structure.nodes if self.structure.node(node.id) != node),
                       tuple(edge.id for edge in structure.edges if self.structure.edge(edge.id) != edge))

    def update(self, structure: Structure) -> Changes:
        """Replace the content. Styles stay stored by id; the selection keeps only elements that still exist."""
        changes = self.changes(structure)
        self.structure = structure
        self.selection &= structure.element_ids()
        return changes

    def copy(self) -> "Diagram":
        """An independent copy, for work done outside the pipeline's lock."""
        return replace(self, transcript=list(self.transcript), selection=set(self.selection),
                       _node_styles=dict(self._node_styles), _edge_styles=dict(self._edge_styles))

    def set_styles(self, nodes: dict[str, NodeStyle], edges: dict[str, EdgeStyle]) -> None:
        self._node_styles.update(nodes)
        self._edge_styles.update(edges)

    def node_style(self, node_id: str) -> NodeStyle:
        return self._node_styles.get(node_id, _default_styles(self.kind)[0])

    def edge_style(self, edge_id: str) -> EdgeStyle:
        return self._edge_styles.get(edge_id, _default_styles(self.kind)[1])

    def select(self, element_ids: list[str]) -> None:
        self.selection = set(element_ids) & self.structure.element_ids()

    def recent_lines(self) -> list[str]:
        return self.transcript[self.summarized:]

    def fold_into_summary(self, summary: str, lines: int) -> None:
        """`summary` now covers the transcript up to `lines` more lines than before."""
        self.summary = summary
        self.summarized += lines

    def routing_summary(self) -> str:
        labels = ", ".join(node.label for node in self.structure.nodes) or "none"
        return f"Diagram {self.number}: {self.title or 'untitled'} (nodes: {labels})"

    def view(self, pointed: str | None = None) -> dict[str, Any]:
        """Everything the renderer needs, JSON-ready."""
        focal = [node.id for node in self.structure.nodes if self.node_style(node.id).focal][:MAX_FOCAL_NODES]
        return {
            "number": self.number,
            "title": self.title,
            "kind": self.kind.value,
            "nodes": [{"id": node.id, "label": node.label, "shape": self.node_style(node.id).shape.value,
                       "focal": node.id in focal} for node in self.structure.nodes],
            "edges": [{"id": edge.id, "source": edge.source, "target": edge.target, "label": edge.label,
                       "style": self.edge_style(edge.id).value} for edge in self.structure.edges],
            "groups": [{"id": group.id, "label": group.label, "members": list(group.members)}
                       for group in self.structure.groups],
            "selected": sorted(self.selection),
            "pointed": pointed,
        }


class DiagramSet:
    """All diagrams of a sketch run, numbered from 1, and which one the speaker is on."""

    def __init__(self) -> None:
        self._diagrams: list[Diagram] = []
        self._active: Diagram | None = None

    @property
    def diagrams(self) -> tuple[Diagram, ...]:
        return tuple(self._diagrams)

    @property
    def active(self) -> Diagram | None:
        return self._active

    def get(self, number: int) -> Diagram | None:
        return next((diagram for diagram in self._diagrams if diagram.number == number), None)

    def overview(self) -> Overview:
        return Overview(self._active.number if self._active is not None else None,
                        tuple((diagram.number, diagram.routing_summary()) for diagram in self._diagrams))

    def copy(self) -> "DiagramSet":
        """An independent copy of every diagram, keeping which one is active."""
        copied = DiagramSet()
        copied._diagrams = [diagram.copy() for diagram in self._diagrams]
        if self._active is not None:
            copied._active = copied.get(self._active.number)
        return copied

    def route(self, number: int | None, kind: DiagramKind) -> Diagram | None:
        """Open a new diagram of `kind` (number None) or the existing one; it becomes active. Unknown number: None."""
        if number is None:
            return self.create(kind)
        return self.activate(number)

    def activate(self, number: int) -> Diagram | None:
        diagram = self.get(number)
        if diagram is not None:
            self._active = diagram
        return diagram

    def create(self, kind: DiagramKind) -> Diagram:
        """Open the next numbered diagram; it becomes active."""
        diagram = Diagram(len(self._diagrams) + 1, kind)
        self._diagrams.append(diagram)
        self._active = diagram
        return diagram
