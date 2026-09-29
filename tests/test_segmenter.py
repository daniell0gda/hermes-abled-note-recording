import numpy as np
import pytest

from listening_app.config import SegmentationConfig
from listening_app.models import SAMPLE_RATE, Audio
from listening_app.segmenter import FRAME_MS, Segmenter, SegmenterSettings, SpeechSegment

FRAME_S = FRAME_MS / 1000
PADDING_S = 7 * FRAME_S  # 200 ms rounded up to whole frames


class EnergyDetector:
    """Stands in for Silero: a frame is speech when it is loud."""

    def probability(self, frame: Audio) -> float:
        return 1.0 if float(np.sqrt(np.mean(np.square(frame)))) > 0.02 else 0.0

    def reset(self) -> None:
        pass


def speech(seconds: float, seed: int = 0) -> Audio:
    return np.random.default_rng(seed).normal(0, 0.1, round(seconds * SAMPLE_RATE)).astype(np.float32)


def silence(seconds: float) -> Audio:
    return np.zeros(round(seconds * SAMPLE_RATE), dtype=np.float32)


def run(audio: Audio) -> list[SpeechSegment]:
    segmenter = Segmenter(SegmenterSettings.from_config(SegmentationConfig()), EnergyDetector())
    segments = segmenter.feed(audio)
    last = segmenter.flush()
    return segments + ([last] if last else [])


def times(segment: SpeechSegment) -> tuple[float, float]:
    return segment.start_sample / SAMPLE_RATE, segment.end_sample / SAMPLE_RATE


def test_utterance_is_kept_with_padding_on_both_sides() -> None:
    segments = run(np.concatenate([silence(1), speech(2), silence(2)]))

    assert len(segments) == 1
    start, end = times(segments[0])
    assert start == pytest.approx(1.0 - PADDING_S, abs=FRAME_S)
    assert end == pytest.approx(3.0 + PADDING_S, abs=FRAME_S)


def test_pause_shorter_than_pause_ms_keeps_one_segment() -> None:
    audio = np.concatenate([speech(1), silence(0.5), speech(1, seed=1), silence(2)])

    assert len(run(audio)) == 1


def test_pause_of_pause_ms_splits_segments() -> None:
    audio = np.concatenate([speech(1), silence(1.2), speech(1, seed=1), silence(2)])

    segments = run(audio)

    assert len(segments) == 2
    first_end, second_start = times(segments[0])[1], times(segments[1])[0]
    assert first_end <= second_start


def test_segment_shorter_than_min_segment_ms_is_dropped() -> None:
    assert run(np.concatenate([silence(1), speech(0.2), silence(2)])) == []


def test_silence_produces_no_segments() -> None:
    assert run(silence(10)) == []


def test_long_speech_is_split_at_the_quietest_point_of_the_last_two_seconds() -> None:
    audio = np.concatenate([speech(28.8), silence(0.1), speech(40, seed=1), silence(2)])

    segments = run(audio)

    assert all(times(segment)[1] - times(segment)[0] <= 30.0 for segment in segments)
    assert 28.8 < times(segments[0])[1] <= 28.9  # inside the quiet dip
    assert times(segments[-1])[1] == pytest.approx(68.9 + PADDING_S, abs=FRAME_S)


def test_split_segments_cover_the_speech_without_gaps() -> None:
    segments = run(np.concatenate([speech(75), silence(2)]))

    assert len(segments) == 3
    for previous, following in zip(segments, segments[1:]):
        assert previous.end_sample == following.start_sample


def test_flush_closes_an_open_segment() -> None:
    segmenter = Segmenter(SegmenterSettings.from_config(SegmentationConfig()), EnergyDetector())

    assert segmenter.feed(np.concatenate([silence(0.5), speech(1)])) == []
    last = segmenter.flush()

    assert last is not None
    assert times(last)[1] == pytest.approx(1.5, abs=FRAME_S)


def test_chunk_size_does_not_change_the_result() -> None:
    audio = np.concatenate([silence(1), speech(2), silence(1.5), speech(1, seed=1), silence(2)])
    segmenter = Segmenter(SegmenterSettings.from_config(SegmentationConfig()), EnergyDetector())

    chunked = [segment for offset in range(0, len(audio), 777) for segment in segmenter.feed(audio[offset:offset + 777])]

    assert [(s.start_sample, s.end_sample) for s in chunked] == [(s.start_sample, s.end_sample) for s in run(audio)]
