"""Rotating file logging with API-key redaction."""

import logging
import sys
import threading
from collections.abc import Iterable
from logging.handlers import RotatingFileHandler
from pathlib import Path

LOG_FILE_NAME = "listening_app.log"
_FORMAT = "%(asctime)s %(levelname)-7s [%(threadName)s] %(name)s: %(message)s"
_MAX_BYTES = 2_000_000
_BACKUP_COUNT = 5
_NOISY_LOGGERS = ("LiteLLM", "litellm", "httpx", "httpcore", "openai")
_REDACTED = "***"


class RedactingFormatter(logging.Formatter):
    """Replaces known secret values anywhere in the formatted record, tracebacks included."""

    def __init__(self) -> None:
        super().__init__(_FORMAT)
        self._secrets: tuple[str, ...] = ()

    def set_secrets(self, secrets: Iterable[str]) -> None:
        self._secrets = tuple(secret for secret in secrets if secret)

    def format(self, record: logging.LogRecord) -> str:
        text = super().format(record)
        for secret in self._secrets:
            text = text.replace(secret, _REDACTED)
        return text


def setup_logging(log_dir: Path, level: str, console: bool) -> RedactingFormatter:
    log_dir.mkdir(parents=True, exist_ok=True)
    formatter = RedactingFormatter()
    handlers: list[logging.Handler] = [
        RotatingFileHandler(log_dir / LOG_FILE_NAME, maxBytes=_MAX_BYTES, backupCount=_BACKUP_COUNT, encoding="utf-8")
    ]
    if console and sys.stderr is not None:
        handlers.append(logging.StreamHandler(sys.stderr))
    root = logging.getLogger()
    for handler in handlers:
        handler.setFormatter(formatter)
        root.addHandler(handler)
    set_level(level)
    for name in _NOISY_LOGGERS:
        logging.getLogger(name).setLevel(logging.WARNING)
    threading.excepthook = _log_thread_exception
    return formatter


def _log_thread_exception(args: threading.ExceptHookArgs) -> None:
    """The windowed exe has no stderr, so uncaught errors in threads must reach the log file."""
    thread_name = args.thread.name if args.thread else "unknown"
    exc_info = (args.exc_type, args.exc_value, args.exc_traceback) if args.exc_value else None
    logging.getLogger(__name__).error("Unhandled error in thread %s", thread_name, exc_info=exc_info)


def set_level(level: str) -> None:
    logging.getLogger().setLevel(level)
