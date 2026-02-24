"""@when decorator and rules() function for Python rule integration."""

import re
from typing import Callable, List

# Global registry (populated at import time by decorated functions)
_decorator_registry: List[dict] = []
_inline_rules: List[str] = []

# Matches $variable_name in patterns (word chars after a $)
_VAR_RE = re.compile(r'\$(\w+)')


def _extract_variables(pattern: str) -> List[str]:
    """Extract variable names from a pattern (segments starting with $)."""
    return _VAR_RE.findall(pattern)


def when(pattern: str):
    """Decorator that registers a function to fire when a pattern matches.

    Usage:
        @cmx.when("task.$t.status == complete")
        def on_complete(t):
            cmx.set(f"task.{t}.reviewed", True)

    Variables from the pattern (segments starting with $) become
    keyword arguments to the decorated function.
    """
    def decorator(func: Callable) -> Callable:
        variables = _extract_variables(pattern)
        _decorator_registry.append({
            'pattern': pattern,
            'handler': func,
            'variables': variables,
            'name': func.__name__,
        })
        return func
    return decorator


def rules(text: str) -> None:
    """Register inline declarative rules.

    Usage:
        cmx.rules(\"\"\"
        task.$t.status == complete --> agent.$t.assignee.status = idle
        \"\"\")
    """
    _inline_rules.append(text.strip())


def get_registered_decorators() -> list:
    """Return all registered @when decorators."""
    return list(_decorator_registry)


def get_registered_rules() -> list:
    """Return all registered inline rules text."""
    return list(_inline_rules)
