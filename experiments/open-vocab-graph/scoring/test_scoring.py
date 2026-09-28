"""Tests for Epic 014 Task 4 scoring (grounding, acceptance, reference set).

Runs with plain asserts and no model download or checkpoint load:

    .venv/bin/python experiments/open-vocab-graph/scoring/test_scoring.py

The function names are pytest-compatible, so `pytest` also works if installed.

The centerpiece regression (`test_line7_fact_is_grounded_and_cited_from_line_7`)
produces a real Task 3 run directory via the harness runner with an injected
fake engine emitting the `daftprompt (Application) uses akar (Tool)` fact on
snapshot line 7, then asserts the full grounding path accepts the claim with a
snapshot-derived citation from line 7 — while explicitly NOT asserting that a
model has recovered the fact (recovery requires human-reviewed support, which
stays "unreviewed" here).
"""
from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from harness import runner as harness_runner  # noqa: E402
from scoring import accept  # noqa: E402
from scoring import score as scoring_score  # noqa: E402
from scoring.grounding import (  # noqa: E402
    SnapshotGrounding,
    SnapshotMismatchError,
    text_match,
)
from scoring.reference import (  # noqa: E402
    ReferenceFact,
    canonical_predicate_for_hint,
    diagnose_fact,
    fact_shape_present,
    load_reference_set,
    name_match,
)
from scoring.report import ScorecardExistsError, render_markdown  # noqa: E402
from scoring.review import build_review_template  # noqa: E402
from scoring.taxonomy import (  # noqa: E402
    CATEGORY_ABSENT_SCHEMA_LABEL,
    CATEGORY_BOUNDARY_LOSS,
    CATEGORY_EVIDENCE_MISMATCH,
    CATEGORY_UNSUPPORTED_BUT_ACCEPTED,
    CATEGORY_WRONG_TYPE,
    STATUS_MATCHED_EVIDENCE_MISMATCH,
    STATUS_MATCHED_PENDING_REVIEW,
    STATUS_RECOVERED,
)
from schema import core  # noqa: E402

FROZEN_SNAPSHOT = HERE.parent / "frozen-inputs" / "AGENTS.md"
SCHEMA_ARTIFACT = HERE.parent / "schema" / "gliner25-schema.json"
REFERENCE_SET = HERE.parent / "frozen-inputs" / "reference-set.json"
EXPECTED_SNAPSHOT_SHA256 = (
    "a15dbdb069bcb58542f49bab0775004e62e792b52939aecd2c77886f33567423"
)

SNAPSHOT_TEXT = FROZEN_SNAPSHOT.read_text(encoding="utf-8")
SCHEMA = core.load_schema(SCHEMA_ARTIFACT)

LINE7_HEAD_NEEDLE = "daftprompt is a Rust app"
LINE141_HEAD_NEEDLE = "daftprompt owns application state"
LINE141_TAIL_NEEDLE = "rendered by akar**"


def _fact(head_text, head_kind, head_needle, tail_text, tail_kind, tail_needle, label):
    return {
        "head_text": head_text,
        "head_kind": head_kind,
        "head_needle": head_needle,
        "tail_text": tail_text,
        "tail_kind": tail_kind,
        "tail_needle": tail_needle,
        "label": label,
    }


LINE7_AKAR_FACT = _fact(
    "daftprompt", "Application", LINE7_HEAD_NEEDLE,
    "akar", "Tool", "akar** (a GPU", "uses",
)
LINE141_AKAR_FACT = _fact(
    "daftprompt", "Application", LINE141_HEAD_NEEDLE,
    "akar", "Tool", LINE141_TAIL_NEEDLE, "uses",
)
WRONG_TYPE_FACT = _fact(
    "daftprompt", "Tool", LINE7_HEAD_NEEDLE,
    "akar", "Tool", "akar** (a GPU", "uses",
)


class ScriptedJointIEEngine:
    """Stands in for a loaded checkpoint; emits scripted facts per chunk."""

    variant = "jointie"

    def __init__(self, facts):
        self.facts = facts

    def bind_schema(self, schema) -> None:
        pass

    def threshold_record(self) -> dict:
        return {"scripted_engine": True}

    def extract_chunk(self, text: str) -> dict:
        out = {"feasible": True, "entities": [], "relations": []}
        for index, fact in enumerate(self.facts):
            head = text.find(fact["head_needle"])
            tail = text.find(fact["tail_needle"])
            if head < 0 or tail < 0:
                continue
            # the needle locates the phrase; the emitted span covers exactly
            # the mention text inside it (as a real model span would)
            head_at = head + fact["head_needle"].index(fact["head_text"])
            tail_at = tail + fact["tail_needle"].index(fact["tail_text"])
            out["entities"].append(
                {"id": f"h{index}", "type": fact["head_kind"], "text": fact["head_text"],
                 "start": head_at, "end": head_at + len(fact["head_text"]), "confidence": 0.9}
            )
            out["entities"].append(
                {"id": f"t{index}", "type": fact["tail_kind"], "text": fact["tail_text"],
                 "start": tail_at, "end": tail_at + len(fact["tail_text"]), "confidence": 0.85}
            )
            out["relations"].append(
                {"type": fact["label"], "head": f"h{index}", "tail": f"t{index}",
                 "confidence": 0.8}
            )
        return out


def _run_harness(tmp: str, facts) -> Path:
    out_root = Path(tmp) / "gliner-runs"
    config = harness_runner.RunConfig(
        model_id="fake/scoring-test-model",
        variant="jointie",
        snapshot_path=FROZEN_SNAPSHOT,
        schema_path=SCHEMA_ARTIFACT,
        out_root=out_root,
        chunk_words=384,
        overlap_lines=2,
    )
    summary = harness_runner.run(config, engine=ScriptedJointIEEngine(facts))
    return Path(summary["run_directory"])


def _claim_by_predicate(scorecard, predicate):
    for claim in scorecard["candidates"]["claims"]:
        if claim["predicate"] == predicate:
            return claim
    return None


def _fact_diagnosis(scorecard, fact_id):
    for fact in scorecard["reference_relations"]["facts"]:
        if fact["id"] == fact_id:
            return fact
    return None


# ---------------------------------------------------------------------------
# grounding unit tests
# ---------------------------------------------------------------------------

def test_grounding_maps_spans_to_physical_lines():
    grounding = SnapshotGrounding(FROZEN_SNAPSHOT)
    grounding.verify(EXPECTED_SNAPSHOT_SHA256, 146)
    # locate the line-7 mention via its full phrase: "daftprompt" itself also
    # occurs on line 3, but R1's evidence mention is the line-7 one
    line7_start = SNAPSHOT_TEXT.index(LINE7_HEAD_NEEDLE)
    start = SNAPSHOT_TEXT.index("daftprompt", line7_start)
    span = grounding.ground(start, start + len("daftprompt"))
    assert span.resolved and not span.codes
    assert (span.start_line, span.end_line) == (7, 7)
    assert span.slice_text == "daftprompt"
    assert span.line_text == SNAPSHOT_TEXT.splitlines()[6]
    # a span crossing a line boundary maps to the covering line range;
    # line 8 is empty in the frozen snapshot, so the span is centered on the
    # newline that terminates line 7 (crossing into line 8 only)
    boundary = SNAPSHOT_TEXT.index("\n", start)
    crossing = grounding.ground(boundary - 2, boundary + 2)
    assert (crossing.start_line, crossing.end_line) == (7, 8)
    assert crossing.line_text == "\n".join(
        [SNAPSHOT_TEXT.splitlines()[6], SNAPSHOT_TEXT.splitlines()[7]]
    )


def test_grounding_flags_out_of_bounds_and_empty_spans():
    grounding = SnapshotGrounding(FROZEN_SNAPSHOT)
    span = grounding.ground(len(SNAPSHOT_TEXT) + 10, len(SNAPSHOT_TEXT) + 20)
    assert not span.resolved
    assert "span_out_of_bounds" in span.codes
    span = grounding.ground(5, 5)
    assert not span.resolved and "empty_span" in span.codes


def test_citation_text_is_always_snapshot_derived():
    grounding = SnapshotGrounding(FROZEN_SNAPSHOT)
    line7_start = SNAPSHOT_TEXT.index(LINE7_HEAD_NEEDLE)
    start = SNAPSHOT_TEXT.index("daftprompt", line7_start)
    span = grounding.ground(start, start + len("daftprompt"))
    # the mention text plays no role in what the citation says
    exact, normalized_equal = text_match(span, "totally different model text")
    assert not exact and not normalized_equal
    assert span.line_text == SNAPSHOT_TEXT.splitlines()[6], (
        "citation text comes from the snapshot regardless of the mention"
    )


# ---------------------------------------------------------------------------
# name/predicate matching unit tests
# ---------------------------------------------------------------------------

def test_name_match_tiers():
    assert name_match("daftprompt", "daftprompt") == "strong"
    assert name_match("akar", "akar (GPU UI component library on wgpu + glyphon + taffy)") == "strong"
    assert name_match("hybrid search indexer",
                      "hybrid search indexer (SQLite FTS5 + sqlite-vec embeddings)") == "strong"
    assert name_match("graph build", "graph build / graph search") == "strong"
    assert name_match(".rs, .ts, .tsx, .js, and .jsx", ".rs, .ts, .tsx, .js, .jsx") == "weak"
    assert name_match("node_modules/, vendor/, dist/, build/, .next/, coverage/, *.min.js",
                      "node_modules/ (also vendor/, dist/, build/, .next/, coverage/, *.min.js)") == "weak"
    assert name_match("gitoxide", "akar (GPU UI component library on wgpu + glyphon + taffy)") is None
    assert name_match("model download failure",
                      "search indexing (embedding model load failure)") is None


def test_predicate_hint_resolution_covers_all_reference_facts():
    refset = load_reference_set(REFERENCE_SET)
    expected = {
        "R1": "uses", "R2": "uses", "R3": "uses",
        "R4": "serves_as", "R5": "indexes", "R6": "excludes",
        "R7": "fuses", "R8": "requires", "R9": "degrades_to",
    }
    for fact in refset["facts"]:
        assert canonical_predicate_for_hint(SCHEMA, fact.predicate_hint) == expected[fact.id]
        shape_present, reasons = fact_shape_present(SCHEMA, fact)
        assert shape_present, f"{fact.id} must be expressible: {reasons}"


def test_fact_shape_detects_schema_gap():
    fact = ReferenceFact(
        id="RX",
        subject_name="daftprompt",
        subject_kind_hint="application",
        predicate_hint="publishes releases on a schedule",
        object_name="semver.org",
        object_kind_hint="tool",
        evidence=(),
        verification_note="",
    )
    shape_present, reasons = fact_shape_present(SCHEMA, fact)
    assert not shape_present and reasons
    diagnosis = diagnose_fact(
        fact,
        canonical_predicate_for_hint(SCHEMA, fact.predicate_hint),
        shape_present,
        reasons,
        matches=[],
        claims=[],
        entity_mentions=[],
    )
    assert diagnosis["status"] == "schema_gap"
    assert CATEGORY_ABSENT_SCHEMA_LABEL in diagnosis["categories"]


# ---------------------------------------------------------------------------
# the line-7 regression (harness run -> full grounding path)
# ---------------------------------------------------------------------------

def test_line7_fact_is_grounded_and_cited_from_line_7():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _run_harness(tmp, [LINE7_AKAR_FACT])
        scorecard = scoring_score.score_run(run_dir)

        claims = scorecard["candidates"]["claims"]
        assert len(claims) == 1
        claim = claims[0]
        assert claim["predicate"] == "uses"
        assert claim["subject"]["kind"] == "Application"
        assert claim["object"]["kind"] == "Tool"
        assert claim["accepted"] is True
        assert claim["citation_valid"] is True
        assert claim["support"] == "unreviewed", (
            "no model output may be treated as reviewed support"
        )
        # the citation is derived from the frozen snapshot, line 7 only
        citation = claim["citation"]
        assert citation["start_line"] == 7 and citation["end_line"] == 7
        assert citation["derived_from"] == "snapshot"
        assert citation["text"] == SNAPSHOT_TEXT.splitlines()[6]
        assert claim["subject"]["snapshot_slice"] == "daftprompt"
        assert claim["subject"]["citation_lines"] == [7, 7]
        assert claim["object"]["snapshot_slice"] == "akar"
        assert claim["grounding"]["chunk_locality_ok"] is True

        # every accepted citation resolves to the stored snapshot
        lines = SNAPSHOT_TEXT.splitlines()
        assert citation["text"] == "\n".join(
            lines[citation["start_line"] - 1 : citation["end_line"]]
        )

        # R1 matches structurally but is NOT reported recovered pre-review
        r1 = _fact_diagnosis(scorecard, "R1")
        assert r1["status"] == STATUS_MATCHED_PENDING_REVIEW
        assert r1["recovered"] is False
        assert r1["best_match"]["tier"] == "name"
        assert r1["best_match"]["evidence_overlap"] is True

        metrics_block = scorecard["metrics"]
        assert metrics_block["reference_relations"]["recovered"] == 0
        assert metrics_block["reference_relations"]["recovered_ids"] == []
        assert metrics_block["reference_relations"]["matched_pending_review"] == 1
        assert metrics_block["claims"]["valid"] == 1
        assert metrics_block["claims"]["unreviewed"] == 1
        assert metrics_block["claims"]["duplicates"] == 0

        # question coverage is evidence-based: Q1 partially covered (line 7 of 2)
        questions = {q["id"]: q for q in scorecard["questions"]}
        assert questions["Q1"]["coverage"] == "partial"
        assert questions["Q1"]["pending_support_review"] is True
        assert questions["Q5"]["coverage"] == "pending_review"
        assert questions["Q6"]["coverage"] == "pending_review"

        # the wrong-type category must not fire for a clean accepted claim
        assert CATEGORY_WRONG_TYPE not in scorecard["failure_categories"]


def test_reviewed_support_drives_recovery_and_misleading_category():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _run_harness(tmp, [LINE7_AKAR_FACT])
        first = scoring_score.score_run(run_dir)
        template = build_review_template(first)
        claim_id = first["candidates"]["claims"][0]["claim_id"]

        # support = supported -> R1 recovered
        review_path = Path(tmp) / "review-supported.json"
        template["claims"][0]["support"] = "supported"
        template["claims"][0]["reviewer"] = "test"
        review_path.write_text(json.dumps(template), encoding="utf-8")
        scorecard = scoring_score.score_run(run_dir, review_path=review_path)
        r1 = _fact_diagnosis(scorecard, "R1")
        assert r1["status"] == STATUS_RECOVERED and r1["recovered"] is True
        assert scorecard["metrics"]["reference_relations"]["recovered"] == 1
        assert scorecard["metrics"]["reference_relations"]["recovered_ids"] == ["R1"]
        assert scorecard["metrics"]["claims"]["supported"] == 1
        assert scorecard["review"]["claims_reviewed"] == 1

        # support = misleading -> unsupported-but-accepted, not recovered
        template["claims"][0]["support"] = "misleading"
        review_path = Path(tmp) / "review-misleading.json"
        review_path.write_text(json.dumps(template), encoding="utf-8")
        scorecard = scoring_score.score_run(run_dir, review_path=review_path)
        r1 = _fact_diagnosis(scorecard, "R1")
        assert r1["recovered"] is False
        assert CATEGORY_UNSUPPORTED_BUT_ACCEPTED in r1["categories"]
        assert scorecard["metrics"]["claims"]["misleading"] == 1
        assert CATEGORY_UNSUPPORTED_BUT_ACCEPTED in scorecard["failure_categories"]

        # an unrecognized support value is a warning, never silently applied
        template["claims"][0]["support"] = "definitely-fine"
        review_path = Path(tmp) / "review-bad.json"
        review_path.write_text(json.dumps(template), encoding="utf-8")
        scorecard = scoring_score.score_run(run_dir, review_path=review_path)
        assert scorecard["candidates"]["claims"][0]["support"] == "unreviewed"
        assert any("definitely-fine" in w for w in scorecard["review"]["warnings"])


# ---------------------------------------------------------------------------
# failure categories
# ---------------------------------------------------------------------------

def test_evidence_mismatch_when_match_cites_wrong_lines():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _run_harness(tmp, [LINE141_AKAR_FACT])
        scorecard = scoring_score.score_run(run_dir)
        r1 = _fact_diagnosis(scorecard, "R1")
        assert r1["status"] == STATUS_MATCHED_EVIDENCE_MISMATCH
        assert r1["best_match"]["evidence_overlap"] is False
        assert r1["best_match"]["citation_range"] == [141, 141]
        assert CATEGORY_EVIDENCE_MISMATCH in r1["categories"]
        assert CATEGORY_EVIDENCE_MISMATCH in scorecard["failure_categories"]
        assert scorecard["metrics"]["reference_relations"]["omissions"] >= 1


def test_wrong_type_candidate_recorded_and_diagnosed():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _run_harness(tmp, [WRONG_TYPE_FACT])
        scorecard = scoring_score.score_run(run_dir)
        claims = scorecard["candidates"]["claims"]
        assert len(claims) == 1
        claim = claims[0]
        assert claim["accepted"] is False
        assert "disallowed_pair" in claim["schema_verdict"]["codes"]
        assert CATEGORY_WRONG_TYPE in claim["categories"]
        # the rejected candidate stays in the report with its reasons
        assert scorecard["metrics"]["claims"]["rejected"] == 1
        r1 = _fact_diagnosis(scorecard, "R1")
        assert r1["status"] == "attempted_rejected"
        assert CATEGORY_WRONG_TYPE in r1["categories"]


def test_empty_run_records_omissions_and_uncovered_questions():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _run_harness(tmp, [])
        scorecard = scoring_score.score_run(run_dir)
        assert scorecard["candidates"]["claims"] == []
        refs = scorecard["metrics"]["reference_relations"]
        assert refs["recovered"] == 0 and refs["omissions"] == 9
        for fact in scorecard["reference_relations"]["facts"]:
            assert fact["status"] in ("missing_entity", "missing_relation")
        questions = {q["id"]: q for q in scorecard["questions"]}
        for supported in ("Q1", "Q2", "Q3", "Q4"):
            assert questions[supported]["coverage"] == "not_covered"
        for unknown in ("Q5", "Q6"):
            assert questions[unknown]["coverage"] == "pending_review"


def test_cross_chunk_locality_rejects_claim():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _write_synthetic_run(
            tmp,
            subject_span=_span_of("daftprompt"),
            object_span=_span_of_last("akar"),
            split_after_line=30,
        )
        scorecard = scoring_score.score_run(run_dir)
        claim = scorecard["candidates"]["claims"][0]
        assert claim["accepted"] is False
        assert "cross_chunk_locality" in claim["codes"]
        assert CATEGORY_BOUNDARY_LOSS in claim["categories"]
        assert claim["grounding"]["chunk_locality_ok"] is False


def test_model_text_cannot_supply_citation_text():
    with tempfile.TemporaryDirectory() as tmp:
        # object text claims more than its span covers: exact match fails and
        # the claim is rejected; the recorded citation text is still the
        # snapshot's own lines
        akar_start, akar_end = _span_of_last("akar")
        run_dir = _write_synthetic_run(
            tmp,
            subject_span=_span_of("daftprompt"),
            object_span=(akar_start, akar_end + 1),
            object_text_override="akar5",
            single_chunk=True,
        )
        scorecard = scoring_score.score_run(run_dir)
        claim = scorecard["candidates"]["claims"][0]
        assert claim["accepted"] is False
        assert "span_text_mismatch" in claim["codes"]
        assert CATEGORY_BOUNDARY_LOSS in claim["categories"]
        assert claim["object"]["snapshot_slice"] != claim["object"]["text"]
        lines = SNAPSHOT_TEXT.splitlines()
        citation = claim["citation"]
        assert citation["text"] == "\n".join(
            lines[citation["start_line"] - 1 : citation["end_line"]]
        )


def test_duplicate_surface_triples_counted_across_distinct_spans():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _run_harness(tmp, [LINE7_AKAR_FACT, LINE141_AKAR_FACT])
        scorecard = scoring_score.score_run(run_dir)
        assert len(scorecard["candidates"]["claims"]) == 2
        duplicates = scorecard["candidates"]["duplicates"]
        assert len(duplicates) == 1
        assert duplicates[0]["duplicate_count"] == 1
        assert scorecard["metrics"]["claims"]["duplicates"] == 1
        # Q1 needs both passages (lines 7 and 141): both are now cited
        questions = {q["id"]: q for q in scorecard["questions"]}
        assert questions["Q1"]["coverage"] == "covered"
        assert questions["Q1"]["pending_support_review"] is True


# ---------------------------------------------------------------------------
# snapshot / run integrity
# ---------------------------------------------------------------------------

def test_snapshot_mismatch_fails_closed():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _write_synthetic_run(
            tmp,
            subject_span=_span_of("daftprompt"),
            object_span=_span_of_last("akar"),
            manifest_snapshot_sha="0" * 64,
        )
        try:
            scoring_score.score_run(run_dir)
        except SnapshotMismatchError as error:
            assert "sha256" in str(error)
        else:
            raise AssertionError("scoring must refuse a run from another snapshot")


def test_missing_run_files_fail_with_clear_error():
    with tempfile.TemporaryDirectory() as tmp:
        empty = Path(tmp) / "not-a-run"
        empty.mkdir()
        try:
            scoring_score.score_run(empty)
        except accept.ScoringError as error:
            assert "manifest.json" in str(error)
        else:
            raise AssertionError("missing run files must raise ScoringError")


# ---------------------------------------------------------------------------
# report + CLI
# ---------------------------------------------------------------------------

def test_scorecard_writing_and_review_template():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _run_harness(tmp, [LINE7_AKAR_FACT])
        out_dir = Path(tmp) / "scoring"
        scoring_score.score_run(run_dir, write=True, out_dir=out_dir)
        assert (out_dir / "scorecard.json").is_file()
        assert (out_dir / "scorecard.md").is_file()
        assert (out_dir / "review.json").is_file() is False
        template_path = out_dir / "review-template.json"
        assert template_path.is_file()
        template = json.loads(template_path.read_text(encoding="utf-8"))
        assert template["claims"][0]["support"] == "unreviewed"

        # reviewer fills the template, drops it as review.json, re-score --force
        review = dict(template)
        review["claims"][0]["support"] = "supported"
        (out_dir / "review.json").write_text(json.dumps(review), encoding="utf-8")
        try:
            scoring_score.score_run(run_dir, write=True, out_dir=out_dir)
        except ScorecardExistsError:
            pass
        else:
            raise AssertionError("existing scorecard must not be overwritten silently")
        scorecard = scoring_score.score_run(
            run_dir, write=True, out_dir=out_dir, force=True
        )
        assert scorecard["review"]["source"].endswith("review.json")
        assert scorecard["metrics"]["reference_relations"]["recovered"] == 1
        markdown = render_markdown(scorecard)
        assert "R1" in markdown and "recovered" in markdown


def test_cli_scores_run_directory():
    import contextlib
    import io

    from scoring import cli

    with tempfile.TemporaryDirectory() as tmp:
        run_dir = _run_harness(tmp, [LINE7_AKAR_FACT])
        out_dir = Path(tmp) / "cli-scoring"
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            code = cli.main(["score", str(run_dir), "--out", str(out_dir)])
        assert code == 0
        payload = json.loads(buffer.getvalue())
        assert payload["metrics"]["claims"]["valid"] == 1
        assert (out_dir / "scorecard.json").is_file()
        assert (out_dir / "scorecard.md").is_file()

        # --no-write prints metrics without writing
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            code = cli.main(["score", str(run_dir), "--out", str(out_dir), "--no-write"])
        assert code == 0

        # missing run dir -> exit 1
        with contextlib.redirect_stderr(io.StringIO()):
            code = cli.main(["score", str(Path(tmp) / "missing-run"), "--no-write"])
        assert code == 1


# ---------------------------------------------------------------------------
# synthetic run-directory helper (Task 3 output format)
# ---------------------------------------------------------------------------

def _span_of(needle: str) -> tuple[int, int]:
    start = SNAPSHOT_TEXT.index(needle)
    return start, start + len(needle)


def _span_of_last(needle: str) -> tuple[int, int]:
    start = SNAPSHOT_TEXT.rindex(needle)
    return start, start + len(needle)


def _endpoint(span, kind, text, chunk_indexes):
    return {
        "kind": kind,
        "text": text,
        "start": span[0],
        "end": span[1],
        "confidence": 0.9,
        "occurrences": 1,
        "source_chunks": chunk_indexes,
        "span_verified": True,
    }


def _write_synthetic_run(
    tmp: str,
    subject_span,
    object_span,
    split_after_line: int | None = None,
    object_text_override: str | None = None,
    single_chunk: bool = False,
    manifest_snapshot_sha: str | None = None,
) -> Path:
    run_dir = Path(tmp) / "synthetic-run"
    (run_dir / "chunks").mkdir(parents=True)
    lines = SNAPSHOT_TEXT.splitlines(keepends=True)

    if single_chunk or split_after_line is None:
        chunk_spans = [(0, len(SNAPSHOT_TEXT))]
    else:
        split_char = sum(len(line) for line in lines[:split_after_line])
        chunk_spans = [(0, split_char), (split_char, len(SNAPSHOT_TEXT))]

    def chunk_for(span):
        return [
            index
            for index, (start, end) in enumerate(chunk_spans)
            if start <= span[0] and span[1] <= end
        ]

    subject_chunks = chunk_for(subject_span)
    object_chunks = chunk_for(object_span)
    object_text = object_text_override or SNAPSHOT_TEXT[object_span[0] : object_span[1]]

    relation = {
        "predicate": "uses",
        "model_label": "uses",
        "subject": _endpoint(subject_span, "Application", "daftprompt", subject_chunks),
        "object": _endpoint(object_span, "Tool", object_text, object_chunks),
        "confidence": 0.8,
        "occurrences": 1,
        "source_chunks": sorted(set(subject_chunks) | set(object_chunks)),
        "span_verified": True,
        "verdict": {"accepted": True, "codes": ["ok"], "details": []},
    }
    (run_dir / "relations.json").write_text(
        json.dumps(
            {
                "schema_id": SCHEMA.schema_id,
                "variant": "jointie",
                "count": 1,
                "accepted_count": 1,
                "rejected_count": 0,
                "relations": [relation],
            },
            indent=2,
        ),
        encoding="utf-8",
    )
    entities = [
        _endpoint(subject_span, "Application", "daftprompt", subject_chunks),
        _endpoint(object_span, "Tool", SNAPSHOT_TEXT[object_span[0] : object_span[1]], object_chunks),
    ]
    (run_dir / "entities.json").write_text(
        json.dumps(
            {"schema_id": SCHEMA.schema_id, "variant": "jointie",
             "count": len(entities), "entities": entities},
            indent=2,
        ),
        encoding="utf-8",
    )
    for index, (start, end) in enumerate(chunk_spans):
        (run_dir / "chunks" / f"chunk-{index:03d}.json").write_text(
            json.dumps(
                {
                    "chunk": {
                        "index": index,
                        "start_char": start,
                        "end_char": end,
                        "start_line": 1,
                        "end_line": 146,
                        "word_count": len(SNAPSHOT_TEXT[start:end].split()),
                        "overlap_lines": [],
                        "oversized": False,
                        "text": SNAPSHOT_TEXT[start:end],
                    },
                    "status": "ok",
                    "raw_response": None,
                    "entity_count": 0,
                    "relation_count": 0,
                    "dropped": [],
                    "notes": [],
                    "error": None,
                    "elapsed_seconds": 0.01,
                },
                indent=2,
            ),
            encoding="utf-8",
        )
    manifest = {
        "model": {"id": "synthetic/scoring-test"},
        "variant": "jointie",
        "status": "completed",
        "started_utc": "2026-09-28T00:00:00Z",
        "finished_utc": "2026-09-28T00:00:01Z",
        "elapsed_seconds": 1.0,
        "snapshot": {
            "sha256": manifest_snapshot_sha or EXPECTED_SNAPSHOT_SHA256,
            "line_count": 146,
        },
        "schema": {"artifact_sha256": "synthetic"},
        "counts": {},
    }
    (run_dir / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return run_dir


# ---------------------------------------------------------------------------

def main() -> int:
    tests = [
        (name, fn)
        for name, fn in sorted(globals().items())
        if name.startswith("test_") and callable(fn)
    ]
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
