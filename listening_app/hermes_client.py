"""Hermes streaming: chunking, payload builders, ordered delivery through a persistent outbox, retry with backoff."""

import json
import logging
import re
import threading
from collections import deque
from collections.abc import Callable, Iterable, Mapping, Sequence
from dataclasses import dataclass, field
from enum import StrEnum
from pathlib import Path
from typing import Any, Protocol

import httpx
from pydantic import BaseModel, ConfigDict, ValidationError

from listening_app.config import AppConfig, PayloadMode
from listening_app.files import append_line, read_lines
from listening_app.models import HermesStatus, TranscriptSegment

log = logging.getLogger(__name__)

DEFAULT_MODEL = "hermes-agent"
MAX_BACKOFF_S = 60.0
PERMANENT_REJECTIONS = frozenset({400, 409, 413, 415, 422})
"""Answers a retry cannot fix. Everything else that is not 2xx (401, 404, 429, 5xx, ...) is retried."""

_PROFILE_PATTERN = re.compile(r"/p/([^/]+)/")
_DEFAULT_PROFILE = "default"
_CLOSE_TIMEOUT_S = 2.0
_PORTION_FIELDS = {"seq", "source", "start", "end", "wall_start", "text", "language"}


class EventKind(StrEnum):
    SESSION_START = "session_start"
    TRANSCRIPT_CHUNK = "transcript_chunk"
    SESSION_END = "session_end"


class HermesEvent(BaseModel):
    model_config = ConfigDict(frozen=True)

    kind: EventKind
    session_id: str
    seqs: tuple[int, ...] = ()
    """Seq of every transcript portion the event carries."""
    fields: dict[str, Any]

    @property
    def key(self) -> str:
        """Stable per event, used as the Idempotency-Key so a resend is never processed twice.

        Every portion belongs to exactly one chunk, so a chunk is keyed by the seq of its first portion.
        """
        return f"{self.session_id}:{self.seqs[0] if self.seqs else self.kind}"

    def to_raw(self) -> dict[str, Any]:
        return {"type": self.kind.value, "session_id": self.session_id, **self.fields}


def session_start_event(session_id: str, started_at: str, devices: Mapping[str, str], stt_model: str,
                        language: str) -> HermesEvent:
    fields = {"started_at": started_at, "devices": dict(devices), "stt_model": stt_model, "language": language}
    return HermesEvent(kind=EventKind.SESSION_START, session_id=session_id, fields=fields)


def chunk_event(session_id: str, segments: Sequence[TranscriptSegment]) -> HermesEvent:
    fields = {"portions": [_portion(segment) for segment in segments]}
    return HermesEvent(kind=EventKind.TRANSCRIPT_CHUNK, session_id=session_id,
                       seqs=tuple(segment.seq for segment in segments), fields=fields)


def _portion(segment: TranscriptSegment) -> dict[str, Any]:
    return segment.model_dump(mode="json", include=_PORTION_FIELDS)


class ChunkBuffer:
    """Holds transcript portions until their JSON adds up to `limit_bytes`, then releases them as one chunk."""

    def __init__(self, limit_bytes: int) -> None:
        self._limit_bytes = limit_bytes
        self._segments: list[TranscriptSegment] = []
        self._size = 0

    def add(self, segment: TranscriptSegment) -> list[TranscriptSegment]:
        """The full chunk once the limit is reached, otherwise an empty list."""
        self._segments.append(segment)
        self._size += len(json.dumps(_portion(segment), ensure_ascii=False).encode())
        return self.flush() if self._size >= self._limit_bytes else []

    def flush(self) -> list[TranscriptSegment]:
        """Release whatever is held, full or not."""
        chunk, self._segments, self._size = self._segments, [], 0
        return chunk


def session_end_event(session_id: str, ended_at: str, segments: int, failed: int, duration_s: int) -> HermesEvent:
    fields = {"ended_at": ended_at, "segments": segments, "failed": failed, "duration_s": duration_s}
    return HermesEvent(kind=EventKind.SESSION_END, session_id=session_id, fields=fields)


@dataclass(frozen=True)
class HermesSettings:
    url: str
    api_key: str = field(repr=False)
    payload_mode: PayloadMode
    model: str
    raw_path: str
    timeout_s: float

    @classmethod
    def from_config(cls, config: AppConfig) -> "HermesSettings":
        hermes = config.hermes
        return cls(hermes.url, config.hermes_key().value, hermes.payload_mode, hermes.model or model_from_url(hermes.url),
                   hermes.raw_path, hermes.timeout_s)


def model_from_url(url: str) -> str:
    """Hermes names each profile's model after the profile (/p/<profile>/v1)."""
    match = _PROFILE_PATTERN.search(url + "/")
    if match is None or match.group(1) == _DEFAULT_PROFILE:
        return DEFAULT_MODEL
    return match.group(1)


@dataclass(frozen=True)
class HttpRequest:
    url: str
    headers: dict[str, str] = field(repr=False)
    body: dict[str, Any]


def build_request(event: HermesEvent, settings: HermesSettings) -> HttpRequest:
    headers = {"Idempotency-Key": event.key}
    if settings.api_key:
        headers["Authorization"] = f"Bearer {settings.api_key}"
    return _BUILDERS[settings.payload_mode](event, settings, headers)


def _raw_request(event: HermesEvent, settings: HermesSettings, headers: dict[str, str]) -> HttpRequest:
    url = f"{settings.url}/{settings.raw_path.strip('/')}" if settings.raw_path.strip("/") else settings.url
    return HttpRequest(url, headers, event.to_raw())


def _chat_request(event: HermesEvent, settings: HermesSettings, headers: dict[str, str]) -> HttpRequest:
    body = {
        "model": settings.model,
        "messages": [{"role": "user", "content": _as_message(event)}],
        "stream": False,
        "metadata": _metadata(event),
    }
    return HttpRequest(f"{settings.url}/chat/completions", {**headers, "X-Hermes-Session-Id": event.session_id}, body)


def _responses_request(event: HermesEvent, settings: HermesSettings, headers: dict[str, str]) -> HttpRequest:
    body = {
        "model": settings.model,
        "input": _as_message(event),
        "conversation": event.session_id,
        "store": True,
        "background": True,
        "metadata": _metadata(event),
    }
    return HttpRequest(f"{settings.url}/responses", headers, body)


_BUILDERS: dict[PayloadMode, Callable[[HermesEvent, HermesSettings, dict[str, str]], HttpRequest]] = {
    PayloadMode.RAW: _raw_request,
    PayloadMode.CHAT: _chat_request,
    PayloadMode.RESPONSES: _responses_request,
}


def _as_message(event: HermesEvent) -> str:
    return json.dumps(event.to_raw(), ensure_ascii=False)


def _metadata(event: HermesEvent) -> dict[str, str]:
    return {"session_id": event.session_id, "type": event.kind.value, "key": event.key}


class JournalOp(StrEnum):
    ENQUEUED = "enqueued"
    DELIVERED = "delivered"
    FAILED = "failed"


class JournalEntry(BaseModel):
    op: JournalOp
    key: str
    event: HermesEvent | None = None
    detail: str | None = None


def read_journal(path: Path) -> list[JournalEntry]:
    entries = []
    for line in read_lines(path):
        try:
            entries.append(JournalEntry.model_validate_json(line))
        except ValidationError:
            log.warning("Skipping unreadable line in %s", path.name)
    return entries


def pending_events(path: Path) -> list[HermesEvent]:
    """Events journaled as enqueued but never delivered or failed, in their original order."""
    entries = read_journal(path)
    done = {entry.key for entry in entries if entry.op is not JournalOp.ENQUEUED}
    pending: dict[str, HermesEvent] = {}
    for entry in entries:
        if entry.op is JournalOp.ENQUEUED and entry.event and entry.key not in done:
            pending.setdefault(entry.key, entry.event)
    return list(pending.values())


def delivery_statuses(path: Path) -> dict[int, HermesStatus]:
    """Hermes status of every transcript portion in a session's journal, by seq."""
    statuses: dict[int, HermesStatus] = {}
    seqs_by_key: dict[str, tuple[int, ...]] = {}
    for entry in read_journal(path):
        if entry.op is JournalOp.ENQUEUED and entry.event and entry.event.seqs:
            seqs_by_key[entry.key] = entry.event.seqs
            for seq in entry.event.seqs:
                statuses.setdefault(seq, HermesStatus.QUEUED)
        elif entry.key in seqs_by_key:
            status = HermesStatus.DELIVERED if entry.op is JournalOp.DELIVERED else HermesStatus.FAILED
            statuses.update(dict.fromkeys(seqs_by_key[entry.key], status))
    return statuses


class Outbox:
    """Ordered queue of Hermes events. Every change is journaled to the event's session file first."""

    def __init__(self) -> None:
        self._queue: deque[tuple[HermesEvent, Path]] = deque()

    def push(self, event: HermesEvent, journal: Path) -> None:
        append_line(journal, JournalEntry(op=JournalOp.ENQUEUED, key=event.key, event=event).model_dump_json())
        self._queue.append((event, journal))

    def head(self) -> HermesEvent | None:
        return self._queue[0][0] if self._queue else None

    def complete_head(self, op: JournalOp, detail: str) -> Path:
        """Remove the head event, journaling the result. Returns the journal it belongs to."""
        event, journal = self._queue.popleft()
        append_line(journal, JournalEntry(op=op, key=event.key, detail=detail).model_dump_json())
        return journal

    def pending(self, session_id: str | None = None) -> int:
        return sum(1 for event, _ in self._queue if session_id is None or event.session_id == session_id)

    def restore(self, journal: Path) -> int:
        """Queue the undelivered events of a journal left by an earlier run."""
        queued = {event.key for event, _ in self._queue}
        restored = [event for event in pending_events(journal) if event.key not in queued]
        self._queue.extend((event, journal) for event in restored)
        return len(restored)


class Outcome(StrEnum):
    DELIVERED = "delivered"
    REJECTED = "rejected"
    RETRY = "retry"


def classify(status_code: int) -> Outcome:
    if 200 <= status_code < 300:
        return Outcome.DELIVERED
    if status_code in PERMANENT_REJECTIONS:
        return Outcome.REJECTED
    return Outcome.RETRY


def backoff_delay(failures: int) -> float:
    return float(min(2 ** (failures - 1), MAX_BACKOFF_S))


Transport = Callable[[HttpRequest, float], int]
"""Sends a request and returns the HTTP status code; raises on network errors."""


class HttpTransport:
    def __init__(self) -> None:
        self._client = httpx.Client()

    def __call__(self, request: HttpRequest, timeout_s: float) -> int:
        response = self._client.post(request.url, headers=request.headers, json=request.body, timeout=timeout_s)
        log.debug("Hermes answered %d: %s", response.status_code, response.text[:300])
        return response.status_code


class HermesListener(Protocol):
    """Called from the sender thread."""

    def hermes_problem(self, title: str, message: str) -> None: ...

    def hermes_recovered(self) -> None: ...

    def session_drained(self, journal: Path) -> None:
        """Every event of the session owning `journal` has been delivered or has failed."""


class HermesClient:
    """Sends events to Hermes one at a time, in order, on a background thread.

    An event leaves the outbox only on a 2xx answer (delivered) or a permanent rejection (failed).
    Anything else is retried with backoff, so nothing is lost or reordered while Hermes is down.
    The Idempotency-Key lets Hermes ignore a resend of a request it already processed.
    """

    def __init__(self, settings: HermesSettings, enabled: bool, listener: HermesListener,
                 transport: Transport | None = None) -> None:
        self._settings = settings
        self._enabled = enabled
        self._listener = listener
        self._transport = transport or HttpTransport()
        self._outbox = Outbox()
        self._cond = threading.Condition()
        self._closed = False
        self._wake = False
        self._failures = 0
        self._last_rejection: str | None = None
        self._thread = threading.Thread(target=self._run, name="hermes", daemon=True)

    def start(self) -> None:
        self._thread.start()

    def close(self) -> None:
        with self._cond:
            self._closed = True
            self._cond.notify_all()
        self._thread.join(timeout=_CLOSE_TIMEOUT_S)

    @property
    def enabled(self) -> bool:
        with self._cond:
            return self._enabled

    def set_enabled(self, enabled: bool) -> None:
        """Pause or resume streaming. While paused, queued events wait in the outbox."""
        with self._cond:
            self._enabled = enabled
            self._wake = True
            self._cond.notify_all()

    def update_settings(self, settings: HermesSettings) -> None:
        with self._cond:
            self._settings = settings
            self._wake = True
            self._cond.notify_all()

    def submit(self, event: HermesEvent, journal: Path) -> None:
        with self._cond:
            self._outbox.push(event, journal)
            self._cond.notify_all()

    def restore(self, journals: Iterable[Path]) -> int:
        with self._cond:
            restored = sum(self._outbox.restore(journal) for journal in journals)
            self._cond.notify_all()
        return restored

    def pending(self, session_id: str | None = None) -> int:
        with self._cond:
            return self._outbox.pending(session_id)

    def wait_until_sent(self, session_id: str, timeout_s: float) -> bool:
        with self._cond:
            return self._cond.wait_for(lambda: self._outbox.pending(session_id) == 0, timeout=timeout_s)

    def _run(self) -> None:
        while (job := self._next_job()) is not None:
            event, settings = job
            outcome, detail = self._deliver(event, settings)
            if outcome is Outcome.RETRY:
                self._back_off(detail)
            else:
                self._complete(event, outcome, detail)

    def _next_job(self) -> tuple[HermesEvent, HermesSettings] | None:
        with self._cond:
            while not self._closed:
                event = self._outbox.head() if self._enabled else None
                if event is not None:
                    return event, self._settings
                self._cond.wait()
            return None

    def _deliver(self, event: HermesEvent, settings: HermesSettings) -> tuple[Outcome, str]:
        try:
            status_code = self._transport(build_request(event, settings), settings.timeout_s)
        except Exception as exc:
            return Outcome.RETRY, f"{type(exc).__name__}: {exc}"
        return classify(status_code), f"HTTP {status_code}"

    def _complete(self, event: HermesEvent, outcome: Outcome, detail: str) -> None:
        op = JournalOp.DELIVERED if outcome is Outcome.DELIVERED else JournalOp.FAILED
        with self._cond:
            journal = self._outbox.complete_head(op, detail)
            drained = self._outbox.pending(event.session_id) == 0
            self._cond.notify_all()
        log.info("Hermes %s %s (%s)", op, event.key, detail)
        self._report_recovery()
        if outcome is Outcome.REJECTED:
            self._report_rejection(event, detail)
        if drained:
            self._listener.session_drained(journal)

    def _back_off(self, detail: str) -> None:
        self._failures += 1
        if self._failures == 1:
            log.warning("Hermes unreachable: %s", detail)
            self._listener.hermes_problem("Hermes unreachable",
                                          f"{detail}. Transcript portions are queued and will be resent.")
        with self._cond:
            self._wake = False
            self._cond.wait_for(lambda: self._closed or self._wake, timeout=backoff_delay(self._failures))

    def _report_recovery(self) -> None:
        if self._failures:
            self._failures = 0
            log.info("Hermes reachable again")
            self._listener.hermes_recovered()

    def _report_rejection(self, event: HermesEvent, detail: str) -> None:
        log.error("Hermes rejected %s: %s", event.key, detail)
        if detail != self._last_rejection:
            self._last_rejection = detail
            self._listener.hermes_problem("Hermes rejected a transcript portion",
                                          f"{detail} - check the hermes settings in the config.")
