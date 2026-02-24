"""Reactive watch -- fire callbacks when state changes match patterns."""

import re
import threading
import time
from typing import Callable, Dict, Any, Optional, List

# Matches $variable_name in patterns (word chars after a $)
_VAR_RE = re.compile(r'\$(\w+)')


class WatchEntry:
    """A registered watch."""

    def __init__(self, pattern: str, callback: Callable, variables: List[str]):
        self.pattern = pattern
        self.callback = callback
        self.variables = variables


class WatchManager:
    """Manages watch registrations and polls for changes.

    Watches are patterns against the CMX namespace store. When a value
    matching a pattern changes, the registered callback fires with
    bound variables as keyword arguments.
    """

    def __init__(self, client):
        self._client = client
        self._watches: List[WatchEntry] = []
        self._snapshots: Dict[int, Any] = {}
        self._poll_thread: Optional[threading.Thread] = None
        self._running = False
        self._lock = threading.Lock()

    def watch(self, pattern: str, callback: Callable) -> int:
        """Register a callback for a state change pattern.

        The callback receives bound variables as keyword arguments.
        Returns the watch index.
        """
        variables = _VAR_RE.findall(pattern)
        entry = WatchEntry(pattern, callback, variables)
        with self._lock:
            idx = len(self._watches)
            self._watches.append(entry)
            self._snapshots[idx] = None
        return idx

    def unwatch(self, index: int) -> None:
        """Remove a watch by index."""
        with self._lock:
            if 0 <= index < len(self._watches):
                self._watches[index] = None
                self._snapshots.pop(index, None)

    def start(self, poll_interval: float = 0.5) -> None:
        """Start polling for changes in a background thread."""
        if self._running:
            return
        self._running = True
        self._poll_thread = threading.Thread(
            target=self._poll_loop, args=(poll_interval,), daemon=True
        )
        self._poll_thread.start()

    def stop(self) -> None:
        """Stop the polling thread."""
        self._running = False
        if self._poll_thread is not None:
            self._poll_thread.join(timeout=5.0)
            self._poll_thread = None

    def _poll_loop(self, interval: float) -> None:
        """Background polling loop."""
        while self._running:
            try:
                self.poll_once()
            except Exception:
                pass
            time.sleep(interval)

    def poll_once(self) -> List[Dict[str, Any]]:
        """Check for changes once and fire matching callbacks.

        Returns list of fired watch results.
        """
        fired = []
        with self._lock:
            watches = list(enumerate(self._watches))

        for idx, entry in watches:
            if entry is None:
                continue
            try:
                current = self._client.get(entry.pattern)
            except (KeyError, Exception):
                current = None

            prev = self._snapshots.get(idx)
            if current != prev:
                with self._lock:
                    self._snapshots[idx] = current
                try:
                    result = entry.callback()
                    fired.append({"index": idx, "pattern": entry.pattern, "result": result})
                except Exception as exc:
                    fired.append({"index": idx, "pattern": entry.pattern, "error": str(exc)})

        return fired
