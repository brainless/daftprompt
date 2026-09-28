"""CLI for Epic 014 Task 4 scoring.

Usage (from anywhere; the package bootstraps its own sys.path):

    cd experiments/open-vocab-graph
    ../../.venv/bin/python -m scoring score gliner-runs/<model>/<variant>/<stamp>
    ../../.venv/bin/python -m scoring score <run_dir> --review my-review.json
    ../../.venv/bin/python -m scoring score <run_dir> --out /tmp/scoring --force

Writes `scorecard.json`, `scorecard.md`, and (when no review file exists yet)
`review-template.json` into `<run_dir>/scoring/` by default. Exit codes:
0 scored · 1 scoring failure (missing run files, snapshot mismatch,
existing scorecard without --force).
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from . import DEFAULT_REFERENCE_SET_PATH, DEFAULT_SCHEMA_PATH, DEFAULT_SNAPSHOT_PATH, SCORING_DIR_NAME
from .accept import ScoringError
from .grounding import SnapshotMismatchError
from .report import ScorecardExistsError
from .score import score_run


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="scoring",
        description="Epic 014 Task 4: ground evidence and score a harness run directory.",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    score = sub.add_parser("score", help="score one Task 3 run directory")
    score.add_argument("run_dir", type=Path, help="harness run directory to score")
    score.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA_PATH,
                       help="versioned schema artifact")
    score.add_argument("--snapshot", type=Path, default=DEFAULT_SNAPSHOT_PATH,
                       help="frozen snapshot (default: frozen-inputs/AGENTS.md)")
    score.add_argument("--reference-set", type=Path, default=DEFAULT_REFERENCE_SET_PATH,
                       help="frozen reference set (default: frozen-inputs/reference-set.json)")
    score.add_argument("--review", type=Path, default=None,
                       help="human-review JSON (default: <run_dir>/scoring/review.json if present)")
    score.add_argument("--out", type=Path, default=None,
                       help=f"scoring output directory (default: <run_dir>/{SCORING_DIR_NAME})")
    score.add_argument("--force", action="store_true",
                       help="overwrite an existing scorecard")
    score.add_argument("--no-write", action="store_true",
                       help="print the metrics summary without writing scorecards")
    return parser


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    if args.command != "score":
        return 2
    try:
        scorecard = score_run(
            args.run_dir,
            schema_path=args.schema,
            snapshot_path=args.snapshot,
            reference_set_path=args.reference_set,
            review_path=args.review,
            write=not args.no_write,
            out_dir=args.out,
            force=args.force,
        )
    except (ScoringError, SnapshotMismatchError, ScorecardExistsError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    except FileNotFoundError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    summary = {
        "run_directory": scorecard["run_directory"],
        "metrics": scorecard["metrics"],
        "failure_categories": scorecard["failure_categories"],
        "review": {
            "source": scorecard["review"]["source"],
            "claims_reviewed": scorecard["review"]["claims_reviewed"],
            "questions_reviewed": scorecard["review"]["questions_reviewed"],
        },
    }
    print(json.dumps(summary, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
