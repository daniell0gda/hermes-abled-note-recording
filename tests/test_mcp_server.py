import socket
from collections.abc import Callable
from datetime import datetime
from pathlib import Path
from typing import Any

import anyio
import pytest
from mcp import Client
from mcp_types import CallToolResult, TextContent, Tool

from listening_app import app, main, mcp_server
from listening_app.app import PRIVACY_REMINDER, AppController
from listening_app.config import AppConfig, ConfigManager
from listening_app.devices import AudioDevice
from listening_app.hotkey import Hotkey
from listening_app.logging_setup import RedactingFormatter
from listening_app.mcp_http import LOCALHOST, McpHttpServer
from listening_app.mcp_server import McpApp
from listening_app.models import HermesStatus, SegmentStatus, Source, TranscriptSegment
from listening_app.session import SessionError
from listening_app.sketch.feed import typed_lines
from listening_app.sketch.pipeline import SketchState
from listening_app.transcript_store import Devices, SessionFiles, SessionMeta, TranscriptStore

SESSION = "2026-09-29T10-00-00_ab12"
CONFIG = """\
output_dir: "{output}"
hotkey: ""
hermes:
  enabled: false
stt:
  selected: groq/whisper-large-v3-turbo
  models: [groq/whisper-large-v3-turbo, openai/gpt-4o-mini-transcribe]
"""


class FakeAudio:
    def inputs(self) -> list[AudioDevice]:
        return [AudioDevice(1, "Headset Microphone", 1, 48_000)]

    def outputs(self) -> list[AudioDevice]:
        return [AudioDevice(2, "Headset Earphone", 2, 48_000)]

    def default_input_name(self) -> str:
        return "Headset Microphone"

    def default_output_name(self) -> str:
        return "Headset Earphone"


class FakeSession:
    """Stands in for RecordingSession: writes one transcribed segment instead of capturing audio."""

    def __init__(self, config: AppConfig, *_: object) -> None:
        self.session_id = SESSION
        self.files = SessionFiles(config.output_path(), SESSION)
        self._store: TranscriptStore | None = None

    def start(self) -> None:
        self._store = TranscriptStore(self.files, meta(SESSION))
        self._store.append(segment(0))

    def stop(self) -> SessionFiles:
        assert self._store is not None
        self._store.close(datetime.now().astimezone())
        return self.files


class FakeSketch:
    """Stands in for LiveSketch: each typed sentence becomes a diagram node instead of a Grok drawing."""

    started: list["FakeSketch"] = []

    def __init__(self, config: AppConfig, *_: object, **__: object) -> None:
        self.folder = config.output_path() / "diagrams" / SESSION
        self.listening: bool | None = None
        self.lines: list[str] = []
        self.stopped = False

    def start(self, recording: object, listen: bool = True) -> None:
        self.listening = listen
        FakeSketch.started.append(self)

    def stop(self) -> None:
        self.stopped = True

    def say(self, text: str) -> None:
        self.lines += typed_lines(text)

    def wait_until_drawn(self, timeout_s: float) -> bool:
        return True

    def snapshot(self) -> SketchState:
        view = {"number": 1, "title": "Login", "kind": "flowchart", "nodes": self.lines}
        return SketchState(views=(view,) if self.lines else (), active=1 if self.lines else None, updated=None)

    def file(self, number: int) -> Path | None:
        return self.folder / f"{number:02d}-login.html"


class FakeHotkey:
    """Stands in for GlobalHotkey: records registrations instead of claiming the key in Windows."""

    registered: list[Hotkey] = []

    def __init__(self, hotkey: Hotkey, action: Callable[[], None]) -> None:
        self.hotkey = hotkey

    def start(self) -> None:
        FakeHotkey.registered.append(self.hotkey)

    def stop(self) -> None:
        pass


class FailingSession(FakeSession):
    def start(self) -> None:
        raise SessionError("No usable microphone or audio output found")


def meta(session_id: str) -> SessionMeta:
    return SessionMeta(session_id=session_id, started_at="2026-09-29T10:00:00+02:00", stt_model="groq/whisper",
                       devices=Devices(mic="Headset Microphone", output="Headset Earphone"), language="pl")


def segment(seq: int) -> TranscriptSegment:
    return TranscriptSegment(seq=seq, source=Source.ME, start=float(seq), end=seq + 1.0,
                             wall_start="2026-09-29T10:00:00.000+02:00", text=f"zdanie {seq}", language="pl",
                             stt_model="groq/whisper", status=SegmentStatus.OK, hermes_status=HermesStatus.DISABLED)


def transcripts(tmp_path: Path) -> Path:
    return tmp_path / "transcripts"


def make_controller(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, hotkeys: bool,
                    extra_config: str = "") -> AppController:
    monkeypatch.setattr(app, "AudioSystem", FakeAudio)
    monkeypatch.setattr(app, "RecordingSession", FakeSession)
    monkeypatch.setattr(app, "LiveSketch", FakeSketch)
    monkeypatch.setattr(FakeSketch, "started", [])
    path = tmp_path / "config.yaml"
    path.write_text(CONFIG.format(output=transcripts(tmp_path).as_posix()) + extra_config, encoding="utf-8")
    configs = ConfigManager(path)
    configs.reload()
    return AppController(configs, RedactingFormatter(), hotkeys=hotkeys)


@pytest.fixture
def headless(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> McpApp:
    controller = make_controller(tmp_path, monkeypatch, hotkeys=False)
    frontend = McpApp(controller)
    controller.attach(frontend)
    return frontend


def call(frontend: McpApp, tool: str, **arguments: Any) -> CallToolResult:
    async def run() -> CallToolResult:
        async with Client(frontend.server) as client:
            return await client.call_tool(tool, arguments)

    return anyio.run(run)


def list_tools(frontend: McpApp) -> list[Tool]:
    async def run() -> list[Tool]:
        async with Client(frontend.server) as client:
            return (await client.list_tools()).tools

    return anyio.run(run)


def data(result: CallToolResult) -> dict[str, Any]:
    assert not result.is_error, error_text(result)
    content = result.structured_content
    assert isinstance(content, dict)
    return content


def error_text(result: CallToolResult) -> str:
    return " ".join(block.text for block in result.content if isinstance(block, TextContent))


def test_queries_are_marked_read_only_and_actions_are_not(headless: McpApp) -> None:
    tools = list_tools(headless)

    read_only = {tool.name for tool in tools if tool.annotations and tool.annotations.read_only_hint}

    assert read_only == {"get_status", "list_devices", "list_sessions", "get_transcript", "get_sketch"}
    assert {"start_recording", "stop_recording", "select_microphone", "select_model",
            "start_sketch", "sketch_text", "stop_sketch"} <= {tool.name for tool in tools}


def test_status_shows_the_idle_app_and_its_selections(headless: McpApp) -> None:
    status = data(call(headless, "get_status"))

    assert status["recording"] is False
    assert status["session_id"] is None
    assert status["microphone"] is None
    assert status["model"] == "groq/whisper-large-v3-turbo"
    assert status["models"] == ["groq/whisper-large-v3-turbo", "openai/gpt-4o-mini-transcribe"]
    assert status["hermes_streaming"] is False


def test_devices_list_the_selectable_names_and_the_defaults(headless: McpApp) -> None:
    devices = data(call(headless, "list_devices"))

    assert devices["microphones"] == ["Headset Microphone"]
    assert devices["outputs"] == ["Headset Earphone"]
    assert devices["default_microphone"] == "Headset Microphone"
    assert devices["selected_microphone"] is None


def test_recording_is_started_and_stopped_with_the_final_transcript(headless: McpApp) -> None:
    started = data(call(headless, "start_recording"))
    recording = data(call(headless, "get_status"))
    stopped = data(call(headless, "stop_recording"))

    assert started == {"session_id": SESSION, "reminder": PRIVACY_REMINDER}
    assert recording["recording"] is True
    assert recording["session_id"] == SESSION
    assert stopped["stats"]["segments"] == 1
    assert Path(stopped["transcript_file"]).name == f"{SESSION}.json"
    assert data(call(headless, "get_status"))["recording"] is False


def test_start_and_stop_in_the_wrong_state_are_refused(headless: McpApp) -> None:
    stop = call(headless, "stop_recording")
    data(call(headless, "start_recording"))
    start_again = call(headless, "start_recording")
    data(call(headless, "stop_recording"))

    assert stop.is_error
    assert "Not recording" in error_text(stop)
    assert start_again.is_error
    assert "Already recording" in error_text(start_again)


def test_a_recording_that_cannot_start_says_why(headless: McpApp, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(app, "RecordingSession", FailingSession)

    result = call(headless, "start_recording")
    status = data(call(headless, "get_status"))

    assert result.is_error
    assert "No usable microphone" in error_text(result)
    assert status["recording"] is False
    assert status["busy"] is False
    assert status["notifications"][-1]["title"] == "Recording could not start"


def test_unknown_devices_and_models_are_refused(headless: McpApp) -> None:
    microphone = call(headless, "select_microphone", name="Laptop Microphone")
    model = call(headless, "select_model", model="groq/whisper-tiny")
    status = data(call(headless, "get_status"))

    assert microphone.is_error
    assert "Headset Microphone" in error_text(microphone)
    assert model.is_error
    assert status["microphone"] is None
    assert status["model"] == "groq/whisper-large-v3-turbo"


def test_selections_are_saved_to_the_config(headless: McpApp, tmp_path: Path) -> None:
    microphone = data(call(headless, "select_microphone", name="Headset Microphone"))
    model = data(call(headless, "select_model", model="openai/gpt-4o-mini-transcribe"))
    saved = (tmp_path / "config.yaml").read_text(encoding="utf-8")
    back_to_default = data(call(headless, "select_microphone", name=None))

    assert microphone["microphone"] == "Headset Microphone"
    assert model["model"] == "openai/gpt-4o-mini-transcribe"
    assert "mic: Headset Microphone" in saved
    assert "selected: openai/gpt-4o-mini-transcribe" in saved
    assert back_to_default["microphone"] is None


def test_live_transcript_is_read_in_pages_by_offset(headless: McpApp, tmp_path: Path,
                                                    monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(mcp_server, "MAX_SEGMENTS_PER_CALL", 2)
    store = TranscriptStore(SessionFiles(transcripts(tmp_path), SESSION), meta(SESSION))
    for seq in range(3):
        store.append(segment(seq))

    first = data(call(headless, "get_transcript"))
    rest = data(call(headless, "get_transcript", offset=first["next_offset"]))
    store.close(datetime.now().astimezone())

    assert first["session_id"] == SESSION
    assert [line["seq"] for line in first["segments"]] == [0, 1]
    assert first["has_more"] is True
    assert [line["seq"] for line in rest["segments"]] == [2]
    assert rest["has_more"] is False
    assert rest["next_offset"] == 3


def test_transcripts_outside_the_transcripts_folder_are_refused(headless: McpApp, tmp_path: Path) -> None:
    (tmp_path / "secret.jsonl").write_text(segment(0).to_json_line() + "\n", encoding="utf-8")

    for session_id in ("..\\secret", "../secret", "2026-01-01T00-00-00_none"):
        result = call(headless, "get_transcript", session_id=session_id)

        assert result.is_error, session_id
        assert "list_sessions" in error_text(result)


def test_sessions_are_listed_newest_first(headless: McpApp, tmp_path: Path) -> None:
    older, newer = "2026-09-28T09-00-00_aaaa", "2026-09-29T09-00-00_bbbb"
    for session_id in (older, newer):
        TranscriptStore(SessionFiles(transcripts(tmp_path), session_id), meta(session_id)).close(
            datetime.fromisoformat("2026-09-29T11:00:00+02:00"))

    sessions = data(call(headless, "list_sessions"))["sessions"]

    assert [session["session_id"] for session in sessions] == [newer, older]
    assert sessions[0]["ended_at"] == "2026-09-29T11:00:00+02:00"
    assert sessions[0]["finalized"] is False
    assert sessions[0]["recording"] is False


def test_live_sketch_started_by_an_agent_keeps_the_microphone_off(headless: McpApp) -> None:
    started = data(call(headless, "start_sketch"))

    assert started["on"] is True
    assert started["diagrams"] == []
    assert [sketch.listening for sketch in FakeSketch.started] == [False]


def test_live_sketch_can_also_listen_to_the_microphone(headless: McpApp) -> None:
    data(call(headless, "start_sketch", listen=True))

    assert [sketch.listening for sketch in FakeSketch.started] == [True]


def test_typed_text_is_drawn_and_its_diagram_file_is_returned(headless: McpApp) -> None:
    data(call(headless, "start_sketch"))

    drawn = data(call(headless, "sketch_text", text="The user logs in. The server checks the password."))

    assert FakeSketch.started[0].lines == ["The user logs in.", "The server checks the password."]
    assert drawn["drawing"] is False
    assert drawn["diagrams"] == [{"number": 1, "title": "Login", "kind": "flowchart", "active": True,
                                  "file": str(FakeSketch.started[0].file(1))}]


def test_stopping_live_sketch_returns_the_saved_diagrams(headless: McpApp) -> None:
    data(call(headless, "start_sketch"))
    data(call(headless, "sketch_text", text="The user logs in."))

    stopped = data(call(headless, "stop_sketch"))

    assert stopped["on"] is False
    assert [diagram["title"] for diagram in stopped["diagrams"]] == ["Login"]
    assert FakeSketch.started[0].stopped is True
    assert data(call(headless, "get_sketch"))["on"] is False


def test_live_sketch_in_the_wrong_state_is_refused(headless: McpApp) -> None:
    text_while_off = call(headless, "sketch_text", text="The user logs in.")
    stop_while_off = call(headless, "stop_sketch")
    data(call(headless, "start_sketch"))
    start_again = call(headless, "start_sketch")

    assert text_while_off.is_error
    assert "start_sketch" in error_text(text_while_off)
    assert stop_while_off.is_error
    assert start_again.is_error
    assert "already on" in error_text(start_again)


def test_the_tray_app_serves_the_tools_over_http(headless: McpApp) -> None:
    async def status_over_http(url: str) -> CallToolResult:
        async with Client(url) as client:
            return await client.call_tool("get_status", {})

    endpoint = McpHttpServer(headless.server, port=0)
    endpoint.start()
    try:
        status = data(anyio.run(status_over_http, endpoint.url))
    finally:
        endpoint.stop()

    assert endpoint.url.startswith(f"http://{LOCALHOST}:")
    assert status["recording"] is False


def test_a_taken_port_is_announced_and_the_tray_app_runs_without_mcp(tmp_path: Path,
                                                                     monkeypatch: pytest.MonkeyPatch) -> None:
    with socket.create_server((LOCALHOST, 0)) as taken:
        port = taken.getsockname()[1]
        controller = make_controller(tmp_path, monkeypatch, hotkeys=True, extra_config=f"mcp:\n  port: {port}\n")
        notices: list[tuple[str, str]] = []

        endpoint = main._serve_mcp(controller, notices)

    assert endpoint is None
    assert notices == [("MCP server not available", f"Port {port} is in use. Choose another mcp.port.")]


def test_the_headless_mode_leaves_the_global_hotkeys_to_the_tray_app(headless: McpApp,
                                                                     monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(app, "GlobalHotkey", FakeHotkey)
    monkeypatch.setattr(FakeHotkey, "registered", [])

    data(call(headless, "reload_config"))

    assert FakeHotkey.registered == []


def test_the_tray_app_registers_its_global_hotkeys(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(app, "GlobalHotkey", FakeHotkey)
    monkeypatch.setattr(FakeHotkey, "registered", [])
    controller = make_controller(tmp_path, monkeypatch, hotkeys=True)

    controller.reload_config()

    assert len(FakeHotkey.registered) == 2
