"""Command-line tools for checking the config, devices, capture, segmentation and STT."""

import ctypes
import sys
import time
from pathlib import Path

import numpy as np

from listening_app.capture import AudioCapture
from listening_app.config import AppConfig, ConfigError, describe, load_config
from listening_app.devices import AudioSystem, describe_devices
from listening_app.models import SAMPLE_RATE, Audio, Source
from listening_app.segmenter import Segmenter, SegmenterSettings
from listening_app.transcriber import SttSettings, litellm_transcribe
from listening_app.vad import SileroVad
from listening_app.wav import encode_wav, read_wav, write_wav

_ATTACH_PARENT_PROCESS = -1


def attach_parent_console() -> None:
    """Let the windowed (--noconsole) exe print to the PowerShell window it was started from."""
    if sys.stdout is not None:
        return
    if ctypes.WinDLL("kernel32").AttachConsole(_ATTACH_PARENT_PROCESS):
        sys.stdout = sys.stderr = open("CONOUT$", "w", encoding="utf-8")


def check_config(config_path: Path) -> int:
    print(f"Config file: {config_path}")
    try:
        config = load_config(config_path)
    except ConfigError as exc:
        print(f"INVALID: {exc}", file=sys.stderr)
        return 1
    print(describe(config))
    return 0


def list_devices() -> int:
    audio = AudioSystem()
    try:
        print(describe_devices(audio))
    finally:
        audio.close()
    return 0


def record_test(config: AppConfig, seconds: float, out_dir: Path) -> int:
    """Record both sources side by side into record_me.wav and record_others.wav."""
    audio = AudioSystem()
    try:
        mic, output = audio.resolve_mic(config.devices.mic), audio.resolve_output(config.devices.output)
        print(f"me:     {mic.name}\nothers: {output.name} (loopback)\nRecording {seconds:.0f} s ...")
        chunks: dict[Source, list[Audio]] = {Source.ME: [], Source.OTHERS: []}
        start = time.monotonic()
        captures = [
            AudioCapture(audio.pa, mic.capture, Source.ME, start, False, chunks[Source.ME].append, _print_loss),
            AudioCapture(audio.pa, output.capture, Source.OTHERS, start, True, chunks[Source.OTHERS].append,
                         _print_loss),
        ]
        for capture in captures:
            capture.start()
        time.sleep(seconds)
        for capture in captures:
            capture.stop()
    finally:
        audio.close()
    for source, parts in chunks.items():
        samples = np.concatenate(parts) if parts else np.zeros(0, dtype=np.float32)
        path = out_dir / f"record_{source}.wav"
        write_wav(path, samples)
        print(f"{path}: {len(samples) / SAMPLE_RATE:.2f} s, peak {float(np.abs(samples).max(initial=0)):.3f}")
    return 0


def _print_loss(source: Source, reason: str) -> None:
    print(f"Lost {source}: {reason}", file=sys.stderr)


def segment_wav(config: AppConfig, path: Path, transcribe: bool) -> int:
    """Split a WAV file at pauses like a live recording would, optionally transcribing each segment."""
    samples = read_wav(path)
    segmenter = Segmenter(SegmenterSettings.from_config(config.segmentation), SileroVad())
    segments = segmenter.feed(samples)
    last = segmenter.flush()
    if last is not None:
        segments.append(last)
    stt = SttSettings.from_config(config)
    print(f"{path}: {len(samples) / SAMPLE_RATE:.1f} s -> {len(segments)} segments")
    for number, segment in enumerate(segments, start=1):
        start, end = segment.start_sample / SAMPLE_RATE, segment.end_sample / SAMPLE_RATE
        line = f"{number:3d}  {start:7.2f} - {end:7.2f}  ({end - start:5.2f} s)"
        if transcribe:
            line += "  " + litellm_transcribe(encode_wav(segment.audio), stt)
        print(line)
    return 0
