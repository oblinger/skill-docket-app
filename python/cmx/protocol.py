"""Wire protocol for communicating with the CMX daemon socket.

Matches the Rust service.rs format: 4-byte big-endian u32 length prefix
followed by that many bytes of JSON payload. Max frame size is 16 MiB.
"""

import json
import struct
import socket
from typing import Any, Optional


# Maximum frame size matches Rust side (16 MiB).
MAX_FRAME_SIZE = 16 * 1024 * 1024


def encode_request(command: dict) -> bytes:
    """Encode a command dict as length-prefixed JSON.

    Returns 4-byte big-endian length prefix + UTF-8 JSON payload.
    """
    payload = json.dumps(command).encode('utf-8')
    return struct.pack('>I', len(payload)) + payload


def decode_response(data: bytes) -> dict:
    """Decode a length-prefixed JSON response.

    Expects at least 4 bytes for the length prefix, then the JSON payload.
    Raises ValueError if data is too short or payload is truncated.
    """
    if len(data) < 4:
        raise ValueError("Response too short: need at least 4 bytes for length prefix")
    length = struct.unpack('>I', data[:4])[0]
    if len(data) < 4 + length:
        raise ValueError(f"Response truncated: expected {length} bytes, got {len(data) - 4}")
    payload = data[4:4 + length]
    return json.loads(payload)


def _recv_exact(sock: socket.socket, n: int) -> bytes:
    """Read exactly n bytes from a socket."""
    buf = bytearray()
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("Socket closed before all data received")
        buf.extend(chunk)
    return bytes(buf)


class SocketConnection:
    """Manages a Unix domain socket connection to cmx.sock."""

    def __init__(self, path: str = None):
        self.path = path or self._default_path()
        self._sock: Optional[socket.socket] = None

    def _default_path(self) -> str:
        """Default socket path: ~/.config/cmx/cmx.sock"""
        import os
        return os.path.expanduser("~/.config/cmx/cmx.sock")

    def connect(self):
        """Connect to the daemon socket."""
        if self._sock is not None:
            return
        self._sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._sock.connect(self.path)

    def send(self, command: dict) -> dict:
        """Send a command and receive the response.

        Automatically connects if not already connected.
        Returns the parsed response dict.
        """
        if self._sock is None:
            self.connect()

        # Send length-prefixed JSON
        frame = encode_request(command)
        self._sock.sendall(frame)

        # Read 4-byte length prefix
        len_bytes = _recv_exact(self._sock, 4)
        length = struct.unpack('>I', len_bytes)[0]

        if length > MAX_FRAME_SIZE:
            raise ValueError(f"Response frame too large: {length} bytes")

        # Read payload
        payload = _recv_exact(self._sock, length)
        return json.loads(payload)

    def close(self):
        """Close the socket connection."""
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
            self._sock = None

    def __enter__(self):
        self.connect()
        return self

    def __exit__(self, *args):
        self.close()
