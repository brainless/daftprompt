"""Line-preserving chunker for the Epic 014 GLiNER2.5 extraction harness.

Chunking policy (Epic 014 design constraint + frozen comparison plan):
chunks are packed from **whole physical lines** so a mention never straddles a
chunk boundary mid-line and Task 4's line-citation mapping stays exact. Chunk
boundaries, character offsets, word counts, overlap lines, and oversized
single-line chunks are all recorded; nothing is hidden.

Deliberate deviation from gliner2's native chunker
(`gliner2.inference.chunking.split_text_into_chunks`): that splitter cuts on
whitespace word tokens and can split a chunk mid-line. We keep whole lines and
accept the packing overhead instead. Overlap is therefore counted in whole
lines (default 2), not word tokens — recorded per chunk in `overlap_lines`.

JointIE locality: a relation survives only when both endpoint mentions fall in
the same chunk. Whole-line packing plus recorded overlap lines lets the
analysis distinguish cross-chunk misses from extraction misses.

Pure stdlib: importing this module must not import gliner2 or torch.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Optional


@dataclass(frozen=True)
class Chunk:
    """One recorded chunk with offsets into the original document.

    `start_line`/`end_line` are 1-based and inclusive; `start_char`/`end_char`
    are half-open offsets into the document text, so
    `document[start_char:end_char] == text`. `overlap_lines` lists the 1-based
    line numbers this chunk re-sends from the previous chunk (empty for the
    first chunk or when the overlap could not fit). `oversized` marks a chunk
    whose single physical line exceeded the word budget (kept whole anyway).
    """

    index: int
    text: str
    start_char: int
    end_char: int
    start_line: int
    end_line: int
    word_count: int
    overlap_lines: tuple[int, ...]
    oversized: bool

    def to_dict(self) -> dict:
        return {
            "index": self.index,
            "start_char": self.start_char,
            "end_char": self.end_char,
            "start_line": self.start_line,
            "end_line": self.end_line,
            "word_count": self.word_count,
            "overlap_lines": list(self.overlap_lines),
            "oversized": self.oversized,
            "text": self.text,
        }


def chunk_lines(text: str, chunk_words: int = 384, overlap_lines: int = 2) -> list[Chunk]:
    """Pack whole physical lines into chunks of at most `chunk_words` words.

    A line longer than the budget gets its own oversized chunk (kept whole, so
    citations stay resolvable). Each following chunk prepends up to
    `overlap_lines` complete trailing lines of the previous chunk — trimmed
    deterministically until the overlap plus the chunk's first new line fits
    the budget. Every line is covered by at least one chunk and every chunk
    after the first contains at least one line beyond the previous chunk's end
    (no overlap-only chunks, no infinite loops).
    """
    if chunk_words <= 0:
        raise ValueError("chunk_words must be greater than 0")
    if overlap_lines < 0:
        raise ValueError("overlap_lines must be non-negative")

    lines = text.splitlines(keepends=True)
    if not lines:
        return [
            Chunk(
                index=0,
                text="",
                start_char=0,
                end_char=0,
                start_line=0,
                end_line=0,
                word_count=0,
                overlap_lines=(),
                oversized=False,
            )
        ]

    line_starts: list[int] = []
    offset = 0
    for line in lines:
        line_starts.append(offset)
        offset += len(line)
    words = [len(line.split()) for line in lines]

    def make(index: int, members: list[int]) -> Chunk:
        start_char = line_starts[members[0]]
        last = members[-1]
        end_char = line_starts[last] + len(lines[last])
        word_count = sum(words[i] for i in members)
        return Chunk(
            index=index,
            text=text[start_char:end_char],
            start_char=start_char,
            end_char=end_char,
            start_line=members[0] + 1,
            end_line=last + 1,
            word_count=word_count,
            overlap_lines=(),
            oversized=word_count > chunk_words,
        )

    def overlap_suffix(prev: list[int], next_word_count: int) -> list[int]:
        suffix = prev[-overlap_lines:] if overlap_lines > 0 else []
        while suffix and sum(words[i] for i in suffix) + next_word_count > chunk_words:
            suffix = suffix[1:]
        return suffix

    chunks: list[Chunk] = []
    current: list[int] = []
    current_words = 0
    for i, word_count in enumerate(words):
        if current and current_words + word_count > chunk_words:
            chunks.append(make(len(chunks), current))
            suffix = overlap_suffix(current, word_count)
            current = list(suffix)
            current_words = sum(words[j] for j in current)
        current.append(i)
        current_words += word_count
    if current:
        chunks.append(make(len(chunks), current))

    # Rebuild overlap bookkeeping in a second pass: the suffix a chunk re-sends
    # is recorded on the chunk that contains it.
    if overlap_lines > 0 and len(chunks) > 1:
        rebuilt: list[Chunk] = []
        prev_members: list[int] = []
        for chunk in chunks:
            members = list(range(chunk.start_line - 1, chunk.end_line))
            overlap = [i for i in members if i in set(prev_members)]
            rebuilt.append(
                Chunk(
                    index=chunk.index,
                    text=chunk.text,
                    start_char=chunk.start_char,
                    end_char=chunk.end_char,
                    start_line=chunk.start_line,
                    end_line=chunk.end_line,
                    word_count=chunk.word_count,
                    overlap_lines=tuple(i + 1 for i in overlap),
                    oversized=chunk.oversized,
                )
            )
            prev_members = members
        return rebuilt
    return chunks


def remap_span(
    chunk: Chunk,
    local_start: int,
    local_end: int,
    document_text: str,
    mention_text: Optional[str] = None,
) -> tuple[int, int, bool, bool]:
    """Map chunk-local character offsets to document offsets.

    Returns `(doc_start, doc_end, clamped, verified)`. Offsets are clamped into
    the chunk's own character range first (a span escaping the chunk input is a
    model defect and is flagged, not silently trusted). `verified` is True when
    `mention_text` was given and the remapped slice equals it.
    """
    doc_start = chunk.start_char + int(local_start)
    doc_end = chunk.start_char + int(local_end)
    clamped = False
    if doc_start < chunk.start_char:
        doc_start = chunk.start_char
        clamped = True
    if doc_end > chunk.end_char:
        doc_end = chunk.end_char
        clamped = True
    if doc_end < doc_start:
        doc_end = doc_start
        clamped = True
    verified = (
        mention_text is not None
        and 0 <= doc_start <= doc_end <= len(document_text)
        and document_text[doc_start:doc_end] == mention_text
    )
    return doc_start, doc_end, clamped, verified
