"""Bucket B: clean the spoken transcript with Grok, judge each chunk with Jev, rebuild the diagram from scratch.

Bucket A keeps drawing live for the speaker. Bucket B runs in the background on a separate, empty
diagram. When a rebuild finishes, the live view is soft-swapped to B's result (no apply button).
A rebuild is abandoned as soon as its generation is no longer current (newer run, cancel, or stop).
"""

from __future__ import annotations

import logging
import re
from collections.abc import Callable
from dataclasses import dataclass
from typing import Protocol

from listening_app.sketch.diagrams import (
    Changes,
    Diagram,
    DiagramKind,
    EdgeStyle,
    NodeStyle,
    Overview,
)
from listening_app.sketch.drawer import SpokenLine
from listening_app.sketch.jev_client import JevError, Routing
from listening_app.sketch.structure import Structure, parse_structure

log = logging.getLogger(__name__)

PREVIOUS_CHUNKS = 3
YES = 0.5
MIN_CHUNK_CHARS = 8
MAX_CHUNKS = 24

# Split on sentence ends or line breaks; the sentence mark stays with the left side.
_CHUNK_SPLIT = re.compile(r"(?<=[.!?;])\s+|\n+")
_BULLET = re.compile(r"^(?:[-*\u2022]|\d+[.)])\s+")


class CleanArtist(Protocol):
    def clean_transcript(self, lines: list[str], summary: str = "") -> str: ...

    def draw(self, diagram: Diagram, lines: list[SpokenLine]) -> str: ...


class ChunkJudge(Protocol):
    def route(self, segment: str, previous: list[str], overview: Overview) -> Routing: ...

    def style(self, diagram: Diagram, changes: Changes) -> tuple[dict[str, NodeStyle], dict[str, EdgeStyle]]: ...


@dataclass(frozen=True)
class BucketBResult:
    """A finished rebuild, ready to soft-swap onto the live diagram with the same number."""

    diagram_number: int
    kind: DiagramKind
    structure: Structure
    node_styles: dict[str, NodeStyle]
    edge_styles: dict[str, EdgeStyle]
    generation: int
    clean_len: int
    chunk_count: int
    drawn_chunks: int


def chunk_clean_text(text: str, max_chunks: int = MAX_CHUNKS) -> list[str]:
    """Split cleaned prose into short chunks: one sentence, step or atomic fact each."""
    chunks: list[str] = []
    for part in _CHUNK_SPLIT.split(text.strip()):
        piece = _BULLET.sub("", " ".join(part.split())).strip()
        if len(piece) >= MIN_CHUNK_CHARS:
            chunks.append(piece)
    if len(chunks) > max_chunks:
        # Keep the order; fold the tail into the last chunk so nothing said is lost.
        chunks = chunks[:max_chunks - 1] + [" ".join(chunks[max_chunks - 1:])]
    return chunks


def rebuild_from_clean(
    source: Diagram,
    drawer: CleanArtist,
    judge: ChunkJudge,
    generation: int,
    *,
    is_current: Callable[[int], bool] | None = None,
) -> BucketBResult | None:
    """Grok-clean -> chunk -> Jev one by one -> draw on an empty diagram. None when stale or failed."""
    alive = is_current or (lambda _generation: True)
    number = source.number
    lines = list(source.transcript)
    if not lines and not source.summary:
        log.info("BucketB skip diagram %d: empty transcript", number)
        return None

    log.info("BucketB start diagram %d gen=%d (%d transcript lines)", number, generation, len(lines))
    try:
        clean = drawer.clean_transcript(lines, source.summary)
    except Exception as exc:
        log.warning("BucketB clean fail diagram %d: %s", number, exc)
        return None
    if not alive(generation):
        log.info("BucketB cancel diagram %d gen=%d (stale after clean)", number, generation)
        return None

    chunks = chunk_clean_text(clean)
    log.info("BucketB clean diagram %d: %d chars -> %d chunks", number, len(clean), len(chunks))
    if not chunks:
        log.warning("BucketB fail diagram %d: no chunks after clean", number)
        return None

    try:
        kind = judge.route(clean, [], Overview(None, ())).kind
    except JevError as exc:
        log.warning("BucketB Jev fail diagram %d (kind): %s", number, exc)
        return None
    log.info("BucketB kind diagram %d: %s", number, kind.value)

    built = Diagram(number, kind)
    drawn = 0
    for index, chunk in enumerate(chunks, start=1):
        if not alive(generation):
            log.info("BucketB cancel diagram %d gen=%d at chunk %d/%d", number, generation, index, len(chunks))
            return None
        previous = chunks[max(0, index - 1 - PREVIOUS_CHUNKS):index - 1]
        overview = Overview(number, ((number, built.routing_summary()),))
        try:
            routing = judge.route(chunk, previous, overview)
        except JevError as exc:
            log.warning("BucketB Jev fail diagram %d chunk %d/%d: %s", number, index, len(chunks), exc)
            return None
        if routing.structural < YES:
            log.info("BucketB Jev chunk %d/%d ok: no structure (%.2f)", index, len(chunks), routing.structural)
            continue
        log.info("BucketB Jev chunk %d/%d ok: structure (%.2f)", index, len(chunks), routing.structural)
        try:
            structure = parse_structure(drawer.draw(built, [SpokenLine(chunk)]))
        except Exception as exc:
            log.warning("BucketB draw fail diagram %d chunk %d/%d: %s", number, index, len(chunks), exc)
            continue
        changes = built.update(structure)
        built.transcript.append(chunk)
        try:
            built.set_styles(*judge.style(built, changes))
        except JevError as exc:
            log.warning("BucketB style fail diagram %d chunk %d/%d: %s", number, index, len(chunks), exc)
        drawn += 1

    if not alive(generation):
        log.info("BucketB cancel diagram %d gen=%d before swap", number, generation)
        return None
    if not built.structure.nodes:
        log.warning("BucketB fail diagram %d: rebuild produced no nodes (%d/%d chunks drawn)",
                    number, drawn, len(chunks))
        return None

    structure = built.structure
    result = BucketBResult(
        diagram_number=number,
        kind=built.kind,
        structure=structure,
        node_styles={node.id: built.node_style(node.id) for node in structure.nodes},
        edge_styles={edge.id: built.edge_style(edge.id) for edge in structure.edges},
        generation=generation,
        clean_len=len(clean),
        chunk_count=len(chunks),
        drawn_chunks=drawn,
    )
    log.info("BucketB rebuild diagram %d: %d nodes, %d edges, %d groups, kind=%s (%d/%d chunks drawn)",
             number, len(structure.nodes), len(structure.edges), len(structure.groups), built.kind.value,
             drawn, len(chunks))
    return result
