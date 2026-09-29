"""Speech-to-text worker pool: litellm transcription, retries, and in-order emission per source."""

import logging
import os
import threading
import time
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from typing import Any

from listening_app.config import AppConfig, KeySource, Language
from listening_app.models import CapturedSegment, SegmentStatus, Source
from listening_app.wav import encode_wav

log = logging.getLogger(__name__)

MAX_ATTEMPTS = 3
BACKOFF_S = 1.0
REQUEST_TIMEOUT_S = 30.0
GROK_NOT_AUTHORIZED = "Grok is not authorized. Use 'Authorize Grok' in the tray menu."


class GrokAuthError(Exception):
    pass


@dataclass(frozen=True)
class SttSettings:
    model: str
    language: Language
    prompt: str
    api_key: str = field(repr=False)
    workers: int
    grok_auth: bool = False

    @classmethod
    def from_config(cls, config: AppConfig) -> "SttSettings":
        model = config.stt.selected
        key = config.stt_key(model)
        return cls(model, config.language, config.stt.prompt, key.value, config.stt.workers,
                   key.source is KeySource.GROK_AUTH)


@dataclass(frozen=True)
class Transcription:
    segment: CapturedSegment
    text: str
    status: SegmentStatus


TranscribeFn = Callable[[bytes, SttSettings], str]


def import_litellm() -> Any:
    """Import litellm without its network fetch of the model price map (the import itself takes seconds)."""
    os.environ.setdefault("LITELLM_LOCAL_MODEL_COST_MAP", "True")
    import litellm

    litellm.suppress_debug_info = True
    return litellm


def authorize_grok() -> None:
    """Sign in with a Grok account in the browser. Blocks until xAI redirects back (at most 3 minutes)."""
    import_litellm()
    from litellm.llms.xai.oauth import XAIOAuthAuthenticator

    XAIOAuthAuthenticator().login(force=True)


def grok_access_token() -> str:
    """Access token of the saved Grok authorization, refreshed first when it has expired."""
    import_litellm()
    from litellm.llms.xai.oauth import XAIOAuthAuthenticator, XAIOAuthLoginRequiredError

    try:
        return XAIOAuthAuthenticator().get_access_token()
    except XAIOAuthLoginRequiredError as exc:
        raise GrokAuthError(GROK_NOT_AUTHORIZED) from exc


def litellm_transcribe(wav: bytes, settings: SttSettings) -> str:
    litellm = import_litellm()
    # drop_params: providers without glossary support (xAI) would otherwise reject the prompt
    options: dict[str, object] = {"timeout": REQUEST_TIMEOUT_S, "max_retries": 0, "drop_params": True}
    if settings.language is not Language.AUTO:
        options["language"] = settings.language.value
    if settings.prompt:
        options["prompt"] = settings.prompt
    api_key = grok_access_token() if settings.grok_auth else settings.api_key
    if api_key:
        options["api_key"] = api_key
    response = litellm.transcription(model=settings.model, file=("segment.wav", wav, "audio/wav"), **options)
    return str(response.text or "")


class ReorderBuffer[T]:
    """Releases items strictly in index order 0, 1, 2, ... whatever order they arrive in."""

    def __init__(self) -> None:
        self._next = 0
        self._waiting: dict[int, T] = {}

    def add(self, index: int, item: T) -> list[T]:
        self._waiting[index] = item
        ready = []
        while self._next in self._waiting:
            ready.append(self._waiting.pop(self._next))
            self._next += 1
        return ready


class Transcriber:
    """Transcribes segments on a worker pool so capture never blocks.

    `on_ready` is called one at a time, in `source_index` order within each source, even when
    transcriptions finish out of order.
    """

    def __init__(
        self,
        settings: SttSettings,
        on_ready: Callable[[Transcription], None],
        transcribe: TranscribeFn = litellm_transcribe,
        sleep: Callable[[float], None] = time.sleep,
    ) -> None:
        self._settings = settings
        self._on_ready = on_ready
        self._transcribe = transcribe
        self._sleep = sleep
        self._pool = ThreadPoolExecutor(max_workers=settings.workers, thread_name_prefix="stt")
        self._buffers: dict[Source, ReorderBuffer[Transcription]] = {source: ReorderBuffer() for source in Source}
        self._emit_lock = threading.Lock()

    def submit(self, segment: CapturedSegment) -> None:
        self._pool.submit(self._process, segment)

    def close(self) -> None:
        """Wait until every submitted segment has been transcribed and emitted."""
        self._pool.shutdown(wait=True)

    def _process(self, segment: CapturedSegment) -> None:
        result = self._transcribe_safely(segment)
        with self._emit_lock:
            for ready in self._buffers[segment.source].add(segment.source_index, result):
                self._emit(ready)

    def _transcribe_safely(self, segment: CapturedSegment) -> Transcription:
        try:
            return self._transcribe_with_retries(segment)
        except Exception:
            log.exception("Transcribing seq %d failed unexpectedly", segment.seq)
            return Transcription(segment, "", SegmentStatus.STT_FAILED)

    def _transcribe_with_retries(self, segment: CapturedSegment) -> Transcription:
        wav = encode_wav(segment.audio)
        for attempt in range(1, MAX_ATTEMPTS + 1):
            text = self._attempt(wav, segment, attempt)
            if text is not None:
                return Transcription(segment, text, SegmentStatus.OK if text else SegmentStatus.EMPTY)
            if attempt < MAX_ATTEMPTS:
                self._sleep(BACKOFF_S * 2 ** (attempt - 1))
        return Transcription(segment, "", SegmentStatus.STT_FAILED)

    def _attempt(self, wav: bytes, segment: CapturedSegment, attempt: int) -> str | None:
        started = time.monotonic()
        try:
            text = self._transcribe(wav, self._settings).strip()
        except Exception as exc:
            log.warning("STT attempt %d/%d for seq %d failed: %s", attempt, MAX_ATTEMPTS, segment.seq, exc)
            return None
        log.info("seq %d (%s, %.1f-%.1f s) transcribed in %.1f s, %d chars", segment.seq, segment.source,
                 segment.start, segment.end, time.monotonic() - started, len(text))
        log.debug("seq %d text: %s", segment.seq, text)
        return text

    def _emit(self, transcription: Transcription) -> None:
        try:
            self._on_ready(transcription)
        except Exception:
            log.exception("Handling transcription of seq %d failed", transcription.segment.seq)
