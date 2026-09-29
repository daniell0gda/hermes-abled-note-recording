import json

from listening_app.config import PayloadMode
from listening_app.hermes_client import (
    HermesSettings,
    build_request,
    classify,
    model_from_url,
    portion_event,
    Outcome,
)
from listening_app.models import HermesStatus, SegmentStatus, Source, TranscriptSegment

URL = "http://10.30.28.25:8642/p/simple-ng-proj/v1"
SEGMENT = TranscriptSegment(seq=12, source=Source.OTHERS, start=83.42, end=91.1,
                            wall_start="2026-09-28T10:15:03.420+02:00", text="No to ustalmy, że release idzie w piątek.",
                            language="pl", stt_model="groq/whisper-large-v3-turbo", status=SegmentStatus.OK,
                            hermes_status=HermesStatus.QUEUED)
EVENT = portion_event("2026-09-28T10-13-40_a1b2", SEGMENT)


def settings(mode: PayloadMode, raw_path: str = "") -> HermesSettings:
    return HermesSettings(URL, "secret", mode, model_from_url(URL), raw_path, 10)


def test_raw_mode_posts_the_portion_itself() -> None:
    request = build_request(EVENT, settings(PayloadMode.RAW, raw_path="/ingest/"))

    assert request.url == f"{URL}/ingest"
    assert request.body == {
        "type": "transcript_portion", "session_id": "2026-09-28T10-13-40_a1b2", "seq": 12, "source": "others",
        "start": 83.42, "end": 91.1, "wall_start": "2026-09-28T10:15:03.420+02:00",
        "text": "No to ustalmy, że release idzie w piątek.", "language": "pl",
    }


def test_responses_mode_keeps_the_meeting_in_one_conversation() -> None:
    request = build_request(EVENT, settings(PayloadMode.RESPONSES))

    assert request.url == f"{URL}/responses"
    assert request.body["conversation"] == "2026-09-28T10-13-40_a1b2"
    assert request.body["model"] == "simple-ng-proj"
    assert request.body["background"] is True
    assert json.loads(request.body["input"])["text"] == SEGMENT.text


def test_chat_mode_sends_one_user_message_with_the_session_header() -> None:
    request = build_request(EVENT, settings(PayloadMode.CHAT))

    assert request.url == f"{URL}/chat/completions"
    assert request.body["messages"][0]["role"] == "user"
    assert request.headers["X-Hermes-Session-Id"] == "2026-09-28T10-13-40_a1b2"


def test_every_request_carries_auth_and_a_stable_idempotency_key() -> None:
    for mode in PayloadMode:
        headers = build_request(EVENT, settings(mode)).headers
        assert headers["Authorization"] == "Bearer secret"
        assert headers["Idempotency-Key"] == "2026-09-28T10-13-40_a1b2:12"


def test_model_defaults_to_the_profile_name() -> None:
    assert model_from_url(URL) == "simple-ng-proj"
    assert model_from_url("http://host:8642/v1") == "hermes-agent"
    assert model_from_url("http://host:8642/p/default/v1") == "hermes-agent"


def test_status_codes_are_classified() -> None:
    assert classify(200) is Outcome.DELIVERED
    assert classify(202) is Outcome.DELIVERED
    assert classify(422) is Outcome.REJECTED
    assert classify(401) is Outcome.RETRY
    assert classify(429) is Outcome.RETRY
    assert classify(503) is Outcome.RETRY
