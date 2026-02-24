"""Tests for the wire protocol (length-prefixed JSON)."""

import json
import struct
import pytest
from cmx.protocol import encode_request, decode_response


class TestEncodeRequest:
    def test_encode_request(self):
        """Encode a dict, verify 4-byte big-endian length prefix + JSON payload."""
        cmd = {"command": "status"}
        result = encode_request(cmd)

        # First 4 bytes are the length prefix
        length = struct.unpack('>I', result[:4])[0]
        payload = result[4:]

        assert length == len(payload)
        parsed = json.loads(payload)
        assert parsed == cmd

    def test_encode_with_nested_values(self):
        """Encode a command with nested values."""
        cmd = {"command": "ns.set", "path": "task.t1.status", "value": "complete"}
        result = encode_request(cmd)

        length = struct.unpack('>I', result[:4])[0]
        payload = result[4:]
        assert length == len(payload)
        assert json.loads(payload) == cmd


class TestDecodeResponse:
    def test_decode_response(self):
        """Decode length-prefixed JSON bytes back to a dict."""
        original = {"status": "ok", "output": "hello world"}
        payload = json.dumps(original).encode('utf-8')
        data = struct.pack('>I', len(payload)) + payload

        result = decode_response(data)
        assert result == original

    def test_decode_too_short(self):
        """Raise ValueError when data is shorter than 4 bytes."""
        with pytest.raises(ValueError, match="too short"):
            decode_response(b"\x00\x01")

    def test_decode_truncated_payload(self):
        """Raise ValueError when payload is shorter than declared length."""
        # Declare 100 bytes but only provide 5
        data = struct.pack('>I', 100) + b"hello"
        with pytest.raises(ValueError, match="truncated"):
            decode_response(data)


class TestRoundTrip:
    def test_round_trip(self):
        """Encode then decode, verify the result matches the original."""
        cmd = {"command": "ns.get", "path": "agent.w1.status"}
        encoded = encode_request(cmd)

        # decode_response expects a response, but the wire format is the same
        # (length-prefixed JSON). Verify the bytes decode correctly.
        decoded = decode_response(encoded)
        assert decoded == cmd

    def test_round_trip_with_unicode(self):
        """Round-trip preserves unicode characters."""
        cmd = {"command": "ns.set", "path": "label", "value": "cafe\u0301"}
        encoded = encode_request(cmd)
        decoded = decode_response(encoded)
        assert decoded == cmd
