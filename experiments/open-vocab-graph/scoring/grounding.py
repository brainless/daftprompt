"""Deterministic offset-to-line grounding against the frozen source snapshot.

Task 4's first invariant: a model may supply character offsets and mention
text, but it can never supply citation text. Every citation emitted by the
scorer is derived from the stored snapshot (`frozen-inputs/AGENTS.md`):

- a span `(start, end)` maps to the 1-based inclusive physical-line range
  covering it, and the citation text is always the snapshot's own lines;
- a candidate mention counts as grounded only when its text equals the
  snapshot slice exactly; whitespace-normalized equality is recorded
  separately as an ambiguity signal and still rejects the claim;
- the snapshot sha256 is verified against both the schema artifact's frozen
  record and the run manifest before any scoring happens.

Line-range conventions match the frozen reference set: 1-based inclusive,
citation text is the complete physical lines joined with "\\n" (no trailing
newline). Pure stdlib; no gliner2/torch imports.
"""
from __future__ import annotations

import bisect
import hashlib
from dataclasses import dataclass
from pathlib import Path


class SnapshotMismatchError(RuntimeError):
    """The snapshot does not match the expected frozen sha256/line count."""


def norm_words(text: str) -> str:
    """Whitespace-normalized text (the frozen checker's matching rule)."""
    return " ".join(text.split())


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


@dataclass(frozen=True)
class GroundedSpan:
    """A character span resolved against the snapshot's physical lines.

    `slice_text` is the exact snapshot slice `text[start:end]`; `line_text`
    is the complete physical lines covering the span, derived from the
    snapshot — never from the candidate. `resolved` is False when the span
    escaped the snapshot or was empty, in which case `codes` says why.
    """

    start: int
    end: int
    start_line: int
    end_line: int
    slice_text: str
    line_text: str
    resolved: bool
    codes: tuple[str, ...]

    @property
    def line_range(self) -> tuple[int, int]:
        return (self.start_line, self.end_line)

    def to_dict(self) -> dict:
        return {
            "start": self.start,
            "end": self.end,
            "start_line": self.start_line,
            "end_line": self.end_line,
            "slice_text": self.slice_text,
            "line_text": self.line_text,
            "resolved": self.resolved,
            "codes": list(self.codes),
        }


class SnapshotGrounding:
    """Character-offset to line-range index over one snapshot file."""

    def __init__(self, snapshot_path: Path):
        data = Path(snapshot_path).read_bytes()
        self.path = Path(snapshot_path)
        self.sha256 = sha256_bytes(data)
        self.text = data.decode("utf-8")
        # keepends keeps every physical line with its terminator so character
        # offsets stay exact; citation text strips only the final newline.
        self.lines: list[str] = self.text.splitlines(keepends=True)
        self.line_starts: list[int] = []
        offset = 0
        for line in self.lines:
            self.line_starts.append(offset)
            offset += len(line)
        self.line_count = len(self.lines)

    def verify(self, expected_sha256: str, expected_line_count: int | None = None) -> None:
        """Fail closed when the snapshot drifted from the frozen record."""
        if self.sha256 != expected_sha256:
            raise SnapshotMismatchError(
                f"snapshot {self.path} sha256 {self.sha256} does not match the "
                f"expected frozen sha256 {expected_sha256}; scoring is defined "
                "only against the frozen experiment snapshot"
            )
        if expected_line_count is not None and self.line_count != expected_line_count:
            raise SnapshotMismatchError(
                f"snapshot {self.path} has {self.line_count} lines; expected "
                f"{expected_line_count}"
            )

    def line_for_offset(self, offset: int) -> int | None:
        """1-based line number containing `offset` (len(text) is the last line)."""
        if offset < 0 or offset > len(self.text):
            return None
        if not self.line_starts:
            return None
        index = bisect.bisect_right(self.line_starts, offset) - 1
        return index + 1

    def lines_text(self, start_line: int, end_line: int) -> str:
        """Snapshot text of physical lines `start_line..end_line` (1-based inclusive)."""
        start = max(1, start_line)
        end = min(self.line_count, end_line)
        if start > end:
            return ""
        selected = self.lines[start - 1 : end]
        joined = "\n".join(line[:-1] if line.endswith("\n") else line for line in selected)
        return joined

    def ground(self, start: int, end: int) -> GroundedSpan:
        """Resolve a character span to lines + snapshot text; clamp + flag escapes."""
        codes: list[str] = []
        s, e = int(start), int(end)
        if e < s:
            s, e = e, s
        if s < 0 or e > len(self.text) or s >= len(self.text):
            codes.append("span_out_of_bounds")
        s = max(0, min(s, len(self.text)))
        e = max(0, min(e, len(self.text)))
        if e <= s:
            codes.append("empty_span")
        start_line = self.line_for_offset(s) or 1
        end_line = self.line_for_offset(max(e - 1, s)) or start_line
        return GroundedSpan(
            start=s,
            end=e,
            start_line=start_line,
            end_line=end_line,
            slice_text=self.text[s:e],
            line_text=self.lines_text(start_line, end_line),
            resolved=not codes,
            codes=tuple(codes),
        )


def text_match(grounded: GroundedSpan, mention_text: str) -> tuple[bool, bool]:
    """Compare a mention with its snapshot slice.

    Returns `(exact, normalized_equal)`. Only `exact` grounds a claim;
    `normalized_equal` without `exact` is recorded as an ambiguous boundary
    defect and still rejects.
    """
    exact = bool(mention_text) and grounded.slice_text == mention_text
    normalized_equal = (
        bool(mention_text) and norm_words(grounded.slice_text) == norm_words(mention_text)
    )
    return exact, normalized_equal
