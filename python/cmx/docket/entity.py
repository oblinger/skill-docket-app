"""Entity extraction from markdown (M9.3).

Parses outline-layout headings into entity trees. Extracts name, title,
status, and KV fields from each heading section.

Matches Rust skill-docket entity.rs exactly.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .frontmatter import DocketFrontmatter
from .kv import KvFormat, parse_kv, detect_format, serialize_kv
from .status import StatusMap


@dataclass
class Entity:
    """A parsed entity from a docket-marked markdown file."""

    name: str
    title: str
    entity_type: str = ""
    status: str | None = None
    status_raw: str | None = None
    fields: dict[str, str] = field(default_factory=dict)
    kv_format: KvFormat = KvFormat.COLONS
    body: str = ""
    heading_level: int = 2
    line_number: int = 0


@dataclass
class EntityTree:
    """A tree of entities reflecting the heading hierarchy."""

    entity: Entity
    children: list[EntityTree] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def parse_outline(
    body: str,
    frontmatter: DocketFrontmatter,
    status_map: StatusMap,
) -> list[EntityTree]:
    """Parse an outline-layout document into entity trees.

    Each heading becomes an entity. Body extends until the next same-or-higher
    heading.
    """
    default_type = frontmatter.docket_type or ""
    format_hint = _parse_format_hint(frontmatter.docket_format)

    # Collect raw entities with their heading levels
    raw_entities: list[tuple[int, Entity]] = []
    current_heading: tuple[int, str, str, str | None, str | None, int] | None = None
    current_body_lines: list[str] = []

    for line_idx, line in enumerate(body.splitlines()):
        trimmed = line.strip()
        heading = _parse_heading(trimmed)
        if heading is not None:
            level, rest = heading
            # Flush previous entity
            if current_heading is not None:
                h_level, name, title, status, status_raw, h_line = current_heading
                entity_body = "\n".join(current_body_lines) + ("\n" if current_body_lines else "")
                entity = _build_entity(
                    name, title, default_type, status, status_raw,
                    entity_body, h_level, h_line, format_hint,
                )
                raw_entities.append((h_level, entity))
                current_body_lines = []

            # Parse this heading
            status, status_raw, name, title = _parse_heading_content(rest, status_map)

            if name:
                current_heading = (level, name, title, status, status_raw, line_idx + 1)
            else:
                current_heading = None
        elif current_heading is not None:
            current_body_lines.append(line)

    # Flush last entity
    if current_heading is not None:
        h_level, name, title, status, status_raw, h_line = current_heading
        entity_body = "\n".join(current_body_lines) + ("\n" if current_body_lines else "")
        entity = _build_entity(
            name, title, default_type, status, status_raw,
            entity_body, h_level, h_line, format_hint,
        )
        raw_entities.append((h_level, entity))

    return _nest_entities(raw_entities)


def parse_file_entity(
    body: str,
    frontmatter: DocketFrontmatter,
    status_map: StatusMap,
) -> Entity | None:
    """Parse a single file-layout entity (the entire file is one entity)."""
    default_type = frontmatter.docket_type or ""
    format_hint = _parse_format_hint(frontmatter.docket_format)

    for line_idx, line in enumerate(body.splitlines()):
        trimmed = line.strip()
        heading = _parse_heading(trimmed)
        if heading is not None:
            _level, rest = heading
            status, status_raw, name, title = _parse_heading_content(rest, status_map)
            if name:
                remaining_lines = body.splitlines()[line_idx + 1:]
                remaining_body = "\n".join(remaining_lines)
                return _build_entity(
                    name, title, default_type, status, status_raw,
                    remaining_body, 1, line_idx + 1, format_hint,
                )
    return None


def create_entity_markdown(
    name: str,
    title: str,
    fields: dict[str, str],
    heading_level: int,
    fmt: KvFormat,
    status_map: StatusMap,
) -> str:
    """Generate markdown for a new entity using the format template."""
    hashes = "#" * heading_level
    out = ""

    # Build heading with optional status
    status_value = fields.get("status") or fields.get("Status")
    if status_value is not None:
        write_form = status_map.write_form(status_value)
        if write_form is not None:
            out += f"{hashes} {write_form} {name} \u2014 {title}\n"
        else:
            out += f"{hashes} {name} \u2014 {title}\n"
    else:
        out += f"{hashes} {name} \u2014 {title}\n"

    # Add KV fields (exclude status since it's in the heading)
    non_status_fields = {
        k: v for k, v in fields.items()
        if k.lower() != "status"
    }

    if non_status_fields:
        out += serialize_kv(non_status_fields, fmt)
        out += "\n"

    return out


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

def _parse_heading(line: str) -> tuple[int, str] | None:
    """Parse a markdown heading line.

    Returns (level, rest_content) or None.
    """
    if not line.startswith("#"):
        return None
    level = 0
    for ch in line:
        if ch == "#":
            level += 1
        else:
            break
    if level < 1 or level > 6:
        return None
    rest = line[level:].strip()
    if not rest:
        return None
    return level, rest


def _parse_heading_content(
    rest: str,
    status_map: StatusMap,
) -> tuple[str | None, str | None, str, str]:
    """Parse heading content into (status, status_raw, name, title)."""
    status, status_raw, remainder = _extract_status_prefix(rest, status_map)
    name, title = _split_name_title(remainder)
    return status, status_raw, name, title


def _extract_status_prefix(
    s: str,
    status_map: StatusMap,
) -> tuple[str | None, str | None, str]:
    """Try to extract a status marker from the beginning of a heading."""
    for canonical, representations in status_map.forward.items():
        for repr_str in representations:
            if s.startswith(repr_str):
                rest = s[len(repr_str):].lstrip()
                return canonical, repr_str, rest
    return None, None, s


def _split_name_title(s: str) -> tuple[str, str]:
    """Split heading content on em-dash or spaced double-hyphen."""
    # Try em-dash first
    pos = s.find("\u2014")
    if pos >= 0:
        name = s[:pos].strip()
        title = s[pos + 1:].strip()  # em-dash is 1 char in Python
        return name, title
    # Try spaced double-hyphen
    pos = s.find(" -- ")
    if pos >= 0:
        name = s[:pos].strip()
        title = s[pos + 4:].strip()
        return name, title
    # No separator -- entire string is the name
    return s.strip(), s.strip()


def _parse_format_hint(format_str: str | None) -> KvFormat:
    """Parse the docket-format string into a KvFormat."""
    if format_str is None:
        return KvFormat.COLONS
    if format_str == "kv-colons" or "{kv-colons}" in format_str:
        return KvFormat.COLONS
    if format_str == "kv-packed" or "{kv-packed}" in format_str:
        return KvFormat.PACKED
    if format_str == "kv-table" or "{kv-table}" in format_str:
        return KvFormat.TABLE
    if format_str == "kv-frontmatter" or "{kv-frontmatter}" in format_str:
        return KvFormat.FRONTMATTER
    return KvFormat.COLONS


def _build_entity(
    name: str,
    title: str,
    default_type: str,
    status: str | None,
    status_raw: str | None,
    body: str,
    heading_level: int,
    line_number: int,
    format_hint: KvFormat,
) -> Entity:
    """Build an Entity from parsed heading and body data."""
    # Detect format from body, or use hint
    if body.strip():
        kv_format = detect_format(body)
    else:
        kv_format = format_hint

    fields = parse_kv(body, kv_format)

    # Check for inline Type override
    entity_type = fields.pop("Type", None) or fields.pop("type", None) or default_type

    # If status is in fields but not in heading, use the field value
    effective_status = status
    if effective_status is None:
        effective_status = fields.pop("Status", None) or fields.pop("status", None)

    return Entity(
        name=name,
        title=title,
        entity_type=entity_type,
        status=effective_status,
        status_raw=status_raw,
        fields=fields,
        kv_format=kv_format,
        body=body,
        heading_level=heading_level,
        line_number=line_number,
    )


def _nest_entities(items: list[tuple[int, Entity]]) -> list[EntityTree]:
    """Build a tree from a flat list of (depth, Entity) pairs."""
    if not items:
        return []

    roots: list[EntityTree] = []
    stack: list[tuple[int, EntityTree]] = []

    for depth, entity in items:
        tree = EntityTree(entity=entity, children=[])

        # Pop stack entries at same level or deeper
        while stack and stack[-1][0] >= depth:
            _, popped = stack.pop()
            if stack:
                stack[-1][1].children.append(popped)
            else:
                roots.append(popped)

        stack.append((depth, tree))

    # Flush remaining stack
    while stack:
        _, popped = stack.pop()
        if stack:
            stack[-1][1].children.append(popped)
        else:
            roots.append(popped)

    return roots
