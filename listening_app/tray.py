"""Tray icon, menu and notifications (pystray)."""

import logging
from collections.abc import Callable, Iterator

import pystray
from PIL import Image, ImageDraw

from listening_app.app import AppController
from listening_app.config import PayloadMode

log = logging.getLogger(__name__)

APP_TITLE = "Listening App"
_SIZE = 64
_GREY = "#8a8a8a"
_RED = "#d93025"
_ORANGE = "#e8710a"
_YELLOW = "#f9ab00"
_DARK = "#202124"
_TOOLTIP_LIMIT = 127
PAYLOAD_MODE_LABELS = {
    PayloadMode.RESPONSES: "Responses (/responses)",
    PayloadMode.CHAT: "Chat completions (/chat/completions)",
    PayloadMode.RAW: "Raw event (raw_path)",
}

Item = pystray.MenuItem
Menu = pystray.Menu


def make_icon(recording: bool, error: bool, paused: bool = False) -> Image.Image:
    """Grey = idle, red = recording, orange = paused, yellow "!" = error (a yellow badge while recording)."""
    image = Image.new("RGBA", (_SIZE, _SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    if error and not recording:
        draw.ellipse((2, 2, 62, 62), fill=_YELLOW)
        _draw_exclamation(draw, 32, 32, 1.0)
        return image
    if recording and paused:
        fill = _ORANGE
    elif recording:
        fill = _RED
    else:
        fill = _GREY
    draw.ellipse((2, 2, 62, 62), fill=fill)
    if recording and paused:
        _draw_pause_bars(draw)
    if error:
        draw.ellipse((32, 32, 63, 63), fill=_YELLOW, outline=_DARK, width=2)
        _draw_exclamation(draw, 47.5, 47.5, 0.5)
    return image


def _draw_pause_bars(draw: ImageDraw.ImageDraw) -> None:
    draw.rectangle((22, 18, 30, 46), fill=_DARK)
    draw.rectangle((34, 18, 42, 46), fill=_DARK)


def _draw_exclamation(draw: ImageDraw.ImageDraw, x: float, y: float, scale: float) -> None:
    half_width = 4 * scale
    draw.rectangle((x - half_width, y - 20 * scale, x + half_width, y + 5 * scale), fill=_DARK)
    draw.ellipse((x - half_width, y + 11 * scale, x + half_width, y + 19 * scale), fill=_DARK)


class TrayApp:
    def __init__(self, controller: AppController) -> None:
        self._controller = controller
        self._icon = pystray.Icon("ListeningApp", make_icon(False, False), APP_TITLE, menu=Menu(self._menu_items))

    def run(self, on_ready: Callable[[], None]) -> None:
        """Blocks until Quit. `on_ready` runs on a helper thread once the icon exists."""
        def setup(icon: pystray.Icon) -> None:
            icon.visible = True
            on_ready()

        self._icon.run(setup=setup)

    def notify(self, title: str, message: str) -> None:
        self._icon.notify(message, title)

    def refresh(self) -> None:
        controller = self._controller
        self._icon.icon = make_icon(controller.is_recording, controller.error is not None, controller.is_paused)
        self._icon.title = self._tooltip()[:_TOOLTIP_LIMIT]
        self._icon.update_menu()

    def _tooltip(self) -> str:
        controller = self._controller
        if controller.is_recording and controller.is_paused:
            state = "Paused"
        elif controller.is_recording:
            state = "Recording"
        else:
            state = "Idle"
        sketch = " - Live sketch" if controller.is_sketching else ""
        return f"{APP_TITLE} - {state}{sketch}" + (f" - {controller.error}" if controller.error else "")

    def _record_label(self) -> str:
        controller = self._controller
        if controller.is_busy:
            return "Working..."
        label = "Stop recording" if controller.is_recording else "Start recording"
        return f"{label}\t{controller.hotkey_label}" if controller.hotkey_label else label

    def _pause_label(self) -> str:
        controller = self._controller
        label = "Resume" if controller.is_paused else "Pause"
        return f"{label}\t{controller.pause_hotkey_label}" if controller.pause_hotkey_label else label

    def _sketch_label(self) -> str:
        controller = self._controller
        label = "Working..." if controller.is_sketch_busy else "Live sketch"
        return f"{label}\t{controller.sketch_hotkey_label}" if controller.sketch_hotkey_label else label

    def _authorize_grok_label(self) -> str:
        return "Authorize Grok  !" if self._controller.needs_grok_authorization else "Authorize Grok"

    def _menu_items(self) -> Iterator[pystray.MenuItem]:
        controller = self._controller
        devices = controller.config.devices
        yield Item(self._record_label(), _safe(controller.toggle_recording), default=True, enabled=not controller.is_busy)
        yield Item(self._pause_label(), _safe(controller.toggle_pause),
                   enabled=controller.is_recording and not controller.is_busy)
        yield Item(self._sketch_label(), _safe(controller.toggle_sketch), checked=lambda _: controller.is_sketching,
                   enabled=not controller.is_sketch_busy)
        yield Menu.SEPARATOR
        yield Item("Microphone", Menu(*self._device_items(
            controller.microphones(), devices.mic, controller.default_mic_name(), controller.select_mic)))
        yield Item("Audio output", Menu(*self._device_items(
            controller.outputs(), devices.output, controller.default_output_name(), controller.select_output)))
        yield Item("Transcription model", Menu(*self._model_items()))
        if controller.grok_auth_enabled:
            yield Item(self._authorize_grok_label(), _safe(controller.authorize_grok),
                       enabled=not controller.is_authorizing_grok)
        yield Item("Hermes streaming", _safe(controller.toggle_hermes), checked=lambda _: controller.hermes_enabled)
        yield Item("Hermes endpoint", Menu(*self._payload_mode_items()))
        yield Menu.SEPARATOR
        yield Item("Refresh devices", _safe(controller.refresh_devices),
                   enabled=not (controller.is_recording or controller.is_busy or controller.is_sketching))
        if controller.unfinished_count:
            yield Item(f"Finalize unfinished sessions ({controller.unfinished_count})",
                       _safe(controller.finalize_unfinished), enabled=not controller.is_busy)
        yield Item("Open transcripts folder", _safe(controller.open_transcripts))
        yield Item("Open diagrams folder", _safe(controller.open_diagrams))
        yield Item("Open config file", _safe(controller.open_config))
        yield Item("Reload config", _safe(controller.reload_config))
        yield Menu.SEPARATOR
        yield Item("Quit", self._quit)

    @staticmethod
    def _device_items(names: list[str], selected: str | None, default_name: str | None,
                      select: Callable[[str | None], None]) -> Iterator[pystray.MenuItem]:
        default_label = f"System default ({default_name})" if default_name else "System default"
        options: list[tuple[str, str | None]] = [(default_label, None), *((name, name) for name in names)]
        for label, value in options:
            yield Item(label, _choose(select, value), checked=_is_selected(selected, value), radio=True)
        if selected is not None and selected not in names:
            yield Item(f"{selected} (not connected)", None, checked=_is_selected(selected, selected), radio=True,
                       enabled=False)

    def _model_items(self) -> Iterator[pystray.MenuItem]:
        stt = self._controller.config.stt
        for model in stt.choices():
            yield Item(model, _choose(self._controller.select_model, model), checked=_is_selected(stt.selected, model),
                       radio=True)

    def _payload_mode_items(self) -> Iterator[pystray.MenuItem]:
        selected = self._controller.config.hermes.payload_mode
        for mode, label in PAYLOAD_MODE_LABELS.items():
            yield Item(label, _choose(self._controller.select_payload_mode, mode), checked=_is_selected(selected, mode),
                       radio=True)

    def _quit(self) -> None:
        self._icon.stop()


def _choose[T](select: Callable[[T], None], value: T) -> Callable[[], None]:
    return _safe(lambda: select(value))


def _is_selected[T](current: T, value: T) -> Callable[[pystray.MenuItem], bool]:
    return lambda _: current == value


def _safe(action: Callable[[], None]) -> Callable[[], None]:
    """Menu callback that logs failures instead of killing the tray thread."""
    def handler() -> None:
        try:
            action()
        except Exception:
            log.exception("Menu action failed")

    return handler
