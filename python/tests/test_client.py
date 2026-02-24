"""Tests for the CmxClient (mock socket, no daemon required)."""

import json
import struct
import pytest
from unittest.mock import MagicMock, patch
from cmx.client import CmxClient


def _make_response(status: str, **kwargs) -> dict:
    """Build a response dict matching the Rust Response enum."""
    resp = {"status": status}
    resp.update(kwargs)
    return resp


def _mock_client_with_response(response_dict: dict) -> CmxClient:
    """Create a CmxClient whose SocketConnection.send returns the given dict."""
    client = CmxClient.__new__(CmxClient)
    mock_conn = MagicMock()
    mock_conn.send.return_value = response_dict
    client._conn = mock_conn
    return client


class TestGet:
    def test_get_returns_typed_value(self):
        """Mock response with string output, verify string returned."""
        client = _mock_client_with_response(
            _make_response("ok", output="running")
        )
        result = client.get("agent.w1.status")
        assert result == "running"
        client._conn.send.assert_called_once_with(
            {"command": "ns.get", "path": "agent.w1.status"}
        )

    def test_get_not_found_raises(self):
        """Mock NotFound response, verify KeyError raised."""
        client = _mock_client_with_response(
            _make_response("error", message="path not found: agent.x.y")
        )
        with pytest.raises(KeyError, match="not found"):
            client.get("agent.x.y")


class TestSet:
    def test_set_sends_correct_command(self):
        """Verify the command dict format for SET."""
        client = _mock_client_with_response(_make_response("ok", output=""))
        client.set("task.t1.status", "complete")
        client._conn.send.assert_called_once_with(
            {"command": "ns.set", "path": "task.t1.status", "value": "complete"}
        )

    def test_set_with_numeric_value(self):
        """Verify SET with an integer value."""
        client = _mock_client_with_response(_make_response("ok", output=""))
        client.set("task.t1.progress", 42)
        client._conn.send.assert_called_once_with(
            {"command": "ns.set", "path": "task.t1.progress", "value": 42}
        )


class TestAppend:
    def test_append_sends_correct_command(self):
        """Verify the command dict format for APPEND."""
        client = _mock_client_with_response(_make_response("ok", output=""))
        client.append("task.t1.log", "step completed")
        client._conn.send.assert_called_once_with(
            {"command": "ns.append", "path": "task.t1.log", "value": "step completed"}
        )


class TestKeys:
    def test_keys_returns_list(self):
        """Verify keys returns a list of strings."""
        client = _mock_client_with_response(
            _make_response("ok", output="task.t1.status\ntask.t2.status")
        )
        result = client.keys("task.*.status")
        assert result == ["task.t1.status", "task.t2.status"]

    def test_keys_empty(self):
        """Verify keys returns empty list when no matches."""
        client = _mock_client_with_response(_make_response("ok", output=""))
        result = client.keys("nothing.*")
        assert result == []


class TestDelete:
    def test_delete_sends_correct_command(self):
        """Verify the command dict format for DELETE."""
        client = _mock_client_with_response(_make_response("ok", output=""))
        client.delete("task.t1.stale")
        client._conn.send.assert_called_once_with(
            {"command": "ns.delete", "path": "task.t1.stale"}
        )


class TestContextManager:
    def test_context_manager(self):
        """Verify the client can be used as a context manager."""
        client = CmxClient.__new__(CmxClient)
        mock_conn = MagicMock()
        mock_conn.send.return_value = _make_response("ok", output="")
        client._conn = mock_conn

        with client as c:
            assert c is client
            mock_conn.connect.assert_called_once()
        mock_conn.close.assert_called_once()
