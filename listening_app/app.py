"""Application controller: owns the long-lived services and implements every tray action."""

import logging
import os
import subprocess
import threading
from pathlib import Path
from typing import Protocol

from listening_app.config import AppConfig, ConfigError, ConfigManager, PayloadMode
from listening_app.devices import AudioSystem, com_initialized
from listening_app.hermes_client import HermesClient, HermesSettings
from listening_app.hotkey import GlobalHotkey, HotkeyError, parse_hotkey
from listening_app.logging_setup import RedactingFormatter, set_level
from listening_app.session import RecordingSession, finalize
from listening_app.transcriber import authorize_grok, import_litellm
from listening_app.transcript_store import SessionFiles, find_outboxes, find_unfinished_sessions

log = logging.getLogger(__name__)

PRIVACY_REMINDER = "Remember to tell the other participants that the meeting is being transcribed."


class Ui(Protocol):
    def notify(self, title: str, message: str) -> None: ...

    def refresh(self) -> None: ...


class AppController:
    def __init__(self, configs: ConfigManager, formatter: RedactingFormatter) -> None:
        self._configs = configs
        self._formatter = formatter
        self._audio = AudioSystem()
        config = configs.config
        self._hermes = HermesClient(HermesSettings.from_config(config), config.hermes.enabled, self)
        self._ui: Ui | None = None
        self._lock = threading.Lock()
        self._finalize_lock = threading.Lock()
        self._session: RecordingSession | None = None
        self._busy = False
        self._authorizing_grok = False
        self._error: str | None = None
        self._hermes_error: str | None = None
        self._hotkey: GlobalHotkey | None = None
        self._hotkey_text = ""
        self._unfinished: list[SessionFiles] = []

    def attach(self, ui: Ui) -> None:
        self._ui = ui

    def start_services(self, notices: list[tuple[str, str]], config_error: str | None) -> None:
        self._error = config_error
        for title, message in notices:
            self.notify(title, message)
        self._hermes.start()
        threading.Thread(target=import_litellm, name="litellm-preload", daemon=True).start()
        self._register_hotkey(self.config.hotkey)
        self._restore_outboxes()
        self._scan_unfinished(announce=True)
        self._refresh()

    def shutdown(self) -> None:
        with self._lock:
            session, self._session = self._session, None
        if session is not None:
            self._stop_session(session)
        self._unregister_hotkey()
        self._hermes.close()
        self._audio.close()

    # State shown in the tray

    @property
    def config(self) -> AppConfig:
        return self._configs.config

    @property
    def is_recording(self) -> bool:
        return self._session is not None

    @property
    def is_busy(self) -> bool:
        return self._busy

    @property
    def error(self) -> str | None:
        return self._error or self._hermes_error

    @property
    def hermes_enabled(self) -> bool:
        return self._hermes.enabled

    @property
    def grok_auth_enabled(self) -> bool:
        return self.config.uses_grok_auth()

    @property
    def is_authorizing_grok(self) -> bool:
        return self._authorizing_grok

    @property
    def hotkey_label(self) -> str:
        return "+".join(part.capitalize() for part in self._hotkey_text.split("+")) if self._hotkey else ""

    @property
    def unfinished_count(self) -> int:
        return len(self._unfinished)

    def microphones(self) -> list[str]:
        return [device.name for device in self._audio.inputs()]

    def outputs(self) -> list[str]:
        return [device.name for device in self._audio.outputs()]

    def default_mic_name(self) -> str | None:
        return self._audio.default_input_name()

    def default_output_name(self) -> str | None:
        return self._audio.default_output_name()

    # Tray actions

    def toggle_recording(self) -> None:
        with self._lock:
            if self._busy:
                return
            self._busy = True
            recording = self._session is not None
        self._refresh()
        worker = self._stop_worker if recording else self._start_worker
        threading.Thread(target=worker, name="session-control", daemon=True).start()

    def select_mic(self, name: str | None) -> None:
        self._save(("devices", "mic"), name)

    def select_output(self, name: str | None) -> None:
        self._save(("devices", "output"), name)

    def select_model(self, model: str) -> None:
        self._save(("stt", "selected"), model)

    def toggle_hermes(self) -> None:
        enabled = not self._hermes.enabled
        self._hermes.set_enabled(enabled)
        if not enabled:
            self._hermes_error = None
        self._save(("hermes", "enabled"), enabled)

    def select_payload_mode(self, mode: PayloadMode) -> None:
        self._save(("hermes", "payload_mode"), mode.value)

    def authorize_grok(self) -> None:
        """Open the xAI sign-in page; the browser redirects back to a local port the app listens on."""
        if self._authorizing_grok:
            return
        self._authorizing_grok = True
        self._refresh()
        threading.Thread(target=self._authorize_grok_worker, name="grok-auth", daemon=True).start()

    def refresh_devices(self) -> None:
        if self.is_recording or self._busy:
            self.notify("Stop recording first", "Devices can only be refreshed between recordings.")
            return
        self._audio.refresh()
        self.notify("Devices refreshed", f"{len(self.microphones())} microphones, {len(self.outputs())} outputs.")
        self._refresh()

    def finalize_unfinished(self) -> None:
        self._scan_unfinished(announce=False)
        written = 0
        for files in self._unfinished:
            try:
                self._finalize(files)
                written += 1
            except (OSError, ValueError) as exc:
                log.exception("Finalizing %s failed", files.session_id)
                self.notify("Finalizing failed", f"{files.session_id}: {exc}")
        self._scan_unfinished(announce=False)
        self.notify("Sessions finalized", f"{written} transcript(s) written to {self.config.output_path()}")
        self._refresh()

    def open_transcripts(self) -> None:
        folder = self.config.output_path()
        folder.mkdir(parents=True, exist_ok=True)
        os.startfile(folder)

    def open_config(self) -> None:
        try:
            os.startfile(self._configs.path)
        except OSError:
            subprocess.Popen(["notepad.exe", str(self._configs.path)])

    def reload_config(self) -> None:
        try:
            config = self._configs.reload()
        except ConfigError as exc:
            self._error = f"Invalid config: {exc}"
            self.notify("Invalid config", f"{exc}. The previous settings stay in effect.")
            self._refresh()
            return
        self._error = None
        self._apply(config)
        self.notify("Config reloaded", "Changes apply to the next recording.")
        self._refresh()

    # HermesListener

    def hermes_problem(self, title: str, message: str) -> None:
        self._hermes_error = message
        self.notify(title, message)
        self._refresh()

    def hermes_recovered(self) -> None:
        self._hermes_error = None
        self.notify("Hermes reachable again", "Queued transcript portions are being sent.")
        self._refresh()

    def session_drained(self, journal: Path) -> None:
        """Late deliveries after Stop: rewrite the final JSON so its hermes_status values are current."""
        files = SessionFiles.from_outbox(journal)
        if files.final.exists():
            self._finalize(files)

    # Internals

    def notify(self, title: str, message: str) -> None:
        log.info("Notification: %s - %s", title, message)
        if self._ui is not None and self.config.notifications:
            self._ui.notify(title, message)

    def _refresh(self) -> None:
        if self._ui is not None:
            self._ui.refresh()

    def _start_worker(self) -> None:
        session = RecordingSession(self.config, self._audio, self._hermes, self.notify, self._source_lost)
        try:
            with com_initialized():
                session.start()
        except Exception as exc:
            log.exception("Recording could not start")
            self._error = str(exc)
            self.notify("Recording could not start", str(exc))
            self._finish_transition(None)
            return
        self._error = None
        self._finish_transition(session)
        self.notify("Recording started", PRIVACY_REMINDER)

    def _stop_worker(self) -> None:
        session = self._session
        if session is not None:
            with com_initialized():
                self._stop_session(session)
        self._finish_transition(None)

    def _stop_session(self, session: RecordingSession) -> None:
        try:
            transcript = self._finalize(session.stop())
        except Exception as exc:
            log.exception("Stopping the recording failed")
            self.notify("Stopping the recording failed", f"{exc}. The live .jsonl transcript is kept.")
            return
        self.notify("Recording stopped", f"Transcript saved: {transcript.name}")

    def _authorize_grok_worker(self) -> None:
        try:
            authorize_grok()
        except Exception as exc:
            log.exception("Grok authorization failed")
            self.notify("Grok authorization failed", str(exc))
        else:
            self.notify("Grok authorized", "xAI transcription models now use your Grok account.")
        finally:
            self._authorizing_grok = False
            self._refresh()

    def _finish_transition(self, session: RecordingSession | None) -> None:
        with self._lock:
            self._session = session
            self._busy = False
        self._refresh()

    def _finalize(self, files: SessionFiles) -> Path:
        with self._finalize_lock:
            return finalize(files)

    def _source_lost(self) -> None:
        self._error = "An audio device was lost"
        self._refresh()

    def _save(self, keys: tuple[str, ...], value: str | bool | None) -> None:
        try:
            config = self._configs.update(keys, value)
        except (ConfigError, OSError) as exc:
            self.notify("Could not save the setting", str(exc))
        else:
            self._apply(config)
        self._refresh()

    def _apply(self, config: AppConfig) -> None:
        self._formatter.set_secrets(config.secret_values())
        set_level(config.log_level)
        self._hermes.update_settings(HermesSettings.from_config(config))
        self._hermes.set_enabled(config.hermes.enabled)
        if config.hotkey != self._hotkey_text:
            self._unregister_hotkey()
            self._register_hotkey(config.hotkey)

    def _register_hotkey(self, text: str) -> None:
        self._hotkey_text = text
        if not text:
            return
        hotkey = GlobalHotkey(parse_hotkey(text), self.toggle_recording)
        try:
            hotkey.start()
        except HotkeyError as exc:
            self.notify("Hotkey not available", f"{text}: {exc}")
            return
        self._hotkey = hotkey

    def _unregister_hotkey(self) -> None:
        if self._hotkey is not None:
            self._hotkey.stop()
            self._hotkey = None

    def _restore_outboxes(self) -> None:
        restored = self._hermes.restore(find_outboxes(self.config.output_path()))
        if restored:
            self.notify("Hermes outbox", f"{restored} transcript portion(s) from an earlier run are queued for Hermes.")

    def _scan_unfinished(self, announce: bool) -> None:
        active = {self._session.session_id} if self._session is not None else set()
        self._unfinished = find_unfinished_sessions(self.config.output_path(), exclude=active)
        if announce and self._unfinished:
            self.notify("Unfinished recordings found",
                        f"{len(self._unfinished)} recording(s) were not finalized. "
                        "Use 'Finalize unfinished sessions' in the tray menu.")
