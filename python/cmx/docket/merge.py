"""Multi-file merge and write-back (M9.4).

When the same entity appears in multiple files, fields are merged
with per-field source tracking. Write-back goes to the file where
each field was originally read.

Matches Rust skill-docket merge.rs exactly.
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path

from .entity import (
    Entity,
    EntityTree,
    create_entity_markdown,
    parse_file_entity,
    parse_outline,
)
from .frontmatter import (
    DocketFile,
    DocketFrontmatter,
    DocketLayout,
    parse_file,
    scan_directory,
)
from .kv import KvFormat, serialize_kv
from .status import StatusMap


class FieldSourceKind(Enum):
    PRIMARY = "primary"
    SECONDARY = "secondary"


@dataclass
class FieldSource:
    """Tracks which file a field value came from."""

    path: Path
    modified_ms: int = 0


@dataclass
class MergedEntity:
    """A merged entity combining fields from multiple source files."""

    name: str
    title: str
    entity_type: str = ""
    status: str | None = None
    fields: dict[str, str] = field(default_factory=dict)
    field_sources: dict[str, FieldSource] = field(default_factory=dict)
    primary_file: Path = field(default_factory=lambda: Path())
    primary_format: KvFormat = KvFormat.COLONS
    children: list[MergedEntity] = field(default_factory=list)


class MergeStore:
    """The merge store -- accumulates entities from multiple files."""

    def __init__(self, status_map: StatusMap | None = None) -> None:
        self.entities: dict[str, MergedEntity] = {}
        self.status_map = status_map if status_map is not None else StatusMap.default_map()
        self.loaded_files: list[Path] = []

    def load_file(self, docket_file: DocketFile) -> None:
        """Load a single docket file and merge its entities."""
        mtime = _file_mtime(docket_file.path)
        layout = (
            docket_file.frontmatter.docket_layout
            if docket_file.frontmatter.docket_layout is not None
            else DocketLayout.OUTLINE
        )

        if layout == DocketLayout.OUTLINE:
            trees = parse_outline(
                docket_file.body,
                docket_file.frontmatter,
                self.status_map,
            )
            for tree in trees:
                self._merge_tree(tree, docket_file.path, mtime)
        elif layout == DocketLayout.FILE:
            entity = parse_file_entity(
                docket_file.body,
                docket_file.frontmatter,
                self.status_map,
            )
            if entity is not None:
                self._merge_entity(entity, docket_file.path, mtime)
        elif layout == DocketLayout.FOLDER:
            entity = parse_file_entity(
                docket_file.body,
                docket_file.frontmatter,
                self.status_map,
            )
            if entity is not None:
                self._merge_entity(entity, docket_file.path, mtime)

        self.loaded_files.append(docket_file.path)

    def load_directory(self, dir_path: Path) -> None:
        """Scan a directory and load all docket files found."""
        files = scan_directory(dir_path)
        for f in files:
            self.load_file(f)

    def load_string(self, path: Path, content: str) -> None:
        """Load from a markdown string (useful for testing)."""
        docket_file = parse_file(path, content)
        if docket_file is not None:
            self.load_file(docket_file)

    def get(self, name: str) -> MergedEntity | None:
        """Get a merged entity by name."""
        return self.entities.get(name)

    def all(self) -> list[MergedEntity]:
        """Get all top-level entities."""
        return list(self.entities.values())

    def set_field(self, entity_name: str, field_name: str, value: str) -> None:
        """Update a field on an entity."""
        entity = self.entities.get(entity_name)
        if entity is None:
            raise KeyError(f"Entity '{entity_name}' not found")

        entity.fields[field_name] = value

        if field_name.lower() == "status":
            entity.status = value

        # Track field source -- use primary file for new fields
        if field_name not in entity.field_sources:
            entity.field_sources[field_name] = FieldSource(
                path=entity.primary_file,
                modified_ms=_now_ms(),
            )
        else:
            entity.field_sources[field_name].modified_ms = _now_ms()

    def write_back(self) -> list[Path]:
        """Write back all modified entities to their source files."""
        written: list[Path] = []

        # Group entities by their primary file
        by_file: dict[Path, list[MergedEntity]] = {}
        for entity in self.entities.values():
            by_file.setdefault(entity.primary_file, []).append(entity)

        for path, entities in by_file.items():
            if not path.exists():
                continue
            content = path.read_text()
            updated = _update_file_content(content, entities, self.status_map)
            path.write_text(updated)
            written.append(path)

        return written

    def create_entity(
        self,
        name: str,
        title: str,
        fields: dict[str, str],
        target_file: Path,
        heading_level: int = 2,
        fmt: KvFormat = KvFormat.COLONS,
    ) -> None:
        """Create a new entity and write it to the appropriate file."""
        md = create_entity_markdown(name, title, fields, heading_level, fmt, self.status_map)

        # Append to file
        if target_file.exists():
            content = target_file.read_text()
        else:
            content = ""

        if content and not content.endswith("\n"):
            content += "\n"
        content += md

        target_file.write_text(content)

        # Add to in-memory store
        mtime = _now_ms()
        field_sources: dict[str, FieldSource] = {}
        for key in fields:
            field_sources[key] = FieldSource(path=target_file, modified_ms=mtime)

        merged = MergedEntity(
            name=name,
            title=title,
            entity_type="",
            status=fields.get("status") or fields.get("Status"),
            fields=dict(fields),
            field_sources=field_sources,
            primary_file=target_file,
            primary_format=fmt,
            children=[],
        )
        self.entities[name] = merged

    # --- Internal ---

    def _merge_tree(self, tree: EntityTree, path: Path, mtime: int) -> None:
        self._merge_entity(tree.entity, path, mtime)

        parent_name = tree.entity.name
        child_names: list[str] = []

        for child in tree.children:
            self._merge_tree(child, path, mtime)
            child_names.append(child.entity.name)

        # Link children to parent
        cloned_children = [
            self.entities[n] for n in child_names if n in self.entities
        ]

        parent = self.entities.get(parent_name)
        if parent is not None:
            for child in cloned_children:
                if not any(c.name == child.name for c in parent.children):
                    parent.children.append(child)

    def _merge_entity(self, entity: Entity, path: Path, mtime: int) -> None:
        source = FieldSource(path=path, modified_ms=mtime)

        existing = self.entities.get(entity.name)
        if existing is not None:
            # Merge fields -- most recently modified wins
            for key, value in entity.fields.items():
                existing_source = existing.field_sources.get(key)
                should_update = existing_source is None or mtime >= existing_source.modified_ms
                if should_update:
                    existing.fields[key] = value
                    existing.field_sources[key] = FieldSource(path=path, modified_ms=mtime)

            # Update status if newer
            if entity.status is not None:
                existing_source = existing.field_sources.get("status")
                should_update = existing_source is None or mtime >= existing_source.modified_ms
                if should_update:
                    existing.status = entity.status
                    existing.field_sources["status"] = FieldSource(path=path, modified_ms=mtime)

            # Update primary file if this file has more fields
            this_field_count = len(entity.fields) + (1 if entity.status is not None else 0)
            existing_primary_count = sum(
                1 for fs in existing.field_sources.values()
                if fs.path == existing.primary_file
            )
            if this_field_count > existing_primary_count:
                existing.primary_file = path
                existing.primary_format = entity.kv_format
        else:
            # New entity
            field_sources: dict[str, FieldSource] = {}
            for key in entity.fields:
                field_sources[key] = FieldSource(path=path, modified_ms=mtime)
            if entity.status is not None:
                field_sources["status"] = FieldSource(path=path, modified_ms=mtime)

            merged = MergedEntity(
                name=entity.name,
                title=entity.title,
                entity_type=entity.entity_type,
                status=entity.status,
                fields=dict(entity.fields),
                field_sources=field_sources,
                primary_file=path,
                primary_format=entity.kv_format,
                children=[],
            )
            self.entities[entity.name] = merged


# ---------------------------------------------------------------------------
# File content update
# ---------------------------------------------------------------------------

def _update_file_content(
    content: str,
    entities: list[MergedEntity],
    status_map: StatusMap,
) -> str:
    """Update a file's content with modified entity fields."""
    result: list[str] = []
    lines = content.splitlines()
    i = 0

    while i < len(lines):
        line = lines[i]
        trimmed = line.strip()

        if trimmed.startswith("#"):
            entity = _find_entity_for_heading(trimmed, entities, status_map)
            if entity is not None:
                # Rewrite the heading with updated status
                level = 0
                for ch in trimmed:
                    if ch == "#":
                        level += 1
                    else:
                        break
                hashes = "#" * level

                if entity.status is not None:
                    write_form = status_map.write_form(entity.status)
                    if write_form is not None:
                        result.append(f"{hashes} {write_form} {entity.name} \u2014 {entity.title}")
                    else:
                        result.append(f"{hashes} {entity.name} \u2014 {entity.title}")
                else:
                    result.append(f"{hashes} {entity.name} \u2014 {entity.title}")

                i += 1

                # Replace KV lines in the body
                kv_lines_written = False
                while i < len(lines):
                    body_line = lines[i].strip()
                    # Stop at next heading
                    if body_line.startswith("#"):
                        break

                    if _is_kv_line(body_line, entity.primary_format):
                        if not kv_lines_written:
                            non_status = {
                                k: v for k, v in entity.fields.items()
                                if k.lower() != "status"
                            }
                            if non_status:
                                result.append(serialize_kv(non_status, entity.primary_format))
                            kv_lines_written = True
                        i += 1
                        continue

                    result.append(lines[i])
                    i += 1

                if not kv_lines_written:
                    non_status = {
                        k: v for k, v in entity.fields.items()
                        if k.lower() != "status"
                    }
                    if non_status:
                        result.append(serialize_kv(non_status, entity.primary_format))

                continue

        result.append(line)
        i += 1

    return "\n".join(result) + "\n"


def _is_kv_line(line: str, fmt: KvFormat) -> bool:
    """Check if a line is a KV field line in the given format."""
    if fmt == KvFormat.COLONS:
        return ":: " in line or (line.endswith("::") and not line.startswith("#"))
    elif fmt == KvFormat.PACKED:
        return line.startswith(":: ")
    elif fmt == KvFormat.TABLE:
        return line.startswith("|") and line.endswith("|")
    elif fmt == KvFormat.FRONTMATTER:
        return line == "---" or (not line.startswith("#") and ": " in line)
    return False


def _find_entity_for_heading(
    heading: str,
    entities: list[MergedEntity],
    status_map: StatusMap,
) -> MergedEntity | None:
    """Find which entity a heading line belongs to."""
    level = 0
    for ch in heading:
        if ch == "#":
            level += 1
        else:
            break
    rest = heading[level:].strip()

    # Strip status marker if present
    rest = _strip_status_prefix(rest, status_map)

    # Extract name (before em-dash)
    pos = rest.find("\u2014")
    if pos >= 0:
        name = rest[:pos].strip()
    else:
        pos = rest.find(" -- ")
        if pos >= 0:
            name = rest[:pos].strip()
        else:
            name = rest.strip()

    for e in entities:
        if e.name == name:
            return e
    return None


def _strip_status_prefix(s: str, status_map: StatusMap) -> str:
    """Strip a status representation from the start of a string."""
    for representations in status_map.forward.values():
        for repr_str in representations:
            if s.startswith(repr_str):
                return s[len(repr_str):].lstrip()
    return s


def _file_mtime(path: Path) -> int:
    """Get file modification time in milliseconds since epoch."""
    try:
        return int(path.stat().st_mtime * 1000)
    except OSError:
        return 0


def _now_ms() -> int:
    """Get current time in milliseconds since epoch."""
    return int(time.time() * 1000)
