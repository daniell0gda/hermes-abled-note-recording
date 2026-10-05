"""Per-segment live sketch steps: Jev routes, the drawing model writes the structure, Jev styles, listeners render.

Routing runs on one thread and drawing on another, so segments keep being routed while a draw is in flight;
they are then drawn together in the next request. Nothing here blocks the caller of `add`.
"""

import logging
import re
import queue
import threading
from collections.abc import Callable
from dataclasses import dataclass, replace
from typing import Any, Protocol

from listening_app.sketch.diagrams import Changes, Destination, Diagram, DiagramSet, EdgeStyle, NodeStyle, Overview
from listening_app.sketch.drawer import RoutedDrawing, SpokenLine
from listening_app.sketch.jev_client import JevError, Routing
from listening_app.sketch.structure import Structure, StructureError, parse_structure

log = logging.getLogger(__name__)

MAX_ATTEMPTS = 2
PREVIOUS_SEGMENTS = 3
SUMMARY_AFTER_LINES = 16
SUMMARY_FOLD_LINES = 8
STOP_TIMEOUT_S = 2.0
DEFAULT_RETROSPECT_EVERY_N = 5
DEFAULT_RETROSPECT_MAX_NODES = 9

# Spoken cues that usually mean the speaker is revising something just said (EN + PL).
_CORRECTION_CUE = re.compile(
    r"(?i)(?:"
    r"\b(?:actually|instead|wait|rather|correction)\b|"
    r"\bi\s+meant\b|"
    r"\bit'?s\s+(?:actually\s+)?(?:different|not)\b|"
    r"\bnot\s+(?:a\s+|an\s+|the\s+)?\w+|"
    r"\bwłaściwie\b|"
    r"\braczej\b|"
    r"\bzamiast\b|"
    r"\bpoprawka\b|"
    r"\bkorekta\b|"
    r"\bchodziło\s+mi\b|"
    r"\bmam\s+na\s+myśli\b|"
    r"\bto\s+nie\s+tak\b|"
    r"\bnie\s+(?:jest\s+)?(?:to\s+)?\w+"
    r")"
)


Styles = tuple[dict[str, NodeStyle], dict[str, EdgeStyle]]


class Judge(Protocol):
    def route(self, segment: str, previous: list[str], overview: Overview) -> Routing: ...

    def style(self, diagram: Diagram, changes: Changes) -> Styles: ...


class Artist(Protocol):
    def draw(self, diagram: Diagram, lines: list[SpokenLine]) -> str: ...

    def draw_routed(self, diagrams: DiagramSet, previous: list[str], lines: list[SpokenLine]) -> RoutedDrawing: ...

    def summarize(self, diagram: Diagram, lines: list[str]) -> str: ...

    def retrospect(self, diagram: Diagram) -> str: ...


@dataclass(frozen=True)
class SketchState:
    """Every diagram's render view, the current one, and which one just changed (None: none was redrawn)."""

    views: tuple[dict[str, Any], ...]
    active: int | None
    updated: int | None


class SketchListener(Protocol):
    def sketch_updated(self, state: SketchState) -> None: ...

    def update_skipped(self, number: int | None, reason: str) -> None: ...

    def jev_unavailable(self, reason: str) -> None: ...


@dataclass(frozen=True)
class _Pending:
    line: SpokenLine
    destination: Destination
    previous: tuple[str, ...]
    attempts: int = 0


class SketchPipeline:
    def __init__(self, drawer: Artist, judge: Judge | None, listener: SketchListener,
                 *, retrospect_every_n: int = DEFAULT_RETROSPECT_EVERY_N,
                 retrospect_on_correction: bool = True,
                 retrospect_max_nodes: int = DEFAULT_RETROSPECT_MAX_NODES) -> None:
        self._drawer = drawer
        self._judge = judge
        self._listener = listener
        self._retrospect_every_n = retrospect_every_n
        self._retrospect_on_correction = retrospect_on_correction
        self._retrospect_max_nodes = retrospect_max_nodes
        self._diagrams = DiagramSet()
        self._changed = threading.Condition()
        self._pending: list[_Pending] = []
        self._recent: list[str] = []
        self._segments: queue.Queue[SpokenLine | None] = queue.Queue()
        self._unrouted = 0
        self._drawing = False
        self._running = False
        self._threads: list[threading.Thread] = []
        self._draws_since_retrospect: dict[int, int] = {}
        self._retrospect_requested: set[int] = set()

    def start(self) -> None:
        self._running = True
        self._threads = [threading.Thread(target=self._route_loop, name="sketch-route", daemon=True),
                         threading.Thread(target=self._draw_loop, name="sketch-draw", daemon=True)]
        for thread in self._threads:
            thread.start()

    def stop(self) -> None:
        """Stop both threads. A draw still in flight is abandoned."""
        with self._changed:
            self._running = False
            self._changed.notify_all()
        self._segments.put(None)
        for thread in self._threads:
            thread.join(STOP_TIMEOUT_S)

    def add(self, line: SpokenLine) -> None:
        with self._changed:
            self._unrouted += 1
        self._segments.put(line)

    def wait_until_routed(self, timeout_s: float) -> bool:
        with self._changed:
            return self._changed.wait_for(lambda: self._unrouted == 0, timeout_s)

    def wait_until_idle(self, timeout_s: float) -> bool:
        with self._changed:
            return self._changed.wait_for(lambda: not (self._unrouted or self._pending or self._drawing), timeout_s)

    def activate(self, number: int) -> None:
        with self._changed:
            self._diagrams.activate(number)

    def select(self, number: int, element_ids: list[str]) -> None:
        with self._changed:
            diagram = self._diagrams.get(number)
            if diagram is not None:
                diagram.select(element_ids)

    def snapshot(self) -> SketchState:
        with self._changed:
            return self._state(None)

    def route(self, line: SpokenLine) -> None:
        """Step 1: decide where the segment goes. A new diagram is opened at once, so its tab shows immediately."""
        with self._changed:
            previous = tuple(self._recent[-PREVIOUS_SEGMENTS:])
            self._recent.append(line.text)
            overview, judge = self._diagrams.overview(), self._judge
        routing = self._routing(judge, line.text, list(previous), overview)
        destination = routing.destination(bool(overview.summaries)) if routing is not None else Destination.by_drawer()
        log.debug("Live sketch routed %r to %s", line.text, destination)
        if destination is None:
            self._switch(routing.switches_to(overview.active) if routing is not None else None)
            return
        with self._changed:
            switched = self._enqueue(line, destination, previous)
            state = self._state(None)
        if switched:
            self._publish(state)

    def draw_next(self) -> bool:
        """Steps 2-4 for every pending segment bound for the same diagram. False when nothing was pending."""
        with self._changed:
            batch = self._take_batch()
            if not batch:
                return False
            self._drawing = True
        try:
            self._draw_batch(batch)
        finally:
            with self._changed:
                self._drawing = False
                self._changed.notify_all()
        return True

    def summarize_if_due(self) -> None:
        """Fold the oldest lines of a long diagram transcript into its rolling summary."""
        with self._changed:
            due = next((diagram.copy() for diagram in self._diagrams.diagrams
                        if len(diagram.recent_lines()) > SUMMARY_AFTER_LINES), None)
        if due is None:
            return
        lines = due.recent_lines()[:SUMMARY_FOLD_LINES]
        try:
            summary = self._drawer.summarize(due, lines)
        except Exception as exc:
            log.warning("Summarizing diagram %d failed: %s", due.number, exc)
            return
        with self._changed:
            live = self._diagrams.get(due.number)
            if live is not None:
                live.fold_into_summary(summary, len(lines))

    def retrospect_if_due(self) -> None:
        """Rewrite one diagram that is due for a full revision (correction, schedule or size)."""
        with self._changed:
            number = self._next_retrospect()
            if number is None:
                return
            diagram = self._diagram(number).copy()
            self._retrospect_requested.discard(number)
            self._draws_since_retrospect[number] = 0
        try:
            structure = parse_structure(self._drawer.retrospect(diagram))
        except Exception as exc:
            log.warning("Retrospecting diagram %d failed: %s", diagram.number, exc)
            return
        styles = self._styles(diagram, diagram.update(structure))
        with self._changed:
            live = self._diagram(diagram.number)
            live.update(structure)
            live.set_styles(*styles)
            state = self._state(live.number)
        self._publish(state)

    def _enqueue(self, line: SpokenLine, destination: Destination, previous: tuple[str, ...]) -> bool:
        """Queue the line for drawing, opening or activating its diagram. True when the current diagram changed."""
        before = self._diagrams.active
        if destination.number is None and not destination.drawer_decides:
            destination = Destination.existing(self._diagrams.create(destination.kind).number)
        elif destination.number is not None:
            self._diagrams.activate(destination.number)
        self._pending.append(_Pending(line, destination, previous))
        self._changed.notify_all()
        return self._diagrams.active is not before

    def _routing(self, judge: Judge | None, text: str, previous: list[str], overview: Overview) -> Routing | None:
        """Jev's judgment, or None when the drawing model has to route."""
        if judge is None:
            return None
        try:
            routing = judge.route(text, previous, overview)
        except JevError as exc:
            self._jev_failed(str(exc))
            return None
        log.debug("Jev routing: %s", routing)
        return routing

    def _switch(self, number: int | None) -> None:
        if number is None:
            return
        with self._changed:
            if self._diagrams.activate(number) is None:
                return
            state = self._state(None)
        self._publish(state)

    def _take_batch(self) -> list[_Pending]:
        if not self._pending:
            return []
        destination = self._pending[0].destination
        count = next((index for index, pending in enumerate(self._pending) if pending.destination != destination),
                     len(self._pending))
        batch = self._pending[:count]
        del self._pending[:count]
        return batch

    def _draw_batch(self, batch: list[_Pending]) -> None:
        lines = [pending.line for pending in batch]
        try:
            drawn = self._drawn(batch[0], lines)
        except Exception as exc:
            log.warning("Live sketch update skipped: %s", exc)
            self._retry(batch, batch[0].destination.number, str(exc) or type(exc).__name__)
            return
        if drawn is not None:
            self._commit(*drawn, lines)

    def _drawn(self, first: _Pending, lines: list[SpokenLine]) -> tuple[Diagram, Structure] | None:
        number = first.destination.number
        if number is None:
            return self._drawn_routed(first, lines)
        with self._changed:
            diagram = self._diagram(number).copy()
        return diagram, parse_structure(self._drawer.draw(diagram, lines))

    def _drawn_routed(self, first: _Pending, lines: list[SpokenLine]) -> tuple[Diagram, Structure] | None:
        with self._changed:
            diagrams = self._diagrams.copy()
        routed = self._drawer.draw_routed(diagrams, list(first.previous), lines)
        if routed.destination is None:
            return None
        structure = parse_structure(routed.text)
        with self._changed:
            diagram = self._diagrams.route(routed.destination.number, routed.destination.kind)
            if diagram is None:
                raise StructureError(f"diagram {routed.destination.number} does not exist")
            return diagram.copy(), structure

    def _commit(self, diagram: Diagram, structure: Structure, lines: list[SpokenLine]) -> None:
        texts = [line.text for line in lines]
        diagram.transcript += texts
        styles = self._styles(diagram, diagram.update(structure))
        with self._changed:
            live = self._diagram(diagram.number)
            live.transcript += texts
            live.update(structure)
            live.set_styles(*styles)
            self._note_draw(live, texts)
            state = self._state(live.number)
        self._publish(state)

    def _note_draw(self, diagram: Diagram, texts: list[str]) -> None:
        """After a successful draw, schedule a retrospect when cues, cadence or size say so."""
        number = diagram.number
        self._draws_since_retrospect[number] = self._draws_since_retrospect.get(number, 0) + 1
        if self._retrospect_on_correction and any(looks_like_correction(text) for text in texts):
            self._retrospect_requested.add(number)
        every = self._retrospect_every_n
        if every and self._draws_since_retrospect[number] >= every:
            self._retrospect_requested.add(number)
        if len(diagram.structure.nodes) > self._retrospect_max_nodes:
            self._retrospect_requested.add(number)

    def _next_retrospect(self) -> int | None:
        """Pick one diagram that should be revised, preferring an explicit request."""
        for number in sorted(self._retrospect_requested):
            if self._diagrams.get(number) is not None:
                return number
        for diagram in self._diagrams.diagrams:
            if len(diagram.structure.nodes) > self._retrospect_max_nodes:
                return diagram.number
        return None

    def _styles(self, diagram: Diagram, changes: Changes) -> Styles:
        with self._changed:
            judge = self._judge
        if judge is None:
            return {}, {}
        try:
            return judge.style(diagram, changes)
        except JevError as exc:
            self._jev_failed(str(exc))
            return {}, {}

    def _retry(self, batch: list[_Pending], number: int | None, reason: str) -> None:
        retry = [replace(pending, attempts=pending.attempts + 1) for pending in batch
                 if pending.attempts + 1 < MAX_ATTEMPTS]
        with self._changed:
            self._pending[:0] = retry
            self._changed.notify_all()
        self._notify(self._listener.update_skipped, number, reason)

    def _jev_failed(self, reason: str) -> None:
        with self._changed:
            if self._judge is None:
                return
            self._judge = None
        log.warning("Jev is unavailable, the drawing model routes from now on: %s", reason)
        self._notify(self._listener.jev_unavailable, reason)

    def _diagram(self, number: int) -> Diagram:
        diagram = self._diagrams.get(number)
        if diagram is None:
            raise StructureError(f"diagram {number} does not exist")
        return diagram

    def _state(self, updated: int | None) -> SketchState:
        active = self._diagrams.active
        return SketchState(tuple(diagram.view() for diagram in self._diagrams.diagrams),
                           active.number if active is not None else None, updated)

    def _publish(self, state: SketchState) -> None:
        self._notify(self._listener.sketch_updated, state)

    @staticmethod
    def _notify(handler: Callable[..., None], *args: object) -> None:
        try:
            handler(*args)
        except Exception:
            log.exception("Live sketch listener failed")

    def _route_loop(self) -> None:
        while (line := self._segments.get()) is not None:
            self._run_safely(lambda: self.route(line))
            with self._changed:
                self._unrouted -= 1
                self._changed.notify_all()

    def _draw_loop(self) -> None:
        while True:
            with self._changed:
                self._changed.wait_for(lambda: bool(self._pending) or not self._running)
                if not self._running:
                    return
            self._run_safely(self.draw_next)
            with self._changed:
                idle = not self._pending
            if idle:
                self._run_safely(self.retrospect_if_due)
                self._run_safely(self.summarize_if_due)

    @staticmethod
    def _run_safely(step: Callable[[], object]) -> None:
        try:
            step()
        except Exception:
            log.exception("Live sketch step failed")


def looks_like_correction(text: str) -> bool:
    """True when `text` sounds like the speaker is revising something they just said."""
    return bool(_CORRECTION_CUE.search(text))
