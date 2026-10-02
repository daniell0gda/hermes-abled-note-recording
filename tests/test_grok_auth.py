import json
import time
from pathlib import Path
from types import SimpleNamespace

import pytest

from listening_app import transcriber
from listening_app.config import AppConfig, Language
from listening_app.transcriber import GrokAuthError, SttSettings, grok_access_token, grok_authorized, litellm_transcribe
from tests.test_notifications import controller_with

GROK_AUTH_CONFIG = "providers:\n  xai:\n    grok_auth: true\n"

def grok_config(selected: str) -> AppConfig:
    return AppConfig.model_validate({"stt": {"selected": selected}, "providers": {"xai": {"grok_auth": True}}})


GROK_CONFIG = grok_config("xai/grok-voice-transcribe-2.0")


class FakeLitellm:
    def __init__(self) -> None:
        self.options: dict[str, object] = {}

    def transcription(self, **options: object) -> SimpleNamespace:
        self.options = options
        return SimpleNamespace(text="Dzień dobry")


def test_settings_for_an_xai_model_use_grok_auth() -> None:
    assert SttSettings.from_config(GROK_CONFIG).grok_auth


def test_settings_for_other_providers_keep_their_api_key() -> None:
    assert not SttSettings.from_config(grok_config("groq/whisper-large-v3-turbo")).grok_auth


def test_transcription_sends_the_grok_access_token(monkeypatch: pytest.MonkeyPatch) -> None:
    litellm = FakeLitellm()
    monkeypatch.setattr(transcriber, "import_litellm", lambda: litellm)
    monkeypatch.setattr(transcriber, "grok_access_token", lambda: "grok-token")

    text = litellm_transcribe(b"wav", SttSettings.from_config(GROK_CONFIG))

    assert text == "Dzień dobry"
    assert litellm.options["api_key"] == "grok-token"
    assert litellm.options["language"] == Language.POLISH.value


def test_missing_authorization_points_to_the_tray_menu(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("XAI_OAUTH_TOKEN_DIR", str(tmp_path))

    with pytest.raises(GrokAuthError, match="Authorize Grok"):
        grok_access_token()


def use_token_dir(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    token_dir = tmp_path / "xai_oauth"
    token_dir.mkdir()
    monkeypatch.setenv("XAI_OAUTH_TOKEN_DIR", str(token_dir))
    return token_dir


def save_unexpired_authorization(token_dir: Path) -> None:
    record = {"access_token": "access", "refresh_token": "refresh", "expires_at": time.time() + 3600}
    (token_dir / "auth.json").write_text(json.dumps(record), encoding="utf-8")


def test_a_saved_unexpired_authorization_is_valid(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    save_unexpired_authorization(use_token_dir(tmp_path, monkeypatch))

    assert grok_authorized()


def test_a_missing_authorization_is_not_valid(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    use_token_dir(tmp_path, monkeypatch)

    assert not grok_authorized()


def test_the_menu_flags_a_missing_grok_authorization(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    use_token_dir(tmp_path, monkeypatch)
    controller, _ = controller_with(tmp_path, monkeypatch, GROK_AUTH_CONFIG)

    controller.check_grok_authorization()

    assert controller.needs_grok_authorization


def test_the_menu_does_not_flag_a_valid_grok_authorization(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    save_unexpired_authorization(use_token_dir(tmp_path, monkeypatch))
    controller, _ = controller_with(tmp_path, monkeypatch, GROK_AUTH_CONFIG)

    controller.check_grok_authorization()

    assert not controller.needs_grok_authorization


def test_the_grok_authorization_is_not_flagged_when_grok_auth_is_off(tmp_path: Path,
                                                                     monkeypatch: pytest.MonkeyPatch) -> None:
    use_token_dir(tmp_path, monkeypatch)
    controller, _ = controller_with(tmp_path, monkeypatch, "language: pl\n")

    controller.check_grok_authorization()

    assert not controller.needs_grok_authorization
