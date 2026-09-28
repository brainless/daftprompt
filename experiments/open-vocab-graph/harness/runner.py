"""Run orchestration for the Epic 014 extraction harness.

One `run()` executes the full pipeline against one checkpoint:

1. load the versioned schema artifact and fail closed if the snapshot's
   sha256 no longer matches the artifact's recorded `source_snapshot.sha256`;
2. verify the checkpoint's relation head from `config.json` (before any
   weight download);
3. chunk the snapshot with the recorded whole-line policy;
4. extract per chunk (per-chunk raw output is kept verbatim, including empty
   and infeasible chunks);
5. normalize, dedup, and write the run directory (manifest, per-chunk
   records, entities, relations, summary) — never overwriting an existing
   run, Epic 013's `output/`, or `frozen-inputs/`.

The engine is injectable so tests can exercise the full pipeline without a
checkpoint.
"""
from __future__ import annotations

import hashlib
import json
import platform
import shlex
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from importlib.metadata import version as package_version
from pathlib import Path
from typing import Any, Optional

from schema import core

from . import DEFAULT_OUT_ROOT, DEFAULT_SCHEMA_PATH, DEFAULT_SNAPSHOT_PATH
from . import adapter, chunking, normalize


class SnapshotMismatchError(RuntimeError):
    """The snapshot does not match the schema artifact's frozen sha256."""


class RunDirectoryExistsError(RuntimeError):
    """Refusing to overwrite an existing run directory."""


@dataclass
class RunConfig:
    model_id: str
    variant: str = "jointie"
    snapshot_path: Path = DEFAULT_SNAPSHOT_PATH
    schema_path: Path = DEFAULT_SCHEMA_PATH
    out_root: Path = DEFAULT_OUT_ROOT
    chunk_words: int = 384
    overlap_lines: int = 2
    device: str = "auto"
    cache_dir: Optional[Path] = None
    revision: Optional[str] = None
    local_files_only: bool = False
    run_label: str = ""
    # jointie decoding knobs (JointIEConfig fields)
    optimizer: str = "beam"
    beam_size: int = 32
    candidate_threshold: float = 0.05
    relation_role_threshold: float = 0.05
    entity_threshold: Optional[float] = None
    top_k_entities: int = 32
    top_k_roles: int = 12
    # independent decoding knob
    threshold: float = 0.5

    def jointie_config_kwargs(self) -> dict:
        return {
            "optimizer": self.optimizer,
            "beam_size": self.beam_size,
            "candidate_threshold": self.candidate_threshold,
            "relation_role_threshold": self.relation_role_threshold,
            "entity_threshold": self.entity_threshold,
            "top_k_entities": self.top_k_entities,
            "top_k_roles": self.top_k_roles,
        }

    def chunking_record(self) -> dict:
        return {
            "policy": "whole-physical-lines",
            "chunk_words": self.chunk_words,
            "overlap_lines": self.overlap_lines,
            "overlap_unit": "lines",
            "word_count_unit": "whitespace-split words per physical line",
        }


def _write_json(path: Path, payload: Any) -> None:
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2, sort_keys=True, ensure_ascii=False)
        handle.write("\n")


def _slug(model_id: str) -> str:
    return model_id.replace("/", "_").strip("./") or "model"


def peak_memory_record(resolved_device: Optional[str]) -> dict:
    """Best-effort peak memory for reproducibility metadata (Task 5).

    `peak_memory_bytes` is the process RSS high-water mark (ru_maxrss; bytes on
    macOS, KiB on Linux). Scoring reads `manifest.peak_memory_bytes` or
    `manifest.resources.peak_memory_bytes`. Rejected alternative: sampling RSS
    in a background thread — a single high-water mark is simpler and matches
    what the scorer reports. Introspection must never block a run.
    """
    record: dict[str, Any] = {}
    try:
        import resource

        high_water = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        scale = 1 if sys.platform == "darwin" else 1024
        record["peak_memory_bytes"] = int(high_water) * scale
        record["peak_memory_source"] = "resource.getrusage(RUSAGE_SELF).ru_maxrss"
        record["peak_memory_scope"] = "process RSS high-water mark (includes model load)"
    except Exception as exc:  # non-Unix or introspection failure
        record["peak_memory_error"] = repr(exc)
    try:
        import torch

        if resolved_device == "cuda" and torch.cuda.is_available():
            record["cuda_peak_memory_bytes"] = int(torch.cuda.max_memory_allocated())
    except Exception as exc:
        record["cuda_peak_memory_error"] = repr(exc)
    return record


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_path(path: Path) -> str:
    return sha256_bytes(Path(path).read_bytes())


def verify_snapshot(schema: core.ExperimentSchema, snapshot_path: Path) -> dict:
    """Fail closed when the snapshot drifted from the frozen artifact."""
    snapshot_bytes = Path(snapshot_path).read_bytes()
    digest = sha256_bytes(snapshot_bytes)
    expected = schema.data["source_snapshot"]["sha256"]
    line_count = len(snapshot_bytes.decode("utf-8").splitlines())
    if digest != expected:
        raise SnapshotMismatchError(
            f"snapshot {snapshot_path} sha256 {digest} does not match the schema "
            f"artifact's frozen source_snapshot sha256 {expected}; the comparison is "
            "defined on experiments/open-vocab-graph/frozen-inputs/AGENTS.md and must "
            "not run against drifted text"
        )
    recorded_lines = schema.data["source_snapshot"].get("line_count")
    if recorded_lines is not None and line_count != recorded_lines:
        raise SnapshotMismatchError(
            f"snapshot {snapshot_path} has {line_count} lines; artifact records "
            f"{recorded_lines}"
        )
    return {"path": str(snapshot_path), "sha256": digest, "line_count": line_count}


def _package_versions() -> dict:
    versions: dict[str, Any] = {
        "python": platform.python_version(),
        "platform": platform.platform(),
    }
    for name in ("gliner2", "torch", "transformers", "huggingface_hub"):
        try:
            versions[name] = package_version(name)
        except Exception:
            versions[name] = None
    return versions


def _run_directory(config: RunConfig, started: datetime) -> Path:
    name = started.strftime("%Y%m%dT%H%M%SZ")
    if config.run_label:
        name = f"{name}-{config.run_label}"
    run_dir = config.out_root / _slug(config.model_id) / config.variant / name
    if run_dir.exists():
        raise RunDirectoryExistsError(
            f"run directory {run_dir} already exists; refusing to overwrite — "
            "use --run-label to disambiguate"
        )
    return run_dir


def _model_record(config: RunConfig, verification: Optional[dict]) -> dict:
    record: dict[str, Any] = {
        "id": config.model_id,
        "known_checkpoint": config.model_id in adapter.KNOWN_MODEL_IDS,
        "revision": None,
        "revision_note": None,
    }
    if Path(config.model_id).is_dir():
        record["local_path"] = str(Path(config.model_id).resolve())
    if verification is not None:
        record.update(verification)
        sha, note = adapter.probe_revision(config.model_id, revision=config.revision)
        record["revision"] = sha
        record["revision_note"] = note
    else:
        record["revision_note"] = "revision probe skipped (engine injected without load)"
    return record


def run(config: RunConfig, engine: Optional[Any] = None) -> dict:
    """Execute one extraction run; returns the summary dict."""
    started_at = datetime.now(timezone.utc)
    start_clock = time.perf_counter()

    schema = core.load_schema(config.schema_path)
    snapshot_record = verify_snapshot(schema, config.snapshot_path)
    snapshot_text = Path(config.snapshot_path).read_text(encoding="utf-8")

    chunks = chunking.chunk_lines(snapshot_text, config.chunk_words, config.overlap_lines)
    if config.variant not in adapter.VARIANTS:
        raise ValueError(f"unknown variant {config.variant!r}; expected one of {adapter.VARIANTS}")

    verification: Optional[dict] = None
    resolved_device: Optional[str] = None
    if engine is None:
        options = adapter.hub_kwargs(config.cache_dir, config.revision, config.local_files_only)
        adapter.apply_hub_env(options)
        verification = adapter.verify_checkpoint(config.model_id, options)
        resolved_device = adapter.resolve_device(config.device)
        engine = adapter.load_engine(
            config.variant,
            config.model_id,
            resolved_device,
            options,
            jointie_config_kwargs=config.jointie_config_kwargs(),
            independent_threshold=config.threshold,
        )
    engine.bind_schema(schema)

    chunk_records: list[dict] = []
    all_entities: list[normalize.NormalizedEntity] = []
    all_relations: list[normalize.NormalizedRelation] = []
    for chunk in chunks:
        chunk_record: dict[str, Any] = {
            "chunk": chunk.to_dict(),
            "status": "error",
            "raw_response": None,
            "entity_count": 0,
            "relation_count": 0,
            "dropped": [],
            "notes": [],
            "error": None,
            "elapsed_seconds": None,
        }
        chunk_start = time.perf_counter()
        try:
            raw = engine.extract_chunk(chunk.text)
            elapsed = time.perf_counter() - chunk_start
            status = normalize.chunk_status(raw, config.variant)
            entities, relations, notes, dropped = normalize.normalize_chunk(
                raw, chunk, schema, snapshot_text, config.variant
            )
            chunk_record.update(
                {
                    "status": status,
                    "raw_response": raw,
                    "entity_count": len(entities),
                    "relation_count": len(relations),
                    "dropped": dropped,
                    "notes": notes,
                    "elapsed_seconds": elapsed,
                }
            )
            all_entities.extend(entities)
            all_relations.extend(relations)
        except Exception as exc:  # one bad chunk must not hide the others
            chunk_record["error"] = f"{type(exc).__name__}: {exc}"
            chunk_record["elapsed_seconds"] = time.perf_counter() - chunk_start
        chunk_records.append(chunk_record)

    merged_entities = normalize.merge_entities(all_entities)
    merged_relations = normalize.merge_relations(all_relations)

    status_counts = {"ok": 0, "empty": 0, "infeasible": 0, "error": 0}
    for record in chunk_records:
        status_counts[record["status"]] = status_counts.get(record["status"], 0) + 1

    accepted = [r for r in merged_relations if r.verdict.get("accepted")]
    rejected = [r for r in merged_relations if not r.verdict.get("accepted")]

    if status_counts["error"]:
        run_status = "completed_with_chunk_errors"
    elif status_counts["infeasible"] == len(chunks) and chunks:
        run_status = "failed_infeasible"
    elif status_counts["infeasible"]:
        run_status = "completed_with_infeasible_chunks"
    else:
        run_status = "completed"

    elapsed_total = time.perf_counter() - start_clock
    finished_at = datetime.now(timezone.utc)

    run_dir = _run_directory(config, started_at)
    (run_dir / "chunks").mkdir(parents=True)
    try:
        for record in chunk_records:
            index = record["chunk"]["index"]
            _write_json(run_dir / "chunks" / f"chunk-{index:03d}.json", record)
        _write_json(
            run_dir / "entities.json",
            {
                "schema_id": schema.schema_id,
                "variant": config.variant,
                "count": len(merged_entities),
                "entities": [e.to_dict() for e in merged_entities],
            },
        )
        _write_json(
            run_dir / "relations.json",
            {
                "schema_id": schema.schema_id,
                "variant": config.variant,
                "count": len(merged_relations),
                "accepted_count": len(accepted),
                "rejected_count": len(rejected),
                "relations": [r.to_dict() for r in merged_relations],
            },
        )

        summary = {
            "run_directory": str(run_dir),
            "model_id": config.model_id,
            "variant": config.variant,
            "status": run_status,
            "chunks": {
                "total": len(chunks),
                "by_status": {k: v for k, v in status_counts.items() if v},
            },
            "entities": {"deduplicated": len(merged_entities), "mentions": len(all_entities)},
            "relations": {
                "deduplicated": len(merged_relations),
                "accepted": len(accepted),
                "rejected": len(rejected),
                "candidates": len(all_relations),
            },
            "elapsed_seconds": elapsed_total,
            "started_utc": started_at.isoformat(),
            "finished_utc": finished_at.isoformat(),
        }
        _write_json(run_dir / "summary.json", summary)

        manifest = {
            "run_id": run_dir.name,
            "command": list(sys.argv),
            "command_line": shlex.join(sys.argv),
            "started_utc": started_at.isoformat(),
            "finished_utc": finished_at.isoformat(),
            "elapsed_seconds": elapsed_total,
            "status": run_status,
            "model": _model_record(config, verification),
            "package_versions": _package_versions(),
            "device": adapter.device_info(config.device, resolved_device or "engine-injected"),
            "variant": config.variant,
            "thresholds": engine.threshold_record(),
            "chunking": {
                **config.chunking_record(),
                "chunk_count": len(chunks),
                "snapshot_words_document": len(snapshot_text.split()),
                "chunk_words_including_overlap": sum(c.word_count for c in chunks),
            },
            "schema": {
                "schema_id": schema.schema_id,
                "schema_version": schema.schema_version,
                "artifact_path": str(config.schema_path),
                "artifact_sha256": sha256_path(config.schema_path),
                "seed_xxh3": schema.data["seed"]["xxh3"],
            },
            "snapshot": snapshot_record,
            "counts": {
                "chunks_total": len(chunks),
                "chunks_by_status": {k: v for k, v in status_counts.items() if v},
                "entity_mentions": len(all_entities),
                "entities_deduplicated": len(merged_entities),
                "relation_candidates": len(all_relations),
                "relations_deduplicated": len(merged_relations),
                "relations_accepted": len(accepted),
                "relations_rejected": len(rejected),
            },
            "engine_injected": verification is None,
            "resources": peak_memory_record(resolved_device),
        }
        _write_json(run_dir / "manifest.json", manifest)
    except Exception:
        import shutil

        shutil.rmtree(run_dir, ignore_errors=True)
        raise

    return summary
