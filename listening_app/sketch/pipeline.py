"""Per-segment live sketch steps: Jev routes, the drawing model writes the structure, Jev styles, listeners render.

Routing runs on one thread and drawing on another, so segments keep being routed while a draw is in flight;
they are then drawn together in the next request. Nothing here blocks the caller of `add`.
"""

import logging
import re
import queue
import threading
import time
from collections.abc import Callable
from dataclasses import dataclass, replace
from typing import Any, Protocol

from listening_app.sketch.bucket_b import BucketBResult, rebuild_from_clean
from listening_app.sketch.diagrams import Changes, Destination, Diagram, DiagramSet, EdgeStyle, NodeStyle, Overview
from listening_app.sketch.drawer import Revision, RoutedDrawing, SpokenLine
from listening_app.sketch.jev_client import JevError, Routing
from listening_app.sketch.structure import Structure, StructureError, parse_structure

log = logging.getLogger(__name__)

MAX_ATTEMPTS = 2
PREVIOUS_SEGMENTS = 3
SUMMARY_AFTER_LINES = 16
SUMMARY_FOLD_LINES = 8
STOP_TIMEOUT_S = 2.0
DEFAULT_RETROSPECT_EVERY_N = 1
DEFAULT_RETROSPECT_MAX_NODES = 9
DEFAULT_BUCKET_B_MIN_NEW_LINES = 2
DEFAULT_BUCKET_B_DEBOUNCE_S = 2.0
DEFAULT_BUCKET_B_MAX_WAIT_S = 10.0

Styles = tuple[dict[str, NodeStyle], dict[str, EdgeStyle]]


class Judge(Protocol):
    def route(self, segment: str, previous: list[str], overview: Overview) -> Routing: ...

    def style(self, diagram: Diagram, changes: Changes) -> Styles: ...


class Artist(Protocol):
    def draw(self, diagram: Diagram, lines: list[SpokenLine]) -> str: ...

    def draw_routed(self, diagrams: DiagramSet, previous: list[str], lines: list[SpokenLine]) -> RoutedDrawing: ...

    def summarize(self, diagram: Diagram, lines: list[str]) -> str: ...

    def retrospect(self, diagram: Diagram) -> Revision: ...

    def clean_transcript(self, lines: list[str], summary: str = "") -> str: ...


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
                 retrospect_max_nodes: int = DEFAULT_RETROSPECT_MAX_NODES,
                 retrospect_on_correction: bool = False,
                 bucket_b: bool = False,
                 bucket_b_min_new_lines: int = DEFAULT_BUCKET_B_MIN_NEW_LINES,
                 bucket_b_debounce_s: float = DEFAULT_BUCKET_B_DEBOUNCE_S,
                 bucket_b_max_wait_s: float = DEFAULT_BUCKET_B_MAX_WAIT_S) -> None:
        # retrospect_on_correction is ignored (kept for older call sites); keywords are not used.
        self._drawer = drawer
        self._judge = judge
        self._listener = listener
        self._retrospect_every_n = retrospect_every_n
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
        # Bucket B: background clean rebuild (Grok clean text -> Jev per chunk -> draw from scratch).
        self._bucket_b = bucket_b and judge is not None
        self._b_judge = judge
        self._b_min_new_lines = max(1, bucket_b_min_new_lines)
        self._b_debounce_s = max(0.0, bucket_b_debounce_s)
        self._b_max_wait_s = max(self._b_debounce_s, bucket_b_max_wait_s)
        self._b_generation = 0
        self._b_stopped = False
        self._b_swaps = 0
        self._b_requested: set[int] = set()
        self._b_requested_at = 0.0
        self._b_last_line_at = 0.0
        self._b_lines_done: dict[int, int] = {}
        if bucket_b and judge is None:
            log.warning("BucketB disabled: Jev unavailable (no TYPESAFE_API_KEY); live view only")

    def start(self) -> None:
        self._running = True
        self._threads = [threading.Thread(target=self._route_loop, name="sketch-route", daemon=True),
                         threading.Thread(target=self._draw_loop, name="sketch-draw", daemon=True)]
        if self._bucket_b:
            self._threads.append(threading.Thread(target=self._bucket_b_loop, name="sketch-bucket-b", daemon=True))
        for thread in self._threads:
            thread.start()

    def stop(self) -> None:
        """Stop both threads. A draw still in flight is abandoned."""
        with self._changed:
            self._running = False
            self._b_stopped = True
            self._b_generation += 1
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
        """Rewrite one diagram that is due for a full revision (after a draw, idle, or size)."""
        with self._changed:
            number = self._next_retrospect()
            if number is None:
                return
            diagram = self._diagram(number).copy()
            self._retrospect_requested.discard(number)
            self._draws_since_retrospect[number] = 0
            before_nodes = len(diagram.structure.nodes)
            before_edges = len(diagram.structure.edges)
            swaps = self._b_swaps
        log.info("Retrospect start diagram %d (%d nodes, %d edges, kind=%s)",
                 diagram.number, before_nodes, before_edges, diagram.kind.value)
        try:
            revision = self._drawer.retrospect(diagram)
            structure = parse_structure(revision.text)
        except Exception as exc:
            log.warning("Retrospect fail diagram %d (%d nodes, %d edges): %s",
                        diagram.number, before_nodes, before_edges, exc)
            return
        if revision.kind is not None:
            diagram.set_kind(revision.kind)
        styles = self._styles(diagram, diagram.update(structure))
        with self._changed:
            if swaps != self._b_swaps:
                log.info("Retrospect dropped diagram %d: BucketB swapped meanwhile", diagram.number)
                return
            live = self._diagram(diagram.number)
            if revision.kind is not None:
                live.set_kind(revision.kind)
            live.update(structure)
            live.set_styles(*styles)
            state = self._state(live.number)
            after_nodes = len(live.structure.nodes)
            after_edges = len(live.structure.edges)
            kind = live.kind.value
        log.info("Retrospect ok diagram %d (%d->%d nodes, %d->%d edges, kind=%s)",
                 diagram.number, before_nodes, after_nodes, before_edges, after_edges, kind)
        self._publish(state)

    # Bucket B

    def cancel_bucket_b(self) -> None:
        """Make any in-flight Bucket B rebuild stale; it will not be swapped in."""
        with self._changed:
            self._b_generation += 1

    def run_bucket_b(self, number: int | None = None) -> bool:
        """One Bucket B rebuild of `number` (default: the next requested diagram). True when the view was swapped."""
        with self._changed:
            if number is None:
                number = self._next_bucket_b()
            if number is None:
                return False
            self._b_requested.discard(number)
            diagram = self._diagrams.get(number)
            judge = self._b_judge
            if diagram is None or judge is None:
                return False
            source = diagram.copy()
            self._b_generation += 1
            generation = self._b_generation
        try:
            result = rebuild_from_clean(source, self._drawer, judge, generation, is_current=self._b_current)
        except Exception:
            log.exception("BucketB fail diagram %d", number)
            result = None
        if result is None:
            with self._changed:
                # Do not rebuild the same transcript again after a failure; new lines re-arm it.
                self._b_lines_done[number] = max(self._b_lines_done.get(number, 0), len(source.transcript))
            return False
        return self._swap_bucket_b(result, len(source.transcript))

    def _swap_bucket_b(self, result: BucketBResult, lines_used: int) -> bool:
        """Soft-swap: replace the live diagram's structure, kind and styles; keep its transcript and selection."""
        with self._changed:
            if result.generation != self._b_generation or self._b_stopped:
                log.info("BucketB drop diagram %d gen=%d: outdated", result.diagram_number, result.generation)
                return False
            live = self._diagrams.get(result.diagram_number)
            if live is None:
                return False
            before_nodes, before_edges = len(live.structure.nodes), len(live.structure.edges)
            live.set_kind(result.kind)
            live.update(result.structure)
            live.set_styles(result.node_styles, result.edge_styles)
            self._b_swaps += 1
            self._b_lines_done[live.number] = max(self._b_lines_done.get(live.number, 0), lines_used)
            if len(live.transcript) - lines_used >= self._b_min_new_lines:
                self._request_bucket_b(live.number)
            state = self._state(live.number)
        log.info("BucketB swap diagram %d gen=%d (%d->%d nodes, %d->%d edges, kind=%s)",
                 result.diagram_number, result.generation, before_nodes, len(result.structure.nodes),
                 before_edges, len(result.structure.edges), result.kind.value)
        self._publish(state)
        return True

    def _b_current(self, generation: int) -> bool:
        with self._changed:
            return generation == self._b_generation and not self._b_stopped

    def _note_bucket_b(self, diagram: Diagram) -> None:
        """Called under the lock after a live draw: arm Bucket B once enough new transcript arrived."""
        if not self._bucket_b:
            return
        self._b_last_line_at = time.monotonic()
        if len(diagram.transcript) - self._b_lines_done.get(diagram.number, 0) >= self._b_min_new_lines:
            self._request_bucket_b(diagram.number)

    def _request_bucket_b(self, number: int) -> None:
        if not self._b_requested:
            self._b_requested_at = time.monotonic()
        self._b_requested.add(number)
        self._changed.notify_all()

    def _next_bucket_b(self) -> int | None:
        return next((number for number in sorted(self._b_requested) if self._diagrams.get(number) is not None), None)

    def _bucket_b_due_in(self) -> float:
        """Seconds until a requested rebuild should start: after a quiet spell, or at the latest after max wait."""
        now = time.monotonic()
        quiet = self._b_last_line_at + self._b_debounce_s - now
        overdue = self._b_requested_at + self._b_max_wait_s - now
        return max(0.0, min(quiet, overdue))

    def _bucket_b_loop(self) -> None:
        while True:
            with self._changed:
                self._changed.wait_for(lambda: bool(self._b_requested) or not self._running)
                while self._running and (due := self._bucket_b_due_in()) > 0:
                    self._changed.wait(due)
                if not self._running:
                    return
            self._run_safely(self.run_bucket_b)

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
        with self._changed:
            swaps = self._b_swaps
        try:
            drawn = self._drawn(batch[0], lines)
        except Exception as exc:
            log.warning("Live sketch update skipped: %s", exc)
            self._retry(batch, batch[0].destination.number, str(exc) or type(exc).__name__)
            return
        if drawn is None:
            return
        with self._changed:
            stale = swaps != self._b_swaps
            if stale:
                self._pending[:0] = batch
                self._changed.notify_all()
        if stale:
            log.info("Live draw redone on top of the BucketB swap")
            return
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
            self._note_bucket_b(live)
            state = self._state(live.number)
        self._publish(state)

    def _note_draw(self, diagram: Diagram, texts: list[str]) -> None:
        """After a successful draw, schedule a retrospect (every N draws; N=1 means after each draw)."""
        number = diagram.number
        self._draws_since_retrospect[number] = self._draws_since_retrospect.get(number, 0) + 1
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
            # Retrospect right after a draw so the speaker sees the cleaned graph while the window is open.
            self._run_safely(self.retrospect_if_due)
            with self._changed:
                idle = not self._pending
            if idle:
                self._run_safely(self.summarize_if_due)

    @staticmethod
    def _run_safely(step: Callable[[], object]) -> None:
        try:
            step()
        except Exception:
            log.exception("Live sketch step failed")


