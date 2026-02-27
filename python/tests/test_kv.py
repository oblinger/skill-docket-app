"""Tests for cmx.docket.kv -- KV format parsers and serializers."""

from cmx.docket.kv import (
    KvFormat,
    detect_format,
    parse_kv,
    serialize_kv,
)


# --- kv-colons tests ---

def test_parse_colons_basic():
    """Matches Rust test: parse_colons_basic."""
    body = "Status:: in_progress\nAssignee:: worker1\nPriority:: high\n"
    fields = parse_kv(body, KvFormat.COLONS)
    assert fields["Status"] == "in_progress"
    assert fields["Assignee"] == "worker1"
    assert fields["Priority"] == "high"


def test_parse_colons_empty_value():
    """Matches Rust test: parse_colons_empty_value."""
    body = "Status:: in_progress\nNotes::\n"
    fields = parse_kv(body, KvFormat.COLONS)
    assert fields["Notes"] == ""


def test_parse_colons_ignores_prose_colons():
    """Matches Rust test: parse_colons_ignores_prose_colons."""
    body = "Note: this is important\nStatus:: pending\n"
    fields = parse_kv(body, KvFormat.COLONS)
    assert len(fields) == 1
    assert fields["Status"] == "pending"


# --- kv-packed tests ---

def test_parse_packed_basic():
    """Matches Rust test: parse_packed_basic."""
    body = ":: status:in_progress, assignee:worker1, priority:high\n"
    fields = parse_kv(body, KvFormat.PACKED)
    assert fields["status"] == "in_progress"
    assert fields["assignee"] == "worker1"
    assert fields["priority"] == "high"


def test_parse_packed_whitespace():
    """Matches Rust test: parse_packed_whitespace."""
    body = "::  status : pending ,  assignee : nobody \n"
    fields = parse_kv(body, KvFormat.PACKED)
    assert fields["status"] == "pending"


# --- kv-table tests ---

def test_parse_table_basic():
    """Matches Rust test: parse_table_basic."""
    body = "| Field | Value |\n|-------|-------|\n| Status | in_progress |\n| Assignee | worker1 |\n"
    fields = parse_kv(body, KvFormat.TABLE)
    assert fields["Status"] == "in_progress"
    assert fields["Assignee"] == "worker1"


def test_parse_table_stops_at_non_table_line():
    """Matches Rust test: parse_table_stops_at_non_table_line."""
    body = "| Field | Value |\n|-------|-------|\n| Status | pending |\n\nSome text after.\n"
    fields = parse_kv(body, KvFormat.TABLE)
    assert len(fields) == 1


# --- kv-frontmatter tests ---

def test_parse_section_frontmatter_basic():
    """Matches Rust test: parse_section_frontmatter_basic."""
    body = "---\nstatus: in_progress\nassignee: worker1\n---\nSome body text.\n"
    fields = parse_kv(body, KvFormat.FRONTMATTER)
    assert fields["status"] == "in_progress"
    assert fields["assignee"] == "worker1"


def test_parse_section_frontmatter_numeric():
    """Matches Rust test: parse_section_frontmatter_numeric."""
    body = "---\npriority: 3\nretries: 0\n---\n"
    fields = parse_kv(body, KvFormat.FRONTMATTER)
    assert fields["priority"] == "3"


# --- format detection tests ---

def test_detect_colons_format():
    """Matches Rust test: detect_colons_format."""
    body = "Status:: pending\nAssignee:: nobody\n"
    assert detect_format(body) == KvFormat.COLONS


def test_detect_packed_format():
    """Matches Rust test: detect_packed_format."""
    body = ":: status:pending, assignee:nobody\nSome text.\n"
    assert detect_format(body) == KvFormat.PACKED


def test_detect_table_format():
    """Matches Rust test: detect_table_format."""
    body = "| Field | Value |\n|-------|-------|\n| Status | pending |\n"
    assert detect_format(body) == KvFormat.TABLE


def test_detect_frontmatter_format():
    """Matches Rust test: detect_frontmatter_format."""
    body = "---\nstatus: pending\n---\nBody text.\n"
    assert detect_format(body) == KvFormat.FRONTMATTER


# --- serialization round-trip tests ---

def test_colons_round_trip():
    """Matches Rust test: colons_round_trip."""
    fields = {"Status": "pending", "Assignee": "worker1"}
    serialized = serialize_kv(fields, KvFormat.COLONS)
    reparsed = parse_kv(serialized, KvFormat.COLONS)
    assert reparsed == fields


def test_packed_round_trip():
    """Matches Rust test: packed_round_trip."""
    fields = {"status": "pending", "assignee": "worker1"}
    serialized = serialize_kv(fields, KvFormat.PACKED)
    reparsed = parse_kv(serialized, KvFormat.PACKED)
    assert reparsed == fields


def test_table_round_trip():
    """Matches Rust test: table_round_trip."""
    fields = {"Status": "pending", "Assignee": "worker1"}
    serialized = serialize_kv(fields, KvFormat.TABLE)
    reparsed = parse_kv(serialized, KvFormat.TABLE)
    assert reparsed == fields


def test_frontmatter_round_trip():
    """Matches Rust test: frontmatter_round_trip."""
    fields = {"status": "pending", "assignee": "worker1"}
    serialized = serialize_kv(fields, KvFormat.FRONTMATTER)
    reparsed = parse_kv(serialized, KvFormat.FRONTMATTER)
    assert reparsed == fields


# --- cross-format equivalence test ---

def test_same_data_all_formats():
    """Matches Rust test: same_data_all_formats."""
    colons_body = "status:: in_progress\nassignee:: worker1\npriority:: high\n"
    packed_body = ":: status:in_progress, assignee:worker1, priority:high\n"
    table_body = "| Field | Value |\n|-------|-------|\n| status | in_progress |\n| assignee | worker1 |\n| priority | high |\n"
    fm_body = "---\nstatus: in_progress\nassignee: worker1\npriority: high\n---\n"

    colons_fields = parse_kv(colons_body, KvFormat.COLONS)
    packed_fields = parse_kv(packed_body, KvFormat.PACKED)
    table_fields = parse_kv(table_body, KvFormat.TABLE)
    fm_fields = parse_kv(fm_body, KvFormat.FRONTMATTER)

    assert colons_fields == packed_fields
    assert packed_fields == table_fields
    assert table_fields == fm_fields
