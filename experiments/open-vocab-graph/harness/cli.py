"""CLI for the Epic 014 GLiNER2.5 extraction harness.

Usage (from anywhere; the package bootstraps its own sys.path):

    cd experiments/open-vocab-graph
    ../../.venv/bin/python -m harness extract --model fastino/gliner2.5-small-v1
    ../../.venv/bin/python -m harness extract --model fastino/gliner2.5-base-v1 \
        --variant independent --run-label indep
    ../../.venv/bin/python -m harness plan     # chunk plan only, no model

Exit codes: 0 completed; 1 startup/verification/load failure; 2 argparse
error; 3 infeasible chunks recorded; 4 per-chunk extraction errors recorded.
Empty chunks are a normal outcome and exit 0.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from . import DEFAULT_OUT_ROOT, DEFAULT_SCHEMA_PATH, DEFAULT_SNAPSHOT_PATH
from . import adapter, chunking, runner


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="harness",
        description="Epic 014 GLiNER2.5 local extraction adapter (Task 3).",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    extract = sub.add_parser("extract", help="run one extraction and write a run directory")
    extract.add_argument(
        "--model", required=True,
        help="explicit checkpoint ID (e.g. fastino/gliner2.5-small-v1) or local directory",
    )
    extract.add_argument(
        "--variant", choices=adapter.VARIANTS, default="jointie",
        help="jointie (default; typed endpoints) or independent (untyped extract_relations)",
    )
    extract.add_argument("--snapshot", type=Path, default=DEFAULT_SNAPSHOT_PATH,
                         help="source snapshot (default: frozen-inputs/AGENTS.md)")
    extract.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA_PATH,
                         help="versioned schema artifact")
    extract.add_argument("--out-root", type=Path, default=DEFAULT_OUT_ROOT,
                         help="run output root (default: gliner-runs/)")
    extract.add_argument("--chunk-words", type=int, default=384,
                         help="soft word budget per whole-line chunk (default 384)")
    extract.add_argument("--overlap-lines", type=int, default=2,
                         help="whole trailing lines re-sent in the next chunk (default 2)")
    extract.add_argument("--device", default="auto",
                         help="auto (default) | cpu | mps | cuda")
    extract.add_argument("--cache-dir", type=Path, default=None,
                         help="HF cache directory for downloaded checkpoints")
    extract.add_argument("--revision", default=None,
                         help="HF revision (branch/tag/commit) to pin the checkpoint")
    extract.add_argument("--local-files-only", action="store_true",
                         help="never touch the network; fail if the checkpoint is not cached")
    extract.add_argument("--run-label", default="",
                         help="suffix for the run directory name")
    # jointie decoding knobs
    extract.add_argument("--optimizer", choices=("beam", "greedy", "auto"), default="beam")
    extract.add_argument("--beam-size", type=int, default=32)
    extract.add_argument("--candidate-threshold", type=float, default=0.05)
    extract.add_argument("--relation-role-threshold", type=float, default=0.05)
    extract.add_argument("--entity-threshold", type=float, default=None)
    extract.add_argument("--top-k-entities", type=int, default=32)
    extract.add_argument("--top-k-roles", type=int, default=12)
    # independent decoding knob
    extract.add_argument("--threshold", type=float, default=0.5,
                         help="independent-variant extraction threshold (default 0.5)")
    extract.add_argument(
        "--plan-only", action="store_true",
        help="verify inputs and print the chunk plan as JSON without loading a model",
    )

    plan = sub.add_parser("plan", help="print the chunk plan for the snapshot (no model)")
    plan.add_argument("--snapshot", type=Path, default=DEFAULT_SNAPSHOT_PATH)
    plan.add_argument("--schema", type=Path, default=DEFAULT_SCHEMA_PATH)
    plan.add_argument("--chunk-words", type=int, default=384)
    plan.add_argument("--overlap-lines", type=int, default=2)

    return parser


def _print_plan(args) -> int:
    from schema import core

    schema = core.load_schema(args.schema)
    snapshot_record = runner.verify_snapshot(schema, args.snapshot)
    snapshot_text = Path(args.snapshot).read_text(encoding="utf-8")
    chunks = chunking.chunk_lines(snapshot_text, args.chunk_words, args.overlap_lines)
    print(json.dumps({
        "snapshot": snapshot_record,
        "chunking": {
            "policy": "whole-physical-lines",
            "chunk_words": args.chunk_words,
            "overlap_lines": args.overlap_lines,
        },
        "chunk_count": len(chunks),
        "chunks": [chunk.to_dict() for chunk in chunks],
    }, indent=2, ensure_ascii=False))
    return 0


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    if args.command == "plan":
        return _print_plan(args)

    try:
        config = runner.RunConfig(
            model_id=args.model,
            variant=args.variant,
            snapshot_path=args.snapshot,
            schema_path=args.schema,
            out_root=args.out_root,
            chunk_words=args.chunk_words,
            overlap_lines=args.overlap_lines,
            device=args.device,
            cache_dir=args.cache_dir,
            revision=args.revision,
            local_files_only=args.local_files_only,
            run_label=args.run_label,
            optimizer=args.optimizer,
            beam_size=args.beam_size,
            candidate_threshold=args.candidate_threshold,
            relation_role_threshold=args.relation_role_threshold,
            entity_threshold=args.entity_threshold,
            top_k_entities=args.top_k_entities,
            top_k_roles=args.top_k_roles,
            threshold=args.threshold,
        )
        if args.plan_only:
            from schema import core

            schema = core.load_schema(config.schema_path)
            snapshot_record = runner.verify_snapshot(schema, config.snapshot_path)
            snapshot_text = Path(config.snapshot_path).read_text(encoding="utf-8")
            chunks = chunking.chunk_lines(
                snapshot_text, config.chunk_words, config.overlap_lines
            )
            options = adapter.hub_kwargs(
                config.cache_dir, config.revision, config.local_files_only
            )
            adapter.apply_hub_env(options)
            verification = adapter.verify_checkpoint(config.model_id, options)
            print(json.dumps({
                "plan_only": True,
                "model": config.model_id,
                "checkpoint_verification": verification,
                "snapshot": snapshot_record,
                "chunk_count": len(chunks),
                "chunks": [chunk.to_dict() for chunk in chunks],
            }, indent=2, ensure_ascii=False))
            return 0
        summary = runner.run(config)
    except (
        runner.SnapshotMismatchError,
        runner.RunDirectoryExistsError,
        adapter.CheckpointCapabilityError,
    ) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    except Exception as error:  # load/extract failures must stay visible
        print(f"error: {type(error).__name__}: {error}", file=sys.stderr)
        return 1

    print(json.dumps(summary, indent=2, ensure_ascii=False))
    counts = summary["chunks"]["by_status"]
    if counts.get("error"):
        print(
            f"warning: {counts['error']} chunk(s) failed extraction; see chunk records",
            file=sys.stderr,
        )
        return 4
    if counts.get("infeasible"):
        print(
            f"warning: {counts['infeasible']} chunk(s) infeasible under the declared "
            "constraints; relations in those chunks were not decoded",
            file=sys.stderr,
        )
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
