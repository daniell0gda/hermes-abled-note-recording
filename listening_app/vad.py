"""Silero VAD through onnxruntime (no PyTorch needed)."""

from pathlib import Path

import numpy as np
import onnxruntime

from listening_app.models import SAMPLE_RATE, Audio
from listening_app.paths import ASSETS_DIR

MODEL_PATH = ASSETS_DIR / "silero_vad.onnx"
_CONTEXT_SAMPLES = 64
_STATE_SHAPE = (2, 1, 128)


class SileroVad:
    """Speech probability per 512-sample frame. Stateful: use one instance per audio stream."""

    def __init__(self, model_path: Path = MODEL_PATH) -> None:
        options = onnxruntime.SessionOptions()
        options.inter_op_num_threads = 1
        options.intra_op_num_threads = 1
        self._session = onnxruntime.InferenceSession(
            str(model_path), sess_options=options, providers=["CPUExecutionProvider"]
        )
        self._sample_rate = np.array(SAMPLE_RATE, dtype=np.int64)
        self._state = np.zeros(_STATE_SHAPE, dtype=np.float32)
        self._context = np.zeros((1, _CONTEXT_SAMPLES), dtype=np.float32)

    def reset(self) -> None:
        self._state = np.zeros(_STATE_SHAPE, dtype=np.float32)
        self._context = np.zeros((1, _CONTEXT_SAMPLES), dtype=np.float32)

    def probability(self, frame: Audio) -> float:
        model_input = np.concatenate([self._context, frame.reshape(1, -1)], axis=1)
        output, self._state = self._session.run(
            None, {"input": model_input, "state": self._state, "sr": self._sample_rate}
        )
        self._context = model_input[:, -_CONTEXT_SAMPLES:]
        return float(output[0][0])
