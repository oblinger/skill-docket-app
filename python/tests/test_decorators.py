"""Tests for the @when decorator and rules() function."""

import pytest
from cmx.decorators import (
    when, rules,
    get_registered_decorators, get_registered_rules,
    _decorator_registry, _inline_rules,
)


@pytest.fixture(autouse=True)
def clear_registries():
    """Clear global registries before each test."""
    _decorator_registry.clear()
    _inline_rules.clear()
    yield
    _decorator_registry.clear()
    _inline_rules.clear()


class TestWhenDecorator:
    def test_when_decorator_registers(self):
        """Decorate a function, verify it appears in the registry."""
        @when("task.t1.status == complete")
        def on_complete():
            pass

        registered = get_registered_decorators()
        assert len(registered) == 1
        assert registered[0]['name'] == 'on_complete'
        assert registered[0]['pattern'] == 'task.t1.status == complete'
        assert registered[0]['handler'] is on_complete

    def test_when_extracts_variables(self):
        """Pattern with $t and $a, verify variables extracted."""
        @when("task.$t.agent.$a.status == idle")
        def on_idle(t, a):
            pass

        registered = get_registered_decorators()
        assert len(registered) == 1
        assert registered[0]['variables'] == ['t', 'a']

    def test_when_preserves_function(self):
        """Decorated function is still callable."""
        @when("task.$t.status == done")
        def handler(t):
            return f"handled {t}"

        assert handler(t="T1") == "handled T1"

    def test_multiple_decorators(self):
        """Register 3 decorators, verify all in registry in order."""
        @when("a.$x == 1")
        def first(x):
            pass

        @when("b.$y == 2")
        def second(y):
            pass

        @when("c.$z == 3")
        def third(z):
            pass

        registered = get_registered_decorators()
        assert len(registered) == 3
        assert [r['name'] for r in registered] == ['first', 'second', 'third']

    def test_no_variables(self):
        """Pattern without $ variables extracts empty list."""
        @when("system.ready == true")
        def on_ready():
            pass

        registered = get_registered_decorators()
        assert registered[0]['variables'] == []


class TestRules:
    def test_rules_registers_text(self):
        """Call rules("..."), verify text registered."""
        rules("task.$t.status == complete --> agent.$t.assignee.status = idle")
        registered = get_registered_rules()
        assert len(registered) == 1
        assert "complete" in registered[0]
        assert "idle" in registered[0]

    def test_rules_strips_whitespace(self):
        """Rules text is stripped of leading/trailing whitespace."""
        rules("""
        task.$t.done == true --> cleanup.$t = pending
        """)
        registered = get_registered_rules()
        assert not registered[0].startswith("\n")
        assert not registered[0].endswith("\n")

    def test_multiple_rules_calls(self):
        """Multiple calls append independently."""
        rules("a == 1 --> b = 2")
        rules("c == 3 --> d = 4")
        registered = get_registered_rules()
        assert len(registered) == 2
