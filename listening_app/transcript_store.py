"""Local transcript files: append-only JSONL while recording, consolidated JSON on stop, crash recovery."""

import logging
import os
import threading
from collections.abc import Collection, Mapping
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

from pydantic import BaseModel, ValidationError

from listening_app.files import atomic_write_text, read_lines
from listening_app.models import Audio, HermesStatus, SegmentStatus, Source, TranscriptSegment
from listening_app.wav import write_wav

log = logging.getLogger(__name__)

SCHEMA_VERSION = 1
_OUTBOX_SUFFIX = ".outbox.jsonl"
_UNDELIVERED = (HermesStatus.QUEUED, HermesStatus.FAILED)


@dataclass(frozen=True)
class SessionFiles:
    """Every file of one session lives in <transcripts folder>/<session_id>/."""

    directory: Path
    session_id: str

    @classmethod
    def from_outbox(cls, outbox: Path) -> "SessionFiles":
        return cls(outbox.parent.parent, outbox.name.removesuffix(_OUTBOX_SUFFIX))

    @property
    def folder(self) -> Path:
        return self.directory / self.session_id

    @property
    def jsonl(self) -> Path:
        return self.folder / f"{self.session_id}.jsonl"

    @property
    def final(self) -> Path:
        return self.folder / f"{self.session_id}.json"

    @property
    def meta(self) -> Path:
        return self.folder / f"{self.session_id}.meta.json"

    @property
    def outbox(self) -> Path:
        return self.folder / f"{self.session_id}{_OUTBOX_SUFFIX}"

    @property
    def audio_dir(self) -> Path:
        return self.folder / "audio"


class Devices(BaseModel):
    mic: str
    output: str


class SessionMeta(BaseModel):
    session_id: str
    started_at: str
    ended_at: str | None = None
    devices: Devices
    stt_model: str
    language: str


class SessionStats(BaseModel):
    segments: int
    failed: int
    hermes_undelivered: int
    duration_s: int


class FinalTranscript(BaseModel):
    schema_version: int = SCHEMA_VERSION
    session_id: str
    started_at: str
    ended_at: str
    devices: Devices
    stt_model: str
    language: str
    segments: list[TranscriptSegment]
    stats: SessionStats


class TranscriptStore:
    """Writes one session's files. `append` is thread-safe and durable once it returns."""

    def __init__(self, files: SessionFiles, meta: SessionMeta) -> None:
        self.files = files
        self.meta = meta
        self._lock = threading.Lock()
        self._segments = 0
        self._failed = 0
        files.folder.mkdir(parents=True, exist_ok=True)
        atomic_write_text(files.meta, meta.model_dump_json(indent=2))
        self._jsonl = files.jsonl.open("a", encoding="utf-8", newline="\n")

    def append(self, segment: TranscriptSegment) -> None:
        with self._lock:
            self._jsonl.write(segment.to_json_line() + "\n")
            self._jsonl.flush()
            os.fsync(self._jsonl.fileno())
            self._segments += 1
            self._failed += segment.status is SegmentStatus.STT_FAILED

    def save_audio(self, seq: int, source: Source, audio: Audio) -> str:
        """Save a segment WAV; returns its path relative to the session folder."""
        path = self.files.audio_dir / f"{seq:05d}_{source}.wav"
        write_wav(path, audio)
        return path.relative_to(self.files.folder).as_posix()

    def counts(self) -> tuple[int, int]:
        """(segments, failed) written so far."""
        with self._lock:
            return self._segments, self._failed

    def close(self, ended_at: datetime) -> None:
        with self._lock:
            self._jsonl.close()
        ended = self.meta.model_copy(update={"ended_at": ended_at.isoformat(timespec="seconds")})
        atomic_write_text(self.files.meta, ended.model_dump_json(indent=2))


def read_segments(path: Path) -> list[TranscriptSegment]:
    """Segments of a JSONL file. A line cut off by a crash is skipped."""
    segments = []
    for number, line in enumerate(read_lines(path), start=1):
        try:
            segments.append(TranscriptSegment.model_validate_json(line))
        except ValidationError:
            log.warning("Skipping unreadable line %d of %s", number, path.name)
    return segments


def finalize_session(files: SessionFiles, delivery: Mapping[int, HermesStatus]) -> Path:
    """Write <session_id>.json from the JSONL (plus Hermes delivery results by seq). Safe to repeat."""
    segments = sorted(
        (_with_delivery(segment, delivery) for segment in read_segments(files.jsonl)),
        key=lambda segment: (segment.start, segment.seq),
    )
    meta = _read_meta(files, segments)
    ended_at = meta.ended_at or _modified_at(files.jsonl)
    transcript = FinalTranscript(
        session_id=meta.session_id,
        started_at=meta.started_at,
        ended_at=ended_at,
        devices=meta.devices,
        stt_model=meta.stt_model,
        language=meta.language,
        segments=segments,
        stats=SessionStats(
            segments=len(segments),
            failed=sum(segment.status is SegmentStatus.STT_FAILED for segment in segments),
            hermes_undelivered=sum(segment.hermes_status in _UNDELIVERED for segment in segments),
            duration_s=round((datetime.fromisoformat(ended_at) - datetime.fromisoformat(meta.started_at)).total_seconds()),
        ),
    )
    atomic_write_text(files.final, transcript.model_dump_json(indent=2, exclude_none=True))
    log.info("Wrote %s (%d segments)", files.final.name, len(segments))
    return files.final


def find_sessions(directory: Path) -> list[SessionFiles]:
    """Every session in the transcripts folder (every folder with a JSONL), oldest first."""
    if not directory.is_dir():
        return []
    candidates = (SessionFiles(directory, folder.name) for folder in sorted(directory.iterdir()) if folder.is_dir())
    return [files for files in candidates if files.jsonl.exists()]


def find_unfinished_sessions(directory: Path, exclude: Collection[str] = ()) -> list[SessionFiles]:
    """Sessions with a JSONL but no final JSON, e.g. after a crash."""
    return [files for files in find_sessions(directory)
            if not files.final.exists() and files.session_id not in exclude]


def find_outboxes(directory: Path) -> list[Path]:
    """Hermes delivery journals of every session in the transcripts folder."""
    return sorted(directory.glob(f"*/*{_OUTBOX_SUFFIX}")) if directory.is_dir() else []


def _with_delivery(segment: TranscriptSegment, delivery: Mapping[int, HermesStatus]) -> TranscriptSegment:
    status = delivery.get(segment.seq)
    return segment.model_copy(update={"hermes_status": status}) if status else segment


def read_meta(files: SessionFiles) -> SessionMeta | None:
    try:
        return SessionMeta.model_validate_json(files.meta.read_text(encoding="utf-8"))
    except (OSError, ValidationError):
        return None


def _read_meta(files: SessionFiles, segments: list[TranscriptSegment]) -> SessionMeta:
    meta = read_meta(files)
    if meta is not None:
        return meta
    log.warning("No readable metadata for session %s, reconstructing it", files.session_id)
    started_at = segments[0].wall_start if segments else _modified_at(files.jsonl)
    unknown = "unknown"
    stt_model = segments[0].stt_model if segments else unknown
    language = segments[0].language if segments else unknown
    return SessionMeta(session_id=files.session_id, started_at=started_at, devices=Devices(mic=unknown, output=unknown),
                       stt_model=stt_model, language=language)


def _modified_at(path: Path) -> str:
    return datetime.fromtimestamp(path.stat().st_mtime).astimezone().isoformat(timespec="seconds")
