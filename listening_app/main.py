"""Entry point: the tray app, or one of the command-line tools."""

import argparse
import logging
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Protocol

from listening_app import __version__, cli, paths
from listening_app.app import AppController, Ui
from listening_app.config import AppConfig, ConfigError, ConfigManager, ensure_config_file, load_config, resolve_config_path
from listening_app.logging_setup import setup_logging
from listening_app.tray import TrayApp

log = logging.getLogger(__name__)


class Frontend(Ui, Protocol):
    def run(self, on_ready: Callable[[], None]) -> None: ...


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="listening_app", description="Live meeting transcription from the tray.")
    parser.add_argument("--config", type=Path, help="config.yaml to use instead of the default location")
    tools = parser.add_mutually_exclusive_group()
    tools.add_argument("--check-config", action="store_true", help="validate the config and print it, keys masked")
    tools.add_argument("--list-devices", action="store_true", help="list microphones and audio outputs")
    tools.add_argument("--record-test", type=float, metavar="SECONDS",
                       help="record mic and loopback into record_me.wav / record_others.wav")
    tools.add_argument("--segment-wav", type=Path, metavar="WAV", help="split a WAV file at pauses and list segments")
    tools.add_argument("--mcp", action="store_true",
                       help="run headless, without the tray, as an MCP server on stdin/stdout for AI agents")
    tools.add_argument("--sketch-window", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--transcribe", action="store_true", help="with --segment-wav: transcribe every segment")
    parser.add_argument("--out", type=Path, default=Path("."), help="output folder for --record-test")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.sketch_window:
        from listening_app.sketch.window_app import run_window
        return run_window()
    config_path = resolve_config_path(args.config)
    if args.check_config or args.list_devices or args.record_test or args.segment_wav:
        cli.attach_parent_console()
        try:
            return run_tool(args, config_path)
        except Exception as exc:  # the windowed exe would otherwise show a traceback dialog
            print(f"Error: {type(exc).__name__}: {exc}", file=sys.stderr)
            return 1
    return run_app(config_path, headless=args.mcp)


def run_app(config_path: Path, headless: bool = False) -> int:
    notices: list[tuple[str, str]] = []
    if ensure_config_file(config_path):
        notices.append(("Config created", f"{config_path}: add your API keys there or set GROQ_API_KEY / HERMES_API_KEY."))
    configs = ConfigManager(config_path)
    config_error = None
    try:
        configs.reload()
    except ConfigError as exc:
        config_error = f"Invalid config: {exc}"
        notices.append(("Invalid config", f"{exc}. Using the default settings."))
    config = configs.config
    formatter = setup_logging(paths.logs_dir(), config.log_level, console=not paths.is_frozen())
    formatter.set_secrets(config.secret_values())
    log.info("Listening App %s starting %s with %s", __version__, "headless (MCP)" if headless else "in the tray",
             config_path)
    controller = AppController(configs, formatter)
    frontend = _create_frontend(controller, headless)
    controller.attach(frontend)
    try:
        frontend.run(on_ready=lambda: controller.start_services(notices, config_error))
    finally:
        controller.shutdown()
    log.info("Listening App stopped")
    return 0


def _create_frontend(controller: AppController, headless: bool) -> Frontend:
    if not headless:
        return TrayApp(controller)
    from listening_app.mcp_server import McpApp
    return McpApp(controller)


def run_tool(args: argparse.Namespace, config_path: Path) -> int:
    if args.check_config:
        return cli.check_config(config_path)
    if args.list_devices:
        return cli.list_devices()
    config = _load_or_default(config_path)
    if args.record_test:
        return cli.record_test(config, args.record_test, args.out)
    return cli.segment_wav(config, args.segment_wav, args.transcribe)


def _load_or_default(config_path: Path) -> AppConfig:
    if not config_path.exists():
        print(f"No config at {config_path}, using defaults", file=sys.stderr)
        return AppConfig()
    try:
        return load_config(config_path)
    except ConfigError as exc:
        print(f"Invalid config ({exc}), using defaults", file=sys.stderr)
        return AppConfig()


if __name__ == "__main__":
    sys.exit(main())
