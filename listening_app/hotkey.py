"""System-wide hotkey through the Win32 RegisterHotKey API (no keyboard hook, no admin rights)."""

import ctypes
import logging
import threading
from collections.abc import Callable
from ctypes import wintypes
from dataclasses import dataclass

log = logging.getLogger(__name__)

MOD_ALT = 0x0001
MOD_CONTROL = 0x0002
MOD_SHIFT = 0x0004
MOD_WIN = 0x0008
MOD_NOREPEAT = 0x4000
WM_HOTKEY = 0x0312
WM_QUIT = 0x0012
VK_F1 = 0x70
_HOTKEY_ID = 1
_REGISTRATION_TIMEOUT_S = 5.0

_MODIFIERS = {
    "ctrl": MOD_CONTROL,
    "control": MOD_CONTROL,
    "alt": MOD_ALT,
    "shift": MOD_SHIFT,
    "win": MOD_WIN,
}


class HotkeyError(Exception):
    pass


@dataclass(frozen=True)
class Hotkey:
    modifiers: int
    virtual_key: int


def parse_hotkey(text: str) -> Hotkey:
    """Parse e.g. "ctrl+alt+r" or "ctrl+shift+f9". At least one modifier is required."""
    parts = [part.strip().lower() for part in text.split("+")]
    *modifier_names, key_name = parts
    if not modifier_names:
        raise ValueError(f"hotkey '{text}' needs at least one modifier (ctrl, alt, shift, win)")
    modifiers = 0
    for name in modifier_names:
        if name not in _MODIFIERS:
            raise ValueError(f"unknown hotkey modifier '{name}'")
        modifiers |= _MODIFIERS[name]
    return Hotkey(modifiers, _virtual_key(key_name))


def _virtual_key(name: str) -> int:
    if len(name) == 1 and (name.isascii() and name.isalnum()):
        return ord(name.upper())
    if name.startswith("f") and name[1:].isdigit() and 1 <= int(name[1:]) <= 24:
        return VK_F1 + int(name[1:]) - 1
    raise ValueError(f"unsupported hotkey key '{name}' (use a-z, 0-9 or f1-f24)")


class GlobalHotkey:
    """Runs a message loop on its own thread and calls `callback` on every hotkey press."""

    def __init__(self, hotkey: Hotkey, callback: Callable[[], None]) -> None:
        self._hotkey = hotkey
        self._callback = callback
        self._thread: threading.Thread | None = None
        self._thread_id = 0
        self._registered = threading.Event()
        self._error: str | None = None

    def start(self) -> None:
        self._thread = threading.Thread(target=self._run, name="hotkey", daemon=True)
        self._thread.start()
        self._registered.wait(_REGISTRATION_TIMEOUT_S)
        if self._error:
            raise HotkeyError(self._error)

    def stop(self) -> None:
        if self._thread is None:
            return
        user32 = ctypes.WinDLL("user32", use_last_error=True)
        user32.PostThreadMessageW(self._thread_id, WM_QUIT, 0, 0)
        self._thread.join(timeout=2)
        self._thread = None

    def _run(self) -> None:
        user32 = ctypes.WinDLL("user32", use_last_error=True)
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        self._thread_id = kernel32.GetCurrentThreadId()
        modifiers = self._hotkey.modifiers | MOD_NOREPEAT
        if not user32.RegisterHotKey(None, _HOTKEY_ID, modifiers, self._hotkey.virtual_key):
            self._error = f"the hotkey is already in use (Win32 error {ctypes.get_last_error()})"
            self._registered.set()
            return
        self._registered.set()
        try:
            self._pump_messages(user32)
        finally:
            user32.UnregisterHotKey(None, _HOTKEY_ID)

    def _pump_messages(self, user32: ctypes.WinDLL) -> None:
        msg = wintypes.MSG()
        while user32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
            if msg.message == WM_HOTKEY:
                self._fire()

    def _fire(self) -> None:
        try:
            self._callback()
        except Exception:
            log.exception("Hotkey handler failed")
