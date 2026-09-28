"""Epic 014 Task 4 — evidence grounding and scoring.

Pure-stdlib scoring package consumed by Tasks 5-8. It takes a Task 3 harness
run directory (`gliner-runs/<model>/<variant>/<stamp>/`), grounds every
emitted candidate against the frozen AGENTS.md snapshot, re-validates claims
against the versioned schema, scores them against the frozen Epic 013
reference set (R1-R9, repeated entities, Q1-Q6), and writes a scorecard
(`scorecard.json`, `scorecard.md`) plus a human-review template.

Modules:

- `grounding`  offset -> line-range grounding; citations are always derived
               from the stored snapshot, never from model text
- `taxonomy`   failure-category constants and code -> category mapping
- `reference`  frozen reference-set loading, name/predicate matching,
               per-fact structural matching and diagnosis
- `accept`     run-directory loading, claim acceptance (schema verdict +
               grounding + chunk locality), duplicates
- `review`     human-review template generation/loading (claim-to-evidence
               support and question coverage); deterministic components never
               depend on it — support defaults to "unreviewed"
- `metrics`    metric aggregation
- `report`     scorecard JSON/Markdown writing
- `cli`        `python -m scoring score <run_dir>`

The `sys.path` bootstrap below lets `python -m scoring` run from any working
directory while importing the sibling `schema` package.
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
DEFAULT_REFERENCE_SET_PATH = _PKG_PARENT / "frozen-inputs" / "reference-set.json"
SCORING_DIR_NAME = "scoring"
REVIEW_FILE_NAME = "review.json"
REVIEW_TEMPLATE_NAME = "review-template.json"

__all__ = [
    "DEFAULT_REFERENCE_SET_PATH",
    "DEFAULT_SCHEMA_PATH",
    "DEFAULT_SNAPSHOT_PATH",
    "EXPERIMENT_ROOT",
    "REVIEW_FILE_NAME",
    "REVIEW_TEMPLATE_NAME",
    "SCORING_DIR_NAME",
]
