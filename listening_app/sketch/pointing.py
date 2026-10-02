"""Pointing with the mouse: held still for a moment, it points at the element under it, which resolves "this one"."""

import math
from collections import deque
from dataclasses import dataclass, replace

HISTORY_S = 120.0
COVERAGE_TOLERANCE_S = 0.05


@dataclass(frozen=True)
class Box:
    """An element's bounding box in window pixels."""

    x: float
    y: float
    width: float
    height: float

    def distance(self, x: float, y: float) -> float:
        """0 inside the box, else the distance to its nearest edge."""
        dx = max(self.x - x, 0.0, x - (self.x + self.width))
        dy = max(self.y - y, 0.0, y - (self.y + self.height))
        return math.hypot(dx, dy)


@dataclass(frozen=True)
class _Sample:
    time: float
    x: float
    y: float


@dataclass(frozen=True)
class _Pointing:
    element: str
    start: float
    end: float = math.inf


class PointerTracker:
    """Keeps the last `window_s` of mouse positions (epoch seconds, window pixels).

    When all of them stay within `radius_px` of their centroid, the element under the centroid, or else the nearest
    one within the radius, is pointed at. Pointing counts from the moment the mouse came to rest.
    """

    def __init__(self, radius_px: float, window_s: float) -> None:
        self._radius = radius_px
        self._window = window_s
        self._boxes: dict[str, Box] = {}
        self._samples: deque[_Sample] = deque()
        self._current: _Pointing | None = None
        self._history: deque[_Pointing] = deque()

    @property
    def pointed(self) -> str | None:
        return self._current.element if self._current is not None else None

    def set_boxes(self, boxes: dict[str, Box]) -> None:
        self._boxes = dict(boxes)

    def move(self, time: float, x: float, y: float) -> str | None:
        """Record a mouse position and return the element pointed at now."""
        self._samples.append(_Sample(time, x, y))
        while len(self._samples) > 1 and self._samples[1].time <= time - self._window:
            self._samples.popleft()
        self._point_at(self._resting_element(time), time)
        return self.pointed

    def leave(self, time: float) -> None:
        """The mouse left the diagram."""
        self._samples.clear()
        self._point_at(None, time)

    def pointed_during(self, start: float, end: float) -> str | None:
        """The element pointed at for the longest part of `start`..`end`, if any."""
        overlaps: dict[str, float] = {}
        candidates = [*self._history, *([self._current] if self._current is not None else [])]
        for pointing in candidates:
            overlap = min(end, pointing.end) - max(start, pointing.start)
            if overlap > 0:
                overlaps[pointing.element] = overlaps.get(pointing.element, 0.0) + overlap
        return max(overlaps, key=lambda element: overlaps[element]) if overlaps else None

    def _resting_element(self, time: float) -> str | None:
        if self._samples[0].time > time - self._window + COVERAGE_TOLERANCE_S:
            return None
        cx = sum(sample.x for sample in self._samples) / len(self._samples)
        cy = sum(sample.y for sample in self._samples) / len(self._samples)
        if any(math.hypot(sample.x - cx, sample.y - cy) > self._radius for sample in self._samples):
            return None
        return self._element_at(cx, cy)

    def _element_at(self, x: float, y: float) -> str | None:
        distances = {element: box.distance(x, y) for element, box in self._boxes.items()}
        nearest = min(distances, key=lambda element: distances[element], default=None)
        return nearest if nearest is not None and distances[nearest] <= self._radius else None

    def _point_at(self, element: str | None, time: float) -> None:
        if element == self.pointed:
            return
        rested_since = time - self._window
        if self._current is not None:
            ended = rested_since if element is not None else time
            self._history.append(replace(self._current, end=max(self._current.start, ended)))
        self._current = _Pointing(element, rested_since) if element is not None else None
        while self._history and self._history[0].end < time - HISTORY_S:
            self._history.popleft()
