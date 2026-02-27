"""Tests for cmx.docket.frontmatter -- frontmatter detection, parsing, scanning."""

import tempfile
from pathlib import Path

from cmx.docket.frontmatter import (
    DocketFrontmatter,
    DocketLayout,
    parse_file,
    parse_frontmatter,
    scan_directory,
)


def test_extract_frontmatter_basic():
    """Matches Rust test: extract_frontmatter_basic."""
    content = "---\ndocket-type: task\n---\n# Hello\nBody here\n"
    result = parse_frontmatter(content)
    assert result is not None
    fm, body, _offset = result
    assert fm.docket_type == "task"
    assert "# Hello" in body


def test_extract_frontmatter_none_when_missing():
    """Matches Rust test: extract_frontmatter_none_when_missing."""
    assert parse_frontmatter("# Just a heading\n") is None


def test_parse_docket_yaml_with_type():
    """Matches Rust test: parse_docket_yaml_with_type."""
    content = "---\ndocket-type: task\ndocket-layout: outline\n---\nbody\n"
    result = parse_frontmatter(content)
    assert result is not None
    fm, _, _ = result
    assert fm.docket_type == "task"
    assert fm.docket_layout == DocketLayout.OUTLINE


def test_parse_docket_yaml_ignores_non_docket():
    """Matches Rust test: parse_docket_yaml_ignores_non_docket."""
    content = "---\ntitle: My Document\nauthor: Someone\n---\nbody\n"
    assert parse_frontmatter(content) is None


def test_parse_file_with_docket_markers():
    """Matches Rust test: parse_file_with_docket_markers."""
    content = "---\ndocket-type: task\ndocket-layout: outline\n---\n# Roadmap\n## AUTH \u2014 Login\n"
    df = parse_file(Path("roadmap.md"), content)
    assert df is not None
    assert df.frontmatter.docket_type == "task"
    assert "# Roadmap" in df.body


def test_parse_file_returns_none_without_markers():
    """Matches Rust test: parse_file_returns_none_without_markers."""
    content = "---\ntitle: Not a docket file\n---\n# Hello\n"
    assert parse_file(Path("test.md"), content) is None


def test_parse_docket_status_mapping():
    """Matches Rust test: parse_docket_status_mapping."""
    content = '---\ndocket-type: task\ndocket-status:\n  complete: ["\u2705", "[x]", "DONE"]\n  pending: ["\u23f3", "[ ]"]\n---\nbody\n'
    result = parse_frontmatter(content)
    assert result is not None
    fm, _, _ = result
    status = fm.docket_status
    assert status is not None
    assert len(status["complete"]) == 3
    assert len(status["pending"]) == 2


def test_parse_format_string_level1():
    """Matches Rust test: parse_format_string_level1."""
    content = "---\ndocket-type: task\ndocket-format: kv-colons\n---\nbody\n"
    result = parse_frontmatter(content)
    assert result is not None
    fm, _, _ = result
    assert fm.docket_format == "kv-colons"


def test_parse_format_string_level2():
    """Matches Rust test: parse_format_string_level2."""
    content = '---\ndocket-type: task\ndocket-format: "{status} {name} \\u2014 {title}\\n{kv-colons}"\n---\nbody\n'
    result = parse_frontmatter(content)
    assert result is not None
    fm, _, _ = result
    assert "{status}" in fm.docket_format


def test_scan_directory_finds_docket_files():
    """Matches Rust test: scan_directory_finds_docket_files."""
    with tempfile.TemporaryDirectory() as tmpdir:
        d = Path(tmpdir)
        # Docket-marked file
        (d / "roadmap.md").write_text(
            "---\ndocket-type: task\ndocket-layout: outline\n---\n# Roadmap\n"
        )
        # Non-docket file
        (d / "readme.md").write_text("# README\nJust a readme.\n")
        # Non-markdown file
        (d / "notes.txt").write_text("some notes")

        files = scan_directory(d)
        assert len(files) == 1
        assert files[0].path.name == "roadmap.md"


def test_scan_directory_inherits_folder_marker():
    """Matches Rust test: scan_directory_inherits_folder_marker."""
    with tempfile.TemporaryDirectory() as tmpdir:
        d = Path(tmpdir)
        # Folder marker
        (d / ".docket-folder.md").write_text(
            "---\ndocket-type: task\ndocket-layout: folder\n---\n# Tasks\n"
        )
        # File without its own frontmatter -- should inherit
        (d / "auth.md").write_text("# AUTH \u2014 Login\nStatus:: pending\n")
        # File with its own docket frontmatter -- uses its own
        (d / "data.md").write_text(
            "---\ndocket-type: design-task\n---\n# DATA \u2014 Data Layer\n"
        )

        files = scan_directory(d)
        assert len(files) == 2

        auth = next(f for f in files if f.path.name == "auth.md")
        assert auth.frontmatter.docket_type == "task"
        assert auth.frontmatter.docket_layout == DocketLayout.FOLDER

        data = next(f for f in files if f.path.name == "data.md")
        assert data.frontmatter.docket_type == "design-task"


def test_scan_directory_recurses_into_subdirs():
    """Matches Rust test: scan_directory_recurses_into_subdirs."""
    with tempfile.TemporaryDirectory() as tmpdir:
        d = Path(tmpdir)
        sub = d / "specs"
        sub.mkdir()

        (d / "roadmap.md").write_text(
            "---\ndocket-type: task\n---\n# Roadmap\n"
        )
        (sub / "auth.md").write_text(
            "---\ndocket-type: spec\ndocket-layout: file\n---\n# Auth Spec\n"
        )

        files = scan_directory(d)
        assert len(files) == 2
