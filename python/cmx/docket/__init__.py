"""cmx.docket -- Docket Standard parsing and entity model.

Pure-Python implementation matching the Rust skill-docket crate.
"""

from .entity import Entity, EntityTree, parse_outline, parse_file_entity, create_entity_markdown
from .frontmatter import DocketFrontmatter, DocketFile, DocketLayout, parse_frontmatter, scan_directory
from .kv import KvFormat, parse_kv, serialize_kv, detect_format
from .status import StatusMap
from .merge import MergedEntity, MergeStore, FieldSource
from .trigger import (
    TriggerBlock,
    TriggerClause,
    Condition,
    TriggerAction,
    CompareOp,
    parse_triggers,
)
from .types import TaskStatus, TaskSource, TaskNode

__all__ = [
    "Entity",
    "EntityTree",
    "parse_outline",
    "parse_file_entity",
    "create_entity_markdown",
    "DocketFrontmatter",
    "DocketFile",
    "DocketLayout",
    "parse_frontmatter",
    "scan_directory",
    "KvFormat",
    "parse_kv",
    "serialize_kv",
    "detect_format",
    "StatusMap",
    "MergedEntity",
    "MergeStore",
    "FieldSource",
    "TriggerBlock",
    "TriggerClause",
    "Condition",
    "TriggerAction",
    "CompareOp",
    "parse_triggers",
    "TaskStatus",
    "TaskSource",
    "TaskNode",
]
