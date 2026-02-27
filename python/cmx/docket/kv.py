"""KV encoding format parsers (M9.2).

Four formats for encoding key-value fields within markdown entities:
- kv-colons: ``Key:: value`` (Dataview-compatible)
- kv-packed: ``:: key:value, key:value`` (compact single line)
- kv-table: ``| Field | Value |`` (pipe-delimited table)
- kv-frontmatter: YAML ``---`` blocks within sections

Matches Rust skill-docket kv.rs exactly.
"""

from __future__ import annotations

from enum import Enum

import yaml


class KvFormat(Enum):
    COLONS = "kv-colons"
    PACKED = "kv-packed"
    TABLE = "kv-table"
    FRONTMATTER = "kv-frontmatter"


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def parse_kv(body: str, fmt: KvFormat) -> dict[str, str]:
    """Parse key-value fields from body text using the specified format."""
    if fmt == KvFormat.COLONS:
        return _parse_colons(body)
    elif fmt == KvFormat.PACKED:
        return _parse_packed(body)
    elif fmt == KvFormat.TABLE:
        return _parse_table(body)
    elif fmt == KvFormat.FRONTMATTER:
        return _parse_section_frontmatter(body)
    return {}


def detect_format(body: str) -> KvFormat:
    """Auto-detect the KV format used in body text.

    Detection priority: frontmatter -> packed -> colons -> table.
    """
    for line in body.splitlines():
        trimmed = line.strip()
        # kv-frontmatter (--- within section body)
        if trimmed == "---":
            return KvFormat.FRONTMATTER
        # kv-packed (line starting with ::)
        if trimmed.startswith(":: "):
            return KvFormat.PACKED
        # kv-colons (Key:: value)
        pos = trimmed.find(":: ")
        if pos >= 0:
            before = trimmed[:pos]
            if before and "  " not in before:
                return KvFormat.COLONS
        # Bare Key:: at end of line (empty value)
        if trimmed.endswith("::") and not trimmed.startswith("#"):
            before = trimmed[:-2]
            if before and "  " not in before:
                return KvFormat.COLONS
        # kv-table (| Field | Value |)
        if trimmed.startswith("|") and trimmed.endswith("|"):
            cells = trimmed.split("|")
            if len(cells) >= 3:
                return KvFormat.TABLE
    # Default to colons if nothing detected
    return KvFormat.COLONS


def serialize_kv(fields: dict[str, str], fmt: KvFormat) -> str:
    """Serialize fields back to the specified KV format."""
    if fmt == KvFormat.COLONS:
        return _serialize_colons(fields)
    elif fmt == KvFormat.PACKED:
        return _serialize_packed(fields)
    elif fmt == KvFormat.TABLE:
        return _serialize_table(fields)
    elif fmt == KvFormat.FRONTMATTER:
        return _serialize_section_frontmatter(fields)
    return ""


# ---------------------------------------------------------------------------
# kv-colons
# ---------------------------------------------------------------------------

def _parse_colons(body: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for line in body.splitlines():
        trimmed = line.strip()
        pos = trimmed.find(":: ")
        if pos >= 0:
            key = trimmed[:pos].strip()
            value = trimmed[pos + 3:].strip()
            if key:
                fields[key] = value
        elif trimmed.endswith("::") and not trimmed.startswith("#"):
            key = trimmed[:-2].strip()
            if key:
                fields[key] = ""
    return fields


def _serialize_colons(fields: dict[str, str]) -> str:
    lines = []
    for k in sorted(fields.keys()):
        v = fields[k]
        if v:
            lines.append(f"{k}:: {v}")
        else:
            lines.append(f"{k}::")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# kv-packed
# ---------------------------------------------------------------------------

def _parse_packed(body: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for line in body.splitlines():
        trimmed = line.strip()
        if trimmed.startswith(":: "):
            rest = trimmed[3:]
            for pair in rest.split(","):
                pair = pair.strip()
                pos = pair.find(":")
                if pos >= 0:
                    key = pair[:pos].strip()
                    value = pair[pos + 1:].strip()
                    if key:
                        fields[key] = value
    return fields


def _serialize_packed(fields: dict[str, str]) -> str:
    pairs = sorted(f"{k}:{v}" for k, v in fields.items())
    return ":: " + ", ".join(pairs)


# ---------------------------------------------------------------------------
# kv-table
# ---------------------------------------------------------------------------

def _parse_table(body: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    in_table = False
    header_seen = False

    for line in body.splitlines():
        trimmed = line.strip()
        if not trimmed.startswith("|") or not trimmed.endswith("|"):
            if in_table:
                break  # Table ended
            continue

        cells = [c.strip() for c in trimmed.split("|")]
        # Need at least 4 parts (empty, field, value, empty) from split on |
        if len(cells) < 4:
            continue

        if not in_table:
            # This is the header row
            in_table = True
            continue

        if not header_seen:
            # This is the separator row (|---|---|)
            if all(c == "" or all(ch in "-— " for ch in c) for c in cells[1:-1]):
                header_seen = True
                continue

        # Data row
        key = cells[1].strip()
        value = cells[2].strip()
        if key:
            fields[key] = value

    return fields


def _serialize_table(fields: dict[str, str]) -> str:
    lines = ["| Field | Value |", "|-------|-------|"]
    for k in sorted(fields.keys(), key=str.lower):
        lines.append(f"| {k} | {fields[k]} |")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# kv-frontmatter (YAML blocks within sections)
# ---------------------------------------------------------------------------

def _parse_section_frontmatter(body: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    in_yaml = False
    yaml_buf: list[str] = []

    for line in body.splitlines():
        trimmed = line.strip()
        if trimmed == "---":
            if in_yaml:
                # End of YAML block -- parse it
                yaml_text = "\n".join(yaml_buf)
                try:
                    parsed = yaml.safe_load(yaml_text)
                    if isinstance(parsed, dict):
                        for k, v in parsed.items():
                            fields[str(k)] = _yaml_value_to_string(v)
                except yaml.YAMLError:
                    pass
                yaml_buf.clear()
                in_yaml = False
            else:
                in_yaml = True
            continue
        if in_yaml:
            yaml_buf.append(line)

    return fields


def _yaml_value_to_string(v: object) -> str:
    if v is None:
        return ""
    if isinstance(v, bool):
        return str(v).lower()
    if isinstance(v, (int, float)):
        return str(v)
    if isinstance(v, str):
        return v
    return str(v)


def _serialize_section_frontmatter(fields: dict[str, str]) -> str:
    lines = ["---"]
    for k in sorted(fields.keys(), key=str.lower):
        lines.append(f"{k}: {fields[k]}")
    lines.append("---")
    return "\n".join(lines)
