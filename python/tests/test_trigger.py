"""Tests for cmx.docket.trigger -- trigger block parsing."""

import pytest

from cmx.docket.trigger import (
    CompareOp,
    Condition,
    TriggerAction,
    TriggerBlock,
    TriggerClause,
    parse_triggers,
    _parse_condition,
)


def test_parse_simple_contains_then_block():
    """Matches Rust test: parse_simple_contains_then_block."""
    text = '''
if contains({agent}, "error")
    then cmx tell pm "{agent} has error"
'''
    blocks = parse_triggers(text)
    assert len(blocks) == 1
    assert len(blocks[0].clauses) == 1
    assert not blocks[0].clauses[0].is_else
    c = blocks[0].clauses[0].condition
    assert c.kind == "contains"
    assert c.agent == "{agent}"
    assert c.pattern == "error"
    assert blocks[0].clauses[0].action.command_template == 'cmx tell pm "{agent} has error"'


def test_parse_if_elif_else_chain():
    """Matches Rust test: parse_if_elif_else_chain."""
    text = '''
if contains({agent}, "error")
    then cmx tell pm "error"
elif idle({agent}, 30)
    then cmx tell pm "idle"
else
    then cmx tell pm "ok"
'''
    blocks = parse_triggers(text)
    assert len(blocks) == 1
    assert len(blocks[0].clauses) == 3
    assert not blocks[0].clauses[0].is_else
    assert not blocks[0].clauses[1].is_else
    assert blocks[0].clauses[2].is_else
    assert blocks[0].clauses[2].condition.kind == "always"


def test_parse_and_conjunction():
    """Matches Rust test: parse_and_conjunction."""
    text = '''
if idle({agent}, 30) and status({agent}) == stalled
    then cmx tell pm "stalled and idle"
'''
    blocks = parse_triggers(text)
    assert len(blocks) == 1
    c = blocks[0].clauses[0].condition
    assert c.kind == "and"
    assert c.left.kind == "idle"
    assert c.right.kind == "status"


def test_parse_idle_with_seconds():
    """Matches Rust test: parse_idle_with_seconds."""
    cond = _parse_condition("idle({agent}, 30)")
    assert cond.kind == "idle"
    assert cond.agent == "{agent}"
    assert cond.seconds == 30


def test_parse_context_gt_percent():
    """Matches Rust test: parse_context_gt_percent."""
    cond = _parse_condition("context({agent}) > 80%")
    assert cond.kind == "context"
    assert cond.agent == "{agent}"
    assert cond.op == CompareOp.GT
    assert cond.percent == 80


def test_parse_heartbeat_seconds():
    """Matches Rust test: parse_heartbeat_seconds."""
    cond = _parse_condition("heartbeat 300")
    assert cond.kind == "heartbeat"
    assert cond.seconds == 300


def test_parse_status_eq():
    """Matches Rust test: parse_status_eq."""
    cond = _parse_condition("status({agent}) == stalled")
    assert cond.kind == "status"
    assert cond.agent == "{agent}"
    assert cond.state == "stalled"


def test_reject_malformed_condition():
    """Matches Rust test: reject_malformed_condition."""
    with pytest.raises(ValueError):
        _parse_condition("bogus_func(x)")


def test_parse_block_with_variables_preserved():
    """Matches Rust test: parse_block_with_variables_preserved."""
    text = '''
if contains({agent}, "stalled on {task}")
    then cmx tell pm "{agent} stalled on {task}"
'''
    blocks = parse_triggers(text)
    assert len(blocks) == 1
    c = blocks[0].clauses[0].condition
    assert c.kind == "contains"
    assert c.agent == "{agent}"
    assert c.pattern == "stalled on {task}"
    assert "{agent}" in blocks[0].clauses[0].action.command_template
    assert "{task}" in blocks[0].clauses[0].action.command_template


def test_parse_multiple_blocks():
    """Matches Rust test: parse_multiple_blocks."""
    text = '''
if contains({agent}, "error")
    then cmx tell pm "error"

if idle({agent}, 60)
    then cmx tell pm "idle"
'''
    blocks = parse_triggers(text)
    assert len(blocks) == 2


def test_parse_named_block():
    """Matches Rust test: parse_named_block."""
    text = '''
# Error Watcher
if contains({agent}, "error")
    then cmx tell pm "error"
'''
    blocks = parse_triggers(text)
    assert len(blocks) == 1
    assert blocks[0].name == "Error Watcher"


def test_parse_context_gte():
    """Matches Rust test: parse_context_gte."""
    cond = _parse_condition("context({agent}) >= 90%")
    assert cond.kind == "context"
    assert cond.op == CompareOp.GTE
    assert cond.percent == 90


def test_parse_context_lte():
    cond = _parse_condition("context({agent}) <= 50%")
    assert cond.kind == "context"
    assert cond.op == CompareOp.LTE
    assert cond.percent == 50
