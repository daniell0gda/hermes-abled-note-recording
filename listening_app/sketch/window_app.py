"""The live sketch window process: pywebview (Edge WebView2) needs the main thread of its own process.

Talks to the app over stdin/stdout, one JSON message per line: diagram states in, pointer and click events out.
"""

import html
import json
import sys
import threading
from typing import IO, Any

from listening_app.sketch.export import FONTS_URL, asset

WINDOW_TITLE = "Live sketch"
WINDOW_SIZE = (1200, 760)
PAPER = "#f5f5f5"


def window_page() -> str:
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{html.escape(WINDOW_TITLE)}</title>
<link href="{FONTS_URL}" rel="stylesheet">
<style>{asset("sketch.css")}{asset("window.css")}</style>
</head>
<body>
<div class="sk-page">
<nav class="sk-tabs" id="tabs" role="tablist"></nav>
<header class="sk-header">
<div><div class="sk-eyebrow-title" id="eyebrow"></div><h1 class="sk-title" id="title"></h1></div>
<div class="sk-tools">
<button class="sk-tool" id="mic" type="button" aria-pressed="false" disabled><span class="sk-mic-dot"></span><span id="mic-label">Mic off</span></button>
<button class="sk-tool" id="copy-path" type="button" disabled>Copy path</button>
</div>
</header>
<main class="sk-canvas" id="canvas"><svg id="sketch" role="img" aria-label="Live sketch"></svg></main>
</div>
<div class="sk-warmup"><svg id="warmup"></svg></div>
<div class="sk-notice" id="notice" role="status"></div>
<script>{asset("dagre.min.js")}</script>
<script>{asset("sketch.js")}</script>
<script>{asset("window.js")}</script>
</body>
</html>
"""


class Bridge:
    """Exposed to the page as `window.pywebview.api`; every event goes to the app as one line on stdout."""

    def __init__(self, output: IO[str]) -> None:
        self._output = output
        self._lock = threading.Lock()

    def event(self, message: str) -> None:
        with self._lock:
            self._output.write(json.dumps(json.loads(message)) + "\n")
            self._output.flush()


def _forward_messages(window: Any, source: IO[str]) -> None:
    window.events.loaded.wait()
    for line in source:
        if line.strip():
            window.evaluate_js(f"SketchApp.receive({line.strip()})")
    window.destroy()


def _std_stream(stream: IO[str] | None, fd: int, mode: str) -> IO[str]:
    """The windowed exe may start without sys.stdin/stdout objects even though the app passed pipes."""
    return stream if stream is not None else open(fd, mode, encoding="utf-8", closefd=False)


def run_window() -> int:
    import webview

    source = _std_stream(sys.stdin, 0, "r")
    output = _std_stream(sys.stdout, 1, "w")
    window = webview.create_window(WINDOW_TITLE, html=window_page(), js_api=Bridge(output),
                                   width=WINDOW_SIZE[0], height=WINDOW_SIZE[1], background_color=PAPER)
    webview.start(_forward_messages, (window, source))
    return 0
