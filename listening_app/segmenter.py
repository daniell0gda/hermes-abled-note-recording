"""Pause-based segmentation of a 16 kHz mono stream. Pure logic: the speech detector is injected."""

import math
from collections import deque
from dataclasses import dataclass
from typing import Protocol

import numpy as np

from listening_app.config import SegmentationConfig
from listening_app.models import SAMPLE_RATE, Audio

FRAME_SAMPLES = 512
FRAME_MS = FRAME_SAMPLES * 1000 / SAMPLE_RATE
SPLIT_WINDOW_MS = 2000
END_THRESHOLD_OFFSET = 0.15
"""Hysteresis: once speech started, frames count as speech down to threshold - offset (as in Silero)."""


class SpeechDetector(Protocol):
    def probability(self, frame: Audio) -> float: ...

    def reset(self) -> None: ...


@dataclass(frozen=True)
class SegmenterSettings:
    pause_frames: int
    max_frames: int
    min_speech_frames: int
    padding_frames: int
    split_window_frames: int
    start_threshold: float
    end_threshold: float

    @classmethod
    def from_config(cls, config: SegmentationConfig) -> "SegmenterSettings":
        return cls(
            pause_frames=_frames(config.pause_ms),
            max_frames=int(config.max_segment_s * 1000 // FRAME_MS),
            min_speech_frames=_frames(config.min_segment_ms),
            padding_frames=_frames(config.padding_ms),
            split_window_frames=_frames(SPLIT_WINDOW_MS),
            start_threshold=config.vad_threshold,
            end_threshold=max(config.vad_threshold - END_THRESHOLD_OFFSET, 0.01),
        )


def _frames(milliseconds: float) -> int:
    return math.ceil(milliseconds / FRAME_MS)


@dataclass(frozen=True)
class SpeechSegment:
    start_sample: int
    audio: Audio

    @property
    def end_sample(self) -> int:
        return self.start_sample + len(self.audio)


@dataclass(frozen=True)
class _Frame:
    audio: Audio
    energy: float
    speech: bool


class Segmenter:
    """Feed audio in any chunk size; get back finished speech segments.

    A segment ends after `pause_frames` of silence, is force-split at the quietest frame of its last
    two seconds once it reaches `max_frames`, keeps `padding_frames` of audio around the speech, and is
    dropped if its speech spans fewer than `min_speech_frames`.
    """

    def __init__(self, settings: SegmenterSettings, detector: SpeechDetector) -> None:
        self._settings = settings
        self._detector = detector
        self._detector.reset()
        self._pending = np.zeros(0, dtype=np.float32)
        self._frames_seen = 0
        self._pre_roll: deque[Audio] = deque(maxlen=settings.padding_frames)
        self._segment: list[_Frame] = []
        self._segment_start = 0
        self._silent_run = 0

    def feed(self, samples: Audio) -> list[SpeechSegment]:
        data = np.concatenate([self._pending, samples]) if len(self._pending) else samples
        whole = len(data) - len(data) % FRAME_SAMPLES
        self._pending = data[whole:]
        finished = (self._process(data[offset:offset + FRAME_SAMPLES]) for offset in range(0, whole, FRAME_SAMPLES))
        return [segment for segment in finished if segment is not None]

    def flush(self) -> SpeechSegment | None:
        """End of stream: close the open segment, if any."""
        self._pending = np.zeros(0, dtype=np.float32)
        return self._close() if self._segment else None

    def _process(self, frame: Audio) -> SpeechSegment | None:
        probability = self._detector.probability(frame)
        self._frames_seen += 1
        if not self._segment:
            self._wait_for_speech(frame, probability)
            return None
        return self._extend(frame, probability)

    def _wait_for_speech(self, frame: Audio, probability: float) -> None:
        if probability < self._settings.start_threshold:
            self._pre_roll.append(frame)
            return
        self._segment = [_Frame(audio, _rms(audio), False) for audio in self._pre_roll]
        self._segment.append(_Frame(frame, _rms(frame), True))
        self._segment_start = self._frames_seen - len(self._segment)
        self._pre_roll.clear()
        self._silent_run = 0

    def _extend(self, frame: Audio, probability: float) -> SpeechSegment | None:
        speech = probability >= self._settings.end_threshold
        self._segment.append(_Frame(frame, _rms(frame), speech))
        self._silent_run = 0 if speech else self._silent_run + 1
        if self._silent_run >= self._settings.pause_frames:
            return self._close()
        if len(self._segment) >= self._settings.max_frames:
            return self._split()
        return None

    def _close(self) -> SpeechSegment | None:
        frames, self._segment = self._segment, []
        speech_at = [index for index, frame in enumerate(frames) if frame.speech]
        end = min(speech_at[-1] + 1 + self._settings.padding_frames, len(frames)) if speech_at else 0
        self._pre_roll.extend(frame.audio for frame in frames[end:])
        return self._segment_of(frames[:end], self._segment_start)

    def _split(self) -> SpeechSegment | None:
        frames = self._segment
        window_start = max(1, len(frames) - self._settings.split_window_frames)
        quietest = min(range(window_start, len(frames)), key=lambda index: frames[index].energy)
        cut = quietest + 1
        head, self._segment = frames[:cut], frames[cut:]
        head_start = self._segment_start
        self._segment_start += cut
        self._silent_run = _trailing_silence(self._segment)
        return self._segment_of(head, head_start)

    def _segment_of(self, frames: list[_Frame], start_frame: int) -> SpeechSegment | None:
        speech_at = [index for index, frame in enumerate(frames) if frame.speech]
        if not speech_at or speech_at[-1] - speech_at[0] + 1 < self._settings.min_speech_frames:
            return None
        audio = np.concatenate([frame.audio for frame in frames])
        return SpeechSegment(start_frame * FRAME_SAMPLES, audio)


def _rms(frame: Audio) -> float:
    return float(np.sqrt(np.mean(np.square(frame))))


def _trailing_silence(frames: list[_Frame]) -> int:
    count = 0
    for frame in reversed(frames):
        if frame.speech:
            break
        count += 1
    return count
