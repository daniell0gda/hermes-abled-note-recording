"""Plain text to the Windows clipboard through the Win32 API."""

import ctypes
from ctypes import wintypes

CF_UNICODETEXT = 13
GMEM_MOVEABLE = 0x0002

_user32 = ctypes.WinDLL("user32", use_last_error=True)
_kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
_kernel32.GlobalAlloc.argtypes = [wintypes.UINT, ctypes.c_size_t]
_kernel32.GlobalAlloc.restype = wintypes.HGLOBAL
_kernel32.GlobalLock.argtypes = [wintypes.HGLOBAL]
_kernel32.GlobalLock.restype = wintypes.LPVOID
_kernel32.GlobalUnlock.argtypes = [wintypes.HGLOBAL]
_kernel32.GlobalFree.argtypes = [wintypes.HGLOBAL]
_user32.OpenClipboard.argtypes = [wintypes.HWND]
_user32.SetClipboardData.argtypes = [wintypes.UINT, wintypes.HANDLE]
_user32.SetClipboardData.restype = wintypes.HANDLE


def copy_text(text: str) -> None:
    """Replace the clipboard contents with `text`. Raises OSError when another program holds the clipboard."""
    handle = _global_text(text)
    if not _user32.OpenClipboard(None):
        _kernel32.GlobalFree(handle)
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        _user32.EmptyClipboard()
        if not _user32.SetClipboardData(CF_UNICODETEXT, handle):
            _kernel32.GlobalFree(handle)
            raise ctypes.WinError(ctypes.get_last_error())
    finally:
        _user32.CloseClipboard()


def _global_text(text: str) -> int:
    buffer = ctypes.create_unicode_buffer(text)
    size = ctypes.sizeof(buffer)
    handle = _kernel32.GlobalAlloc(GMEM_MOVEABLE, size)
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    ctypes.memmove(_kernel32.GlobalLock(handle), buffer, size)
    _kernel32.GlobalUnlock(handle)
    return int(handle)
