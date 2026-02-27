"""Tests for cmx.docket.entity -- heading parsing, outline nesting, file layout."""

from cmx.docket.entity import (
    Entity,
    EntityTree,
    create_entity_markdown,
    parse_file_entity,
    parse_outline,
)
from cmx.docket.frontmatter import DocketFrontmatter, DocketLayout
from cmx.docket.kv import KvFormat
from cmx.docket.status import StatusMap


def _test_frontmatter() -> DocketFrontmatter:
    return DocketFrontmatter(
        docket_type="task",
        docket_layout=DocketLayout.OUTLINE,
        docket_format="kv-colons",
    )


def test_parse_outline_basic():
    """Matches Rust test: parse_outline_basic."""
    body = (
        "## AUTH \u2014 Authentication\n"
        "Status:: in_progress\n"
        "### AUTH1 \u2014 Login API\n"
        "Status:: complete\n"
        "### AUTH2 \u2014 Token refresh\n"
        "Status:: pending\n"
        "## DATA \u2014 Data Layer\n"
        "Status:: complete\n"
    )
    fm = _test_frontmatter()
    status_map = StatusMap.default_map()
    trees = parse_outline(body, fm, status_map)

    assert len(trees) == 2
    assert trees[0].entity.name == "AUTH"
    assert trees[0].entity.status == "in_progress"
    assert len(trees[0].children) == 2
    assert trees[0].children[0].entity.name == "AUTH1"
    assert trees[0].children[0].entity.status == "complete"
    assert trees[0].children[1].entity.name == "AUTH2"
    assert trees[1].entity.name == "DATA"
    assert trees[1].entity.status == "complete"


def test_parse_outline_with_status_markers():
    """Matches Rust test: parse_outline_with_status_markers."""
    body = (
        "## \u2705 AUTH1 \u2014 Implement Login\n"
        "Priority:: high\n"
        "## \U0001f504 AUTH2 \u2014 Token Refresh\n"
        "Priority:: medium\n"
        "## \u23f3 AUTH3 \u2014 OAuth Integration\n"
        "Priority:: low\n"
    )
    fm = _test_frontmatter()
    status_map = StatusMap.default_map()
    trees = parse_outline(body, fm, status_map)

    assert len(trees) == 3
    assert trees[0].entity.status == "complete"
    assert trees[0].entity.name == "AUTH1"
    assert trees[1].entity.status == "in_progress"
    assert trees[2].entity.status == "pending"


def test_parse_outline_with_type_override():
    """Matches Rust test: parse_outline_with_type_override."""
    body = (
        "## DES1 \u2014 Design Authentication Flow\n"
        "Type:: design-task\n"
        "Status:: complete\n"
        "## AUTH1 \u2014 Implement Login\n"
        "Status:: pending\n"
    )
    fm = _test_frontmatter()
    status_map = StatusMap.default_map()
    trees = parse_outline(body, fm, status_map)

    assert trees[0].entity.entity_type == "design-task"
    assert trees[1].entity.entity_type == "task"


def test_parse_outline_deep_nesting():
    """Matches Rust test: parse_outline_deep_nesting."""
    body = (
        "## M1 \u2014 Milestone One\n"
        "### M1.1 \u2014 Sub One\n"
        "#### M1.1.1 \u2014 Leaf\n"
        "### M1.2 \u2014 Sub Two\n"
        "## M2 \u2014 Milestone Two\n"
    )
    fm = _test_frontmatter()
    status_map = StatusMap.default_map()
    trees = parse_outline(body, fm, status_map)

    assert len(trees) == 2
    assert len(trees[0].children) == 2
    assert len(trees[0].children[0].children) == 1
    assert trees[0].children[0].children[0].entity.name == "M1.1.1"


def test_parse_file_entity_basic():
    """Matches Rust test: parse_file_entity_basic."""
    body = "# AUTH1 \u2014 Implement Login\nStatus:: in_progress\nAssignee:: worker1\n"
    fm = DocketFrontmatter(
        docket_type="task",
        docket_layout=DocketLayout.FILE,
        docket_format="kv-colons",
    )
    status_map = StatusMap.default_map()
    entity = parse_file_entity(body, fm, status_map)
    assert entity is not None
    assert entity.name == "AUTH1"
    assert entity.title == "Implement Login"
    assert entity.status == "in_progress"
    assert entity.fields.get("Assignee") == "worker1"


def test_parse_outline_with_packed_format():
    """Matches Rust test: parse_outline_with_packed_format."""
    body = (
        "## AUTH1 \u2014 Implement Login\n"
        ":: status:in_progress, assignee:worker1, priority:high\n"
        "The login endpoint needs to support both email/password and OAuth.\n"
        "## AUTH2 \u2014 Token Refresh\n"
        ":: status:pending, assignee:nobody\n"
    )
    fm = DocketFrontmatter(
        docket_type="task",
        docket_layout=DocketLayout.OUTLINE,
        docket_format="kv-packed",
    )
    status_map = StatusMap.default_map()
    trees = parse_outline(body, fm, status_map)

    assert len(trees) == 2
    assert trees[0].entity.status == "in_progress"
    assert trees[0].entity.fields.get("assignee") == "worker1"


def test_create_entity_markdown_colons():
    """Matches Rust test: create_entity_markdown_colons."""
    fields = {
        "status": "pending",
        "Assignee": "worker1",
        "Priority": "high",
    }
    status_map = StatusMap.default_map()
    md = create_entity_markdown("AUTH1", "Implement Login", fields, 2, KvFormat.COLONS, status_map)
    assert "## \u23f3 AUTH1 \u2014 Implement Login" in md
    assert "Assignee:: worker1" in md
    assert "Priority:: high" in md
    # Status should be in heading, not in KV
    assert "status:: pending" not in md


def test_create_entity_markdown_table():
    """Matches Rust test: create_entity_markdown_table."""
    fields = {
        "status": "complete",
        "Assignee": "worker1",
    }
    status_map = StatusMap.default_map()
    md = create_entity_markdown("AUTH1", "Login", fields, 2, KvFormat.TABLE, status_map)
    assert "## \u2705 AUTH1 \u2014 Login" in md
    assert "| Assignee | worker1 |" in md


def test_parse_outline_ignores_h1_title():
    """Matches Rust test: parse_outline_ignores_h1_title."""
    body = (
        "# Project Roadmap\n"
        "## AUTH \u2014 Authentication\n"
        "Status:: pending\n"
        "## DATA \u2014 Data Layer\n"
        "Status:: complete\n"
    )
    fm = _test_frontmatter()
    status_map = StatusMap.default_map()
    trees = parse_outline(body, fm, status_map)

    # H1 gets parsed but H2 entities are its children
    h2_entities = []
    for t in trees:
        if t.entity.heading_level == 1:
            h2_entities.extend(t.children)
        else:
            h2_entities.append(t)
    assert len(h2_entities) >= 2


def test_parse_outline_empty():
    """Matches Rust test: parse_outline_empty."""
    fm = _test_frontmatter()
    status_map = StatusMap.default_map()
    trees = parse_outline("", fm, status_map)
    assert len(trees) == 0


def test_entity_line_numbers():
    """Matches Rust test: entity_line_numbers."""
    body = (
        "## AUTH \u2014 Authentication\n"
        "Status:: pending\n"
        "### AUTH1 \u2014 Login\n"
        "Status:: complete\n"
    )
    fm = _test_frontmatter()
    status_map = StatusMap.default_map()
    trees = parse_outline(body, fm, status_map)
    assert trees[0].entity.line_number == 1
    assert trees[0].children[0].entity.line_number == 3


def test_split_name_title_emdash():
    """Matches Rust test: split_name_title_emdash."""
    from cmx.docket.entity import _split_name_title
    name, title = _split_name_title("AUTH1 \u2014 Implement Login")
    assert name == "AUTH1"
    assert title == "Implement Login"


def test_split_name_title_double_hyphen():
    """Matches Rust test: split_name_title_double_hyphen."""
    from cmx.docket.entity import _split_name_title
    name, title = _split_name_title("AUTH1 -- Implement Login")
    assert name == "AUTH1"
    assert title == "Implement Login"


def test_split_name_title_no_separator():
    """Matches Rust test: split_name_title_no_separator."""
    from cmx.docket.entity import _split_name_title
    name, title = _split_name_title("JustAName")
    assert name == "JustAName"
    assert title == "JustAName"
