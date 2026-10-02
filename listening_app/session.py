"""A recording session: ids, timing, and the capture -> segment -> transcribe -> store/Hermes pipeline."""

import itertools
import logging
import secrets
import threading
import time
from collections.abc import Callable
from datetime import datetime, timedelta
from functools import partial
from pathlib import Path

from listening_app.capture import AudioCapture, CaptureError
from listening_app.config import AppConfig
from listening_app.devices import AudioSystem, DeviceChoice, DeviceError
from listening_app.hermes_client import (
    ChunkBuffer,
    HermesClient,
    chunk_event,
    delivery_statuses,
    session_end_event,
    session_start_event,
)
from listening_app.models import (
    SAMPLE_RATE,
    Audio,
    CapturedSegment,
    HermesStatus,
    SegmentStatus,
    Source,
    TranscriptSegment,
)
from listening_app.segmenter import Segmenter, SegmenterSettings, SpeechSegment
from listening_app.transcriber import SttSettings, Transcriber, Transcription, grok_access_token
from listening_app.transcript_store import Devices, SessionFiles, SessionMeta, TranscriptStore, finalize_session
from listening_app.vad import SileroVad

log = logging.getLogger(__name__)

HERMES_DRAIN_TIMEOUT_S = 10.0
UNAVAILABLE = "unavailable"
SOURCE_LABELS = {Source.ME: "microphone", Source.OTHERS: "system audio"}

Notify = Callable[[str, str], None]


class SessionError(Exception):
    pass


def new_session_id(started_at: datetime) -> str:
    return f"{started_at:%Y-%m-%dT%H-%M-%S}_{secrets.token_hex(2)}"


def finalize(files: SessionFiles) -> Path:
    """Write the session's final JSON, taking Hermes delivery results from its outbox journal."""
    return finalize_session(files, delivery_statuses(files.outbox))


def to_captured_segment(seq: int, source: Source, source_index: int, speech: SpeechSegment,
                        started_at: datetime) -> CapturedSegment:
    """A segmenter result with its times in seconds since `started_at` and its wall-clock start."""
    start, end = speech.start_sample / SAMPLE_RATE, speech.end_sample / SAMPLE_RATE
    return CapturedSegment(seq, source, source_index, start, end, started_at + timedelta(seconds=start), speech.audio)


class RecordingSession:
    """One recording, from Start to Stop. Built from a config snapshot, so config changes wait for the next one."""

    def __init__(self, config: AppConfig, audio: AudioSystem, hermes: HermesClient, notify: Notify,
                 on_source_lost: Callable[[], None]) -> None:
        self._config = config
        self._audio = audio
        self._hermes = hermes
        self._notify = notify
        self._on_source_lost = on_source_lost
        self._started_at = datetime.now().astimezone()
        self.session_id = new_session_id(self._started_at)
        self.files = SessionFiles(config.output_path(), self.session_id)
        self._stt = SttSettings.from_config(config)
        self._segmenter_settings = SegmenterSettings.from_config(config.segmentation)
        self._segmenters: dict[Source, Segmenter] = {}
        self._segmenter_lock = threading.Lock()
        self._seq = itertools.count()
        self._seq_lock = threading.Lock()
        self._source_index = {source: itertools.count() for source in Source}
        self._captures: dict[Source, AudioCapture] = {}
        self._hermes_lock = threading.Lock()
        self._hermes_started = False
        self._chunks = ChunkBuffer(config.hermes.chunk_kb * 1024)
        self._stt_failure_reported = False
        self._store: TranscriptStore | None = None
        self._transcriber: Transcriber | None = None
        self._listener: Callable[[Transcription], None] | None = None

    def listen(self, handler: Callable[[Transcription], None] | None) -> None:
        """Also hand every transcription to `handler` once it is stored and sent; None stops it.

        A failing handler is logged and never affects the recording.
        """
        self._listener = handler

    def start(self) -> None:
        if self._stt.grok_auth:
            grok_access_token()  # fail now rather than on every segment of the meeting
        choices = self._resolve_devices()
        if not choices:
            raise SessionError("No usable microphone or audio output found")
        self._started_at = datetime.now().astimezone()
        start_time = time.monotonic()
        self.session_id = new_session_id(self._started_at)
        self.files = SessionFiles(self._config.output_path(), self.session_id)
        self._store = TranscriptStore(self.files, self._meta(choices))
        self._transcriber = Transcriber(self._stt, self._on_transcribed)
        self._segmenters = {source: Segmenter(self._segmenter_settings, SileroVad()) for source in choices}
        for source, choice in choices.items():
            self._start_capture(source, choice, start_time)
        if not self._captures:
            self._abort()
            raise SessionError("Could not open any audio device")
        self._start_hermes_session()
        log.info("Session %s started (model %s, language %s)", self.session_id, self._stt.model, self._stt.language)

    def stop(self) -> SessionFiles:
        """Stop capturing, wait for pending transcriptions and give Hermes a moment to catch up."""
        for capture in self._captures.values():
            capture.stop()
        for source in self._segmenters:
            self._flush(source)
        if self._transcriber is not None:
            self._transcriber.close()
        self._send_chunk(self._chunks.flush())
        ended_at = datetime.now().astimezone()
        self._send_session_end(ended_at)
        if self._store is not None:
            self._store.close(ended_at)
        if self._hermes.enabled:
            self._hermes.wait_until_sent(self.session_id, HERMES_DRAIN_TIMEOUT_S)
        log.info("Session %s stopped", self.session_id)
        return self.files

    def _resolve_devices(self) -> dict[Source, DeviceChoice]:
        wanted = {Source.ME: self._config.devices.mic, Source.OTHERS: self._config.devices.output}
        resolvers = {Source.ME: self._audio.resolve_mic, Source.OTHERS: self._audio.resolve_output}
        choices = {}
        for source, resolve in resolvers.items():
            try:
                choice = resolve(wanted[source])
            except DeviceError as exc:
                self._notify(f"Cannot record the {SOURCE_LABELS[source]}", str(exc))
                continue
            if choice.fell_back:
                self._notify("Device not found", f"'{wanted[source]}' is missing, using '{choice.name}' instead.")
            choices[source] = choice
        return choices

    def _meta(self, choices: dict[Source, DeviceChoice]) -> SessionMeta:
        names = {source: choice.name for source, choice in choices.items()}
        return SessionMeta(
            session_id=self.session_id,
            started_at=self._started_at.isoformat(timespec="seconds"),
            devices=Devices(mic=names.get(Source.ME, UNAVAILABLE), output=names.get(Source.OTHERS, UNAVAILABLE)),
            stt_model=self._stt.model,
            language=self._stt.language.value,
        )

    def _start_capture(self, source: Source, choice: DeviceChoice, start_time: float) -> None:
        capture = AudioCapture(self._audio.pa, choice.capture, source, start_time,
                               silent_when_idle=source is Source.OTHERS,
                               on_audio=partial(self._on_audio, source), on_lost=self._on_lost)
        try:
            capture.start()
        except CaptureError as exc:
            self._notify(f"Cannot record the {SOURCE_LABELS[source]}", str(exc))
            return
        self._captures[source] = capture

    def _abort(self) -> None:
        if self._transcriber is not None:
            self._transcriber.close()
        if self._store is not None:
            self._store.close(datetime.now().astimezone())
        for path in (self.files.jsonl, self.files.meta):
            path.unlink(missing_ok=True)
        self.files.folder.rmdir()

    def _on_audio(self, source: Source, samples: Audio) -> None:
        for speech in self._segmenters[source].feed(samples):
            self._submit(source, speech)

    def _on_lost(self, source: Source, reason: str) -> None:
        self._notify(f"Lost the {SOURCE_LABELS[source]}", f"{reason}. The other track keeps recording.")
        self._flush(source)
        self._on_source_lost()

    def _flush(self, source: Source) -> None:
        with self._segmenter_lock:
            speech = self._segmenters[source].flush()
        if speech is not None:
            self._submit(source, speech)

    def _submit(self, source: Source, speech: SpeechSegment) -> None:
        if self._transcriber is None:
            return
        with self._seq_lock:
            seq = next(self._seq)
        segment = to_captured_segment(seq, source, next(self._source_index[source]), speech, self._started_at)
        self._transcriber.submit(segment)

    def _on_transcribed(self, transcription: Transcription) -> None:
        if self._store is None:
            return
        segment = transcription.segment
        send = transcription.status is SegmentStatus.OK and self._hermes.enabled
        record = TranscriptSegment(
            seq=segment.seq,
            source=segment.source,
            start=round(segment.start, 2),
            end=round(segment.end, 2),
            wall_start=segment.wall_start.isoformat(timespec="milliseconds"),
            text=transcription.text,
            language=self._stt.language.value,
            stt_model=self._stt.model,
            status=transcription.status,
            hermes_status=HermesStatus.QUEUED if send else HermesStatus.DISABLED,
            audio_file=self._keep_audio(transcription),
        )
        self._store.append(record)
        if send:
            self._start_hermes_session()
            self._send_chunk(self._chunks.add(record))
        self._hand_to_listener(transcription)

    def _hand_to_listener(self, transcription: Transcription) -> None:
        listener = self._listener
        if listener is None:
            return
        try:
            listener(transcription)
        except Exception:
            log.exception("The transcription listener failed on seq %d", transcription.segment.seq)

    def _send_chunk(self, segments: list[TranscriptSegment]) -> None:
        if segments:
            self._hermes.submit(chunk_event(self.session_id, segments), self.files.outbox)

    def _keep_audio(self, transcription: Transcription) -> str | None:
        failed = transcription.status is SegmentStatus.STT_FAILED
        if self._store is None or not (failed or self._config.save_audio):
            return None
        segment = transcription.segment
        audio_file = self._store.save_audio(segment.seq, segment.source, segment.audio)
        if failed and not self._stt_failure_reported:
            self._stt_failure_reported = True
            self._notify("Transcription failed", f"A segment could not be transcribed; its audio is kept in {audio_file}.")
        return audio_file

    def _start_hermes_session(self) -> None:
        """Send session_start once, before the first portion (also when streaming is switched on mid-session)."""
        if not (self._config.hermes.send_session_events and self._hermes.enabled):
            return
        with self._hermes_lock:
            if self._hermes_started:
                return
            self._hermes_started = True
        devices = self._store.meta.devices.model_dump() if self._store is not None else {}
        event = session_start_event(self.session_id, self._started_at.isoformat(timespec="seconds"), devices,
                                    self._stt.model, self._stt.language.value)
        self._hermes.submit(event, self.files.outbox)

    def _send_session_end(self, ended_at: datetime) -> None:
        if self._store is None or not (self._config.hermes.send_session_events and self._hermes.enabled):
            return
        self._start_hermes_session()
        segments, failed = self._store.counts()
        duration_s = round((ended_at - self._started_at).total_seconds())
        event = session_end_event(self.session_id, ended_at.isoformat(timespec="seconds"), segments, failed, duration_s)
        self._hermes.submit(event, self.files.outbox)
