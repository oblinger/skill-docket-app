"""Tests for the WatchManager."""

import pytest
from unittest.mock import MagicMock
from cmx.watch import WatchManager, WatchEntry


class TestWatchRegistration:
    def test_watch_registers_callback(self):
        """Register a watch, verify entry stored."""
        mock_client = MagicMock()
        mgr = WatchManager(mock_client)

        called = []
        def on_change():
            called.append(True)

        idx = mgr.watch("task.t1.status", on_change)
        assert idx == 0
        assert len(mgr._watches) == 1
        assert mgr._watches[0].callback is on_change

    def test_watch_extracts_variables(self):
        """Variables are extracted from pattern."""
        mock_client = MagicMock()
        mgr = WatchManager(mock_client)

        idx = mgr.watch("task.$t.agent.$a.status", lambda: None)
        assert mgr._watches[idx].variables == ['t', 'a']

    def test_multiple_watches(self):
        """Multiple watches get sequential indices."""
        mock_client = MagicMock()
        mgr = WatchManager(mock_client)

        idx0 = mgr.watch("a.b", lambda: None)
        idx1 = mgr.watch("c.d", lambda: None)
        idx2 = mgr.watch("e.f", lambda: None)

        assert idx0 == 0
        assert idx1 == 1
        assert idx2 == 2
        assert len(mgr._watches) == 3

    def test_unwatch(self):
        """Unwatching sets entry to None."""
        mock_client = MagicMock()
        mgr = WatchManager(mock_client)

        idx = mgr.watch("a.b", lambda: None)
        mgr.unwatch(idx)
        assert mgr._watches[idx] is None

    def test_poll_once_fires_on_change(self):
        """poll_once fires callback when value changes."""
        mock_client = MagicMock()
        mock_client.get.return_value = "new_value"

        mgr = WatchManager(mock_client)
        results = []
        mgr.watch("task.t1.status", lambda: results.append("fired"))

        fired = mgr.poll_once()
        assert len(fired) == 1
        assert results == ["fired"]

    def test_poll_once_no_fire_same_value(self):
        """poll_once does not fire when value is unchanged."""
        mock_client = MagicMock()
        mock_client.get.return_value = "same"

        mgr = WatchManager(mock_client)
        mgr.watch("task.t1.status", lambda: None)

        # First poll: fires because prev is None, current is "same"
        mgr.poll_once()
        # Second poll: should not fire because value unchanged
        fired = mgr.poll_once()
        assert len(fired) == 0
