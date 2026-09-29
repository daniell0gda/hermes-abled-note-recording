"""Configuration: pydantic models, YAML load/save and environment-variable overrides for API keys."""

import json
import os
import shutil
import threading
from dataclasses import dataclass, field
from enum import StrEnum
from io import StringIO
from pathlib import Path
from typing import Any

from pydantic import BaseModel, ConfigDict, Field, SecretStr, ValidationError, field_validator
from ruamel.yaml import YAML
from ruamel.yaml.comments import CommentedMap
from ruamel.yaml.error import YAMLError
from ruamel.yaml.representer import RoundTripRepresenter

from listening_app import paths
from listening_app.files import atomic_write_text
from listening_app.hotkey import parse_hotkey

HERMES_KEY_ENV = "HERMES_API_KEY"
DEFAULT_PROVIDER = "openai"
XAI_PROVIDER = "xai"


class Language(StrEnum):
    POLISH = "pl"
    ENGLISH = "en"
    AUTO = "auto"


class PayloadMode(StrEnum):
    RESPONSES = "responses"
    CHAT = "chat"
    RAW = "raw"


class LogLevel(StrEnum):
    DEBUG = "DEBUG"
    INFO = "INFO"
    WARNING = "WARNING"
    ERROR = "ERROR"


class KeySource(StrEnum):
    ENVIRONMENT = "environment"
    CONFIG = "config"
    GROK_AUTH = "grok_auth"
    MISSING = "missing"


class _Section(BaseModel):
    model_config = ConfigDict(extra="forbid", frozen=True)


class DevicesConfig(_Section):
    mic: str | None = None
    output: str | None = None


class SegmentationConfig(_Section):
    pause_ms: int = Field(default=800, ge=100)
    max_segment_s: float = Field(default=30, ge=5)
    min_segment_ms: int = Field(default=400, ge=0)
    padding_ms: int = Field(default=200, ge=0)
    vad_threshold: float = Field(default=0.5, gt=0, lt=1)


class SttConfig(_Section):
    selected: str = "groq/whisper-large-v3-turbo"
    models: list[str] = Field(
        default_factory=lambda: ["groq/whisper-large-v3-turbo", "openai/gpt-4o-mini-transcribe"]
    )
    prompt: str = ""
    workers: int = Field(default=3, ge=1, le=16)

    def choices(self) -> list[str]:
        """Models offered in the tray; includes `selected` even if it was not added to `models`."""
        return self.models if self.selected in self.models else [*self.models, self.selected]


class ProviderConfig(_Section):
    api_key: SecretStr = SecretStr("")
    grok_auth: bool = False


class HermesConfig(_Section):
    enabled: bool = True
    url: str = ""
    api_key: SecretStr = SecretStr("")
    payload_mode: PayloadMode = PayloadMode.RESPONSES
    model: str = ""
    raw_path: str = ""
    timeout_s: float = Field(default=30, gt=0)
    send_session_events: bool = True

    @field_validator("url")
    @classmethod
    def _http_url(cls, value: str) -> str:
        value = value.strip().rstrip("/")
        if value and not value.startswith(("http://", "https://")):
            raise ValueError("must start with http:// or https://")
        return value


@dataclass(frozen=True)
class ApiKey:
    value: str = field(repr=False)
    source: KeySource


class AppConfig(_Section):
    language: Language = Language.POLISH
    output_dir: str = "%USERPROFILE%\\Documents\\MeetingTranscripts"
    save_audio: bool = False
    hotkey: str = "ctrl+alt+r"
    notifications: bool = True
    log_level: LogLevel = LogLevel.INFO
    devices: DevicesConfig = DevicesConfig()
    segmentation: SegmentationConfig = SegmentationConfig()
    stt: SttConfig = SttConfig()
    providers: dict[str, ProviderConfig] = Field(default_factory=dict)
    hermes: HermesConfig = HermesConfig()

    @field_validator("hotkey")
    @classmethod
    def _valid_hotkey(cls, value: str) -> str:
        value = value.strip()
        if value:
            parse_hotkey(value)
        return value

    @field_validator("providers")
    @classmethod
    def _grok_auth_only_for_xai(cls, providers: dict[str, ProviderConfig]) -> dict[str, ProviderConfig]:
        misplaced = sorted(name for name, provider in providers.items() if provider.grok_auth and name != XAI_PROVIDER)
        if misplaced:
            raise ValueError(f"grok_auth is only supported for {XAI_PROVIDER}, not {', '.join(misplaced)}")
        return providers

    def output_path(self) -> Path:
        return Path(os.path.expandvars(self.output_dir)).expanduser()

    def uses_grok_auth(self) -> bool:
        """xAI models authenticate with the Grok account authorized from the tray instead of an API key."""
        return self.providers.get(XAI_PROVIDER, ProviderConfig()).grok_auth

    def stt_key(self, model: str) -> ApiKey:
        provider = provider_of(model)
        settings = self.providers.get(provider, ProviderConfig())
        if settings.grok_auth:
            return ApiKey("", KeySource.GROK_AUTH)
        return resolve_key(env_var_for(provider), settings.api_key)

    def hermes_key(self) -> ApiKey:
        return resolve_key(HERMES_KEY_ENV, self.hermes.api_key)

    def secret_values(self) -> list[str]:
        """Every API key in effect, so logging can redact them."""
        keys = [resolve_key(env_var_for(name), provider.api_key) for name, provider in self.providers.items()]
        keys += [self.stt_key(model) for model in self.stt.choices()]
        keys.append(self.hermes_key())
        return sorted({key.value for key in keys if key.value})


class ConfigError(Exception):
    pass


def provider_of(model: str) -> str:
    return model.split("/", 1)[0] if "/" in model else DEFAULT_PROVIDER


def env_var_for(provider: str) -> str:
    return f"{provider.upper().replace('-', '_')}_API_KEY"


def resolve_key(env_var: str, configured: SecretStr) -> ApiKey:
    """The environment variable wins over the config value."""
    from_env = os.environ.get(env_var, "").strip()
    if from_env:
        return ApiKey(from_env, KeySource.ENVIRONMENT)
    from_config = configured.get_secret_value().strip()
    if from_config:
        return ApiKey(from_config, KeySource.CONFIG)
    return ApiKey("", KeySource.MISSING)


def resolve_config_path(explicit: Path | None = None) -> Path:
    """config.yaml next to the exe (or project root) if present, otherwise %APPDATA%\\ListeningApp."""
    if explicit is not None:
        return explicit
    beside_exe = paths.install_dir() / paths.CONFIG_FILE_NAME
    if beside_exe.exists():
        return beside_exe
    return paths.app_data_dir() / paths.CONFIG_FILE_NAME


def ensure_config_file(path: Path) -> bool:
    """Create the config from the bundled example if it is missing. Returns True if created."""
    if path.exists():
        return False
    path.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(paths.example_config_path(), path)
    return True


def load_config(path: Path) -> AppConfig:
    try:
        data = YAML(typ="safe").load(path.read_text(encoding="utf-8"))
    except (OSError, YAMLError) as exc:
        raise ConfigError(f"Cannot read {path}: {exc}") from exc
    try:
        return AppConfig.model_validate(data or {})
    except ValidationError as exc:
        raise ConfigError(_describe_errors(exc)) from exc


def _describe_errors(exc: ValidationError) -> str:
    problems = [f"{'.'.join(str(part) for part in error['loc'])}: {error['msg']}" for error in exc.errors()]
    return "; ".join(problems)


def write_setting(path: Path, keys: tuple[str, ...], value: str | bool | None) -> None:
    """Set one value in the YAML file, keeping the user's comments and layout."""
    yaml = _round_trip_yaml()
    document = yaml.load(path.read_text(encoding="utf-8")) or CommentedMap()
    node = document
    for key in keys[:-1]:
        node = node.setdefault(key, CommentedMap())
    node[keys[-1]] = value
    buffer = StringIO()
    yaml.dump(document, buffer)
    atomic_write_text(path, buffer.getvalue())


def _round_trip_yaml() -> YAML:
    yaml = YAML()
    yaml.preserve_quotes = True
    yaml.representer.add_representer(type(None), _represent_null)
    return yaml


def _represent_null(representer: RoundTripRepresenter, _: None) -> Any:
    return representer.represent_scalar("tag:yaml.org,2002:null", "null")


def describe(config: AppConfig) -> str:
    """Human-readable config with every API key masked."""
    lines = [json.dumps(config.model_dump(mode="json"), indent=2, ensure_ascii=False), "", "API keys in effect:"]
    lines += [f"  {model}: {config.stt_key(model).source}" for model in config.stt.choices()]
    lines.append(f"  hermes: {config.hermes_key().source}")
    return "\n".join(lines)


class ConfigManager:
    """Holds the last valid config, reloads it and persists tray selections."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self._config = AppConfig()
        self._lock = threading.Lock()

    @property
    def config(self) -> AppConfig:
        with self._lock:
            return self._config

    def reload(self) -> AppConfig:
        """Load the file. On ConfigError the last valid config stays in effect."""
        config = load_config(self.path)
        with self._lock:
            self._config = config
        return config

    def update(self, keys: tuple[str, ...], value: str | bool | None) -> AppConfig:
        write_setting(self.path, keys, value)
        return self.reload()
