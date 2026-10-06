"""One live sketch run, from switching it on to switching it off."""

import logging
import threading
from collections.abc import Callable
from datetime import datetime
from pathlib import Path
from typing import Any

from listening_app.clipboard import copy_text
from listening_app.config import AppConfig, KeySource
from listening_app.devices import AudioSystem, com_initialized
from listening_app.models import SegmentStatus, Source
from listening_app.session import Notify, RecordingSession, new_session_id
from listening_app.sketch.drawer import Drawer, SpokenLine, credentials_provider
from listening_app.sketch.export import DiagramFiles
from listening_app.sketch.feed import SketchFeed, TranscriptionHandler, typed_lines
from listening_app.sketch.jev_client import JevClient
from listening_app.sketch.microphone import MicTranscription
from listening_app.sketch.pipeline import SketchPipeline, SketchState
from listening_app.sketch.pointing import Box, PointerTracker
from listening_app.sketch.window import SketchWindow
from listening_app.transcriber import Transcription, grok_access_token

log = logging.getLogger(__name__)

DIAGRAMS_FOLDER = "diagrams"
SKIPPED_NOTICE = "Update skipped, retrying with the next lines"
PATH_COPIED_NOTICE = "Path copied"
NOT_SAVED_NOTICE = "This diagram is not saved yet"
COPY_FAILED_NOTICE = "The clipboard is busy, try again"

WindowEvent = dict[str, Any]


class LiveSketch:
    """Hears the microphone (through the recording when one runs), draws in the window and saves every diagram."""

    def __init__(self, config: AppConfig, audio: AudioSystem, notify: Notify, on_closed: Callable[[], None]) -> None:
        self._config = config
        self._audio = audio
        self._notify = notify
        self._on_closed = on_closed
        sketch = config.sketch
        jev_key = config.jev_key()
        self._jev = JevClient(jev_key.value, sketch.jev_model) if jev_key.value else None
        drawer = Drawer(sketch.draw_model, credentials_provider(config), sketch.label_language)
        self._pipeline = SketchPipeline(
            drawer, self._jev, self,
            retrospect_every_n=sketch.retrospect_every_n,
            retrospect_on_correction=sketch.retrospect_on_correction,
            retrospect_max_nodes=sketch.retrospect_max_nodes,
            bucket_b=sketch.bucket_b,
            bucket_b_min_new_lines=sketch.bucket_b_min_new_lines,
            bucket_b_debounce_s=sketch.bucket_b_debounce_s,
            bucket_b_max_wait_s=sketch.bucket_b_max_wait_s,
        )
        self._pointer = PointerTracker(sketch.point_radius_px, sketch.point_window_s)
        self._pointer_lock = threading.Lock()
        self.folder = config.output_path() / DIAGRAMS_FOLDER / new_session_id(datetime.now().astimezone())
        self._files = DiagramFiles(self.folder)
        self._file_error_reported = False
        self._window = SketchWindow(self._window_event)
        self._feed = SketchFeed(self._heard, self._open_microphone)
        self._feed_lock = threading.Lock()
        self._stopped = False
        self._window_events: dict[str, Callable[[WindowEvent], None]] = {
            "pointer": self._pointer_moved, "leave": self._pointer_left, "boxes": self._boxes_drawn,
            "activate": self._tab_chosen, "select": self._selected, "closed": self._window_closed,
            "microphone": self._microphone_toggled, "copy-path": self._copy_path,
        }

    def start(self, recording: RecordingSession | None, listen: bool = True) -> None:
        """Raises when drawing or listening cannot start (Grok not authorized, no microphone).

        With `listen` off the microphone stays muted until it is turned on in the window."""
        if self._config.draw_key().source is KeySource.GROK_AUTH:
            grok_access_token()
        if self._jev is None:
            self._notify("Live sketch without Jev", "TYPESAFE_API_KEY is not set, so the drawing model also "
                                                    "decides which diagram you mean. Updates are slower.")
        self._pipeline.start()
        self._window.open()
        try:
            with self._feed_lock:
                if not listen:
                    self._feed.mute()
                self._feed.start(recording)
        except Exception:
            self.stop()
            raise
        self._window.microphone(listen)

    def stop(self) -> None:
        with self._feed_lock:
            self._stopped = True
            self._feed.stop()
        self._pipeline.stop()
        self._window.close()
        if self._jev is not None:
            self._jev.close()

    def recording_started(self, recording: RecordingSession) -> None:
        with self._feed_lock:
            self._feed.use_recording(recording)

    def recording_stopping(self) -> None:
        try:
            with self._feed_lock:
                self._feed.use_microphone()
        except Exception as exc:
            log.exception("Live sketch could not listen to the microphone on its own")
            self._notify("Live sketch stopped listening", str(exc))

    def say(self, text: str) -> None:
        """Draw from typed text as if the speaker had said it, one sentence at a time."""
        for line in typed_lines(text):
            log.info("Live sketch read: %s", line)
            self._pipeline.add(SpokenLine(line))

    def wait_until_drawn(self, timeout_s: float) -> bool:
        """False when lines are still being routed or drawn after `timeout_s`."""
        return self._pipeline.wait_until_idle(timeout_s)

    def snapshot(self) -> SketchState:
        return self._pipeline.snapshot()

    def file(self, number: int) -> Path | None:
        """The HTML page diagram `number` was last saved to, or None before its first save."""
        return self._files.path(number)

    # SketchListener

    def sketch_updated(self, state: SketchState) -> None:
        self._window.show(state)
        if state.updated is not None:
            self._save(next(view for view in state.views if view["number"] == state.updated))

    def update_skipped(self, number: int | None, reason: str) -> None:
        self._window.notice(SKIPPED_NOTICE)

    def jev_unavailable(self, reason: str) -> None:
        self._notify("Live sketch: Jev unavailable",
                     f"The drawing model now also decides which diagram you mean. ({reason})")

    # Internals

    def _open_microphone(self, handler: TranscriptionHandler) -> MicTranscription:
        return MicTranscription(self._config, self._audio, handler, self._microphone_lost)

    def _microphone_lost(self, reason: str) -> None:
        self._notify("Live sketch lost the microphone", f"{reason}. Turn the mic on again in the live sketch window.")
        threading.Thread(target=self._mute, name="sketch-mute", daemon=True).start()

    def _mute(self) -> None:
        with self._feed_lock:
            self._feed.mute()
        self._window.microphone(False)

    def _microphone_toggled(self, event: WindowEvent) -> None:
        with self._feed_lock, com_initialized():
            if self._stopped:
                return
            if self._feed.listening:
                self._feed.mute()
            else:
                self._unmute()
            listening = self._feed.listening
        self._window.microphone(listening)

    def _unmute(self) -> None:
        try:
            self._feed.unmute()
        except Exception as exc:
            log.exception("Live sketch could not listen to the microphone")
            self._notify("Live sketch could not listen", str(exc))

    def _copy_path(self, event: WindowEvent) -> None:
        path = self._files.path(int(event["number"]))
        if path is None:
            self._window.notice(NOT_SAVED_NOTICE)
            return
        try:
            copy_text(str(path))
        except OSError:
            log.exception("Copying the path of diagram %s failed", event["number"])
            self._window.notice(COPY_FAILED_NOTICE)
            return
        self._window.notice(PATH_COPIED_NOTICE)

    def _heard(self, transcription: Transcription) -> None:
        segment = transcription.segment
        if segment.source is not Source.ME or transcription.status is not SegmentStatus.OK:
            return
        log.info("Live sketch heard: %s", transcription.text)
        start = segment.wall_start.timestamp()
        with self._pointer_lock:
            pointed = self._pointer.pointed_during(start, start + segment.end - segment.start)
        self._pipeline.add(SpokenLine(transcription.text, pointed))

    def _save(self, view: dict[str, Any]) -> None:
        try:
            self._files.save(view)
        except OSError as exc:
            log.exception("Saving diagram %s failed", view["number"])
            if not self._file_error_reported:
                self._file_error_reported = True
                self._notify("Live sketch could not save a diagram", str(exc))

    def _window_event(self, event: WindowEvent) -> None:
        handler = self._window_events.get(str(event.get("type")))
        if handler is not None:
            handler(event)

    def _pointer_moved(self, event: WindowEvent) -> None:
        with self._pointer_lock:
            before = self._pointer.pointed
            after = self._pointer.move(float(event["t"]), float(event["x"]), float(event["y"]))
        if after != before:
            self._window.point(after)

    def _pointer_left(self, event: WindowEvent) -> None:
        with self._pointer_lock:
            before = self._pointer.pointed
            self._pointer.leave(float(event["t"]))
        if before is not None:
            self._window.point(None)

    def _boxes_drawn(self, event: WindowEvent) -> None:
        boxes = {str(element): Box(box["x"], box["y"], box["width"], box["height"])
                 for element, box in event["boxes"].items()}
        with self._pointer_lock:
            self._pointer.set_boxes(boxes)

    def _tab_chosen(self, event: WindowEvent) -> None:
        self._pipeline.activate(int(event["number"]))

    def _selected(self, event: WindowEvent) -> None:
        self._pipeline.select(int(event["number"]), [str(element) for element in event["ids"]])

    def _window_closed(self, event: WindowEvent) -> None:
        self._on_closed()
