"""CMX Python package -- programmatic access to the CMX parameter store."""

from .client import CmxClient
from .decorators import when, rules, get_registered_decorators, get_registered_rules
from .watch import WatchManager

# Module-level convenience instance (connects lazily)
_default_client: CmxClient = None


def _get_client() -> CmxClient:
    global _default_client
    if _default_client is None:
        _default_client = CmxClient()
    return _default_client


def get(path: str):
    """GET a value from the parameter store."""
    return _get_client().get(path)


def set(path: str, value):
    """SET a value in the parameter store."""
    return _get_client().set(path, value)


def append(path: str, value):
    """APPEND a value in the parameter store."""
    return _get_client().append(path, value)


def keys(pattern: str = None):
    """List keys matching a pattern."""
    return _get_client().keys(pattern)


def watch(pattern: str, callback):
    """Register a callback for state changes matching a pattern."""
    mgr = WatchManager(_get_client())
    return mgr.watch(pattern, callback)


def connect(socket_path: str = None) -> CmxClient:
    """Create and return a connected client."""
    global _default_client
    _default_client = CmxClient(socket_path)
    _default_client.connect()
    return _default_client


__all__ = [
    'CmxClient', 'WatchManager',
    'when', 'rules',
    'get', 'set', 'append', 'keys', 'watch', 'connect',
    'get_registered_decorators', 'get_registered_rules',
]
