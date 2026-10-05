"""The minimal line format the drawing model writes: nodes, edges, groups and a title, no styling."""

import re
from dataclasses import dataclass

_ID = r"[a-z][a-z0-9_]*"
_LABEL = r'"([^"]*)"'
_TITLE_LINE = re.compile(rf"^title\s+{_LABEL}$")
_GROUP_LINE = re.compile(rf"^group\s+({_ID})(?:\s+{_LABEL})?\s*:\s*({_ID}(?:\s*,\s*{_ID})*)\s*,?$")
_EDGE_LINE = re.compile(rf"^({_ID})\s*->\s*({_ID})(?:\s+{_LABEL})?$")
_NODE_LINE = re.compile(rf"^({_ID})\s+{_LABEL}$")
_FENCE = "```"


class StructureError(ValueError):
    """The drawing model's answer is not a diagram in the line format."""


@dataclass(frozen=True)
class Node:
    id: str
    label: str


@dataclass(frozen=True)
class Edge:
    id: str
    source: str
    target: str
    label: str


@dataclass(frozen=True)
class Group:
    id: str
    label: str
    members: tuple[str, ...]


@dataclass(frozen=True)
class Structure:
    """A diagram's content. Edge order is meaningful: it is the message order of a sequence diagram."""

    title: str = ""
    nodes: tuple[Node, ...] = ()
    edges: tuple[Edge, ...] = ()
    groups: tuple[Group, ...] = ()

    def element_ids(self) -> set[str]:
        return {node.id for node in self.nodes} | {edge.id for edge in self.edges}

    def node(self, node_id: str) -> Node | None:
        return next((node for node in self.nodes if node.id == node_id), None)

    def edge(self, edge_id: str) -> Edge | None:
        return next((edge for edge in self.edges if edge.id == edge_id), None)

    def to_text(self) -> str:
        """The structure in the line format, as the drawing model is shown its current diagram."""
        lines = [f'title "{self.title}"'] if self.title else []
        lines += [f'{node.id} "{node.label}"' for node in self.nodes]
        lines += [f'{edge.source} -> {edge.target}' + (f' "{edge.label}"' if edge.label else "") for edge in self.edges]
        lines += [f'group {group.id} "{group.label}": {", ".join(group.members)}' for group in self.groups]
        return "\n".join(lines)


class _Builder:
    def __init__(self) -> None:
        self.title = ""
        self.labels: dict[str, str] = {}
        self.edges: list[tuple[str, str, str]] = []
        self.groups: list[tuple[str, str, list[str]]] = []

    def add(self, line: str) -> bool:
        if match := _TITLE_LINE.match(line):
            self.title = match[1]
        elif match := _GROUP_LINE.match(line):
            self.groups.append((match[1], match[2] or match[1], [member.strip() for member in match[3].split(",")]))
        elif match := _EDGE_LINE.match(line):
            self.edges.append((match[1], match[2], match[3] or ""))
        elif match := _NODE_LINE.match(line):
            self.labels[match[1]] = match[2]
        else:
            return False
        return True

    def build(self) -> Structure:
        for source, target, _ in self.edges:
            for end in (source, target):
                self.labels.setdefault(end, _label_from_id(end))
        nodes = tuple(Node(node_id, label) for node_id, label in self.labels.items())
        used = set(self.labels)
        fixed_groups: list[Group] = []
        for group_id, label, members in self.groups:
            gid = group_id
            if gid in used:
                gid = f"{group_id}_group"
                while gid in used:
                    gid = f"{gid}_g"
            used.add(gid)
            kept = tuple(member for member in members if member in self.labels)
            if kept:
                fixed_groups.append(Group(gid, label, kept))
        return Structure(self.title, nodes, _numbered_edges(self.edges), tuple(fixed_groups))


def _label_from_id(node_id: str) -> str:
    return node_id.replace("_", " ").capitalize()


def _numbered_edges(edges: list[tuple[str, str, str]]) -> tuple[Edge, ...]:
    """Edge ids are `source->target`; a repeated pair (a sequence diagram's second message) gets `#2`, `#3`, ..."""
    seen: dict[str, int] = {}
    numbered = []
    for source, target, label in edges:
        base = f"{source}->{target}"
        seen[base] = seen.get(base, 0) + 1
        numbered.append(Edge(base if seen[base] == 1 else f"{base}#{seen[base]}", source, target, label))
    return tuple(numbered)


def parse_structure(text: str) -> Structure:
    """Parse the drawing model's answer. Raises StructureError on any line outside the format or on empty text."""
    builder = _Builder()
    content_lines = 0
    for number, raw in enumerate(text.splitlines(), start=1):
        line = raw.strip()
        if not line or line.startswith(_FENCE):
            continue
        if not builder.add(line):
            raise StructureError(f"line {number}: {line}")
        content_lines += 1
    if not content_lines:
        raise StructureError("the answer is empty")
    return builder.build()

