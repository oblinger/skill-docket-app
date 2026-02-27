"""Tests for cmx.docket.status -- StatusMap."""

from cmx.docket.status import StatusMap


def test_default_map_canonicalize():
    """Matches Rust test: status_map_round_trip."""
    m = StatusMap.default_map()
    assert m.canonicalize("\u2705") == "complete"
    assert m.canonicalize("[x]") == "complete"
    assert m.canonicalize("DONE") == "complete"
    assert m.canonicalize("\U0001f504") == "in_progress"
    assert m.canonicalize("WIP") == "in_progress"


def test_default_map_write_form():
    """Matches Rust test: status_map_round_trip."""
    m = StatusMap.default_map()
    assert m.write_form("complete") == "\u2705"
    assert m.write_form("pending") == "\u23f3"


def test_canonical_name_maps_to_itself():
    """Matches Rust test: status_map_canonical_name_maps_to_itself."""
    m = StatusMap.default_map()
    assert m.canonicalize("complete") == "complete"
    assert m.canonicalize("pending") == "pending"
    assert m.canonicalize("in_progress") == "in_progress"
    assert m.canonicalize("blocked") == "blocked"
    assert m.canonicalize("failed") == "failed"
    assert m.canonicalize("cancelled") == "cancelled"


def test_custom_status_map():
    """Test building a StatusMap from custom raw data."""
    raw = {
        "done": ["OK", "YES"],
        "todo": ["NO", "NOPE"],
    }
    m = StatusMap.from_raw(raw)
    assert m.canonicalize("OK") == "done"
    assert m.canonicalize("YES") == "done"
    assert m.canonicalize("NO") == "todo"
    assert m.write_form("done") == "OK"
    assert m.write_form("todo") == "NO"


def test_unknown_representation_returns_none():
    m = StatusMap.default_map()
    assert m.canonicalize("UNKNOWN") is None


def test_unknown_canonical_write_form_returns_none():
    m = StatusMap.default_map()
    assert m.write_form("nonexistent") is None
