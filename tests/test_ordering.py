import random
import threading
import time
from datetime import datetime

import numpy as np

from listening_app.config import Language
from listening_app.models import CapturedSegment, SegmentStatus, Source
from listening_app.transcriber import MAX_ATTEMPTS, ReorderBuffer, SttSettings, Transcriber, Transcription

SETTINGS = SttSettings("groq/whisper-large-v3-turbo", Language.POLISH, "", "", workers=4)


def segment(seq: int, source: Source, source_index: int) -> CapturedSegment:
    audio = np.zeros(160 * (seq + 1), dtype=np.float32)
    return CapturedSegment(seq, source, source_index, float(seq), seq + 0.5, datetime.now().astimezone(), audio)


def test_reorder_buffer_releases_items_in_index_order() -> None:
    buffer: ReorderBuffer[int] = ReorderBuffer()
    indices = list(range(50))
    random.Random(3).shuffle(indices)

    released = [item for index in indices for item in buffer.add(index, index)]

    assert released == list(range(50))


def test_reorder_buffer_holds_items_until_the_gap_is_filled() -> None:
    buffer: ReorderBuffer[str] = ReorderBuffer()

    assert buffer.add(1, "b") == []
    assert buffer.add(2, "c") == []
    assert buffer.add(0, "a") == ["a", "b", "c"]


def test_transcriptions_are_emitted_in_order_per_source_even_when_finishing_shuffled() -> None:
    rng = random.Random(7)
    delays = {160 * (seq + 1) * 2 + 44: rng.uniform(0, 0.03) for seq in range(40)}

    def slow_transcribe(wav: bytes, _: SttSettings) -> str:
        time.sleep(delays[len(wav)])
        return "tekst"

    emitted: list[Transcription] = []
    lock = threading.Lock()

    def collect(transcription: Transcription) -> None:
        with lock:
            emitted.append(transcription)

    transcriber = Transcriber(SETTINGS, collect, transcribe=slow_transcribe)
    counters = {Source.ME: 0, Source.OTHERS: 0}
    for seq in range(40):
        source = Source.ME if rng.random() < 0.5 else Source.OTHERS
        transcriber.submit(segment(seq, source, counters[source]))
        counters[source] += 1
    transcriber.close()

    assert len(emitted) == 40
    for source in Source:
        indices = [t.segment.source_index for t in emitted if t.segment.source == source]
        assert indices == list(range(counters[source]))


def test_failed_attempts_are_retried_with_exponential_backoff() -> None:
    calls: list[int] = []
    sleeps: list[float] = []

    def flaky(_: bytes, __: SttSettings) -> str:
        calls.append(1)
        if len(calls) < MAX_ATTEMPTS:
            raise ConnectionError("temporary")
        return " Dzień dobry. "

    emitted: list[Transcription] = []
    transcriber = Transcriber(SETTINGS, emitted.append, transcribe=flaky, sleep=sleeps.append)
    transcriber.submit(segment(0, Source.ME, 0))
    transcriber.close()

    assert emitted[0].status is SegmentStatus.OK
    assert emitted[0].text == "Dzień dobry."
    assert sleeps == [1.0, 2.0]


def test_segment_is_marked_stt_failed_after_the_last_attempt() -> None:
    def broken(_: bytes, __: SttSettings) -> str:
        raise ConnectionError("down")

    emitted: list[Transcription] = []
    transcriber = Transcriber(SETTINGS, emitted.append, transcribe=broken, sleep=lambda _: None)
    transcriber.submit(segment(0, Source.OTHERS, 0))
    transcriber.submit(segment(1, Source.OTHERS, 1))
    transcriber.close()

    assert [t.status for t in emitted] == [SegmentStatus.STT_FAILED, SegmentStatus.STT_FAILED]


def test_blank_transcription_is_marked_empty() -> None:
    emitted: list[Transcription] = []
    transcriber = Transcriber(SETTINGS, emitted.append, transcribe=lambda _, __: "  ")
    transcriber.submit(segment(0, Source.ME, 0))
    transcriber.close()

    assert emitted[0].status is SegmentStatus.EMPTY
