from collections.abc import Iterator
from pathlib import Path

import httpx
import pytest

from listening_app import hermes_client
from listening_app.config import PayloadMode
from listening_app.hermes_client import (
    HermesClient,
    HermesEvent,
    HermesSettings,
    HttpRequest,
    JournalOp,
    Outbox,
    delivery_statuses,
    portion_event,
    session_start_event,
)
from listening_app.models import HermesStatus, SegmentStatus, Source, TranscriptSegment

SESSION = "2026-09-28T10-13-40_a1b2"
SETTINGS = HermesSettings("http://hermes.test/v1", "key", PayloadMode.RAW, "hermes-agent", "", 1)


def portion(seq: int) -> HermesEvent:
    segment = TranscriptSegment(seq=seq, source=Source.ME, start=seq, end=seq + 1.0, wall_start="2026-09-28T10:13:41+02:00",
                                text=f"zdanie {seq}", language="pl", stt_model="groq/whisper-large-v3-turbo",
                                status=SegmentStatus.OK, hermes_status=HermesStatus.QUEUED)
    return portion_event(SESSION, segment)


class FakeHermes:
    """Transport that answers from a script of status codes; None means a connection error."""

    def __init__(self, script: list[int | None] | None = None, default: int | None = 200) -> None:
        self._script = list(script or [])
        self._default = default
        self.accepted: list[str] = []

    def __call__(self, request: HttpRequest, timeout_s: float) -> int:
        answer = self._script.pop(0) if self._script else self._default
        if answer is None:
            raise httpx.ConnectError("connection refused")
        if 200 <= answer < 300:
            self.accepted.append(request.headers["Idempotency-Key"])
        return answer


class RecordingListener:
    def __init__(self) -> None:
        self.problems: list[str] = []
        self.recoveries = 0
        self.drained: list[Path] = []

    def hermes_problem(self, title: str, message: str) -> None:
        self.problems.append(title)

    def hermes_recovered(self) -> None:
        self.recoveries += 1

    def session_drained(self, journal: Path) -> None:
        self.drained.append(journal)


@pytest.fixture(autouse=True)
def fast_backoff(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(hermes_client, "backoff_delay", lambda failures: 0.01)


@pytest.fixture
def journal(tmp_path: Path) -> Path:
    return tmp_path / f"{SESSION}.outbox.jsonl"


def running_client(transport: FakeHermes, listener: RecordingListener, enabled: bool = True) -> Iterator[HermesClient]:
    client = HermesClient(SETTINGS, enabled, listener, transport)
    client.start()
    yield client
    client.close()


def test_outbox_journal_restores_undelivered_events_in_order(journal: Path) -> None:
    outbox = Outbox()
    for seq in range(3):
        outbox.push(portion(seq), journal)
    outbox.complete_head(JournalOp.DELIVERED, "HTTP 200")

    restored = Outbox()
    assert restored.restore(journal) == 2
    assert restored.head() == portion(1)
    assert restored.pending(SESSION) == 2


def test_delivery_statuses_follow_the_journal(journal: Path) -> None:
    outbox = Outbox()
    outbox.push(session_start_event(SESSION, "2026-09-28T10:13:40+02:00", {}, "m", "pl"), journal)
    for seq in range(3):
        outbox.push(portion(seq), journal)
    outbox.complete_head(JournalOp.DELIVERED, "HTTP 200")
    outbox.complete_head(JournalOp.DELIVERED, "HTTP 200")
    outbox.complete_head(JournalOp.FAILED, "HTTP 422")

    assert delivery_statuses(journal) == {0: HermesStatus.DELIVERED, 1: HermesStatus.FAILED, 2: HermesStatus.QUEUED}


def test_portions_queued_while_hermes_is_down_arrive_in_order_without_duplicates(journal: Path) -> None:
    transport = FakeHermes(script=[None, None, 503, None])
    listener = RecordingListener()
    for client in running_client(transport, listener):
        for seq in range(5):
            client.submit(portion(seq), journal)
        assert client.wait_until_sent(SESSION, timeout_s=5)

    assert transport.accepted == [f"{SESSION}:{seq}" for seq in range(5)]
    assert listener.problems == ["Hermes unreachable"]
    assert listener.recoveries == 1
    assert listener.drained == [journal]
    assert set(delivery_statuses(journal).values()) == {HermesStatus.DELIVERED}


def test_undelivered_portions_are_resent_after_a_restart(journal: Path) -> None:
    first_run = FakeHermes(script=[200], default=None)
    for client in running_client(first_run, RecordingListener()):
        for seq in range(3):
            client.submit(portion(seq), journal)
        client.wait_until_sent(SESSION, timeout_s=0.3)

    second_run = FakeHermes()
    for client in running_client(second_run, RecordingListener()):
        assert client.restore([journal]) == 2
        assert client.wait_until_sent(SESSION, timeout_s=5)

    assert first_run.accepted == [f"{SESSION}:0"]
    assert second_run.accepted == [f"{SESSION}:1", f"{SESSION}:2"]


def test_rejected_portion_is_marked_failed_and_the_queue_moves_on(journal: Path) -> None:
    transport = FakeHermes(script=[422])
    listener = RecordingListener()
    for client in running_client(transport, listener):
        client.submit(portion(0), journal)
        client.submit(portion(1), journal)
        assert client.wait_until_sent(SESSION, timeout_s=5)

    assert delivery_statuses(journal) == {0: HermesStatus.FAILED, 1: HermesStatus.DELIVERED}
    assert listener.problems == ["Hermes rejected a transcript portion"]


def test_nothing_is_sent_while_streaming_is_off(journal: Path) -> None:
    transport = FakeHermes()
    for client in running_client(transport, RecordingListener(), enabled=False):
        client.submit(portion(0), journal)
        assert not client.wait_until_sent(SESSION, timeout_s=0.2)
        assert transport.accepted == []

        client.set_enabled(True)
        assert client.wait_until_sent(SESSION, timeout_s=5)

    assert transport.accepted == [f"{SESSION}:0"]
