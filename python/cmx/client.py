"""CMX client -- typed access to the parameter store."""

from typing import Any, Optional, List
from .protocol import SocketConnection


class CmxClient:
    """Client for the CMX parameter store.

    Communicates with the CMX daemon over a Unix domain socket using
    length-prefixed JSON frames. Supports get, set, append, keys, and
    delete operations on dotted namespace paths.
    """

    def __init__(self, socket_path: str = None):
        self._conn = SocketConnection(socket_path)

    def _send(self, command: dict) -> dict:
        """Send a command and return the raw response dict."""
        resp = self._conn.send(command)
        if resp.get("status") == "error":
            msg = resp.get("message", "Unknown error")
            if "not found" in msg.lower() or "NotFound" in msg:
                raise KeyError(msg)
            raise RuntimeError(msg)
        return resp

    def get(self, path: str) -> Any:
        """GET a value from the parameter store.

        Returns the typed value (str, int, float, bool, list, dict, None).
        Raises KeyError if path not found.
        For wildcard patterns, returns a dict of {path: value}.
        """
        resp = self._send({"command": "ns.get", "path": path})
        return resp.get("output")

    def set(self, path: str, value: Any) -> None:
        """SET a value in the parameter store."""
        self._send({"command": "ns.set", "path": path, "value": value})

    def append(self, path: str, value: Any) -> None:
        """APPEND a value in the parameter store (array semantics)."""
        self._send({"command": "ns.append", "path": path, "value": value})

    def keys(self, pattern: str = None) -> List[str]:
        """List keys matching a pattern (or all keys)."""
        cmd = {"command": "ns.keys"}
        if pattern is not None:
            cmd["pattern"] = pattern
        resp = self._send(cmd)
        output = resp.get("output", "")
        if not output:
            return []
        return output.split("\n")

    def delete(self, path: str) -> None:
        """Remove a value from the parameter store."""
        self._send({"command": "ns.delete", "path": path})

    def connect(self) -> 'CmxClient':
        """Explicitly connect (also connects on first use)."""
        self._conn.connect()
        return self

    def close(self):
        """Close the socket connection."""
        self._conn.close()

    def __enter__(self):
        self.connect()
        return self

    def __exit__(self, *args):
        self.close()
