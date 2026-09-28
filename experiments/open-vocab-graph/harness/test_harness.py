"""Tests for the Epic 014 Task 3 extraction harness.

Runs with plain asserts and no model download or checkpoint load:

    .venv/bin/python experiments/open-vocab-graph/harness/test_harness.py

The function names are pytest-compatible, so `pytest` also works if installed.
Covers: chunk mapping/boundaries/overlaps, span remapping, normalization of
both variants (label mapping, validation verdicts, untyped-endpoint handling),
span-stable dedup, chunk-status visibility, the checkpoint capability gate,
and a full runner pass with an injected fake engine (no weights).
"""
from __future__ import annotations

import json
import shutil
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from harness import adapter, chunking, normalize, runner  # noqa: E402
from schema import core  # noqa: E402

FROZEN_SNAPSHOT = HERE.parent / "frozen-inputs" / "AGENTS.md"
SCHEMA_ARTIFACT = HERE.parent / "schema" / "gliner25-schema.json"

DOC = (
    "daftprompt is a Rust app.\n"
    "It uses **akar** for rendering.\n"
    "The graph build command requires XIAOMI_API_KEY.\n"
)


def _single_chunk(text: str) -> chunking.Chunk:
    return chunking.chunk_lines(text, chunk_words=1000, overlap_lines=0)[0]


def _span(text: str, needle: str) -> tuple[int, int]:
    start = text.index(needle)
    return start, start + len(needle)


def _entity(entity_id, kind, mention, text, confidence=0.9):
    start, end = _span(text, mention)
    return {"id": entity_id, "type": kind, "text": mention, "start": start, "end": end,
            "confidence": confidence}


# ---------------------------------------------------------------------------
# chunking
# ---------------------------------------------------------------------------

def test_chunking_preserves_whole_lines():
    lines = [f"line {i} " + "word " * (5 + i % 4) + "\n" for i in range(40)]
    text = "".join(lines)
    chunks = chunking.chunk_lines(text, chunk_words=30, overlap_lines=2)
    assert chunks, "must produce chunks"
    covered: set[int] = set()
    for chunk in chunks:
        # every chunk is a concatenation of whole physical lines
        expected = "".join(lines[chunk.start_line - 1 : chunk.end_line])
        assert chunk.text == expected
        assert chunk.text.startswith(lines[chunk.start_line - 1])
        assert chunk.text.endswith(lines[chunk.end_line - 1])
        covered |= set(range(chunk.start_line, chunk.end_line + 1))
    assert covered == set(range(1, len(lines) + 1)), "every line must be covered"
    for previous, nxt in zip(chunks, chunks[1:]):
        # progress: each chunk contains at least one line beyond the previous
        assert nxt.end_line > previous.end_line


def test_chunk_mapping_offsets_and_reconstruction():
    text = "".join(f"line-{i} with some words\n" for i in range(25))
    chunks = chunking.chunk_lines(text, chunk_words=12, overlap_lines=1)
    assert len(chunks) > 1
    for chunk in chunks:
        assert text[chunk.start_char : chunk.end_char] == chunk.text
    # a known mention remaps to document offsets that slice back exactly
    chunk = chunks[1]
    local = chunk.text.index("with")
    doc_start, doc_end, clamped, verified = chunking.remap_span(
        chunk, local, local + 4, text, mention_text="with"
    )
    assert not clamped and verified
    assert text[doc_start:doc_end] == "with"
    assert doc_start == chunk.start_char + local


def test_oversized_line_gets_own_chunk():
    huge = "giant " * 50 + "\n"
    text = "small line one\n" + huge + "small line two\n"
    chunks = chunking.chunk_lines(text, chunk_words=10, overlap_lines=1)
    oversized = [c for c in chunks if c.oversized]
    assert len(oversized) == 1 and oversized[0].word_count > 10
    assert oversized[0].text == huge
    # no overlap is recorded after an oversized line (it cannot fit)
    index = chunks.index(oversized[0])
    if index + 1 < len(chunks):
        assert oversized[0].start_line not in chunks[index + 1].overlap_lines


def test_overlap_lines_recorded():
    lines = [f"line {i} " + "w " * (6 + i % 3) + "\n" for i in range(30)]
    text = "".join(lines)
    chunks = chunking.chunk_lines(text, chunk_words=25, overlap_lines=2)
    assert len(chunks) > 1
    assert chunks[0].overlap_lines == ()
    for previous, nxt in zip(chunks, chunks[1:]):
        overlap = nxt.overlap_lines
        assert overlap, "default policy must overlap adjacent chunks"
        assert all(previous.start_line <= n <= previous.end_line for n in overlap)
        assert overlap == tuple(
            range(previous.end_line - len(overlap) + 1, previous.end_line + 1)
        ), "overlap must be the trailing whole lines of the previous chunk"
    # overlap_lines=0 disables overlap entirely
    chunks = chunking.chunk_lines(text, chunk_words=25, overlap_lines=0)
    assert all(c.overlap_lines == () for c in chunks)
    # empty document still yields one recorded (empty) chunk
    empty = chunking.chunk_lines("", chunk_words=8, overlap_lines=2)
    assert len(empty) == 1 and empty[0].text == "" and empty[0].word_count == 0


def test_remap_span_clamps_and_flags_mismatch():
    text = DOC
    chunk = _single_chunk(text)
    doc_start, doc_end, clamped, verified = chunking.remap_span(
        chunk, 0, len(text) + 50, text, mention_text=text
    )
    assert clamped and doc_end == chunk.end_char
    _, _, _, verified = chunking.remap_span(chunk, 0, 5, text, mention_text="nope!")
    assert not verified


# ---------------------------------------------------------------------------
# normalization (jointie)
# ---------------------------------------------------------------------------

def test_normalize_jointie_maps_labels_and_validates():
    schema = core.load_schema(SCHEMA_ARTIFACT)
    chunk = _single_chunk(DOC)
    raw = {
        "feasible": True,
        "entities": [
            _entity("e1", "Application", "daftprompt", DOC),
            _entity("e2", "Tool", "akar", DOC),
            _entity("e3", "Command", "graph build", DOC),
            _entity("e4", "Configuration", "XIAOMI_API_KEY", DOC),
        ],
        "relations": [
            {"type": "uses", "head": "e1", "tail": "e2", "confidence": 0.91},
            {"type": "requires", "head": "e3", "tail": "e4", "confidence": 0.88},
        ],
    }
    entities, relations, notes, dropped = normalize.normalize_jointie_chunk(
        raw, chunk, schema, DOC
    )
    assert not dropped
    assert len(entities) == 4
    by_text = {e.text: e for e in entities}
    assert by_text["daftprompt"].kind == "Application"
    assert by_text["akar"].kind == "Tool"
    # document offsets slice back into the snapshot
    for entity in entities:
        assert DOC[entity.start : entity.end] == entity.text
        assert entity.span_verified
    assert len(relations) == 2
    uses = next(r for r in relations if r.predicate == "uses")
    assert uses.model_label == "uses"
    assert uses.verdict["accepted"] is True
    assert uses.subject.text == "daftprompt" and uses.object.text == "akar"
    assert uses.confidence == 0.91 and uses.span_verified
    requires = next(r for r in relations if r.predicate == "requires")
    assert requires.verdict["accepted"] is True

    # the motivating fact survives the full normalization path
    claim = core.Claim(
        subject_text="daftprompt", subject_kind="Application", predicate="uses",
        object_text="akar", object_kind="Tool",
    )
    assert core.validate_claim(schema, claim).accepted


def test_normalize_jointie_rejects_disallowed_pair_but_keeps_candidate():
    schema = core.load_schema(SCHEMA_ARTIFACT)
    chunk = _single_chunk(DOC)
    raw = {
        "feasible": True,
        "entities": [
            _entity("e1", "Command", "graph build", DOC),
            # reuse a Subsystem phrase that does not exist in DOC: append-safe span
            {"id": "e2", "type": "Subsystem", "text": "graph build", "start": 0, "end": 11},
        ],
        "relations": [
            {"type": "uses__2", "head": "e1", "tail": "e2", "confidence": 0.5},
        ],
    }
    entities, relations, notes, dropped = normalize.normalize_jointie_chunk(
        raw, chunk, schema, DOC
    )
    assert not dropped and len(relations) == 1
    relation = relations[0]
    assert relation.predicate == "uses", "uses__2 must map to canonical 'uses'"
    assert relation.model_label == "uses__2"
    assert relation.verdict["accepted"] is False
    assert "disallowed_pair" in relation.verdict["codes"]


def test_normalize_jointie_records_missing_endpoint_instead_of_hiding():
    schema = core.load_schema(SCHEMA_ARTIFACT)
    chunk = _single_chunk(DOC)
    raw = {
        "feasible": True,
        "entities": [_entity("e1", "Application", "daftprompt", DOC)],
        "relations": [
            {"type": "uses", "head": "e1", "tail": "e999", "confidence": 0.7},
        ],
    }
    entities, relations, notes, dropped = normalize.normalize_jointie_chunk(
        raw, chunk, schema, DOC
    )
    assert not relations
    assert len(dropped) == 1
    assert "e999" in dropped[0]["reason"]
    assert dropped[0]["raw"]["type"] == "uses"


def test_chunk_status_flags_infeasible_and_empty():
    jointie_empty = {"feasible": True, "entities": [], "relations": []}
    assert normalize.chunk_status(jointie_empty, "jointie") == "empty"
    assert normalize.chunk_status(
        {"feasible": False, "entities": [], "relations": []}, "jointie"
    ) == "infeasible"
    assert normalize.chunk_status(
        {"feasible": True, "entities": [_entity("e1", "Tool", "akar", DOC)], "relations": []},
        "jointie",
    ) == "ok"
    assert normalize.chunk_status(
        {"entities": {}, "relation_extraction": {"uses": []}}, "independent"
    ) == "empty"


# ---------------------------------------------------------------------------
# normalization (independent)
# ---------------------------------------------------------------------------

def test_normalize_independent_attaches_kinds_by_span():
    schema = core.load_schema(SCHEMA_ARTIFACT)
    chunk = _single_chunk(DOC)
    head = _entity("ignored", "Application", "daftprompt", DOC)  # kind ignored by model
    tail = {"text": "akar", "start": _span(DOC, "akar")[0], "end": _span(DOC, "akar")[1],
            "confidence": 0.8}
    raw = {
        "entities": {
            "Application": [{"text": "daftprompt", "start": head["start"], "end": head["end"],
                             "confidence": 0.9}],
            "Tool": [{"text": "akar", "start": tail["start"], "end": tail["end"],
                      "confidence": 0.8}],
        },
        "relation_extraction": {
            "uses": [{"head": {"text": "daftprompt", "start": head["start"], "end": head["end"],
                               "confidence": 0.9},
                      "tail": tail}],
        },
    }
    entities, relations, notes, dropped = normalize.normalize_independent_chunk(
        raw, chunk, schema, DOC
    )
    assert not dropped
    assert len(entities) == 2 and entities[0].kind == "Application"
    assert len(relations) == 1
    relation = relations[0]
    assert relation.predicate == "uses" and relation.model_label == "uses"
    assert relation.subject.kind == "Application", "kind must attach by span match"
    assert relation.object.kind == "Tool"
    assert relation.verdict["accepted"] is True
    assert relation.confidence is not None, "endpoint confidences must survive"


def test_normalize_independent_unmatched_endpoint_recorded_as_rejected():
    schema = core.load_schema(SCHEMA_ARTIFACT)
    chunk = _single_chunk(DOC)
    raw = {
        "entities": {"Tool": []},
        "relation_extraction": {
            "uses": [{
                "head": {"text": "daftprompt",
                         "start": _span(DOC, "daftprompt")[0], "end": _span(DOC, "daftprompt")[1]},
                "tail": {"text": "akar",
                         "start": _span(DOC, "akar")[0], "end": _span(DOC, "akar")[1]},
            }],
        },
    }
    entities, relations, notes, dropped = normalize.normalize_independent_chunk(
        raw, chunk, schema, DOC
    )
    assert not dropped, "untyped endpoints are recorded as rejected, never dropped"
    assert len(relations) == 1
    relation = relations[0]
    assert relation.subject.kind == "" and relation.object.kind == ""
    assert relation.verdict["accepted"] is False
    assert "unknown_subject_kind" in relation.verdict["codes"]
    assert "unknown_object_kind" in relation.verdict["codes"]


# ---------------------------------------------------------------------------
# dedup
# ---------------------------------------------------------------------------

def _norm_entity(kind, text, start, end, confidence, chunk_index, verified=True):
    return normalize.NormalizedEntity(
        kind=kind, text=text, start=start, end=end, confidence=confidence,
        chunk_index=chunk_index, span_verified=verified,
    )


def test_dedup_entities_by_span_distinct_spans_stay_distinct():
    # identical surface text at different document positions stays distinct
    first = _norm_entity("Command", "cargo run", 100, 109, 0.8, 0)
    second = _norm_entity("Command", "cargo run", 400, 409, 0.7, 1)
    merged = normalize.merge_entities([first, second])
    assert len(merged) == 2
    # chunk-overlap copy of the same span collapses, keeping max confidence
    overlap_copy = _norm_entity("Command", "cargo run", 100, 109, 0.95, 1)
    merged = normalize.merge_entities([first, second, overlap_copy])
    assert len(merged) == 2
    kept = next(m for m in merged if m.start == 100)
    assert kept.confidence == 0.95
    assert kept.occurrences == 2
    assert kept.source_chunks == (0, 1)
    # a different kind at the same span is a distinct typed mention
    other_kind = _norm_entity("Tool", "cargo run", 100, 109, 0.6, 0)
    merged = normalize.merge_entities([first, other_kind])
    assert len(merged) == 2


def test_dedup_relations_by_endpoint_spans():
    subject_a = _norm_entity("Application", "daftprompt", 0, 10, 0.9, 0)
    object_a = _norm_entity("Tool", "akar", 20, 24, 0.9, 0)
    subject_b = _norm_entity("Application", "daftprompt", 200, 210, 0.8, 1)
    relation_one = normalize.NormalizedRelation(
        predicate="uses", model_label="uses", subject=subject_a, object=object_a,
        confidence=0.7, chunk_index=0, span_verified=True,
        verdict={"accepted": True, "codes": ["ok"], "details": []},
    )
    relation_two = normalize.NormalizedRelation(
        predicate="uses", model_label="uses", subject=subject_a, object=object_a,
        confidence=0.85, chunk_index=1, span_verified=True,
        verdict={"accepted": True, "codes": ["ok"], "details": []},
    )
    # different subject span = a distinct candidate even with the same text
    relation_three = normalize.NormalizedRelation(
        predicate="uses", model_label="uses", subject=subject_b, object=object_a,
        confidence=0.5, chunk_index=1, span_verified=True,
        verdict={"accepted": True, "codes": ["ok"], "details": []},
    )
    merged = normalize.merge_relations([relation_one, relation_two, relation_three])
    assert len(merged) == 2
    kept = next(m for m in merged if m.subject.start == 0)
    assert kept.occurrences == 2 and kept.confidence == 0.85
    assert kept.source_chunks == (0, 1)
    assert kept.verdict["accepted"] is True
    assert all(m.predicate == "uses" for m in merged)


# ---------------------------------------------------------------------------
# checkpoint capability gate (offline, local config fixtures)
# ---------------------------------------------------------------------------

def _config_dir(tmp: str, config: dict) -> Path:
    directory = Path(tmp) / "ckpt"
    directory.mkdir(parents=True)
    (directory / "config.json").write_text(json.dumps(config), encoding="utf-8")
    return directory


def test_checkpoint_gate_fails_visibly_without_relation_head():
    with tempfile.TemporaryDirectory() as tmp:
        ok = _config_dir(tmp + "/ok", {
            "architecture": "boundary",
            "model_name": "microsoft/deberta-v3-xsmall",
            "config_version": 3,
            "boundary_head": {"enable_relations": True},
        })
        verification = adapter.verify_checkpoint(str(ok))
        assert verification["architecture"] == "boundary"
        assert verification["enable_relations"] is True
        assert len(verification["config_sha256"]) == 64

        span = _config_dir(tmp + "/span", {
            "architecture": "span",
            "model_name": "microsoft/deberta-v3-base",
            "config_version": 3,
            "span_head": {"max_width": 12},
        })
        try:
            adapter.verify_checkpoint(str(span))
        except adapter.CheckpointCapabilityError as error:
            assert "span" in str(error)
        else:
            raise AssertionError("span checkpoint must be rejected")

        norel = _config_dir(tmp + "/norel", {
            "architecture": "boundary",
            "model_name": "microsoft/deberta-v3-xsmall",
            "config_version": 3,
            "boundary_head": {"enable_relations": False},
        })
        try:
            adapter.verify_checkpoint(str(norel))
        except adapter.CheckpointCapabilityError as error:
            assert "relation head" in str(error)
        else:
            raise AssertionError("boundary checkpoint without relation head must be rejected")

        # pre-v3 boundary config migrates to enable_relations False
        legacy = _config_dir(tmp + "/legacy", {
            "architecture": "boundary",
            "model_name": "microsoft/deberta-v3-xsmall",
        })
        try:
            adapter.verify_checkpoint(str(legacy))
        except adapter.CheckpointCapabilityError:
            pass
        else:
            raise AssertionError("legacy boundary config must be rejected")


# ---------------------------------------------------------------------------
# runner end-to-end with an injected fake engine (no checkpoint)
# ---------------------------------------------------------------------------

LINE7_HEAD = "daftprompt is a Rust app"
LINE7_TAIL = "**akar**"


class FakeJointIEEngine:
    """Stands in for adapter.JointIEAdapter; emits the line-7 fact once."""

    variant = "jointie"

    def __init__(self):
        self.bound_schema = None
        self.chunks_seen = 0

    def bind_schema(self, schema) -> None:
        self.bound_schema = schema

    def threshold_record(self) -> dict:
        return {"fake_engine": True}

    def extract_chunk(self, text: str) -> dict:
        self.chunks_seen += 1
        if LINE7_TAIL not in text or LINE7_HEAD not in text:
            return {"feasible": True, "entities": [], "relations": []}
        head_start = text.index(LINE7_HEAD)
        tail_start = text.index(LINE7_TAIL) + 2
        return {
            "feasible": True,
            "entities": [
                {"id": "e1", "type": "Application", "text": "daftprompt",
                 "start": head_start, "end": head_start + len("daftprompt"),
                 "confidence": 0.93},
                {"id": "e2", "type": "Tool", "text": "akar",
                 "start": tail_start, "end": tail_start + len("akar"),
                 "confidence": 0.9},
            ],
            "relations": [
                {"type": "uses", "head": "e1", "tail": "e2", "confidence": 0.88},
            ],
        }


def _runner_config(out_root: Path) -> runner.RunConfig:
    return runner.RunConfig(
        model_id="fake/model-for-tests",
        variant="jointie",
        snapshot_path=FROZEN_SNAPSHOT,
        schema_path=SCHEMA_ARTIFACT,
        out_root=out_root,
        chunk_words=384,
        overlap_lines=2,
    )


def test_apply_hub_env_redirects_cache_and_offline():
    import os

    saved = {
        key: os.environ.get(key)
        for key in ("HF_HUB_CACHE", "HF_HUB_OFFLINE")
    }
    try:
        os.environ.pop("HF_HUB_CACHE", None)
        os.environ.pop("HF_HUB_OFFLINE", None)
        with tempfile.TemporaryDirectory() as tmp:
            cache = Path(tmp) / "model-cache"
            adapter.apply_hub_env({"cache_dir": str(cache), "local_files_only": True})
            assert os.environ["HF_HUB_CACHE"] == str(cache.resolve())
            assert cache.is_dir(), "the redirected cache dir is created"
            assert os.environ["HF_HUB_OFFLINE"] == "1"
        adapter.apply_hub_env({})
        assert os.environ["HF_HUB_CACHE"] == str(cache.resolve()), "no-op without options"
        assert os.environ["HF_HUB_OFFLINE"] == "1"
    finally:
        for key, value in saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


def test_runner_end_to_end_with_fake_engine():
    snapshot_text = FROZEN_SNAPSHOT.read_text(encoding="utf-8")
    with tempfile.TemporaryDirectory() as tmp:
        out_root = Path(tmp) / "gliner-runs"
        config = _runner_config(out_root)
        summary = runner.run(config, engine=FakeJointIEEngine())

        chunk_plan = chunking.chunk_lines(snapshot_text, 384, 2)
        assert summary["chunks"]["total"] == len(chunk_plan)
        by_status = summary["chunks"]["by_status"]
        assert by_status.get("ok") == 1
        assert by_status.get("empty") == len(chunk_plan) - 1, "empty chunks stay recorded"

        run_dir = Path(summary["run_directory"])
        for name in ("manifest.json", "summary.json", "entities.json", "relations.json"):
            assert (run_dir / name).is_file()
        chunk_files = sorted((run_dir / "chunks").glob("chunk-*.json"))
        assert len(chunk_files) == len(chunk_plan)
        records = [json.loads(p.read_text(encoding="utf-8")) for p in chunk_files]
        ok_records = [r for r in records if r["status"] == "ok"]
        empty_records = [r for r in records if r["status"] == "empty"]
        assert len(ok_records) == 1 and len(empty_records) == len(chunk_plan) - 1
        empty_chunk = empty_records[0]
        assert empty_chunk["raw_response"] == {
            "feasible": True, "entities": [], "relations": []
        }, "empty chunks must not be hidden"
        assert ok_records[0]["entity_count"] == 2 and ok_records[0]["relation_count"] == 1
        assert ok_records[0]["elapsed_seconds"] is not None

        manifest = json.loads((run_dir / "manifest.json").read_text(encoding="utf-8"))
        assert manifest["model"]["id"] == "fake/model-for-tests"
        assert manifest["engine_injected"] is True
        assert manifest["model"]["revision"] is None
        assert manifest["package_versions"]["gliner2"] is not None
        assert manifest["chunking"]["policy"] == "whole-physical-lines"
        assert manifest["chunking"]["chunk_count"] == len(chunk_plan)
        assert manifest["snapshot"]["sha256"] == (
            "a15dbdb069bcb58542f49bab0775004e62e792b52939aecd2c77886f33567423"
        )
        assert manifest["schema"]["schema_id"] == "daftprompt-gliner25-experiment"
        assert manifest["command"]  # command line recorded for reproducibility
        resources = manifest["resources"]
        if "peak_memory_error" not in resources:
            assert isinstance(resources["peak_memory_bytes"], int)
            assert resources["peak_memory_bytes"] > 0

        entities = json.loads((run_dir / "entities.json").read_text(encoding="utf-8"))
        assert entities["count"] == 2
        by_text = {e["text"]: e for e in entities["entities"]}
        assert by_text["daftprompt"]["kind"] == "Application"
        assert by_text["akar"]["kind"] == "Tool"
        # chunk-local offsets were remapped onto the frozen snapshot exactly
        assert snapshot_text[
            by_text["daftprompt"]["start"]:by_text["daftprompt"]["end"]
        ] == "daftprompt"
        assert snapshot_text[by_text["akar"]["start"]:by_text["akar"]["end"]] == "akar"
        assert by_text["akar"]["source_chunks"] == [by_text["daftprompt"]["source_chunks"][0]]

        relations = json.loads((run_dir / "relations.json").read_text(encoding="utf-8"))
        assert relations["count"] == 1
        relation = relations["relations"][0]
        assert relation["predicate"] == "uses" and relation["model_label"] == "uses"
        assert relation["verdict"]["accepted"] is True
        assert relations["accepted_count"] == 1 and relations["rejected_count"] == 0

        # never overwrite an existing run directory
        try:
            runner.run(config, engine=FakeJointIEEngine())
        except runner.RunDirectoryExistsError:
            pass
        else:
            raise AssertionError("run directory collision must fail visibly")


def test_runner_fails_closed_on_snapshot_drift():
    with tempfile.TemporaryDirectory() as tmp:
        drifted = Path(tmp) / "AGENTS.md"
        drifted.write_text(FROZEN_SNAPSHOT.read_text(encoding="utf-8") + "drift\n",
                           encoding="utf-8")
        config = runner.RunConfig(
            model_id="fake/model-for-tests",
            snapshot_path=drifted,
            schema_path=SCHEMA_ARTIFACT,
            out_root=Path(tmp) / "runs",
        )
        try:
            runner.run(config, engine=FakeJointIEEngine())
        except runner.SnapshotMismatchError as error:
            assert "sha256" in str(error)
        else:
            raise AssertionError("snapshot drift must fail closed")


def test_cli_plan_only_verifies_offline():
    import contextlib
    import io

    from harness import cli

    with tempfile.TemporaryDirectory() as tmp:
        checkpoint = _config_dir(tmp + "/ckpt", {
            "architecture": "boundary",
            "model_name": "microsoft/deberta-v3-xsmall",
            "config_version": 3,
            "boundary_head": {"enable_relations": True},
        })
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            code = cli.main([
                "extract", "--plan-only",
                "--model", str(checkpoint),
                "--snapshot", str(FROZEN_SNAPSHOT),
                "--schema", str(SCHEMA_ARTIFACT),
                "--chunk-words", "384",
            ])
        assert code == 0
        payload = json.loads(buffer.getvalue())
        assert payload["plan_only"] is True
        assert payload["checkpoint_verification"]["enable_relations"] is True
        assert payload["snapshot"]["line_count"] == 146
        assert payload["chunk_count"] >= 1


def main() -> int:
    tests = [(name, fn) for name, fn in sorted(globals().items())
             if name.startswith("test_") and callable(fn)]
    failures = 0
    for name, fn in tests:
        try:
            fn()
        except AssertionError as error:
            failures += 1
            print(f"FAIL {name}: {error}")
        except Exception as error:  # keep the suite running to show all results
            failures += 1
            print(f"ERROR {name}: {type(error).__name__}: {error}")
        else:
            print(f"ok   {name}")
    print(f"\n{len(tests) - failures}/{len(tests)} passed")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
