"""StatusMap — bidirectional canonical / representation mapping.

Matches Rust skill-docket frontmatter.rs StatusMap exactly.
"""

from __future__ import annotations


class StatusMap:
    """Bidirectional canonical <-> representation mapping."""

    def __init__(
        self,
        forward: dict[str, list[str]],
        reverse: dict[str, str],
    ) -> None:
        self.forward = forward
        self.reverse = reverse

    def canonicalize(self, repr_str: str) -> str | None:
        """Look up canonical status from any representation."""
        return self.reverse.get(repr_str)

    def write_form(self, canonical: str) -> str | None:
        """Get the preferred write representation for a canonical status."""
        reprs = self.forward.get(canonical)
        if reprs and len(reprs) > 0:
            return reprs[0]
        return None

    @staticmethod
    def from_raw(raw: dict[str, list[str]]) -> StatusMap:
        """Build a StatusMap from a canonical -> representations dict."""
        reverse: dict[str, str] = {}
        for canonical, representations in raw.items():
            for r in representations:
                reverse[r] = canonical
            # Also map the canonical name to itself
            reverse[canonical] = canonical
        return StatusMap(forward=dict(raw), reverse=reverse)

    @staticmethod
    def default_map() -> StatusMap:
        """Build the default status map matching Rust defaults."""
        forward: dict[str, list[str]] = {
            "complete": ["\u2705", "[x]", "DONE"],
            "in_progress": ["\U0001f504", "[~]", "WIP"],
            "pending": ["\u23f3", "[ ]", "TODO"],
            "blocked": ["\U0001f6ab", "[!]", "BLOCKED"],
            "failed": ["\u274c", "[F]", "FAILED"],
            "cancelled": ["\u2298", "[-]", "CANCELLED"],
        }
        return StatusMap.from_raw(forward)
