# Listening App

A Windows tray app that transcribes your online meetings live. It works with Teams desktop, Teams in a
browser, or any other call app. It records two tracks side by side:

- **me**: your microphone
- **others**: the system audio of the selected output device (WASAPI loopback)

Each track is cut into segments at natural pauses (Silero VAD). Every segment is transcribed by a
configurable speech-to-text model (via litellm, Polish by default). It is then appended to a local JSONL
transcript and streamed to a Hermes endpoint.

Meeting notes and summaries are out of scope. They happen downstream, e.g. in Hermes.

> **Recording meetings requires informing the participants** (GDPR, company policy). The app shows a
> reminder every time you start recording.

## Setup (from source)

Requires Windows 10/11 and Python 3.11+ (developed on 3.12).

```powershell
py -3.12 -m venv .venv
.venv\Scripts\python.exe -m pip install -e ".[dev]"
.venv\Scripts\python.exe -m listening_app            # starts the tray app
```

On first start, the app creates `config.yaml` from `config.example.yaml` and shows where it put it.

### API keys

Set keys as environment variables (preferred) or in `config.yaml`. **The environment variable wins.**

| Key | Environment variable | Config |
|---|---|---|
| Groq STT | `GROQ_API_KEY` | `providers.groq.api_key` |
| OpenAI STT | `OPENAI_API_KEY` | `providers.openai.api_key` |
| Any other litellm provider `<p>/…` | `<P>_API_KEY` | `providers.<p>.api_key` |
| Hermes | `HERMES_API_KEY` | `hermes.api_key` |

```powershell
[Environment]::SetEnvironmentVariable('GROQ_API_KEY', 'gsk_...', 'User')
[Environment]::SetEnvironmentVariable('HERMES_API_KEY', '...', 'User')
```

**Grok account instead of a key (xAI):** set `providers.xai.grok_auth: true`. The tray menu then shows
**Authorize Grok**. It opens the xAI sign-in page in your browser. After you sign in, `xai/…` models
(e.g. `xai/grok-voice-transcribe-2.0`) use your Grok account, and `XAI_API_KEY` is ignored. The tokens
are saved by litellm in `%USERPROFILE%\.config\litellm\xai_oauth\auth.json` and refreshed automatically.
Recording does not start until Grok is authorized.

## Configuration

**Location:** `config.yaml` next to `ListeningApp.exe` (or the project root when run from source) if one
exists there. Otherwise `%APPDATA%\ListeningApp\config.yaml`. `--config PATH` overrides both. The file
is git-ignored.

All options are documented in [`config.example.yaml`](config.example.yaml). The main ones:

| Option | Default | Meaning |
|---|---|---|
| `language` | `pl` | `pl`, `en` or `auto` (language hint for STT) |
| `output_dir` | `%USERPROFILE%\Documents\MeetingTranscripts` | where transcripts go |
| `save_audio` | `false` | keep every segment WAV (failed segments are always kept) |
| `hotkey` | `ctrl+alt+r` | global Start/Stop; `""` disables it |
| `notifications` | `true` | pop-ups; `false` hides them (they are still logged), the icon still shows state |
| `segmentation.*` | 800 / 30 / 400 / 200 / 0.5 | pause ms, max segment s, min segment ms, padding ms, VAD threshold |
| `stt.selected`, `stt.models` | Groq whisper-large-v3-turbo | any litellm transcription model string |
| `stt.prompt` | `""` | glossary for STT: names, project terms |
| `hermes.payload_mode` | `responses` | `responses`, `chat` or `raw` (see below) |

- **Tray selections** (devices, model, Hermes on/off, Hermes endpoint) are saved to the file immediately, and your
  comments in it are preserved.
- **Reload config** applies an edited file without restarting. Recording settings take effect on the
  next recording. Hermes settings, the hotkey and the log level take effect immediately. An invalid file
  shows an error and the previous settings stay in effect.

## Using it

Tray menu: Start/Stop recording (also a left click, or the hotkey), Microphone ▸, Audio output ▸,
Transcription model ▸, Authorize Grok (only with `providers.xai.grok_auth: true`), Hermes streaming on/off,
Hermes endpoint ▸ (`/responses`, `/chat/completions` or raw), Refresh devices, Finalize unfinished sessions (only shown
when there are any), Open transcripts folder, Open config file, Reload config, Quit.

Icon: **grey** idle, **red** recording, **yellow "!"** error (a yellow badge while recording). Hover for
details. Notifications are shown on start, on stop and on problems, unless `notifications: false`. That
also hides the privacy reminder on start.

**Use a headset.** With speakers, your microphone also picks up the other participants, and their speech
ends up duplicated in the "me" track.

If a saved device is missing, the system default is used and you get a notification. If a device is
unplugged mid-meeting, that track stops cleanly and the other one keeps recording.

### Command-line tools

```powershell
python -m listening_app --check-config             # validate + print config, keys masked
python -m listening_app --list-devices             # microphones and outputs (* = default)
python -m listening_app --record-test 30 --out .   # record_me.wav + record_others.wav
python -m listening_app --segment-wav talk.wav [--transcribe]   # offline segmentation / STT check
```

The exe accepts the same flags (e.g. `ListeningApp.exe --list-devices`) and prints to the PowerShell
window it was started from.

### Headless with an AI agent (MCP)

`--mcp` runs the app without the tray, as an [MCP](https://modelcontextprotocol.io) server on
stdin/stdout. An AI client on this PC (Claude Code, Claude Desktop or any other MCP client) starts the
process and controls it through these tools:

| Tool | What it does |
|---|---|
| `get_status` | recording state, selections, the current problem, the latest notifications |
| `start_recording` / `stop_recording` | as in the tray; Stop returns the final transcript file and its stats |
| `get_transcript` | segments of the running (or any) session; pass `next_offset` back as `offset` to follow the meeting live |
| `list_sessions` | recorded sessions, newest first |
| `list_devices`, `select_microphone`, `select_output`, `refresh_devices` | devices; `null` selects the system default |
| `select_model` | one of `stt.models` |
| `set_hermes_streaming`, `set_hermes_endpoint` | Hermes on/off and the payload mode |
| `finalize_unfinished_sessions`, `reload_config`, `authorize_grok` | as in the tray menu |

Register it in Claude Code:

```powershell
claude mcp add listening-app -- C:\Tools\ListeningApp\ListeningApp.exe --mcp
```

Or in any client that takes an `mcpServers` JSON config (Claude Desktop, a project `.mcp.json`):

```json
{"mcpServers": {"listening-app": {"command": "C:\\Tools\\ListeningApp\\ListeningApp.exe", "args": ["--mcp"]}}}
```

From source, use `.venv\Scripts\python.exe` as the command with `-m listening_app --mcp` as the arguments.

- It uses the same config file, transcripts folder and Hermes streaming as the tray app. Selections are
  saved to `config.yaml`. Pop-ups are not shown. Their messages appear in `get_status` instead.
- When the AI client disconnects, a running recording is stopped and finalized. If the client kills the
  process instead, `finalize_unfinished_sessions` recovers the transcript on the next start.
- Run either the tray app or the MCP server, not both. Each would record on its own and resend the same
  queued Hermes portions.

## Output files

Each recording gets its own folder `output_dir\<session_id>\` (`session_id` = start time + random
suffix, e.g. `2026-09-28T10-13-40_a1b2`) with:

| File | Written | Content |
|---|---|---|
| `<session_id>.jsonl` | live, append-only | one segment per line |
| `<session_id>.json` | on Stop (atomic) | full transcript, segments sorted by start, stats |
| `<session_id>.meta.json` | start / stop | devices, model, start/end time |
| `<session_id>.outbox.jsonl` | live, append-only | Hermes delivery journal |
| `audio\*.wav` | failed segments (all with `save_audio`) | 16 kHz mono segment audio |

Segment line:

```json
{"seq":12,"source":"others","start":83.42,"end":91.1,"wall_start":"2026-09-28T10:15:03.420+02:00","text":"No to ustalmy, że release idzie w piątek.","language":"pl","stt_model":"groq/whisper-large-v3-turbo","status":"ok","hermes_status":"queued"}
```

- `status`: `ok` \| `stt_failed` (after 3 attempts; `audio_file` points at the kept WAV, relative to the
  session folder) \| `empty`.
- `hermes_status` in the JSONL is the state when the line was written: `queued`, or `disabled` if
  streaming is off or there is no text to send. The final `.json` carries the delivery result
  (`delivered` \| `queued` \| `failed` \| `disabled`). It is rewritten automatically if queued portions get
  delivered after Stop.
- **Crash safety:** after a crash, the next start finds session folders that have a `.jsonl` but no `.json` and
  offers **Finalize unfinished sessions** in the tray menu.

## Hermes streaming

Every transcribed segment is sent as soon as it is ready. `me` and `others` portions are sent
separately. Delivery runs on a background thread with a persistent outbox:

- One request at a time, strictly in order. A portion counts as delivered only on a 2xx answer.
- Network errors, 401/403/404, 408, 429 and 5xx are retried with exponential backoff (1 s … 60 s).
  400, 409, 413, 415 and 422 are permanent: the portion is marked `failed` and the queue moves on.
- Every request has `Idempotency-Key: <session_id>:<seq>`, so a resend is never processed twice.
- Undelivered portions survive restarts: they are resent, in order, the next time the app starts.
- "Hermes streaming" in the tray pauses and resumes sending without stopping the recording.

Every request has `Authorization: Bearer <key>`. Events are `session_start`, `transcript_portion` and
`session_end` (sent on Stop with totals when `send_session_events: true`):

```json
{"type":"transcript_portion","session_id":"2026-09-28T10-13-40_a1b2","seq":12,"source":"others","start":83.42,"end":91.1,"wall_start":"2026-09-28T10:15:03.420+02:00","text":"...","language":"pl"}
```

`payload_mode` decides how each event is wrapped. Switch it in the config or with **Hermes endpoint ▸**
in the tray. The change applies immediately, including to portions still queued:

| Mode | Request |
|---|---|
| `responses` (default) | `POST {url}/responses` with `input` = event JSON, `conversation` = session_id (the whole meeting is one Hermes conversation), `store: true`, `background: true` (returns without waiting for the agent turn) |
| `chat` | `POST {url}/chat/completions` with the event JSON as one user message, header `X-Hermes-Session-Id: <session_id>` |
| `raw` | `POST {url}/{raw_path}` with the event JSON itself |

`hermes.model` defaults to the profile name in the URL (`/p/<profile>/v1`), otherwise `hermes-agent`.
Hermes replies are only logged (at DEBUG).

## Logs and privacy

- Logs: `%APPDATA%\ListeningApp\logs\listening_app.log` (rotating).
- API keys are redacted from every log line. Transcript text is only logged at `log_level: DEBUG`.
- Audio is not stored by default.

## Build the exe

```powershell
.\build.ps1        # -> dist\ListeningApp.exe (+ dist\config.example.yaml)
```

To deploy, copy `ListeningApp.exe` and a filled-in `config.yaml` to one folder.

**Autostart:** press Win+R, type `shell:startup`, and create a shortcut to `ListeningApp.exe` in the
folder that opens.

## Development

```powershell
.venv\Scripts\python.exe -m pytest      # unit tests (segmenter, ordering, outbox, config, store, ...)
.venv\Scripts\python.exe -m mypy        # strict type check
```

| Module | Role |
|---|---|
| `main.py` | entry point: CLI tools, the tray app or the headless MCP server |
| `app.py` | controller: services, tray actions, Hermes events |
| `tray.py` | pystray icon, menu, notifications |
| `mcp_server.py` | headless mode: MCP tools for AI agents instead of the tray |
| `session.py` | one recording: capture → segment → transcribe → store + Hermes |
| `capture.py` | one capture thread per source → 16 kHz mono (fills loopback silence gaps) |
| `devices.py` | WASAPI inputs/outputs, loopback lookup, default fallback |
| `vad.py` / `segmenter.py` | Silero VAD (onnxruntime) and the pure pause/max/min/padding logic |
| `transcriber.py` | STT worker pool, retries, per-source in-order emission |
| `transcript_store.py` | JSONL, final JSON, crash recovery |
| `hermes_client.py` | payload builders, outbox, ordered retrying sender |
| `config.py` | pydantic models, YAML load/save, env overrides |

Implementation notes:

- Silero VAD runs through `onnxruntime` with the model vendored in `listening_app/assets` (MIT, see
  `silero_vad.LICENSE`). This avoids PyTorch and keeps the exe small.
- `ruamel.yaml` is used instead of PyYAML so that saving tray selections keeps the comments in
  `config.yaml`.
- WASAPI loopback delivers no frames while nothing plays. The loopback track is padded with silence
  against the clock, so pauses still end segments and timestamps don't drift.

**Adding notes generation later:** the natural hook is `AppController._stop_session` in `app.py`, right
after the final JSON is written (or `RecordingSession._on_transcribed` in `session.py` for live
processing).
