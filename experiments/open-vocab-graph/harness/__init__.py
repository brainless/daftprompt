"""Epic 014 Task 3 — local GLiNER2.5 extraction adapter/harness.

Standalone Python harness that loads a GLiNER2.5 checkpoint by explicit model
ID, passes the versioned schema (`../schema/gliner25-schema.json`) to the
model, extracts from the frozen AGENTS.md snapshot in recorded whole-line
chunks, and writes per-chunk raw output plus normalized entity/relation
candidates under `../gliner-runs/`.

Modules:

- `chunking`   pure line-preserving chunker (no gliner2/torch imports)
- `normalize`  raw-output -> typed candidates + span-stable dedup (pure)
- `adapter`    checkpoint verification and model loading (gliner2 imported lazily)
- `runner`     orchestrates one run and writes the run directory
- `cli`        `python -m harness extract ...`

The `sys.path` bootstrap below lets `python -m harness` run from any working
directory while still importing the sibling `schema` package.
"""
from __future__ import annotations

import sys
from pathlib import Path

_PKG_PARENT = Path(__file__).resolve().parent.parent
if str(_PKG_PARENT) not in sys.path:
    sys.path.insert(0, str(_PKG_PARENT))

EXPERIMENT_ROOT = _PKG_PARENT
DEFAULT_SCHEMA_PATH = _PKG_PARENT / "schema" / "gliner25-schema.json"
DEFAULT_SNAPSHOT_PATH = _PKG_PARENT / "frozen-inputs" / "AGENTS.md"
DEFAULT_OUT_ROOT = _PKG_PARENT / "gliner-runs"

__all__ = [
    "DEFAULT_OUT_ROOT",
    "DEFAULT_SCHEMA_PATH",
    "DEFAULT_SNAPSHOT_PATH",
    "EXPERIMENT_ROOT",
]
