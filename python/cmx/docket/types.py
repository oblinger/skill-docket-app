"""Task types — enums and dataclasses matching Rust skill-docket types.rs."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from enum import Enum


class TaskStatus(Enum):
    PENDING = "pending"
    IN_PROGRESS = "in_progress"
    COMPLETED = "completed"
    FAILED = "failed"
    PAUSED = "paused"
    CANCELLED = "cancelled"


class TaskSource(Enum):
    ROADMAP = "roadmap"
    FILESYSTEM = "filesystem"
    BOTH = "both"


@dataclass
class TaskNode:
    id: str
    title: str
    source: TaskSource
    status: TaskStatus
    result: str | None = None
    agent: str | None = None
    children: list[TaskNode] = field(default_factory=list)
    spec_path: str | None = None

    def to_dict(self) -> dict:
        """Serialize to dict matching Rust serde JSON output."""
        d: dict = {
            "id": self.id,
            "title": self.title,
            "source": self.source.value,
            "status": self.status.value,
            "result": self.result,
            "agent": self.agent,
            "children": [c.to_dict() for c in self.children],
            "spec_path": self.spec_path,
        }
        return d

    def to_json(self) -> str:
        """Serialize to JSON string matching Rust serde output."""
        return json.dumps(self.to_dict(), separators=(",", ":"))

    @classmethod
    def from_dict(cls, d: dict) -> TaskNode:
        """Deserialize from dict matching Rust serde JSON output."""
        return cls(
            id=d["id"],
            title=d["title"],
            source=TaskSource(d["source"]),
            status=TaskStatus(d["status"]),
            result=d.get("result"),
            agent=d.get("agent"),
            children=[cls.from_dict(c) for c in d.get("children", [])],
            spec_path=d.get("spec_path"),
        )

    @classmethod
    def from_json(cls, s: str) -> TaskNode:
        """Deserialize from JSON string."""
        return cls.from_dict(json.loads(s))
