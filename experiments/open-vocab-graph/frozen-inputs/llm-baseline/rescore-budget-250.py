#!/usr/bin/env python3
"""Re-score Epic 013's actual budget-250 run folder as the Epic 014 LLM baseline.

Reads (read-only):
  ../output/runs/budget-250/run-manifest.json
  ../output/runs/budget-250/validated-claims.json   (40 valid claims)
  ../output/runs/budget-250/invalid-claims.json     (24 invalid claims, reasons recorded)
  ../output/runs/budget-250/snapshots/AGENTS.md     (byte-identical to the frozen snapshot)

Writes: budget-250-rescore.json

The `support` field on each claim is a human judgment made against the frozen snapshot
under Epic 013 Task 4's definitions:
  supported  - the cited lines actually state the claimed relation
  weak       - the relation is a mild inference from the cited lines (e.g. a requirement
               inferred from a usage-example line)
  misleading - a wrong entity/action in a slot, a wrong object, or evidence that does
               not state the claim (includes unsupported-but-accepted claims)
Citation text equality is re-verified mechanically here (whitespace-normalized
word-sequence equality, the Epic 013 strict rule) against both the run snapshot and the
frozen snapshot, plus block-range containment. Judgments are keyed by the claim's index
in validated-claims.json; if that file changes, the script refuses to run.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
FROZEN = HERE.parent
RUN = FROZEN.parent / "output" / "runs" / "budget-250"

SUPPORT = [
    # index -> (support, reason)
    (0, "supported", "The line shows the cargo invocation itself; `cargo check --workspace` is a cargo command."),
    (1, "supported", "The line shows the cargo invocation itself."),
    (2, "supported", "The line shows the cargo invocation itself."),
    (3, "misleading", "Wrong action: the line says `--index  # index all sources` - indexing, not searching. This is the recorded index-as-search failure class."),
    (4, "supported", "The line literally says `# CLI unified hybrid search (all sources)`."),
    (5, "misleading", "Wrong action: the line says `# index Rust, TypeScript, ... source` - indexing, not searching."),
    (6, "supported", "The line attaches `(tree-sitter)` to the `--index-code` command."),
    (7, "misleading", "Wrong action: the line says `# index documents` - indexing, not searching."),
    (8, "misleading", "Wrong action: the line says `# index git log only` - indexing, not searching."),
    (9, "weak", "A `--repo .` requirement is a mild inference from a documented example invocation; the line does not state a requirement."),
    (10, "weak", "Same mild inference from the usage-example line (the `--source` flags are shown, not stated as required)."),
    (11, "weak", "Same mild inference from the usage-example line."),
    (12, "weak", "The flag is shown in a usage example whose comment (`GUI but skip startup auto-index`) describes an option, not a requirement; counted weak as the same inference class, but the optionality makes this the least defensible of the four."),
    (13, "misleading", "Wrong action: line 81 states the correctness-gate role, not a search over code sources. This is Epic 014's recorded 'wrong action assigned to a command' example."),
    (14, "supported", "Directory-tree indentation states main.rs sits under src/."),
    (15, "supported", "Directory-tree indentation states state.rs sits under src/."),
    (16, "supported", "Directory-tree indentation states git_log.rs sits under src/."),
    (17, "supported", "Directory-tree indentation states mod.rs sits under ui/ (nested under src/)."),
    (18, "supported", "Directory-tree indentation states adapter.rs sits under ui/."),
    (19, "supported", "Directory-tree indentation states container.rs sits under ui/."),
    (20, "supported", "Directory-tree indentation states render.rs sits under ui/."),
    (21, "supported", "Directory-tree indentation states lib.rs sits under the indexer src/."),
    (22, "supported", "Directory-tree indentation states db.rs sits under the indexer src/."),
    (23, "supported", "Directory-tree indentation states embed.rs sits under the indexer src/."),
    (24, "supported", "Directory-tree indentation states code.rs sits under the indexer src/."),
    (25, "supported", "Directory-tree indentation states documents.rs sits under the indexer src/."),
    (26, "misleading", "Wrong object: line 117's four-space indent places schema.sql inside `crates/daftprompt-indexer/src/` (alongside db.rs etc.), not directly in `crates/daftprompt-indexer/`."),
    (27, "supported", "Line 123 states `items_fts` is the FTS5 index kept in sync for `items`."),
    (28, "supported", "Line 124 literally says `vec_items` holds commit vectors."),
    (29, "supported", "Line 124 literally says `vec_code` holds code vectors."),
    (30, "supported", "Line 124 literally says `vec_documents` holds document vectors."),
    (31, "supported", "Line 126 literally says `code_files` tracks `mtime`."),
    (32, "supported", "Line 126 literally says `code_files` tracks `content_hash`."),
    (33, "supported", "Line 126 literally says `document_files` tracks `mtime`."),
    (34, "supported", "Line 126 literally says `document_files` tracks `content_hash`."),
    (35, "supported", "Line 127 literally says code.rs `owns extension-based language dispatch`."),
    (36, "misleading", "Evidence does not state the claim: line 133 is the comments-policy bullet that merely names db.rs as an example module retaining comments; schema ownership is stated on line 113, which lies in a different block."),
    (37, "misleading", "Evidence does not state the claim: line 133 names code.rs only as a comments-pattern example; the registry/router fact is stated on line 115, in a different block."),
    (38, "supported", "Line 138 states code.rs compiles each query once and reuses it - ownership of the compiled queries."),
    (39, "supported", "Line 142 states screenshot mode captures a frame via akar's `core.take_screenshot`."),
]

R_RECOVERY = [
    # (ref_id, recovered, status, note)
    ("R1", False, "attempted_rejected", "budget-250 b1 emitted `daftprompt (Tool) uses akar (Tool)` citing a partial line-7 quote; rejected by subject kind (seed `uses` is Command->Tool) and by strict evidence match. No valid akar-rendering claim exists (b8's L141 akar-owns claims were also rejected on shape)."),
    ("R2", False, "attempted_rejected", "budget-250 b1 emitted `daftprompt (Tool) uses gitoxide (Tool)`; rejected on the same kind + partial-line-quote grounds."),
    ("R3", False, "attempted_rejected", "budget-250 b1 emitted `daftprompt (Tool) uses SQLite FTS5` and `uses sqlite-vec` (components, not the hybrid indexer); rejected on kind + partial-line quote. No hybrid-indexer claim was accepted."),
    ("R4", False, "never_attempted", "The gate relation was never attempted. The only L81 claim is the misleading `cargo check --workspace searches all code sources in workspace` (wrong action)."),
    ("R5", False, "never_attempted", "L127 was never cited by any claim in this run; the five extension names appear only as the object of the misleading `--index-code searches Rust, TypeScript, ...` claim citing L28."),
    ("R6", False, "never_attempted", "No exclusion claim exists anywhere in the run (b6 emitted nothing for the second half of L127)."),
    ("R7", False, "attempted_rejected", "budget-250 b4 emitted `Graph Search (Tool) uses FTS5 / uses sqlite-vec` citing L92; rejected on subject kind (Tool vs Command). The RRF fusion fact itself was never named."),
    ("R8", False, "attempted_rejected", "budget-250 b3 emitted `graph build requires XIAOMI_API_KEY` and `graph search requires XIAOMI_API_KEY` - shape-valid under this seed (Command->Configuration) - but both were rejected solely because the citation quoted L45-46 without line 44's `Before` clause."),
    ("R9", False, "never_attempted", "No degradation-shaped claim exists in the run (L93 and L135 produced none)."),
]

QUESTION_COVERAGE = [
    ("Q1", "not_covered", "Every line-7 akar claim was rejected (b1) and every L141 akar-owns claim was rejected (b8); the only valid akar claim is `screenshot mode uses akar` (L142), a different fact."),
    ("Q2", "covered_defect", "A valid claim cites the exact expected line (81) but asserts the wrong action (`searches all code sources in workspace`), so the gate fact is mislabeled, not recovered."),
    ("Q3", "partial_defect", "Extension names appear only as the object of a wrong-action claim citing L28; the expected L127 evidence (extensions + exclusions) was never cited and exclusions were never emitted."),
    ("Q4", "not_covered", "Neither L93 nor L135 produced any claim; nothing degradation-shaped exists in the valid or invalid sets."),
    ("Q5", "correct_absence", "No license claim anywhere in the run; expected unknown."),
    ("Q6", "correct_absence", "No release-cadence claim anywhere in the run; expected unknown."),
]


def norm_words(text):
    return " ".join(text.split())


def citation_ok(citation, lines):
    start, end = citation["start_line"], citation["end_line"]
    if start < 1 or end > len(lines) or start > end:
        return False
    return norm_words("\n".join(lines[start - 1 : end])) == norm_words(citation["text"])


def main():
    manifest = json.loads((RUN / "run-manifest.json").read_text(encoding="utf-8"))
    claims = json.loads((RUN / "validated-claims.json").read_text(encoding="utf-8"))
    invalid = json.loads((RUN / "invalid-claims.json").read_text(encoding="utf-8"))
    run_snapshot = (RUN / "snapshots" / "AGENTS.md").read_text(encoding="utf-8").splitlines()
    frozen_snapshot = (FROZEN / "AGENTS.md").read_text(encoding="utf-8").splitlines()

    frozen_manifest = json.loads((FROZEN / "MANIFEST.json").read_text(encoding="utf-8"))

    # Guard: the folder must still be the run we scored, under the frozen inputs.
    assert manifest["word_budget"] == 250 and manifest["model"] == "mimo-v2.5-pro"
    assert manifest["seed_content_hash"] == "c776d7e579d8aed5" == frozen_manifest["artifacts"]["seed-vocabulary.json"]["xxh3_64"]
    assert manifest["reference_set_content_hash"] == "7dad11a105072baf" == frozen_manifest["artifacts"]["reference-set.json"]["xxh3_64"]
    assert manifest["snapshot_hashes"]["AGENTS.md"] == "29d01e36b77b6d33" == frozen_manifest["artifacts"]["AGENTS.md"]["xxh3_64"]
    assert manifest["counts"]["valid_claims"] == len(claims) == 40
    assert manifest["counts"]["invalid_claims"] == len(invalid) == 24
    assert len(SUPPORT) == len(claims), "judgment table out of sync with validated-claims.json"

    block_ranges = {b["block_id"]: (b["start_line"], b["end_line"]) for b in manifest["blocks"]}

    entries = []
    for idx, claim in enumerate(claims):
        support, reason = SUPPORT[idx][1], SUPPORT[idx][2]
        assert SUPPORT[idx][0] == idx
        in_run = citation_ok(claim["evidence"][0], run_snapshot)
        in_frozen = citation_ok(claim["evidence"][0], frozen_snapshot)
        b_start, b_end = block_ranges[claim["block_id"]]
        in_block = all(
            b_start <= c["start_line"] and c["end_line"] <= b_end for c in claim["evidence"]
        )
        entries.append(
            {
                "index": idx,
                "block_id": claim["block_id"],
                "subject": claim["subject"],
                "predicate": claim["predicate"],
                "object": claim["object"],
                "evidence": claim["evidence"],
                "support": support,
                "support_reason": reason,
                "citation_matches_run_snapshot": in_run,
                "citation_matches_frozen_snapshot": in_frozen,
                "citations_within_block": in_block,
            }
        )

    def count(kind):
        return sum(1 for e in entries if e["support"] == kind)

    supported = count("supported")
    weak = count("weak")
    misleading = count("misleading")
    citation_failures = [e["index"] for e in entries if not (e["citation_matches_run_snapshot"] and e["citation_matches_frozen_snapshot"] and e["citations_within_block"])]

    seen = set()
    duplicates = []
    for e in entries:
        key = (e["subject"]["name"], e["predicate"], e["object"]["name"])
        if key in seen:
            duplicates.append(key)
        seen.add(key)

    # Mechanical cross-run check: the same three search-source relations were asserted
    # once each here (unlike the stale 31-claim report column which reported b2/b4
    # duplication); record the fact rather than assume it.
    recovery = [
        {"id": rid, "recovered": rec, "status": status, "note": note}
        for rid, rec, status, note in R_RECOVERY
    ]
    questions = [
        {"id": qid, "coverage": cov, "note": note} for qid, cov, note in QUESTION_COVERAGE
    ]

    result = {
        "epic": "014-gliner25-graph-extraction-experiment",
        "task": "Task 1: LLM baseline re-score (chosen option: re-score the actual budget-250 folder under its seed)",
        "rescored_at_utc": "2026-09-28",
        "judged_by": "human review against the frozen snapshot; citation equality and block containment re-verified mechanically by rescore-budget-250.py",
        "baseline_run": {
            "directory": "experiments/open-vocab-graph/output/runs/budget-250/",
            "model": manifest["model"],
            "word_budget": 250,
            "blocks": manifest["block_count"],
            "seed_xxh3_64": manifest["seed_content_hash"],
            "reference_set_xxh3_64": manifest["reference_set_content_hash"],
            "snapshot_xxh3_64": manifest["snapshot_hashes"]["AGENTS.md"],
            "counts_from_manifest": manifest["counts"],
        },
        "stale_column_warning": "Epic 013's report column 'budget 250' (31 claims, seed 9bd9ffae74304ce2) does not describe this folder (40 claims, seed c776d7e579d8aed5) and is not used anywhere in this re-score.",
        "metrics": {
            "relations_emitted": manifest["counts"]["relations_emitted"],
            "valid_claims": len(claims),
            "invalid_claims": len(invalid),
            "supported_clean": supported,
            "supported_clean_pct": round(100.0 * supported / len(claims), 1),
            "supported_including_weak": supported + weak,
            "weak": weak,
            "misleading": misleading,
            "misleading_pct": round(100.0 * misleading / len(claims), 1),
            "exact_duplicate_claims": len(duplicates),
            "citation_failures": len(citation_failures),
            "model_errors": manifest["counts"]["model_errors"],
            "parse_errors": manifest["counts"]["parse_errors"],
        },
        "reference_relation_recovery": {
            "recovered": sum(1 for r in recovery if r["recovered"]),
            "total": 9,
            "detail": recovery,
        },
        "question_coverage": {"detail": questions},
        "misleading_breakdown": [
            "4x index commands mislabeled as `searches` (L25/L28/L31/L34) - indexing-as-searching",
            "1x wrong directory object: schema.sql is_part_of crates/daftprompt-indexer/ (tree places it under src/)",
            "2x `owns` claims citing L133 (comments policy), which does not state ownership (true lines 113/115 lie in other blocks)",
            "1x correctness gate mislabeled as `searches` (L81)",
        ],
        "claims": entries,
    }

    out = HERE / "budget-250-rescore.json"
    out.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {out}")
    print(
        f"valid={len(claims)} supported={supported} weak={weak} misleading={misleading} "
        f"duplicates={len(duplicates)} citation_failures={len(citation_failures)} "
        f"R1-R9 recovered={result['reference_relation_recovery']['recovered']}/9"
    )
    if citation_failures:
        print(f"UNEXPECTED citation failures at indices {citation_failures}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
