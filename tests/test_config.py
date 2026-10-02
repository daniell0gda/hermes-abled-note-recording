from pathlib import Path

import pytest

from listening_app.config import (
    AppConfig,
    ConfigError,
    ConfigManager,
    KeySource,
    Language,
    PayloadMode,
    describe,
    load_config,
)

EXAMPLE = Path(__file__).resolve().parent.parent / "config.example.yaml"


@pytest.fixture
def config_file(tmp_path: Path) -> Path:
    path = tmp_path / "config.yaml"
    path.write_text(EXAMPLE.read_text(encoding="utf-8"), encoding="utf-8")
    return path


@pytest.fixture(autouse=True)
def clean_env(monkeypatch: pytest.MonkeyPatch) -> None:
    for name in ("GROQ_API_KEY", "OPENAI_API_KEY", "XAI_API_KEY", "HERMES_API_KEY", "TYPESAFE_API_KEY"):
        monkeypatch.delenv(name, raising=False)


def test_example_config_is_valid(config_file: Path) -> None:
    config = load_config(config_file)

    assert config.language is Language.POLISH
    assert config.hermes.payload_mode is PayloadMode.RESPONSES
    assert config.hermes.chunk_kb == 4
    assert config.stt.selected == "groq/whisper-large-v3-turbo"


def test_environment_variable_wins_over_config_key(config_file: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    config_file.write_text("providers:\n  groq: { api_key: from-config }\nhermes:\n  api_key: hermes-config\n")
    monkeypatch.setenv("GROQ_API_KEY", "from-env")

    config = load_config(config_file)

    assert config.stt_key("groq/whisper-large-v3-turbo").value == "from-env"
    assert config.stt_key("groq/whisper-large-v3-turbo").source is KeySource.ENVIRONMENT
    assert config.hermes_key().value == "hermes-config"
    assert config.hermes_key().source is KeySource.CONFIG


def test_missing_key_is_reported_as_missing() -> None:
    assert AppConfig().stt_key("openai/gpt-4o-mini-transcribe").source is KeySource.MISSING


def test_keys_never_appear_in_the_description(config_file: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    config_file.write_text("providers:\n  openai: { api_key: sk-secret-value }\n")
    monkeypatch.setenv("HERMES_API_KEY", "hermes-secret-value")

    text = describe(load_config(config_file))

    assert "sk-secret-value" not in text
    assert "hermes-secret-value" not in text
    assert "hermes: environment" in text


def test_invalid_values_raise_a_readable_error(config_file: Path) -> None:
    config_file.write_text("segmentation:\n  pause_ms: -5\nhermes:\n  payload_mode: carrier-pigeon\n")

    with pytest.raises(ConfigError, match="segmentation.pause_ms") as error:
        load_config(config_file)
    assert "hermes.payload_mode" in str(error.value)


def test_unknown_keys_are_rejected(config_file: Path) -> None:
    config_file.write_text("segmentation:\n  pause_msec: 800\n")

    with pytest.raises(ConfigError, match="pause_msec"):
        load_config(config_file)


def test_invalid_hotkey_is_rejected(config_file: Path) -> None:
    config_file.write_text("hotkey: r\n")

    with pytest.raises(ConfigError, match="hotkey"):
        load_config(config_file)


def test_reload_keeps_the_last_valid_config(config_file: Path) -> None:
    manager = ConfigManager(config_file)
    manager.reload()
    config_file.write_text("language: klingon\n")

    with pytest.raises(ConfigError):
        manager.reload()
    assert manager.config.language is Language.POLISH


def test_update_persists_the_value_and_keeps_comments(config_file: Path) -> None:
    manager = ConfigManager(config_file)
    manager.reload()

    config = manager.update(("devices", "mic"), "Headset Microphone (Jabra)")

    assert config.devices.mic == "Headset Microphone (Jabra)"
    text = config_file.read_text(encoding="utf-8")
    assert "# glossary: names, project terms" in text
    assert "output: null" in text
    assert load_config(config_file).devices.mic == "Headset Microphone (Jabra)"


def test_grok_auth_replaces_the_xai_api_key(config_file: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    config_file.write_text("providers:\n  xai: { api_key: from-config, grok_auth: true }\n")
    monkeypatch.setenv("XAI_API_KEY", "from-env")

    config = load_config(config_file)

    assert config.uses_grok_auth()
    assert config.stt_key("xai/grok-voice-transcribe-2.0").source is KeySource.GROK_AUTH
    assert config.stt_key("xai/grok-voice-transcribe-2.0").value == ""


def test_grok_auth_is_off_by_default(config_file: Path) -> None:
    assert not load_config(config_file).uses_grok_auth()


def test_grok_auth_on_another_provider_is_rejected(config_file: Path) -> None:
    config_file.write_text("providers:\n  groq: { grok_auth: true }\n")

    with pytest.raises(ConfigError, match="grok_auth is only supported for xai, not groq"):
        load_config(config_file)


def test_update_switches_the_hermes_endpoint(config_file: Path) -> None:
    manager = ConfigManager(config_file)
    manager.reload()

    manager.update(("hermes", "payload_mode"), PayloadMode.CHAT.value)

    assert load_config(config_file).hermes.payload_mode is PayloadMode.CHAT
    assert "# responses | chat | raw" in config_file.read_text(encoding="utf-8")


def test_selected_model_missing_from_the_list_is_still_offered() -> None:
    config = AppConfig.model_validate({"stt": {"selected": "openai/whisper-1", "models": ["groq/whisper-large-v3"]}})

    assert config.stt.choices() == ["groq/whisper-large-v3", "openai/whisper-1"]


def test_sketch_defaults(config_file: Path) -> None:
    sketch = load_config(config_file).sketch

    assert sketch.hotkey == "ctrl+alt+d"
    assert sketch.jev_model == "jev-latest"
    assert sketch.draw_model == "xai/grok-4-fast-non-reasoning"
    assert sketch.label_language is Language.AUTO
    assert sketch.point_radius_px == 24
    assert sketch.point_window_s == 2


def test_typesafe_environment_variable_wins_over_the_sketch_key(config_file: Path,
                                                                monkeypatch: pytest.MonkeyPatch) -> None:
    config_file.write_text("sketch:\n  api_key: from-config\n")
    monkeypatch.setenv("TYPESAFE_API_KEY", "from-env")

    key = load_config(config_file).jev_key()

    assert key.value == "from-env"
    assert key.source is KeySource.ENVIRONMENT


def test_missing_typesafe_key_is_reported_as_missing() -> None:
    assert AppConfig().jev_key().source is KeySource.MISSING


def test_empty_sketch_hotkey_disables_it(config_file: Path) -> None:
    config_file.write_text('sketch:\n  hotkey: ""\n')

    assert load_config(config_file).sketch.hotkey == ""


def test_invalid_sketch_hotkey_is_rejected(config_file: Path) -> None:
    config_file.write_text("sketch:\n  hotkey: d\n")

    with pytest.raises(ConfigError, match="sketch.hotkey"):
        load_config(config_file)


def test_sketch_hotkey_must_differ_from_the_recording_hotkey(config_file: Path) -> None:
    config_file.write_text('hotkey: "ctrl+alt+r"\nsketch:\n  hotkey: "Ctrl + Alt + R"\n')

    with pytest.raises(ConfigError, match="sketch.hotkey must differ from hotkey"):
        load_config(config_file)


def test_sketch_keys_are_redacted_from_logs(config_file: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    config_file.write_text("sketch:\n  api_key: typesafe-secret\n  draw_model: mistral/mistral-small-latest\n")
    monkeypatch.setenv("MISTRAL_API_KEY", "mistral-secret")

    secrets = load_config(config_file).secret_values()

    assert "typesafe-secret" in secrets
    assert "mistral-secret" in secrets
