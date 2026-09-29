"""Data types shared across the pipeline."""

from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum

import numpy as np
import numpy.typing as npt
from pydantic import BaseModel, ConfigDict

SAMPLE_RATE = 16_000

Audio = npt.NDArray[np.float32]
"""Mono float32 samples in [-1, 1] at SAMPLE_RATE."""


class Source(StrEnum):
    ME = "me"
    OTHERS = "others"


class SegmentStatus(StrEnum):
    OK = "ok"
    STT_FAILED = "stt_failed"
    EMPTY = "empty"


class HermesStatus(StrEnum):
    DELIVERED = "delivered"
    QUEUED = "queued"
    DISABLED = "disabled"
    FAILED = "failed"


@dataclass(frozen=True)
class CapturedSegment:
    """A speech segment cut from one source, waiting for transcription."""

    seq: int
    source: Source
    source_index: int
    start: float
    end: float
    wall_start: datetime
    audio: Audio


class TranscriptSegment(BaseModel):
    """One line of <session_id>.jsonl and one entry of the final transcript."""

    model_config = ConfigDict(frozen=True)

    seq: int
    source: Source
    start: float
    end: float
    wall_start: str
    text: str
    language: str
    stt_model: str
    status: SegmentStatus
    hermes_status: HermesStatus
    audio_file: str | None = None

    def to_json_line(self) -> str:
        return self.model_dump_json(exclude_none=True)
