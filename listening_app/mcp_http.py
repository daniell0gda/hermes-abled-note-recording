"""The tray app's MCP endpoint: the tools of the headless mode, served over HTTP to this PC only."""

import socket
import threading

import uvicorn
from mcp.server.mcpserver import MCPServer

LOCALHOST = "127.0.0.1"
MCP_PATH = "/mcp"
GRACEFUL_SHUTDOWN_S = 2
STOP_TIMEOUT_S = 5.0


class McpHttpServer:
    """Serves `server` at http://127.0.0.1:<port>/mcp on a background thread, next to the tray."""

    def __init__(self, server: MCPServer, port: int) -> None:
        """Raises OSError when the port is in use. Port 0 picks a free one."""
        self._socket = socket.create_server((LOCALHOST, port))
        self.url = f"http://{LOCALHOST}:{self._socket.getsockname()[1]}{MCP_PATH}"
        app =server.streamable_http_app(streamable_http_path=MCP_PATH, host=LOCALHOST)
        self._uvicorn = uvicorn.Server(uvicorn.Config(app, log_config=None, access_log=False,
                                                      timeout_graceful_shutdown=GRACEFUL_SHUTDOWN_S))
        self._thread = threading.Thread(target=self._uvicorn.run, kwargs={"sockets": [self._socket]},
                                        name="mcp-http", daemon=True)

    def start(self) -> None:
        self._thread.start()

    def stop(self) -> None:
        """Close open MCP sessions and the port."""
        self._uvicorn.should_exit = True
        self._thread.join(STOP_TIMEOUT_S)
        self._socket.close()
