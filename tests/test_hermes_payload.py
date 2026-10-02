import json

from listening_app.config import PayloadMode
from listening_app.hermes_client import (
    ChunkBuffer,
    HermesSettings,
    build_request,
    chunk_event,
    classify,
    model_from_url,
    Outcome,
)
from listening_app.models import HermesStatus, SegmentStatus, Source, TranscriptSegment

URL = "http://10.30.28.25:8642/p/simple-ng-proj/v1"
SEGMENT = TranscriptSegment(seq=12, source=Source.OTHERS, start=83.42, end=91.1,
                            wall_start="2026-09-28T10:15:03.420+02:00", text="No to ustalmy, że release idzie w piątek.",
                            language="pl", stt_model="groq/whisper-large-v3-turbo", status=SegmentStatus.OK,
                            hermes_status=HermesStatus.QUEUED)
NEXT_SEGMENT = SEGMENT.model_copy(update={"seq": 13, "source": Source.ME, "text": "Zgoda."})
EVENT = chunk_event("2026-09-28T10-13-40_a1b2", [SEGMENT])


def settings(mode: PayloadMode, raw_path: str = "", system_prompt: str = "") -> HermesSettings:
    return HermesSettings(URL, "secret", mode, model_from_url(URL), raw_path, 10, system_prompt)


def segment_with_text(seq: int, text: str) -> TranscriptSegment:
    return SEGMENT.model_copy(update={"seq": seq, "text": text})


def test_raw_mode_posts_the_chunk_itself() -> None:
    event = chunk_event("2026-09-28T10-13-40_a1b2", [SEGMENT, NEXT_SEGMENT])

    request = build_request(event, settings(PayloadMode.RAW, raw_path="/ingest/"))

    assert request.url == f"{URL}/ingest"
    assert request.body == {
        "type": "transcript_chunk", "session_id": "2026-09-28T10-13-40_a1b2", "portions": [
            {"seq": 12, "source": "others", "start": 83.42, "end": 91.1, "wall_start": "2026-09-28T10:15:03.420+02:00",
             "text": "No to ustalmy, że release idzie w piątek.", "language": "pl"},
            {"seq": 13, "source": "me", "start": 83.42, "end": 91.1, "wall_start": "2026-09-28T10:15:03.420+02:00",
             "text": "Zgoda.", "language": "pl"},
        ],
    }
    assert request.headers["Idempotency-Key"] == "2026-09-28T10-13-40_a1b2:12"


def test_responses_mode_keeps_the_meeting_in_one_conversation() -> None:
    request = build_request(EVENT, settings(PayloadMode.RESPONSES))

    assert request.url == f"{URL}/responses"
    assert request.body["conversation"] == "2026-09-28T10-13-40_a1b2"
    assert request.body["model"] == "simple-ng-proj"
    assert request.body["background"] is True
    assert json.loads(request.body["input"])["portions"][0]["text"] == SEGMENT.text


def test_chat_mode_sends_each_event_alone_without_a_hermes_session() -> None:
    request = build_request(EVENT, settings(PayloadMode.CHAT))

    assert request.url == f"{URL}/chat/completions"
    assert request.body["messages"] == [{"role": "user", "content": json.dumps(EVENT.to_raw(), ensure_ascii=False)}]
    assert "X-Hermes-Session-Id" not in request.headers


def test_chat_mode_sends_the_system_prompt_for_the_events_session() -> None:
    request = build_request(EVENT, settings(PayloadMode.CHAT, system_prompt="Append this event to sources/{session_id}.md"))

    system, user = request.body["messages"]
    assert system == {"role": "system", "content": "Append this event to sources/2026-09-28T10-13-40_a1b2.md"}
    assert user["role"] == "user"


def test_every_request_carries_auth_and_a_stable_idempotency_key() -> None:
    for mode in PayloadMode:
        headers = build_request(EVENT, settings(mode)).headers
        assert headers["Authorization"] == "Bearer secret"
        assert headers["Idempotency-Key"] == "2026-09-28T10-13-40_a1b2:12"


def test_model_defaults_to_the_profile_name() -> None:
    assert model_from_url(URL) == "simple-ng-proj"
    assert model_from_url("http://host:8642/v1") == "hermes-agent"
    assert model_from_url("http://host:8642/p/default/v1") == "hermes-agent"


def test_portions_are_held_until_their_utf8_json_fills_the_chunk() -> None:
    buffer = ChunkBuffer(limit_bytes=1024)
    first, second = segment_with_text(1, "ż" * 300), segment_with_text(2, "ż" * 300)

    assert buffer.add(first) == []
    assert buffer.add(second) == [first, second]
    assert buffer.flush() == []


def test_a_zero_chunk_size_sends_every_portion_on_its_own() -> None:
    buffer = ChunkBuffer(limit_bytes=0)

    assert buffer.add(SEGMENT) == [SEGMENT]
    assert buffer.add(NEXT_SEGMENT) == [NEXT_SEGMENT]


def test_flush_releases_a_partly_filled_chunk() -> None:
    buffer = ChunkBuffer(limit_bytes=1024)
    buffer.add(SEGMENT)

    assert buffer.flush() == [SEGMENT]
    assert buffer.flush() == []


def test_status_codes_are_classified() -> None:
    assert classify(200) is Outcome.DELIVERED
    assert classify(202) is Outcome.DELIVERED
    assert classify(422) is Outcome.REJECTED
    assert classify(401) is Outcome.RETRY
    assert classify(429) is Outcome.RETRY
    assert classify(503) is Outcome.RETRY
