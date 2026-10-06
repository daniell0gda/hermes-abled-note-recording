"""Headless mode: an AI agent drives the app through MCP tools on stdin/stdout instead of the tray menu."""

import re
import threading
from collections import deque
from collections.abc import Callable
from datetime import datetime
from pathlib import Path
from typing import Annotated

from mcp.server.mcpserver import MCPServer
from mcp.server.mcpserver.exceptions import ToolError
from mcp_types import ToolAnnotations
from pydantic import BaseModel, Field

from listening_app import __version__
from listening_app.app import PRIVACY_REMINDER, AppController, ControlError
from listening_app.config import PayloadMode
from listening_app.models import TranscriptSegment
from listening_app.sketch.controller import LiveSketch
from listening_app.transcript_store import (
    FinalTranscript,
    SessionFiles,
    SessionStats,
    find_sessions,
    read_meta,
    read_segments,
)

SERVER_NAME = "listening-app"
NOTIFICATION_HISTORY = 20
MAX_SEGMENTS_PER_CALL = 200
SKETCH_WAIT_S = 120.0
_SESSION_ID = re.compile(r"[\w-]+")
_READ_ONLY = ToolAnnotations(read_only_hint=True)

INSTRUCTIONS = """\
Listening App records online meetings on this Windows PC and transcribes them live, in two tracks: "me" is the \
microphone, "others" is the system audio of the selected output device.
Typical flow: start_recording, then get_transcript again and again with the returned next_offset to follow the \
meeting live, then stop_recording. The other participants must be told that the meeting is transcribed: remind the \
user when a recording starts.
Device and model selections are saved to the config file and apply to the next recording.
Live sketch draws diagrams in a window on the user's screen from an explanation. Instead of the user talking, you \
can write the explanation: start_sketch, then sketch_text with a few sentences at a time, the way a person explains \
it out loud, then stop_sketch. Every diagram is saved as a self-contained HTML file."""


class Notification(BaseModel):
    time: str
    title: str
    message: str


class Status(BaseModel):
    recording: bool
    busy: bool = Field(description="A recording is being started or stopped")
    session_id: str | None = Field(description="The session being recorded")
    error: str | None = Field(description="The current problem, if any (config, device, Hermes)")
    microphone: str | None = Field(description="Selected microphone; null = system default")
    output: str | None = Field(description="Selected audio output; null = system default")
    model: str = Field(description="Selected transcription model")
    models: list[str] = Field(description="Transcription models that can be selected")
    hermes_streaming: bool
    hermes_endpoint: PayloadMode
    grok_auth: bool = Field(description="xAI models use a Grok account; see authorize_grok")
    authorizing_grok: bool
    unfinished_sessions: int = Field(description="Recordings cut off by a crash; see finalize_unfinished_sessions")
    notifications: list[Notification] = Field(description="Latest messages the tray would have shown, oldest first")


class DeviceList(BaseModel):
    microphones: list[str]
    outputs: list[str]
    default_microphone: str | None
    default_output: str | None
    selected_microphone: str | None = Field(description="null = system default")
    selected_output: str | None = Field(description="null = system default")


class RecordingStarted(BaseModel):
    session_id: str
    reminder: str


class RecordingStopped(BaseModel):
    session_id: str
    transcript_file: str
    stats: SessionStats


class SessionSummary(BaseModel):
    session_id: str
    started_at: str | None
    ended_at: str | None
    finalized: bool = Field(description="The final transcript JSON has been written")
    recording: bool


class SessionList(BaseModel):
    sessions: list[SessionSummary] = Field(description="Newest first")


class Transcript(BaseModel):
    session_id: str
    recording: bool
    segments: list[TranscriptSegment]
    next_offset: int = Field(description="Pass as offset to get only the segments transcribed after these")
    has_more: bool = Field(description="More segments are available right now")


class SketchDiagram(BaseModel):
    number: int
    title: str
    kind: str
    active: bool = Field(description="The diagram shown in the window")
    file: str | None = Field(description="The diagram's HTML page; null until it is first saved")


class Sketch(BaseModel):
    on: bool
    busy: bool = Field(description="Live sketch is being switched on or off")
    drawing: bool = Field(description="Text is still being routed or drawn; later revisions can follow")
    folder: str | None = Field(description="Where the diagrams of this live sketch are saved")
    diagrams: list[SketchDiagram]


class McpApp:
    """The headless counterpart of TrayApp: MCP tools instead of the menu, notifications kept for get_status."""

    def __init__(self, controller: AppController) -> None:
        self._controller = controller
        self._notifications: deque[Notification] = deque(maxlen=NOTIFICATION_HISTORY)
        self._lock = threading.Lock()
        self.server: MCPServer = MCPServer(SERVER_NAME, instructions=INSTRUCTIONS, version=__version__)
        for query in (self.get_status, self.list_devices, self.list_sessions, self.get_transcript, self.get_sketch):
            self.server.add_tool(query, annotations=_READ_ONLY)
        for action in (self.start_recording, self.stop_recording, self.select_microphone, self.select_output,
                       self.select_model, self.set_hermes_streaming, self.set_hermes_endpoint, self.refresh_devices,
                       self.finalize_unfinished_sessions, self.reload_config, self.authorize_grok,
                       self.start_sketch, self.sketch_text, self.stop_sketch):
            self.server.add_tool(action)

    def run(self, on_ready: Callable[[], None]) -> None:
        """Blocks until the AI client disconnects."""
        on_ready()
        self.server.run()

    def notify(self, title: str, message: str) -> None:
        notification = Notification(time=_now(), title=title, message=message)
        with self._lock:
            self._notifications.append(notification)

    def refresh(self) -> None:
        """Nothing to redraw: get_status reads the state when asked."""

    # Queries

    def get_status(self) -> Status:
        """Whether a recording runs, the current selections, the current problem and the latest notifications."""
        controller = self._controller
        config = controller.config
        with self._lock:
            notifications = list(self._notifications)
        return Status(
            recording=controller.is_recording,
            busy=controller.is_busy,
            session_id=controller.session_id,
            error=controller.error,
            microphone=config.devices.mic,
            output=config.devices.output,
            model=config.stt.selected,
            models=config.stt.choices(),
            hermes_streaming=controller.hermes_enabled,
            hermes_endpoint=config.hermes.payload_mode,
            grok_auth=controller.grok_auth_enabled,
            authorizing_grok=controller.is_authorizing_grok,
            unfinished_sessions=controller.unfinished_count,
            notifications=notifications,
        )

    def list_devices(self) -> DeviceList:
        """Microphones and audio outputs that can be selected, the system defaults and the current selection."""
        controller = self._controller
        devices = controller.config.devices
        return DeviceList(
            microphones=controller.microphones(),
            outputs=controller.outputs(),
            default_microphone=controller.default_mic_name(),
            default_output=controller.default_output_name(),
            selected_microphone=devices.mic,
            selected_output=devices.output,
        )

    def list_sessions(self, limit: Annotated[int, Field(ge=1)] = 20) -> SessionList:
        """Recorded sessions in the transcripts folder, newest first."""
        newest_first = find_sessions(self._controller.config.output_path())[::-1]
        return SessionList(sessions=[self._summary(files) for files in newest_first[:limit]])

    def get_transcript(
        self,
        session_id: Annotated[str | None, Field(
            description="A session from list_sessions; default: the one recording now, else the latest")] = None,
        offset: Annotated[int, Field(ge=0, description="Skip this many segments (next_offset of the last call)")] = 0,
    ) -> Transcript:
        """Transcript segments of a session in the order they were transcribed, also while it is being recorded."""
        files = self._session_files(session_id)
        segments = read_segments(files.jsonl)
        page = segments[offset:offset + MAX_SEGMENTS_PER_CALL]
        next_offset = offset + len(page)
        return Transcript(session_id=files.session_id, recording=self._is_recording(files), segments=page,
                          next_offset=next_offset, has_more=next_offset < len(segments))

    def get_sketch(self) -> Sketch:
        """Whether live sketch is on, and its diagrams with the files they are saved in."""
        return self._sketch_state(self._controller.sketch)

    # Actions

    def start_recording(self) -> RecordingStarted:
        """Start recording and transcribing the microphone and the system audio."""
        session_id = _control(self._controller.start_recording)
        return RecordingStarted(session_id=session_id, reminder=PRIVACY_REMINDER)

    def stop_recording(self) -> RecordingStopped:
        """Stop recording, wait for the last transcriptions and write the final transcript."""
        transcript_file = _control(self._controller.stop_recording)
        transcript = FinalTranscript.model_validate_json(transcript_file.read_text(encoding="utf-8"))
        return RecordingStopped(session_id=transcript.session_id, transcript_file=str(transcript_file),
                                stats=transcript.stats)

    def select_microphone(
        self, name: Annotated[str | None, Field(description="A microphone from list_devices; null = system default")],
    ) -> Status:
        """Choose the microphone recorded as "me"."""
        _require_known(name, self._controller.microphones(), "microphone")
        self._controller.select_mic(name)
        return self.get_status()

    def select_output(
        self, name: Annotated[str | None, Field(description="An output from list_devices; null = system default")],
    ) -> Status:
        """Choose the audio output whose sound is recorded as "others"."""
        _require_known(name, self._controller.outputs(), "audio output")
        self._controller.select_output(name)
        return self.get_status()

    def select_model(self, model: Annotated[str, Field(description="One of the models in get_status")]) -> Status:
        """Choose the transcription model."""
        _require_known(model, self._controller.config.stt.choices(), "model")
        self._controller.select_model(model)
        return self.get_status()

    def set_hermes_streaming(self, enabled: bool) -> Status:
        """Pause or resume sending transcript portions to Hermes. Applies immediately, also while recording."""
        if enabled != self._controller.hermes_enabled:
            self._controller.toggle_hermes()
        return self.get_status()

    def set_hermes_endpoint(self, mode: PayloadMode) -> Status:
        """How events are sent to Hermes: /responses, /chat/completions or the raw event. Applies immediately."""
        self._controller.select_payload_mode(mode)
        return self.get_status()

    def refresh_devices(self) -> DeviceList:
        """Re-read the audio devices after a headset was plugged in or out. Only between recordings."""
        if self._controller.is_recording or self._controller.is_busy:
            raise ToolError("Devices can only be refreshed between recordings")
        self._controller.refresh_devices()
        return self.list_devices()

    def finalize_unfinished_sessions(self) -> Status:
        """Write the final transcript of recordings that were cut off by a crash."""
        self._controller.finalize_unfinished()
        return self.get_status()

    def reload_config(self) -> Status:
        """Apply changes made to the config file. If it is invalid, error says why and the old settings stay."""
        self._controller.reload_config()
        return self.get_status()

    def authorize_grok(self) -> Status:
        """Open the xAI sign-in page in the browser. The result arrives as a notification in get_status."""
        if not self._controller.grok_auth_enabled:
            raise ToolError("Grok sign-in is off: set providers.xai.grok_auth: true in the config and reload it")
        self._controller.authorize_grok()
        return self.get_status()

    def start_sketch(
        self, listen: Annotated[bool, Field(
            description="Also draw from what the user says into the microphone; off = only from sketch_text")] = False,
    ) -> Sketch:
        """Open the live sketch window. The user can still turn the microphone on or off in the window."""
        _control(lambda: self._controller.start_sketch(listen))
        return self.get_sketch()

    def sketch_text(
        self,
        text: Annotated[str, Field(min_length=1, description="Explanation in plain spoken sentences")],
        wait: Annotated[bool, Field(description="Return after the text is drawn, at most "
                                                f"{SKETCH_WAIT_S:.0f} s; off = return at once")] = True,
    ) -> Sketch:
        """Draw from text as if the user had said it. Each sentence is one spoken line; a new topic opens a new
        diagram."""
        sketch = self._controller.sketch
        if sketch is None:
            raise ToolError("Live sketch is off. Call start_sketch first.")
        sketch.say(text)
        if wait:
            sketch.wait_until_drawn(SKETCH_WAIT_S)
        return self._sketch_state(sketch)

    def stop_sketch(self) -> Sketch:
        """Close the live sketch window. The diagrams stay saved in the folder."""
        sketch = self._controller.sketch
        if sketch is None:
            raise ToolError("Live sketch is off")
        drawn = self._sketch_state(sketch)
        _control(self._controller.stop_sketch)
        return drawn.model_copy(update={"on": False, "busy": False, "drawing": False})

    # Internals

    def _sketch_state(self, sketch: LiveSketch | None) -> Sketch:
        busy = self._controller.is_sketch_busy
        if sketch is None:
            return Sketch(on=False, busy=busy, drawing=False, folder=None, diagrams=[])
        state = sketch.snapshot()
        diagrams = [SketchDiagram(number=view["number"], title=view["title"], kind=view["kind"],
                                  active=view["number"] == state.active, file=_optional_str(sketch.file(view["number"])))
                    for view in state.views]
        return Sketch(on=True, busy=busy, drawing=not sketch.wait_until_drawn(0), folder=str(sketch.folder),
                      diagrams=diagrams)

    def _summary(self, files: SessionFiles) -> SessionSummary:
        meta = read_meta(files)
        return SessionSummary(
            session_id=files.session_id,
            started_at=meta.started_at if meta else None,
            ended_at=meta.ended_at if meta else None,
            finalized=files.final.exists(),
            recording=self._is_recording(files),
        )

    def _session_files(self, session_id: str | None) -> SessionFiles:
        """The requested session, else the one recording now, else the latest one."""
        output = self._controller.config.output_path()
        wanted = session_id or self._controller.session_id
        if wanted is None:
            sessions = find_sessions(output)
            if not sessions:
                raise ToolError("Nothing has been recorded yet")
            return sessions[-1]
        files = SessionFiles(output, wanted)
        if not (_SESSION_ID.fullmatch(wanted) and files.jsonl.exists()):
            raise ToolError(f"No session '{wanted}'. list_sessions shows the recorded ones.")
        return files

    def _is_recording(self, files: SessionFiles) -> bool:
        return files.session_id == self._controller.session_id


def _control[T](action: Callable[[], T]) -> T:
    """Run a start or stop, handing its failure to the AI as a readable tool error."""
    try:
        return action()
    except ControlError as exc:
        raise ToolError(str(exc)) from exc


def _require_known(value: str | None, known: list[str], kind: str) -> None:
    if value is not None and value not in known:
        raise ToolError(f"Unknown {kind} '{value}'. Choose one of: {', '.join(known)}")


def _optional_str(path: Path | None) -> str | None:
    return str(path) if path is not None else None


def _now() -> str:
    return datetime.now().astimezone().isoformat(timespec="seconds")
