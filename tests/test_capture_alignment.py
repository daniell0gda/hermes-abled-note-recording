from listening_app.capture import TimelineAligner
from listening_app.models import SAMPLE_RATE

CHUNK = 320  # 20 ms


def test_continuous_stream_gets_no_silence() -> None:
    aligner = TimelineAligner(start_time=0.0)

    inserted = [aligner.silence_before_chunk(CHUNK, (index + 1) * 0.02) for index in range(500)]

    assert sum(inserted) == 0


def test_jitter_below_the_threshold_is_ignored() -> None:
    aligner = TimelineAligner(start_time=0.0)

    assert aligner.silence_before_chunk(CHUNK, 0.3) == 0


def test_gap_in_a_loopback_stream_is_filled_with_silence() -> None:
    aligner = TimelineAligner(start_time=0.0)
    aligner.silence_before_chunk(CHUNK, 0.02)

    silence = aligner.silence_before_chunk(CHUNK, 5.02)

    assert silence == 5 * SAMPLE_RATE - CHUNK


def test_idle_stream_is_padded_up_to_the_clock_minus_a_margin() -> None:
    aligner = TimelineAligner(start_time=10.0, idle_margin_s=0.3)

    assert aligner.silence_while_idle(10.5) == 0
    assert aligner.silence_while_idle(13.3) == 3 * SAMPLE_RATE


def test_audio_resuming_after_idle_padding_is_not_padded_twice() -> None:
    aligner = TimelineAligner(start_time=0.0, idle_margin_s=0.3)
    aligner.silence_while_idle(4.0)

    assert aligner.silence_before_chunk(CHUNK, 4.05) == 0
