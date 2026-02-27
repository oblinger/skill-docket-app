"""Trigger parser -- parses trigger blocks from markdown text into structured values.

Recognizes if/elif/else chains with conditions and actions::

    if contains({agent}, "error")
        then cmx tell pm "{agent} has error"
    elif idle({agent}, 30)
        then cmx tell pm "{agent} idle"
    else
        then cmx tell pm "{agent} ok"

Matches Rust skill-docket trigger/parser.rs exactly.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum


class CompareOp(Enum):
    GT = ">"
    LT = "<"
    GTE = ">="
    LTE = "<="
    EQ = "=="


@dataclass
class Condition:
    """Discriminated union via kind field.

    kind is one of: "contains", "status", "idle", "context", "heartbeat", "and", "always".
    """

    kind: str
    agent: str | None = None
    pattern: str | None = None
    state: str | None = None
    seconds: int | None = None
    op: CompareOp | None = None
    percent: int | None = None
    left: Condition | None = None
    right: Condition | None = None


@dataclass
class TriggerAction:
    command_template: str = ""


@dataclass
class TriggerClause:
    condition: Condition
    action: TriggerAction = field(default_factory=TriggerAction)
    is_else: bool = False


@dataclass
class TriggerBlock:
    name: str | None = None
    clauses: list[TriggerClause] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def parse_triggers(text: str) -> list[TriggerBlock]:
    """Parse trigger blocks from a markdown section.

    Each block starts with an ``if`` line and may include ``elif`` and ``else`` lines.
    Action lines start with ``then``. Blocks are separated by blank lines or
    new ``if`` lines.
    """
    blocks: list[TriggerBlock] = []
    current_clauses: list[TriggerClause] = []
    current_name: str | None = None

    for raw_line in text.splitlines():
        line = raw_line.strip()

        if not line:
            # Blank line: flush current block if any
            if current_clauses:
                blocks.append(TriggerBlock(name=current_name, clauses=list(current_clauses)))
                current_clauses.clear()
                current_name = None
            continue

        # Lines starting with '#' are block names
        if line.startswith("#"):
            # Flush previous block
            if current_clauses:
                blocks.append(TriggerBlock(name=current_name, clauses=list(current_clauses)))
                current_clauses.clear()
            current_name = line.lstrip("#").strip()
            continue

        if line.startswith("if "):
            # New block if we already have an if clause
            has_if = any(not c.is_else for c in current_clauses)
            if current_clauses and has_if:
                blocks.append(TriggerBlock(name=current_name, clauses=list(current_clauses)))
                current_clauses.clear()
                current_name = None

            cond_str = line[3:]
            condition = _parse_condition(cond_str)
            current_clauses.append(TriggerClause(
                condition=condition,
                action=TriggerAction(),
                is_else=False,
            ))
        elif line.startswith("elif "):
            cond_str = line[5:]
            condition = _parse_condition(cond_str)
            current_clauses.append(TriggerClause(
                condition=condition,
                action=TriggerAction(),
                is_else=False,
            ))
        elif line == "else":
            current_clauses.append(TriggerClause(
                condition=Condition(kind="always"),
                action=TriggerAction(),
                is_else=True,
            ))
        elif line.startswith("then "):
            cmd = line[5:].strip()
            if current_clauses:
                current_clauses[-1].action.command_template = cmd
            else:
                raise ValueError("'then' without preceding condition")

    # Flush remaining block
    if current_clauses:
        blocks.append(TriggerBlock(name=current_name, clauses=list(current_clauses)))

    return blocks


# ---------------------------------------------------------------------------
# Condition parser
# ---------------------------------------------------------------------------

def _parse_condition(expr: str) -> Condition:
    """Parse a single condition expression."""
    trimmed = expr.strip()

    # Handle "and" conjunction
    and_pos = _find_and_split(trimmed)
    if and_pos is not None:
        left_str = trimmed[:and_pos]
        right_str = trimmed[and_pos + 5:]  # " and " is 5 chars
        left = _parse_condition(left_str)
        right = _parse_condition(right_str)
        return Condition(kind="and", left=left, right=right)

    # contains({agent}, "pattern")
    if trimmed.startswith("contains("):
        return _parse_contains(trimmed)

    # status({agent}) == state
    if trimmed.startswith("status("):
        return _parse_status(trimmed)

    # idle({agent}, N)
    if trimmed.startswith("idle("):
        return _parse_idle(trimmed)

    # context({agent}) > N%
    if trimmed.startswith("context("):
        return _parse_context(trimmed)

    # heartbeat N
    if trimmed.startswith("heartbeat "):
        return _parse_heartbeat(trimmed)

    raise ValueError(f"Unknown condition: '{trimmed}'")


def _find_and_split(s: str) -> int | None:
    """Find the position of ' and ' for splitting conjunctions.

    Respects parentheses and quotes to avoid splitting inside them.
    """
    paren_depth = 0
    in_quote = False

    for i, ch in enumerate(s):
        if ch == '"':
            in_quote = not in_quote
        elif not in_quote:
            if ch == '(':
                paren_depth += 1
            elif ch == ')':
                paren_depth = max(0, paren_depth - 1)
            elif paren_depth == 0 and s[i:i + 5] == " and ":
                return i
    return None


def _extract_parens(s: str, func_name: str) -> str:
    """Extract the content between parentheses after a function name."""
    prefix = f"{func_name}("
    if not s.startswith(prefix):
        raise ValueError(f"Expected '{prefix}' prefix")
    start = len(prefix)
    end = s.rfind(")")
    if end < 0:
        raise ValueError(f"Missing ')' in {func_name} call")
    return s[start:end]


def _split_first_arg(s: str) -> tuple[str, str]:
    """Split on the first comma, returning (first_arg, rest)."""
    comma = s.find(",")
    if comma < 0:
        raise ValueError("Expected comma separating arguments")
    return s[:comma].strip(), s[comma + 1:].strip()


def _extract_quoted(s: str) -> str:
    """Extract a quoted string, stripping the surrounding quotes."""
    trimmed = s.strip()
    if trimmed.startswith('"') and trimmed.endswith('"') and len(trimmed) >= 2:
        return trimmed[1:-1]
    raise ValueError(f"Expected quoted string, got '{s}'")


def _parse_contains(s: str) -> Condition:
    """Parse ``contains({agent}, "pattern")``."""
    inner = _extract_parens(s, "contains")
    agent, rest = _split_first_arg(inner)
    pattern = _extract_quoted(rest.strip())
    return Condition(kind="contains", agent=agent, pattern=pattern)


def _parse_status(s: str) -> Condition:
    """Parse ``status({agent}) == state``."""
    close = s.find(")")
    if close < 0:
        raise ValueError("Missing ')' in status condition")
    agent = s[7:close].strip()  # "status(" is 7 chars
    after = s[close + 1:].strip()
    if not after.startswith("=="):
        raise ValueError(f"Expected '==' after status({agent}), got '{after}'")
    state = after[2:].strip()
    return Condition(kind="status", agent=agent, state=state)


def _parse_idle(s: str) -> Condition:
    """Parse ``idle({agent}, N)``."""
    inner = _extract_parens(s, "idle")
    agent, rest = _split_first_arg(inner)
    try:
        seconds = int(rest.strip())
    except ValueError:
        raise ValueError(f"Invalid seconds in idle: '{rest.strip()}'")
    return Condition(kind="idle", agent=agent, seconds=seconds)


def _parse_context(s: str) -> Condition:
    """Parse ``context({agent}) > N%``."""
    close = s.find(")")
    if close < 0:
        raise ValueError("Missing ')' in context condition")
    agent = s[8:close].strip()  # "context(" is 8 chars
    after = s[close + 1:].strip()

    if after.startswith(">="):
        op = CompareOp.GTE
        rest = after[2:]
    elif after.startswith("<="):
        op = CompareOp.LTE
        rest = after[2:]
    elif after.startswith(">"):
        op = CompareOp.GT
        rest = after[1:]
    elif after.startswith("<"):
        op = CompareOp.LT
        rest = after[1:]
    elif after.startswith("=="):
        op = CompareOp.EQ
        rest = after[2:]
    else:
        raise ValueError(f"Expected comparison operator after context({agent}), got '{after}'")

    pct_str = rest.strip().rstrip("%")
    try:
        percent = int(pct_str.strip())
    except ValueError:
        raise ValueError(f"Invalid percent in context: '{pct_str}'")

    return Condition(kind="context", agent=agent, op=op, percent=percent)


def _parse_heartbeat(s: str) -> Condition:
    """Parse ``heartbeat N``."""
    num_str = s[len("heartbeat "):].strip()
    try:
        seconds = int(num_str)
    except ValueError:
        raise ValueError(f"Invalid seconds in heartbeat: '{num_str}'")
    return Condition(kind="heartbeat", seconds=seconds)
