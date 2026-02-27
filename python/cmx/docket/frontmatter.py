"""Frontmatter marker detection and parsing (M9.1).

Detects YAML frontmatter containing docket-type, docket-layout, docket-format,
and docket-status keys. Files without docket markers are ignored.

Matches Rust skill-docket frontmatter.rs exactly.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path

import yaml

from .kv import KvFormat
from .status import StatusMap


class DocketLayout(Enum):
    OUTLINE = "outline"
    FILE = "file"
    FOLDER = "folder"


@dataclass
class DocketFrontmatter:
    docket_type: str | None = None
    docket_layout: DocketLayout | None = None
    docket_format: str | None = None
    docket_status: dict[str, list[str]] | None = None
    docket_regex: str | None = None


@dataclass
class DocketFile:
    path: Path
    frontmatter: DocketFrontmatter
    body: str
    byte_offset: int  # where body starts after frontmatter


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def parse_frontmatter(content: str) -> tuple[DocketFrontmatter, str, int] | None:
    """Parse a markdown file for docket markers.

    Returns (frontmatter, body, byte_offset) or None if no docket frontmatter.
    """
    extracted = _extract_frontmatter(content)
    if extracted is None:
        return None
    yaml_str, body, offset = extracted
    fm = _parse_docket_yaml(yaml_str)
    if fm is None:
        return None
    return fm, body, offset


def parse_file(path: Path, content: str) -> DocketFile | None:
    """Parse a markdown file for docket markers.

    Returns a DocketFile or None if no docket frontmatter.
    """
    result = parse_frontmatter(content)
    if result is None:
        return None
    fm, body, offset = result
    return DocketFile(path=path, frontmatter=fm, body=body, byte_offset=offset)


def scan_directory(dir_path: Path) -> list[DocketFile]:
    """Scan a directory for docket-marked markdown files.

    Follows folder markers: if a directory contains ``.docket-folder.md``,
    all ``.md`` files in that directory inherit its docket settings.
    """
    results: list[DocketFile] = []
    _scan_dir_recursive(dir_path, None, results)
    return results


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

def _extract_frontmatter(content: str) -> tuple[str, str, int] | None:
    """Extract YAML frontmatter from a markdown string.

    Returns (yaml_str, body, body_offset) or None if no frontmatter.
    """
    trimmed = content.lstrip()
    if not trimmed.startswith("---"):
        return None

    after_first = trimmed[3:]
    if after_first.startswith("\n"):
        after_first = after_first[1:]

    end = after_first.find("\n---")
    if end < 0:
        return None

    yaml_str = after_first[:end]

    # Find where body starts (after the closing --- line)
    rest_after_close = after_first[end + 4:]
    nl_pos = rest_after_close.find("\n")
    if nl_pos >= 0:
        body_start = end + 4 + nl_pos + 1
    else:
        body_start = len(after_first)

    # Compute actual offset in original content
    leading = len(content) - len(trimmed)
    first_fence_len = 3
    strip_newline = 1 if trimmed[3:].startswith("\n") else 0
    offset = leading + first_fence_len + strip_newline + body_start

    body = content[offset:]
    return yaml_str, body, offset


def _parse_docket_yaml(yaml_str: str) -> DocketFrontmatter | None:
    """Parse docket frontmatter from a YAML string.

    Returns None if the YAML doesn't contain any docket keys.
    """
    try:
        data = yaml.safe_load(yaml_str)
    except yaml.YAMLError:
        return None

    if not isinstance(data, dict):
        return None

    fm = DocketFrontmatter()

    fm.docket_type = data.get("docket-type")
    layout_str = data.get("docket-layout")
    if layout_str is not None:
        try:
            fm.docket_layout = DocketLayout(layout_str)
        except ValueError:
            pass
    fm.docket_format = data.get("docket-format")
    fm.docket_status = data.get("docket-status")
    fm.docket_regex = data.get("docket-regex")

    # Must have at least one docket key to be considered a docket file
    if (fm.docket_type is None
            and fm.docket_layout is None
            and fm.docket_format is None
            and fm.docket_status is None):
        return None

    return fm


def _scan_dir_recursive(
    dir_path: Path,
    inherited: DocketFrontmatter | None,
    results: list[DocketFile],
) -> None:
    # Check for folder marker first
    marker_path = dir_path / ".docket-folder.md"
    folder_fm: DocketFrontmatter | None = None
    if marker_path.exists():
        content = marker_path.read_text()
        extracted = _extract_frontmatter(content)
        if extracted is not None:
            yaml_str, _, _ = extracted
            folder_fm = _parse_docket_yaml(yaml_str)

    effective_inherited = folder_fm if folder_fm is not None else inherited

    subdirs: list[Path] = []

    try:
        entries = sorted(dir_path.iterdir())
    except OSError:
        return

    for entry in entries:
        name = entry.name

        if entry.is_dir():
            if not name.startswith("."):
                subdirs.append(entry)
            continue

        # Skip non-markdown and marker files
        if not name.endswith(".md") or name == ".docket-folder.md":
            continue

        try:
            content = entry.read_text()
        except OSError:
            continue

        docket_file = parse_file(entry, content)
        if docket_file is not None:
            results.append(docket_file)
        elif effective_inherited is not None:
            # File has no docket frontmatter but inherits from folder marker
            extracted = _extract_frontmatter(content)
            if extracted is not None:
                _, body, byte_offset = extracted
            else:
                body = content
                byte_offset = 0
            results.append(DocketFile(
                path=entry,
                frontmatter=effective_inherited,
                body=body,
                byte_offset=byte_offset,
            ))

    for subdir in subdirs:
        _scan_dir_recursive(subdir, effective_inherited, results)
