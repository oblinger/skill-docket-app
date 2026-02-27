"""Tests for cmx.docket.merge -- multi-file merge, provenance, write-back."""

import tempfile
from pathlib import Path

from cmx.docket.kv import KvFormat
from cmx.docket.merge import MergeStore
from cmx.docket.status import StatusMap


def test_merge_store_single_file():
    """Matches Rust test: merge_store_single_file."""
    store = MergeStore()
    content = (
        "---\n"
        "docket-type: task\n"
        "docket-layout: outline\n"
        "---\n"
        "## AUTH \u2014 Authentication\n"
        "Status:: in_progress\n"
        "### AUTH1 \u2014 Login API\n"
        "Status:: complete\n"
        "### AUTH2 \u2014 Token refresh\n"
        "Status:: pending\n"
    )
    store.load_string(Path("/tmp/roadmap.md"), content)

    assert store.get("AUTH") is not None
    assert store.get("AUTH1") is not None
    assert store.get("AUTH2") is not None
    assert store.get("AUTH").status == "in_progress"
    assert store.get("AUTH1").status == "complete"


def test_merge_store_two_files():
    """Matches Rust test: merge_store_two_files."""
    store = MergeStore()

    roadmap = (
        "---\n"
        "docket-type: task\n"
        "docket-layout: outline\n"
        "---\n"
        "## AUTH1 \u2014 Login API\n"
        "Status:: in_progress\n"
        "Assignee:: worker1\n"
    )
    store.load_string(Path("/tmp/roadmap.md"), roadmap)

    spec = (
        "---\n"
        "docket-type: task\n"
        "docket-layout: file\n"
        "---\n"
        "# AUTH1 \u2014 Login API\n"
        "Priority:: high\n"
        "Description:: Implement the login endpoint\n"
    )
    store.load_string(Path("/tmp/specs/auth1.md"), spec)

    entity = store.get("AUTH1")
    assert entity is not None
    assert entity.status == "in_progress"
    assert entity.fields.get("Assignee") == "worker1"
    assert entity.fields.get("Priority") == "high"
    assert entity.fields.get("Description") == "Implement the login endpoint"


def test_merge_store_field_provenance():
    """Matches Rust test: merge_store_field_provenance."""
    store = MergeStore()

    roadmap = (
        "---\n"
        "docket-type: task\n"
        "docket-layout: outline\n"
        "---\n"
        "## AUTH1 \u2014 Login\n"
        "Status:: pending\n"
    )
    store.load_string(Path("/tmp/roadmap.md"), roadmap)

    spec = (
        "---\n"
        "docket-type: task\n"
        "docket-layout: file\n"
        "---\n"
        "# AUTH1 \u2014 Login\n"
        "Priority:: high\n"
    )
    store.load_string(Path("/tmp/auth1.md"), spec)

    entity = store.get("AUTH1")
    assert entity is not None
    # Status came from roadmap
    status_source = entity.field_sources.get("status")
    assert status_source is not None
    assert status_source.path == Path("/tmp/roadmap.md")
    # Priority came from spec
    priority_source = entity.field_sources.get("Priority")
    assert priority_source is not None
    assert priority_source.path == Path("/tmp/auth1.md")


def test_merge_store_set_field():
    """Matches Rust test: merge_store_set_field."""
    store = MergeStore()
    content = (
        "---\n"
        "docket-type: task\n"
        "docket-layout: outline\n"
        "---\n"
        "## AUTH1 \u2014 Login\n"
        "Status:: pending\n"
    )
    store.load_string(Path("/tmp/roadmap.md"), content)

    store.set_field("AUTH1", "status", "complete")
    assert store.get("AUTH1").status == "complete"


def test_merge_store_set_field_unknown_entity():
    """Matches Rust test: merge_store_set_field_unknown_entity."""
    store = MergeStore()
    try:
        store.set_field("NOPE", "status", "complete")
        assert False, "Should have raised"
    except KeyError:
        pass


def test_merge_store_create_and_write():
    """Matches Rust test: merge_store_create_and_write."""
    with tempfile.TemporaryDirectory() as tmpdir:
        target = Path(tmpdir) / "roadmap.md"
        target.write_text(
            "---\n"
            "docket-type: task\n"
            "docket-layout: outline\n"
            "---\n"
            "# Roadmap\n"
        )

        store = MergeStore()
        fields = {"status": "pending", "Assignee": "nobody"}
        store.create_entity("NEW1", "New Task", fields, target, 2, KvFormat.COLONS)

        content = target.read_text()
        assert "## \u23f3 NEW1 \u2014 New Task" in content
        assert "Assignee:: nobody" in content


def test_write_back_updates_status_in_heading():
    """Matches Rust test: write_back_updates_status_in_heading."""
    with tempfile.TemporaryDirectory() as tmpdir:
        target = Path(tmpdir) / "roadmap.md"
        original = (
            "---\n"
            "docket-type: task\n"
            "docket-layout: outline\n"
            "---\n"
            "## AUTH1 \u2014 Login\n"
            "Status:: pending\n"
            "Assignee:: worker1\n"
        )
        target.write_text(original)

        store = MergeStore()
        store.load_string(target, original)
        store.set_field("AUTH1", "status", "complete")
        # Fix primary_file to point to actual file
        store.entities["AUTH1"].primary_file = target
        store.write_back()

        updated = target.read_text()
        assert "\u2705 AUTH1 \u2014 Login" in updated


def test_merge_directory_scan():
    """Matches Rust test: merge_directory_scan."""
    with tempfile.TemporaryDirectory() as tmpdir:
        d = Path(tmpdir)
        (d / "roadmap.md").write_text(
            "---\n"
            "docket-type: task\n"
            "docket-layout: outline\n"
            "---\n"
            "## T1 \u2014 First Task\n"
            "Status:: pending\n"
            "## T2 \u2014 Second Task\n"
            "Status:: complete\n"
        )
        (d / "readme.md").write_text("# Not a docket file\n")

        store = MergeStore()
        store.load_directory(d)

        assert store.get("T1") is not None
        assert store.get("T2") is not None
        assert len(store.entities) == 2


def test_merge_children_linked():
    """Matches Rust test: merge_children_linked."""
    store = MergeStore()
    content = (
        "---\n"
        "docket-type: task\n"
        "docket-layout: outline\n"
        "---\n"
        "## PARENT \u2014 Parent Task\n"
        "Status:: in_progress\n"
        "### CHILD1 \u2014 First Child\n"
        "Status:: complete\n"
        "### CHILD2 \u2014 Second Child\n"
        "Status:: pending\n"
    )
    store.load_string(Path("/tmp/roadmap.md"), content)

    parent = store.get("PARENT")
    assert parent is not None
    assert len(parent.children) == 2
    assert parent.children[0].name == "CHILD1"
    assert parent.children[1].name == "CHILD2"


def test_update_file_content_preserves_non_kv_body():
    """Matches Rust test: update_file_content_preserves_non_kv_body."""
    from cmx.docket.merge import MergedEntity, FieldSource, _update_file_content

    content = (
        "---\n"
        "docket-type: task\n"
        "docket-layout: outline\n"
        "---\n"
        "## AUTH1 \u2014 Login\n"
        "Status:: pending\n"
        "Assignee:: worker1\n"
        "\n"
        "This is the body text that should be preserved.\n"
        "\n"
        "## AUTH2 \u2014 Token\n"
        "Status:: complete\n"
    )
    status_map = StatusMap.default_map()

    entity = MergedEntity(
        name="AUTH1",
        title="Login",
        entity_type="task",
        status="complete",
        fields={"Assignee": "worker2"},
        field_sources={},
        primary_file=Path("/tmp/test.md"),
        primary_format=KvFormat.COLONS,
        children=[],
    )

    updated = _update_file_content(content, [entity], status_map)
    assert "This is the body text that should be preserved." in updated
    assert "Assignee:: worker2" in updated
    assert "\u2705 AUTH1" in updated
