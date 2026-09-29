"""Filesystem locations used by the app, for both source runs and the PyInstaller build."""

import os
import sys
from pathlib import Path

APP_NAME = "ListeningApp"
CONFIG_FILE_NAME = "config.yaml"
EXAMPLE_CONFIG_NAME = "config.example.yaml"

_PACKAGE_DIR = Path(__file__).resolve().parent
ASSETS_DIR = _PACKAGE_DIR / "assets"


def is_frozen() -> bool:
    return bool(getattr(sys, "frozen", False))


def install_dir() -> Path:
    """Folder of the .exe when frozen, the project root when run from source."""
    if is_frozen():
        return Path(sys.executable).resolve().parent
    return _PACKAGE_DIR.parent


def bundle_dir() -> Path:
    """Folder holding files bundled with the app (PyInstaller unpacks them to _MEIPASS)."""
    return Path(getattr(sys, "_MEIPASS", _PACKAGE_DIR.parent))


def app_data_dir() -> Path:
    return Path(os.environ.get("APPDATA", str(Path.home()))) / APP_NAME


def logs_dir() -> Path:
    return app_data_dir() / "logs"


def example_config_path() -> Path:
    return bundle_dir() / EXAMPLE_CONFIG_NAME
