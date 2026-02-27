"""Tests for cmx.docket.types -- TaskNode, TaskStatus, TaskSource."""

import json

from cmx.docket.types import TaskNode, TaskSource, TaskStatus


def test_task_node_round_trip():
    """Matches Rust test: task_node_round_trip."""
    task = TaskNode(
        id="CMX1",
        title="Core daemon",
        source=TaskSource.ROADMAP,
        status=TaskStatus.IN_PROGRESS,
        result=None,
        agent="worker-1",
        children=[
            TaskNode(
                id="CMX1A",
                title="Socket protocol",
                source=TaskSource.FILESYSTEM,
                status=TaskStatus.PENDING,
                result=None,
                agent=None,
                children=[],
                spec_path="/tasks/CMX1A/CMX1A.md",
            )
        ],
        spec_path="/tasks/CMX1/CMX1.md",
    )
    json_str = task.to_json()
    back = TaskNode.from_json(json_str)
    assert back.id == "CMX1"
    assert len(back.children) == 1
    assert back.children[0].id == "CMX1A"


def test_task_status_serialization():
    """Matches Rust test: task_status_serde."""
    assert TaskStatus.IN_PROGRESS.value == "in_progress"


def test_task_source_values():
    assert TaskSource.ROADMAP.value == "roadmap"
    assert TaskSource.FILESYSTEM.value == "filesystem"
    assert TaskSource.BOTH.value == "both"


def test_task_node_json_keys():
    """Ensure JSON keys match Rust serde output."""
    task = TaskNode(
        id="T1",
        title="Test",
        source=TaskSource.ROADMAP,
        status=TaskStatus.PENDING,
    )
    d = task.to_dict()
    assert set(d.keys()) == {"id", "title", "source", "status", "result", "agent", "children", "spec_path"}
    assert d["result"] is None
    assert d["agent"] is None
    assert d["children"] == []


def test_task_status_all_values():
    """Verify all status enum values exist."""
    assert TaskStatus.PENDING.value == "pending"
    assert TaskStatus.IN_PROGRESS.value == "in_progress"
    assert TaskStatus.COMPLETED.value == "completed"
    assert TaskStatus.FAILED.value == "failed"
    assert TaskStatus.PAUSED.value == "paused"
    assert TaskStatus.CANCELLED.value == "cancelled"
