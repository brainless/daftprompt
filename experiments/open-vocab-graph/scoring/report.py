"""Scorecard writing for Epic 014 scoring: JSON, Markdown, review template.

Writes into a scoring directory (default: `<run_dir>/scoring/`, which lives
under the git-ignored `gliner-runs/` tree — never into `frozen-inputs/` or
Epic 013's `output/`). Existing scorecards are never overwritten without
`--force` so re-scoring after review passes stays explicit. The review
template is written only when no review file exists yet, so reviewer input
is never clobbered by a re-score.
"""
from __future__ import annotations

import json
from pathlib import Path

from . import REVIEW_FILE_NAME, REVIEW_TEMPLATE_NAME
from .review import build_review_template
from .taxonomy import (
    CATEGORY_EVIDENCE_MISMATCH,
    CATEGORY_UNSUPPORTED_BUT_ACCEPTED,
    SUPPORT_MISLEADING,
    SUPPORT_SUPPORTED,
    SUPPORT_UNREVIEWED,
    STATUS_RECOVERED,
    SUPPORT_WEAK,
)

SCORECARD_JSON = "scorecard.json"
SCORECARD_MD = "scorecard.md"


class ScorecardExistsError(RuntimeError):
    """Refusing to overwrite an existing scorecard without --force."""


def write_scorecard(scorecard: dict, out_dir: Path, force: bool = False,
                    review_path: Path | None = None) -> Path:
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    json_path = out_dir / SCORECARD_JSON
    md_path = out_dir / SCORECARD_MD
    if (json_path.exists() or md_path.exists()) and not force:
        raise ScorecardExistsError(
            f"scorecard already exists in {out_dir}; pass force/--force to "
            "overwrite (re-scoring after a review pass should be explicit)"
        )
    _write_json(json_path, scorecard)
    md_path.write_text(render_markdown(scorecard), encoding="utf-8")
    if review_path is None:
        template_path = out_dir / REVIEW_TEMPLATE_NAME
        if not template_path.exists():
            _write_json(template_path, build_review_template(scorecard))
    return out_dir


def _write_json(path: Path, payload) -> None:
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2, sort_keys=True, ensure_ascii=False)
        handle.write("\n")


def render_markdown(scorecard: dict) -> str:
    m = scorecard["metrics"]
    claims = m["claims"]
    refs = m["reference_relations"]
    lines: list[str] = []
    lines.append("# Epic 014 Task 4 scorecard")
    lines.append("")
    lines.append(f"- Run: `{scorecard['run_directory']}`")
    lines.append(
        f"- Model: `{scorecard['run_manifest'].get('model_id')}` "
        f"(variant `{scorecard['run_manifest'].get('variant')}`, "
        f"status `{scorecard['run_manifest'].get('status')}`)"
    )
    lines.append(
        f"- Snapshot: `{scorecard['snapshot']['path']}` "
        f"(sha256 `{scorecard['snapshot']['sha256'][:16]}…`, "
        f"{scorecard['snapshot']['line_count']} lines)"
    )
    lines.append(
        f"- Generated: {scorecard['generated_utc']} · scoring took "
        f"{scorecard['runtime']['scoring_elapsed_seconds']:.3f}s"
    )
    peak = scorecard["runtime"]["run_peak_memory_bytes"]
    lines.append(
        f"- Run elapsed: {scorecard['runtime']['run_elapsed_seconds']}s · "
        f"peak memory: {peak if peak is not None else 'not recorded'} "
        f"({scorecard['runtime']['run_peak_memory_note']})"
    )
    lines.append("")
    lines.append(
        "Citation validity is **deterministic** (derived from the frozen "
        "snapshot); semantic support is **human-reviewed** and defaults to "
        f"`{SUPPORT_UNREVIEWED}`."
    )
    lines.append("")

    lines.append("## Claims")
    lines.append("")
    lines.append("| Metric | Value |")
    lines.append("|---|---|")
    lines.append(f"| Emitted candidates | {claims['emitted_candidates']} |")
    lines.append(f"| Accepted | {claims['accepted']} |")
    lines.append(f"| Rejected (recorded) | {claims['rejected']} |")
    lines.append(f"| Valid (accepted + citation resolves) | {claims['valid']} |")
    lines.append(f"| Supported (reviewed) | {claims['supported']} |")
    lines.append(f"| Weak (reviewed) | {claims['weak']} |")
    lines.append(f"| Misleading (reviewed) | {claims['misleading']} |")
    lines.append(f"| Unreviewed | {claims['unreviewed']} |")
    lines.append(f"| Duplicates | {claims['duplicates']} |")
    lines.append("")
    lines.append("### Emitted candidates (every candidate, including rejections)")
    lines.append("")
    lines.append("| Claim | Accepted | Citation valid | Support | Codes |")
    lines.append("|---|---|---|---|---|")
    for claim in scorecard["candidates"]["claims"]:
        text = claim["claim"].replace("|", "\\|")
        codes = ", ".join(claim.get("codes") or []) or "-"
        lines.append(
            f"| {claim['claim_id']} {text} "
            f"| {'yes' if claim['accepted'] else 'no'} "
            f"| {'yes' if claim['citation_valid'] else 'no'} "
            f"| {claim['support']} | {codes} |"
        )
    lines.append("")

    lines.append("## Reference relations (R1-R9)")
    lines.append("")
    lines.append(
        f"Recovered **{refs['recovered']}/{refs['total']}** · "
        f"matched pending review {refs['matched_pending_review']} · "
        f"omissions {refs['omissions']} · schema gaps {refs['schema_gaps']}"
    )
    lines.append("")
    lines.append("| Fact | Status | Recovered | Note |")
    lines.append("|---|---|---|---|")
    for fact in scorecard["reference_relations"]["facts"]:
        note = str(fact.get("note", "")).replace("|", "\\|")
        lines.append(
            f"| {fact['id']} | {fact['status']} | {'yes' if fact['recovered'] else 'no'} | {note} |"
        )
    lines.append("")

    lines.append("## Question coverage (evidence-based)")
    lines.append("")
    lines.append("| Question | Expected | Coverage | Pending review | Note |")
    lines.append("|---|---|---|---|---|")
    for question in scorecard["questions"]:
        note = str(question.get("note", "")).replace("|", "\\|")
        lines.append(
            f"| {question['id']} | {question['expected']} | {question['coverage']} "
            f"| {'yes' if question['pending_support_review'] else 'no'} | {note} |"
        )
    lines.append("")

    entities = m["entities"]
    lines.append("## Entities (repeated-entity metrics)")
    lines.append("")
    recall = entities["repeated_entity_recall"]
    precision = entities["repeated_entity_precision"]
    lines.append(
        f"- Recall: {recall['matched']}/{recall['total']}"
        + (f" (missing: {', '.join(recall['missing'])})" if recall["missing"] else "")
    )
    lines.append(
        f"- Precision (reference-supported): "
        f"{precision['on_recorded_lines']}/{precision['matching_mentions']} "
        "mentions on recorded occurrence lines"
    )
    lines.append("")

    lines.append("## Failure taxonomy")
    lines.append("")
    if scorecard["failure_categories"]:
        lines.append("| Category | Claims | Dropped | Fact diagnoses | Total |")
        lines.append("|---|---|---|---|---|")
        for category, counts in sorted(scorecard["failure_categories"].items()):
            lines.append(
                f"| {category} | {counts['claim_rejections']} | "
                f"{counts['dropped_candidates']} | {counts['fact_diagnoses']} | "
                f"{counts['total']} |"
            )
    else:
        lines.append("No failure-category occurrences.")
    lines.append("")

    dropped = scorecard["candidates"]["dropped"]
    if dropped:
        lines.append(f"## Dropped candidates ({len(dropped)})")
        lines.append("")
        for record in dropped:
            reason = str(record.get("reason", "")).replace("\n", " ")
            lines.append(f"- chunk {record['chunk_index']}: {reason}")
        lines.append("")

    lines.append(
        f"Citation-validity vs support: {claims['valid']} citation-valid "
        f"claims, of which {claims['supported']} reviewed supported, "
        f"{claims['misleading']} reviewed misleading "
        f"({CATEGORY_UNSUPPORTED_BUT_ACCEPTED}), {claims['weak']} weak, "
        f"{claims['unreviewed']} awaiting review. Exact-citation "
        f"{CATEGORY_EVIDENCE_MISMATCH} cases are listed per fact above."
    )
    lines.append("")
    lines.append(
        f"Machine-readable data: `{SCORECARD_JSON}` · review file: "
        f"`{REVIEW_FILE_NAME}` (support values: "
        f"{SUPPORT_SUPPORTED} / {SUPPORT_WEAK} / {SUPPORT_MISLEADING})"
    )
    lines.append("")
    return "\n".join(lines)
