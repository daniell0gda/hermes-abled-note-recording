from pathlib import Path
from types import SimpleNamespace

import pytest

from listening_app import transcriber
from listening_app.config import AppConfig, Language
from listening_app.transcriber import GrokAuthError, SttSettings, grok_access_token, litellm_transcribe

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
