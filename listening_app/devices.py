"""WASAPI device listing, loopback lookup and fallback to the system default."""

import ctypes
from collections.abc import Iterator, Sequence
from contextlib import contextmanager
from dataclasses import dataclass
from typing import Any

import pyaudiowpatch as pyaudio

LOOPBACK_SUFFIX = " [Loopback]"
_COINIT_MULTITHREADED = 0x0
_S_OK = 0
_S_FALSE = 1


@contextmanager
def com_initialized() -> Iterator[None]:
    """PortAudio's WASAPI backend needs COM on every thread that opens, polls or closes a stream."""
    ole32 = ctypes.WinDLL("ole32")
    result = ole32.CoInitializeEx(None, _COINIT_MULTITHREADED)
    try:
        yield
    finally:
        if result in (_S_OK, _S_FALSE):
            ole32.CoUninitialize()


class DeviceError(Exception):
    pass


@dataclass(frozen=True)
class AudioDevice:
    index: int
    name: str
    channels: int
    sample_rate: int


@dataclass(frozen=True)
class DeviceChoice:
    """The device to capture from. For an output this is its loopback; `name` stays the output's name."""

    capture: AudioDevice
    name: str
    fell_back: bool


def find_by_name(devices: Sequence[AudioDevice], name: str | None) -> AudioDevice | None:
    return next((device for device in devices if device.name == name), None)


def loopback_for(output_name: str, loopbacks: Sequence[AudioDevice]) -> AudioDevice | None:
    exact = find_by_name(loopbacks, output_name + LOOPBACK_SUFFIX)
    return exact or next((device for device in loopbacks if device.name.startswith(output_name)), None)


class AudioSystem:
    """Owns the PortAudio instance. Device lists are a snapshot taken when PortAudio starts."""

    def __init__(self) -> None:
        self.pa = pyaudio.PyAudio()

    def refresh(self) -> None:
        """Re-enumerate devices (after plugging a headset in or out). Only call while nothing records."""
        self.pa.terminate()
        self.pa = pyaudio.PyAudio()

    def close(self) -> None:
        self.pa.terminate()

    def inputs(self) -> list[AudioDevice]:
        return [_to_device(info, info["maxInputChannels"]) for info in self._wasapi_devices()
                if not info["isLoopbackDevice"] and info["maxInputChannels"] > 0]

    def outputs(self) -> list[AudioDevice]:
        return [_to_device(info, info["maxOutputChannels"]) for info in self._wasapi_devices()
                if not info["isLoopbackDevice"] and info["maxOutputChannels"] > 0]

    def loopbacks(self) -> list[AudioDevice]:
        return [_to_device(info, info["maxInputChannels"]) for info in self._wasapi_devices()
                if info["isLoopbackDevice"]]

    def default_input_name(self) -> str | None:
        return self._default_name("defaultInputDevice")

    def default_output_name(self) -> str | None:
        return self._default_name("defaultOutputDevice")

    def resolve_mic(self, name: str | None) -> DeviceChoice:
        device = self._pick(self.inputs(), name, self.default_input_name(), "microphone")
        return DeviceChoice(device, device.name, fell_back=_fell_back(name, device))

    def resolve_output(self, name: str | None) -> DeviceChoice:
        output = self._pick(self.outputs(), name, self.default_output_name(), "audio output")
        loopback = loopback_for(output.name, self.loopbacks())
        if loopback is None:
            raise DeviceError(f"No loopback device found for '{output.name}'")
        return DeviceChoice(loopback, output.name, fell_back=_fell_back(name, output))

    @staticmethod
    def _pick(devices: list[AudioDevice], wanted: str | None, default: str | None, kind: str) -> AudioDevice:
        device = find_by_name(devices, wanted) or find_by_name(devices, default)
        if device is None and devices:
            device = devices[0]
        if device is None:
            raise DeviceError(f"No {kind} found")
        return device

    def _wasapi_devices(self) -> list[dict[str, Any]]:
        return list(self.pa.get_device_info_generator_by_host_api(host_api_type=pyaudio.paWASAPI))

    def _default_name(self, key: str) -> str | None:
        index = self.pa.get_host_api_info_by_type(pyaudio.paWASAPI)[key]
        if index < 0:
            return None
        return str(self.pa.get_device_info_by_index(index)["name"])


def _fell_back(wanted: str | None, chosen: AudioDevice) -> bool:
    return wanted is not None and wanted != chosen.name


def _to_device(info: dict[str, Any], channels: int) -> AudioDevice:
    return AudioDevice(int(info["index"]), str(info["name"]), int(channels), int(info["defaultSampleRate"]))


def describe_devices(audio: AudioSystem) -> str:
    default_in, default_out = audio.default_input_name(), audio.default_output_name()
    loopbacks = audio.loopbacks()
    lines = ["Microphones (WASAPI inputs):"]
    lines += [f"  {'*' if d.name == default_in else ' '} {d.name}  ({d.channels} ch, {d.sample_rate} Hz)"
              for d in audio.inputs()]
    lines.append("Audio outputs (captured via loopback):")
    for device in audio.outputs():
        marker = "*" if device.name == default_out else " "
        loopback = "loopback ok" if loopback_for(device.name, loopbacks) else "NO loopback"
        lines.append(f"  {marker} {device.name}  ({device.channels} ch, {device.sample_rate} Hz, {loopback})")
    lines.append("* = system default")
    return "\n".join(lines)
