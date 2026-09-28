#!/usr/bin/env python3
"""Verify the Epic 014 frozen experiment inputs BEFORE any new model output is inspected.

Stdlib only (Python >= 3.9). Run from anywhere:

    python3 frozen-inputs/check_inputs.py

Checks:
  1. sha256 of every frozen artifact against MANIFEST.json.
  2. AGENTS.md line 7 expectation (the `daftprompt uses akar` evidence line).
  3. Every reference-set R1-R9 expected evidence passage matches the frozen snapshot
     (whitespace-normalized word-sequence equality, the same strict rule the Epic 013
     harness used - no fuzzy matching).
  4. Every repeated entity occurs on its recorded lines (case-sensitive substring).
  5. The six questions' expected supporting passages resolve to lines carrying the
     expected content (explicit expectations table below; Q5/Q6 must have no passages).
  6. Every test-inputs.json quote matches the frozen snapshot at its recorded lines.
  7. The frozen seed's content fingerprint matches MANIFEST.json (guards against a
     seed swap; the other Epic 013 seed has different kinds/predicates).

Exit code 0 = all checks pass.
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def norm_words(text):
    return " ".join(text.split())


def snapshot_lines(snapshot_path):
    return snapshot_path.read_text(encoding="utf-8").splitlines()


def lines_word_match(lines, start, end, expected_text):
    if start < 1 or end > len(lines) or start > end:
        return False, f"line range {start}-{end} outside 1-{len(lines)}"
    actual = "\n".join(lines[start - 1 : end])
    if norm_words(actual) != norm_words(expected_text):
        return False, f"text mismatch at lines {start}-{end}"
    return True, "ok"


def sha256_of(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    failures = []
    checks = 0

    def check(condition, description, detail=""):
        nonlocal checks
        checks += 1
        if not condition:
            failures.append(f"{description}: {detail}")
            print(f"FAIL  {description}: {detail}")
        else:
            print(f"pass  {description}")

    manifest = json.loads((HERE / "MANIFEST.json").read_text(encoding="utf-8"))

    # 1. Artifact hashes.
    for name, meta in manifest["artifacts"].items():
        path = HERE / name
        check(path.is_file(), f"{name} exists")
        if path.is_file():
            actual = sha256_of(path)
            check(
                actual == meta["sha256"],
                f"{name} sha256 matches manifest",
                f"expected {meta['sha256']}, got {actual}",
            )

    snapshot = HERE / "AGENTS.md"
    lines = snapshot_lines(snapshot)
    refset = json.loads((HERE / "reference-set.json").read_text(encoding="utf-8"))
    seed = json.loads((HERE / "seed-vocabulary.json").read_text(encoding="utf-8"))

    # 2. AGENTS.md line 7.
    line7_expected_prefix = (
        "daftprompt is a Rust app for exploring text repositories (git repos, document "
        "folders) via an infinite-canvas, card-based UI. It uses **akar** (a GPU UI "
        "component library on wgpu + glyphon + taffy) for rendering, combined with a git "
        "commit reader (gitoxide) and a hybrid search indexer (SQLite FTS5 + "
        "sqlite-vec embeddings)."
    )
    ok, detail = lines_word_match(lines, 7, 7, line7_expected_prefix)
    check(ok, "AGENTS.md line 7 is the daftprompt-uses-akar evidence line", detail)
    check(len(lines) == 146, "AGENTS.md line count is 146", f"got {len(lines)}")

    # 3. Reference set R1-R9 evidence passages.
    for rel in refset["supported_relations"]:
        for i, ev in enumerate(rel["expected_evidence"]):
            ok, detail = lines_word_match(
                lines, ev["start_line"], ev["end_line"], ev["text"]
            )
            check(
                ok,
                f"{rel['id']} expected evidence [{i}] matches snapshot",
                detail,
            )
    check(
        [rel["id"] for rel in refset["supported_relations"]] == [f"R{i}" for i in range(1, 10)],
        "reference set contains exactly R1-R9",
    )

    # 4. Repeated entities.
    for ent in refset["repeated_entities"]:
        missing = [n for n in ent["occurrence_lines"] if ent["name"] not in lines[n - 1]]
        check(
            not missing,
            f"repeated entity '{ent['name']}' occurs on all recorded lines",
            f"missing on lines {missing}",
        )

    # 5. Questions.
    questions = {q["id"]: q for q in refset["questions"]}
    check(set(questions) == {f"Q{i}" for i in range(1, 7)}, "reference set has Q1-Q6")
    question_expectations = {
        "Q1": {(7, 7): "It uses **akar**", (141, 141): "UI is rendered by akar"},
        "Q2": {(81, 81): "canonical correctness gate"},
        "Q3": {(127, 127): ".rs`, `.ts`, `.tsx`, `.js`, and `.jsx`"},
        "Q4": {(135, 135): "model download failure → FTS5-only search"},
        "Q5": {},
        "Q6": {},
    }
    expected_answers = {
        "Q1": "supported",
        "Q2": "supported",
        "Q3": "supported",
        "Q4": "supported",
        "Q5": "unknown",
        "Q6": "unknown",
    }
    for qid in sorted(questions):
        q = questions[qid]
        check(
            q["expected"] == expected_answers[qid],
            f"{qid} expected answer is {expected_answers[qid]}",
            f"got {q['expected']}",
        )
        passages = [tuple(p) for p in q["expected_supporting_passages"]]
        check(
            passages == sorted(question_expectations[qid]),
            f"{qid} passages are {sorted(question_expectations[qid])}",
            f"got {passages}",
        )
        for (start, end), needle in sorted(question_expectations[qid].items()):
            if start == end:
                check(
                    needle in lines[start - 1],
                    f"{qid} line {start} contains expected content",
                    needle,
                )

    # 6. Test inputs.
    test_inputs_path = HERE / "test-inputs.json"
    if test_inputs_path.is_file():
        ti = json.loads(test_inputs_path.read_text(encoding="utf-8"))
        n_quotes = 0
        for case in ti["cases"]:
            for inp in case["inputs"]:
                ok, detail = lines_word_match(
                    lines, inp["start_line"], inp["end_line"], inp["text"]
                )
                n_quotes += 1
                expect_match = inp.get("expect_quote_match", True)
                label = f"{case['id']}/{inp.get('label', 'input')}"
                if expect_match:
                    check(ok, f"{label} quote matches snapshot", detail)
                else:
                    # Recorded misquote (e.g. a partial-line citation that a previous
                    # run emitted): the checker asserts it does NOT match the snapshot
                    # lines, i.e. the strict validator must reject it.
                    check(
                        not ok,
                        f"{label} recorded misquote is rejected by strict matching",
                        "unexpectedly matched - misquote no longer represents the failure",
                    )
        check(n_quotes > 0, "test inputs contain at least one snapshot quote")
        line7_cases = [
            c
            for c in ti["cases"]
            if any(
                i["start_line"] <= 7 <= i["end_line"] for i in c["inputs"]
            )
        ]
        check(line7_cases, "test inputs include the AGENTS.md line 7 case")
        refs_covered = set()
        for case in ti["cases"]:
            refs_covered.update(case.get("reference_facts", []))
        check(
            refs_covered == {f"R{i}" for i in range(1, 10)},
            "test inputs cover all nine reference facts",
            f"covered {sorted(refs_covered)}",
        )
    else:
        check(False, "test-inputs.json exists", "missing")

    # 7. Seed fingerprint.
    fp = manifest["artifacts"]["seed-vocabulary.json"]["content_fingerprint"]
    check(
        [k["kind"] for k in seed["entity_kinds"]] == fp["entity_kinds"],
        "seed entity kinds match manifest fingerprint",
    )
    actual_preds = [
        {
            "predicate": p["predicate"],
            "subject_kind": p["subject_kind"],
            "object_kind": p["object_kind"],
        }
        for p in seed["predicates"]
    ]
    check(
        actual_preds == fp["predicates"],
        "seed predicates (with shapes) match manifest fingerprint",
        f"got {actual_preds}",
    )
    check(
        len(seed["example_relations"]) == fp["example_relations"],
        "seed example relation count matches manifest",
    )
    check(
        any(p["predicate"] == "uses" and p["subject_kind"] == "Command" and p["object_kind"] == "Tool" for p in seed["predicates"]),
        "frozen seed still has the narrow uses(Command, Tool) shape the baseline failed under",
    )

    print()
    if failures:
        print(f"{len(failures)} check(s) FAILED out of {checks}.")
        return 1
    print(f"All {checks} checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
