"""16 kHz mono PCM16 WAV encoding and decoding."""

import io
import wave
from pathlib import Path

import numpy as np
import soxr

from listening_app.models import SAMPLE_RATE, Audio

_PCM16_MAX = 32767


def to_pcm16(audio: Audio) -> bytes:
    return (np.clip(audio, -1.0, 1.0) * _PCM16_MAX).astype("<i2").tobytes()


def encode_wav(audio: Audio) -> bytes:
    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as wav_file:
        wav_file.setnchannels(1)
        wav_file.setsampwidth(2)
        wav_file.setframerate(SAMPLE_RATE)
        wav_file.writeframes(to_pcm16(audio))
    return buffer.getvalue()


def write_wav(path: Path, audio: Audio) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(encode_wav(audio))


def read_wav(path: Path) -> Audio:
    """Read a PCM16 WAV of any rate and channel count as 16 kHz mono float32."""
    with wave.open(str(path), "rb") as wav_file:
        if wav_file.getsampwidth() != 2:
            raise ValueError(f"{path} is not 16-bit PCM")
        channels = wav_file.getnchannels()
        rate = wav_file.getframerate()
        frames = wav_file.readframes(wav_file.getnframes())
    samples = np.frombuffer(frames, dtype="<i2").astype(np.float32) / _PCM16_MAX
    mono = samples.reshape(-1, channels).mean(axis=1, dtype=np.float32)
    if rate == SAMPLE_RATE:
        return mono
    return np.asarray(soxr.resample(mono, rate, SAMPLE_RATE), dtype=np.float32)
