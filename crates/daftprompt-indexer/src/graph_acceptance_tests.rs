use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{anyhow, Result};

use super::{
    build, derive_positive, init_schema, model, open_database, prepare_search, render_answer,
    render_text, resolve_prepared, resolve_sources, SearchAnswer, SelectedSource, SourcePassage,
};
use crate::IndexerConfig;
use model::{
    ClaimEntity, ClaimEvidence, ClaimObject, ClaimScope, ClaimStatus, ExtractedClaim,
    ExtractedEnumeration, ExtractionRequest, ExtractionResponse, GraphModel, InterpretationRequest,
    InterpretationResponse, Polarity, QuestionIntent,
};

struct RepairedPrdFixture {
    retries: AtomicUsize,
}

impl GraphModel for RepairedPrdFixture {
    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse> {
        Ok(ExtractionResponse {
            claims: vec![claim(
                entity("project", "Example"),
                "searches_index",
                entity("index", "CodeIndex"),
                passage_evidence(&request.passages[0]),
            )],
            enumerations: vec![],
        })
    }

    fn retry_extraction(
        &self,
        _request: &ExtractionRequest,
        validation_error: &str,
    ) -> Option<Result<ExtractionResponse>> {
        assert!(validation_error.contains("invalid subject entity kind \"project\""));
        self.retries.fetch_add(1, Ordering::SeqCst);
        Some(Ok(ExtractionResponse::default()))
    }

    fn interpret_question(&self, _request: &InterpretationRequest) -> Result<InterpretationResponse> {
        unreachable!()
    }

    fn model_name(&self) -> &str {
        "repaired-prd-fixture"
    }
}

#[test]
fn unsupported_prd_claim_gets_one_correction_before_build_publishes() -> Result<()> {
    let repo = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    fs::write(repo.path().join("PRD.md"), "# Example\nA general project description.\n")?;
    let sources = resolve_sources(repo.path(), &["PRD.md".to_owned()])?;
    let config = IndexerConfig {
        cache_dir: Some(cache.path().to_path_buf()),
        ..IndexerConfig::default()
    };
    let db = open_database(repo.path(), &config)?;
    init_schema(&db)?;
    let fixture = RepairedPrdFixture { retries: AtomicUsize::new(0) };
    build(&db, repo.path(), &sources, &fixture, "fixture-v1")?;
    assert_eq!(fixture.retries.load(Ordering::SeqCst), 1);
    let completed: i64 = db.query_row(
        "SELECT COUNT(*) FROM graph_builds WHERE state='completed'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(completed, 1);
    Ok(())
}

#[derive(Debug)]
struct RepositoryAcceptanceFixture;

impl GraphModel for RepositoryAcceptanceFixture {
    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse> {
        let mut response = ExtractionResponse::default();
        if request.source_path == "README.md" {
            let supported = request
                .passages
                .iter()
                .find(|passage| passage.text.contains("All supported code-search languages"))
                .ok_or_else(|| {
                    anyhow!("documented exhaustive language and dialect list is missing")
                })?;
            let evidence = passage_evidence(supported);
            let index = entity("index", "CodeIndex");
            let languages = ["Rust", "TypeScript", "JavaScript"];
            let dialects = ["TSX", "JSX"];
            for language in languages {
                response.claims.push(claim(
                    index.clone(),
                    "indexes_language",
                    entity("language", language),
                    evidence.clone(),
                ));
            }
            let language_members = (0..languages.len()).collect();
            response.enumerations.push(ExtractedEnumeration {
                subject: index.clone(),
                predicate: "indexes_language".into(),
                scope: ClaimScope::Project,
                exhaustive: true,
                evidence: evidence.clone(),
                member_claim_indexes: language_members,
            });
            let dialect_start = response.claims.len();
            for dialect in dialects {
                response.claims.push(claim(
                    index.clone(),
                    "indexes_dialect",
                    entity("dialect", dialect),
                    evidence.clone(),
                ));
            }
            response.enumerations.push(ExtractedEnumeration {
                subject: index,
                predicate: "indexes_dialect".into(),
                scope: ClaimScope::Project,
                exhaustive: true,
                evidence,
                member_claim_indexes: (dialect_start..response.claims.len()).collect(),
            });

            let search = request
                .passages
                .iter()
                .find(|passage| {
                    passage
                        .text
                        .contains("global search box searches all five languages")
                })
                .ok_or_else(|| anyhow!("documented code-search relationship is missing"))?;
            response.claims.push(claim(
                entity("feature", "CodeSearch"),
                "searches_index",
                entity("index", "CodeIndex"),
                passage_evidence(search),
            ));
        } else if request.source_path == "AGENTS.md" {
            if let Some(parser) = request
                .passages
                .iter()
                .find(|passage| passage.text.contains("Tree-sitter registry/router"))
            {
                response.claims.push(claim(
                    entity("index", "CodeIndex"),
                    "uses_parser",
                    entity("parser", "Tree-sitter"),
                    passage_evidence(parser),
                ));
            }
        }
        Ok(response)
    }

    fn interpret_question(
        &self,
        request: &InterpretationRequest,
    ) -> Result<InterpretationResponse> {
        let question = request.question.to_ascii_lowercase();
        if question.contains("tree-sitter") {
            Ok(interpret("parser", "Tree-sitter", "uses_parser"))
        } else if question.contains("python") {
            Ok(interpret_code_search_language("Python"))
        } else if question.contains("lua") {
            Ok(interpret_code_search_language("Lua"))
        } else {
            Ok(interpret_code_search_language_list())
        }
    }

    fn model_name(&self) -> &str {
        "repository-acceptance-fixture-v1"
    }
}

fn entity(kind: &str, name: &str) -> ClaimEntity {
    ClaimEntity {
        kind: kind.into(),
        name: name.into(),
        aliases: Vec::new(),
    }
}

fn passage_evidence(passage: &SourcePassage) -> ClaimEvidence {
    ClaimEvidence {
        passage_id: passage.id.clone(),
        start_line: passage.start_line,
        end_line: passage.end_line,
        text: passage.text.clone(),
    }
}

fn claim(
    subject: ClaimEntity,
    predicate: &str,
    object: ClaimEntity,
    evidence: ClaimEvidence,
) -> ExtractedClaim {
    ExtractedClaim {
        subject,
        predicate: predicate.into(),
        object: ClaimObject::Entity(object),
        polarity: Polarity::Positive,
        status: ClaimStatus::Current,
        scope: ClaimScope::Project,
        evidence: vec![evidence],
    }
}

fn interpret(kind: &str, name: &str, predicate: &str) -> InterpretationResponse {
    InterpretationResponse {
        intent: QuestionIntent::DirectRelation,
        entities: vec![entity(kind, name)],
        predicates: vec![predicate.into()],
    }
}

fn interpret_code_search_language(name: &str) -> InterpretationResponse {
    InterpretationResponse {
        intent: QuestionIntent::CodeSearchLanguage,
        entities: vec![entity("feature", "CodeSearch"), entity("language", name)],
        predicates: vec!["searches_index".into(), "indexes_language".into()],
    }
}

fn interpret_code_search_language_list() -> InterpretationResponse {
    InterpretationResponse {
        intent: QuestionIntent::ListCodeSearchLanguages,
        entities: vec![entity("feature", "CodeSearch")],
        predicates: vec!["searches_index".into(), "indexes_language".into(), "indexes_dialect".into()],
    }
}

fn copy_documentation_repo(repo: &Path) -> Result<()> {
    for name in ["README.md", "DEVELOP.md", "AGENTS.md"] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../")
                .join(name),
            repo.join(name),
        )?;
    }
    let source_epics = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../epics");
    let target_epics = repo.join("epics");
    fs::create_dir_all(&target_epics)?;
    for entry in fs::read_dir(source_epics)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|ext| ext == "md") {
            fs::copy(entry.path(), target_epics.join(entry.file_name()))?;
        }
    }
    fs::write(repo.join(".gitignore"), "/.daftprompt/graph-search/\n")?;
    Ok(())
}

fn assert_citations_from_snapshot(repo: &Path, answer: &SearchAnswer) -> Result<()> {
    assert!(
        !answer.citations.is_empty(),
        "{} must cite stored evidence",
        answer.answer
    );
    for citation in &answer.citations {
        let snapshot = fs::read_to_string(repo.join(&citation.source_path))?;
        let lines = snapshot.split_inclusive('\n').collect::<Vec<_>>();
        let cited = lines[(citation.start_line - 1) as usize..citation.end_line as usize].concat();
        assert_eq!(
            citation.text, cited,
            "citation must match the selected source snapshot"
        );
        assert_eq!(
            citation.citation,
            format!(
                "{}:{}-{}",
                citation.source_path, citation.start_line, citation.end_line
            )
        );
    }
    Ok(())
}

fn ask(
    db: &rusqlite::Connection,
    fixture: &RepositoryAcceptanceFixture,
    repo: &Path,
    question: &str,
) -> Result<SearchAnswer> {
    let prepared = prepare_search(db, question, fixture, 20)?;
    let resolution = resolve_prepared(db, &prepared)?;
    let answer = render_answer(db, &prepared, resolution)?;
    assert_citations_from_snapshot(repo, &answer)?;
    Ok(answer)
}

#[test]
fn documented_repository_build_answers_acceptance_questions_from_snapshots() -> Result<()> {
    // Keep all fixture state under temporary paths: no cache DB or query log is
    // written into this checkout, and no network model is called.
    let repo = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    copy_documentation_repo(repo.path())?;
    let source_specs = [
        "DEVELOP.md".to_owned(),
        "README.md".to_owned(),
        "AGENTS.md".to_owned(),
        "epics/*.md".to_owned(),
    ];
    let sources: Vec<SelectedSource> = resolve_sources(repo.path(), &source_specs)?;
    assert!(sources.iter().any(|source| source.path == "DEVELOP.md"));
    assert!(sources.iter().any(|source| source.path == "README.md"));
    assert!(sources.iter().any(|source| source.path == "AGENTS.md"));
    assert!(sources
        .iter()
        .any(|source| source.path == "epics/011-document-provenance-graph-cli.md"));
    assert!(
        sources.windows(2).all(|pair| pair[0].path <= pair[1].path),
        "glob selection is sorted"
    );

    let config = IndexerConfig {
        cache_dir: Some(cache.path().to_path_buf()),
        ..IndexerConfig::default()
    };
    let db = open_database(repo.path(), &config)?;
    init_schema(&db)?;
    build(
        &db,
        repo.path(),
        &sources,
        &RepositoryAcceptanceFixture,
        "acceptance-fixture-v1",
    )?;
    assert!(crate::repository_db_path(repo.path(), &config)?.exists());

    let languages = ask(
        &db,
        &RepositoryAcceptanceFixture,
        repo.path(),
        "Which languages can code search find?",
    )?;
    assert_eq!(languages.outcome, "yes");
    assert_eq!(languages.claim_ids.len(), 6, "five list members and the search edge are required");
    for name in ["Rust", "TypeScript", "JavaScript", "TSX", "JSX"] {
        assert!(
            languages.answer.contains(name),
            "language answer should name {name}: {}",
            languages.answer
        );
    }
    assert!(languages
        .citations
        .iter()
        .any(|citation| citation.source_path == "README.md"
            && citation
                .text
                .contains("All supported code-search languages")));
    assert!(languages.citations.iter().any(|citation| citation.text.contains("global search box searches all five languages")));

    let python = ask(
        &db,
        &RepositoryAcceptanceFixture,
        repo.path(),
        "Can we search Python code?",
    )?;
    assert_eq!(python.outcome, "no");
    assert!(python.citations.iter().any(|citation| citation
        .text
        .contains("All supported code-search languages")));

    let lua = ask(
        &db,
        &RepositoryAcceptanceFixture,
        repo.path(),
        "Can we search Lua code?",
    )?;
    assert_eq!(
        lua.outcome, "no",
        "Lua is negative only because an explicit exhaustive fixture list omits it"
    );
    assert!(lua.citations.iter().any(|citation| citation
        .text
        .contains("All supported code-search languages")));

    let tree_sitter = ask(
        &db,
        &RepositoryAcceptanceFixture,
        repo.path(),
        "Does the indexer use Tree-sitter?",
    )?;
    assert_eq!(tree_sitter.outcome, "yes");
    assert!(tree_sitter.answer.contains("Yes"));
    assert!(tree_sitter
        .citations
        .iter()
        .any(|citation| citation.source_path == "AGENTS.md"
            && citation.text.contains("Tree-sitter registry/router")));

    let printed = render_text(&languages);
    assert!(printed.contains("Rust") && printed.contains("TSX") && printed.contains("JSX"));
    Ok(())
}

#[test]
fn repeated_builds_keep_claim_and_list_membership_for_both_builds() -> Result<()> {
    let repo = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    copy_documentation_repo(repo.path())?;
    let sources = resolve_sources(repo.path(), &["README.md".to_owned()])?;
    let config = IndexerConfig {
        cache_dir: Some(cache.path().to_path_buf()),
        ..IndexerConfig::default()
    };
    let db = open_database(repo.path(), &config)?;
    init_schema(&db)?;
    let first = build(
        &db,
        repo.path(),
        &sources,
        &RepositoryAcceptanceFixture,
        "v1",
    )?;
    let second = build(
        &db,
        repo.path(),
        &sources,
        &RepositoryAcceptanceFixture,
        "v1",
    )?;
    assert_ne!(first, second);

    let memberships = |build_id: &str| -> Result<(i64, i64, i64)> {
        Ok((
            db.query_row(
                "SELECT count(*) FROM graph_build_claims WHERE build_id=?1",
                [build_id],
                |r| r.get(0),
            )?,
            db.query_row(
                "SELECT count(*) FROM graph_build_enumerations WHERE build_id=?1",
                [build_id],
                |r| r.get(0),
            )?,
            db.query_row(
                "SELECT count(*) FROM graph_build_enumeration_members WHERE build_id=?1",
                [build_id],
                |r| r.get(0),
            )?,
        ))
    };
    let original = memberships(&first)?;
    assert_eq!(original, memberships(&second)?);
    assert!(original.0 > 0 && original.1 > 0 && original.2 > 0);
    let shared: i64 = db.query_row(
        "SELECT count(*) FROM graph_build_claims a JOIN graph_build_claims b ON a.claim_id=b.claim_id WHERE a.build_id=?1 AND b.build_id=?2",
        rusqlite::params![first, second], |r| r.get(0),
    )?;
    assert_eq!(shared, original.0);
    let shared_lists: i64 = db.query_row(
        "SELECT count(*) FROM graph_build_enumerations a JOIN graph_build_enumerations b ON a.enumeration_id=b.enumeration_id WHERE a.build_id=?1 AND b.build_id=?2",
        rusqlite::params![first, second], |r| r.get(0),
    )?;
    assert_eq!(shared_lists, original.1);
    let shared_members: i64 = db.query_row(
        "SELECT count(*) FROM graph_build_enumeration_members a JOIN graph_build_enumeration_members b ON a.enumeration_id=b.enumeration_id AND a.claim_id=b.claim_id WHERE a.build_id=?1 AND b.build_id=?2",
        rusqlite::params![first, second], |r| r.get(0),
    )?;
    assert_eq!(shared_members, original.2);
    init_schema(&db)?;
    assert_eq!(original, memberships(&first)?);
    assert_eq!(original, memberships(&second)?);
    let answer = ask(
        &db,
        &RepositoryAcceptanceFixture,
        repo.path(),
        "Which languages can code search find?",
    )?;
    assert_eq!(answer.build_id, second);
    assert_eq!(answer.outcome, "yes");
    Ok(())
}

#[derive(Debug)]
struct ChangingExtractionFixture {
    member_index: usize,
    rust_evidence_line: u32,
}

impl GraphModel for ChangingExtractionFixture {
    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse> {
        let evidence = |line: u32| -> Result<ClaimEvidence> {
            let passage = request
                .passages
                .iter()
                .find(|passage| passage.start_line <= line && passage.end_line >= line)
                .ok_or_else(|| anyhow!("missing fixture line {line}"))?;
            let text = passage
                .text
                .split_inclusive('\n')
                .nth((line - passage.start_line) as usize)
                .ok_or_else(|| anyhow!("missing text for fixture line {line}"))?
                .to_owned();
            Ok(ClaimEvidence {
                passage_id: passage.id.clone(),
                start_line: line,
                end_line: line,
                text,
            })
        };
        let index = entity("index", "CodeIndex");
        Ok(ExtractionResponse {
            claims: vec![
                claim(
                    index.clone(),
                    "indexes_language",
                    entity("language", "Rust"),
                    evidence(self.rust_evidence_line)?,
                ),
                claim(
                    index.clone(),
                    "indexes_language",
                    entity("language", "TypeScript"),
                    evidence(3)?,
                ),
            ],
            enumerations: vec![ExtractedEnumeration {
                subject: index,
                predicate: "indexes_language".into(),
                scope: ClaimScope::Project,
                exhaustive: false,
                evidence: evidence(4)?,
                member_claim_indexes: vec![self.member_index],
            }],
        })
    }

    fn interpret_question(&self, _: &InterpretationRequest) -> Result<InterpretationResponse> {
        Ok(interpret_code_search_language("Rust"))
    }

    fn model_name(&self) -> &str {
        "changing-extraction-fixture"
    }
}

fn changing_extraction_db() -> Result<(
    tempfile::TempDir,
    tempfile::TempDir,
    rusqlite::Connection,
    Vec<SelectedSource>,
)> {
    let repo = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    fs::write(repo.path().join("README.md"),
        "# Search\nRust and TypeScript are indexed.\nRust and TypeScript are indexed.\nExample languages: Rust, TypeScript.\n")?;
    let db = open_database(
        repo.path(),
        &IndexerConfig {
            cache_dir: Some(cache.path().to_path_buf()),
            ..IndexerConfig::default()
        },
    )?;
    init_schema(&db)?;
    let sources = resolve_sources(repo.path(), &["README.md".into()])?;
    Ok((repo, cache, db, sources))
}

#[test]
fn migration_does_not_add_later_list_members_to_first_build() -> Result<()> {
    let (repo, _cache, db, sources) = changing_extraction_db()?;
    let first = build(
        &db,
        repo.path(),
        &sources,
        &ChangingExtractionFixture {
            member_index: 0,
            rust_evidence_line: 2,
        },
        "v1",
    )?;
    let second = build(
        &db,
        repo.path(),
        &sources,
        &ChangingExtractionFixture {
            member_index: 1,
            rust_evidence_line: 2,
        },
        "v1",
    )?;
    let members = |build_id: &str| -> Result<Vec<String>> {
        let mut statement = db.prepare(
            "SELECT m.claim_id FROM graph_build_enumeration_members m WHERE m.build_id=?1 ORDER BY m.claim_id")?;
        let rows = statement
            .query_map([build_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    };
    let first_members = members(&first)?;
    let second_members = members(&second)?;
    assert_eq!(first_members.len(), 1);
    assert_eq!(second_members.len(), 1);
    assert_ne!(first_members, second_members);

    // Simulate a legacy writer accumulating members against a shared list ID.
    // Reopening the schema must never replay that mutable table into prior builds.
    db.execute(
        "INSERT OR IGNORE INTO graph_enumeration_members(enumeration_id, claim_id)
         SELECT enumeration_id, claim_id FROM graph_build_enumeration_members WHERE build_id=?1",
        [&second],
    )?;
    init_schema(&db)?;
    assert_eq!(members(&first)?, first_members);
    assert_eq!(members(&second)?, second_members);
    Ok(())
}

#[test]
fn changed_evidence_lines_create_distinct_claim_ids_and_preserve_old_spans() -> Result<()> {
    let (repo, _cache, db, sources) = changing_extraction_db()?;
    let first = build(
        &db,
        repo.path(),
        &sources,
        &ChangingExtractionFixture {
            member_index: 0,
            rust_evidence_line: 2,
        },
        "v1",
    )?;
    let second = build(
        &db,
        repo.path(),
        &sources,
        &ChangingExtractionFixture {
            member_index: 0,
            rust_evidence_line: 3,
        },
        "v1",
    )?;
    let rust_claim = |build_id: &str| -> Result<(String, u32)> {
        Ok(db.query_row(
            "SELECT c.id, sp.start_line FROM graph_build_claims bc
             JOIN graph_claims c ON c.id=bc.claim_id
             JOIN graph_nodes n ON n.id=c.object_node_id
             JOIN graph_claim_evidence ce ON ce.claim_id=c.id
             JOIN graph_spans sp ON sp.id=ce.span_id
             WHERE bc.build_id=?1 AND n.display_name='Rust'",
            [build_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    };
    let original = rust_claim(&first)?;
    let revised = rust_claim(&second)?;
    assert_ne!(original.0, revised.0);
    assert_eq!(original.1, 2);
    assert_eq!(revised.1, 3);
    assert_eq!(rust_claim(&first)?, original);
    Ok(())
}

#[derive(Debug)]
struct PositiveRuleFixture;

impl GraphModel for PositiveRuleFixture {
    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse> {
        Ok(ExtractionResponse {
            claims: vec![
                claim(
                    entity("feature", "CodeSearch"),
                    "searches_index",
                    entity("index", "CodeIndex"),
                    passage_evidence(&request.passages[0]),
                ),
                claim(
                    entity("index", "CodeIndex"),
                    "indexes_language",
                    entity("language", "Python"),
                    passage_evidence(&request.passages[1]),
                ),
            ],
            enumerations: vec![],
        })
    }

    fn interpret_question(&self, _: &InterpretationRequest) -> Result<InterpretationResponse> {
        Ok(interpret_code_search_language("Python"))
    }

    fn model_name(&self) -> &str {
        "positive-rule-fixture"
    }
}

#[test]
fn code_search_positive_requires_both_current_project_or_feature_premises() -> Result<()> {
    let repo = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    let source =
        "# Search\nCode search searches the code index.\n# Index\nThe code index indexes Python.\n";
    fs::write(repo.path().join("README.md"), source)?;
    let db = open_database(
        repo.path(),
        &IndexerConfig {
            cache_dir: Some(cache.path().to_path_buf()),
            ..IndexerConfig::default()
        },
    )?;
    init_schema(&db)?;
    let sources = resolve_sources(repo.path(), &["README.md".into()])?;
    build(
        &db,
        repo.path(),
        &sources,
        &PositiveRuleFixture,
        "fixture-v1",
    )?;
    let prepared = prepare_search(
        &db,
        "Can code search find Python?",
        &PositiveRuleFixture,
        20,
    )?;
    let proof = derive_positive(&db, &prepared)?.expect("both premises prove code search");
    assert_eq!(proof.claim_ids.len(), 2);
    assert_eq!(
        proof.evidence_ids.len(),
        2,
        "both exact supporting passages are required"
    );

    db.execute(
        "UPDATE graph_claims SET active=0 WHERE predicate='searches_index'",
        [],
    )?;
    assert!(
        derive_positive(&db, &prepared)?.is_none(),
        "index membership alone cannot prove searchability"
    );
    db.execute(
        "UPDATE graph_claims SET active=1, scope='epic' WHERE predicate='searches_index'",
        [],
    )?;
    assert!(
        derive_positive(&db, &prepared)?.is_none(),
        "an epic search claim cannot prove current project capability"
    );
    db.execute(
        "UPDATE graph_claims SET scope='project' WHERE predicate='searches_index'",
        [],
    )?;
    db.execute(
        "UPDATE graph_claims SET scope='task' WHERE predicate='indexes_language'",
        [],
    )?;
    assert!(
        derive_positive(&db, &prepared)?.is_none(),
        "a task language claim cannot prove current project capability"
    );
    Ok(())
}

#[derive(Debug)]
struct NegativeListFixture;

#[derive(Debug)]
struct TargetlessIncompleteListFixture;

impl GraphModel for TargetlessIncompleteListFixture {
    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse> {
        NegativeListFixture.extract_claims(request)
    }

    fn interpret_question(&self, request: &InterpretationRequest) -> Result<InterpretationResponse> {
        let mut response = interpret_code_search_language_list();
        response.predicates.retain(|predicate| request.candidate_predicates.contains(predicate));
        Ok(response)
    }

    fn model_name(&self) -> &str { "targetless-incomplete-list-fixture" }
}

#[test]
fn targetless_list_question_is_unknown_without_complete_dialect_evidence() -> Result<()> {
    let repo = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    fs::write(repo.path().join("README.md"),
        "# Search\nCode search searches the code index.\n# Languages\nAll supported code-search languages: Rust.\n")?;
    let db = open_database(repo.path(), &IndexerConfig {
        cache_dir: Some(cache.path().to_path_buf()), ..IndexerConfig::default()
    })?;
    init_schema(&db)?;
    let sources = resolve_sources(repo.path(), &["README.md".into()])?;
    build(&db, repo.path(), &sources, &TargetlessIncompleteListFixture, "fixture-v1")?;
    let prepared = prepare_search(&db, "Which languages can code search find?", &TargetlessIncompleteListFixture, 20)?;
    assert!(!prepared.resolved_entity_keys.iter().any(|key| key.starts_with("language:")));
    let answer = render_answer(&db, &prepared, resolve_prepared(&db, &prepared)?)?;
    assert_eq!(answer.outcome, "unknown");
    Ok(())
}

impl GraphModel for NegativeListFixture {
    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse> {
        let search = request
            .passages
            .iter()
            .find(|passage| passage.text.contains("Code search searches the code index"))
            .ok_or_else(|| anyhow!("search passage is missing"))?;
        let list = request
            .passages
            .iter()
            .find(|passage| passage.text.contains("All supported code-search languages"))
            .ok_or_else(|| anyhow!("list passage is missing"))?;
        Ok(ExtractionResponse {
            claims: vec![
                claim(
                    entity("feature", "CodeSearch"),
                    "searches_index",
                    entity("index", "CodeIndex"),
                    passage_evidence(search),
                ),
                claim(
                    entity("index", "CodeIndex"),
                    "indexes_language",
                    entity("language", "Rust"),
                    passage_evidence(list),
                ),
            ],
            enumerations: vec![ExtractedEnumeration {
                subject: entity("index", "CodeIndex"),
                predicate: "indexes_language".into(),
                scope: ClaimScope::Project,
                exhaustive: true,
                evidence: passage_evidence(list),
                member_claim_indexes: vec![1],
            }],
        })
    }

    fn interpret_question(&self, _: &InterpretationRequest) -> Result<InterpretationResponse> {
        Ok(interpret_code_search_language("Lua"))
    }

    fn model_name(&self) -> &str {
        "negative-list-fixture"
    }
}

#[derive(Debug)]
struct NarrowEvidenceFixture;

impl GraphModel for NarrowEvidenceFixture {
    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse> {
        let passage = request
            .passages
            .first()
            .ok_or_else(|| anyhow!("missing passage"))?;
        let line = |number: u32| -> ClaimEvidence {
            let text = passage
                .text
                .split_inclusive('\n')
                .nth((number - passage.start_line) as usize)
                .unwrap()
                .to_owned();
            ClaimEvidence {
                passage_id: passage.id.clone(),
                start_line: number,
                end_line: number,
                text,
            }
        };
        Ok(ExtractionResponse {
            claims: vec![
                claim(
                    entity("feature", "CodeSearch"),
                    "searches_index",
                    entity("index", "CodeIndex"),
                    line(2),
                ),
                {
                    let mut member = claim(
                        entity("index", "CodeIndex"),
                        "indexes_language",
                        entity("language", "Rust"),
                        line(3),
                    );
                    member.evidence.push(line(4));
                    member
                },
            ],
            enumerations: vec![ExtractedEnumeration {
                subject: entity("index", "CodeIndex"),
                predicate: "indexes_language".into(),
                scope: ClaimScope::Project,
                exhaustive: true,
                evidence: line(4),
                member_claim_indexes: vec![1],
            }],
        })
    }

    fn interpret_question(&self, _: &InterpretationRequest) -> Result<InterpretationResponse> {
        Ok(interpret_code_search_language("Lua"))
    }

    fn model_name(&self) -> &str {
        "narrow-evidence-fixture"
    }
}

#[test]
fn citations_use_exact_claim_and_enumeration_evidence_lines() -> Result<()> {
    let repo = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    fs::write(repo.path().join("README.md"),
        "# Search\nCode search searches the code index.\nRust is indexed.\nAll supported code-search languages: Rust.\nUnrelated note.\n")?;
    let db = open_database(
        repo.path(),
        &IndexerConfig {
            cache_dir: Some(cache.path().to_path_buf()),
            ..IndexerConfig::default()
        },
    )?;
    init_schema(&db)?;
    let sources = resolve_sources(repo.path(), &["README.md".into()])?;
    for _ in 0..2 {
        build(
            &db,
            repo.path(),
            &sources,
            &NarrowEvidenceFixture,
            "fixture-v1",
        )?;
    }
    let prepared = prepare_search(&db, "Can code search find Lua?", &NarrowEvidenceFixture, 20)?;
    let answer = render_answer(&db, &prepared, resolve_prepared(&db, &prepared)?)?;
    assert_eq!(answer.outcome, "no");
    let lines = answer
        .citations
        .iter()
        .map(|citation| citation.start_line)
        .collect::<Vec<_>>();
    assert_eq!(lines, vec![2, 4]);
    let member_start: u32 = db.query_row(
        "SELECT sp.start_line FROM graph_claims c
         JOIN graph_claim_evidence ce ON ce.claim_id=c.id
         JOIN graph_spans sp ON sp.id=ce.span_id
         JOIN graph_build_claims bc ON bc.claim_id=c.id
         WHERE bc.build_id=?1 AND c.predicate='indexes_language'",
        [&prepared.build_id],
        |row| row.get(0),
    )?;
    assert_eq!(member_start, 3);
    assert!(answer
        .citations
        .iter()
        .all(|citation| citation.start_line == citation.end_line));
    assert!(answer
        .citations
        .iter()
        .all(|citation| !citation.text.contains("Unrelated note")));
    Ok(())
}

#[test]
fn exhaustive_omission_cites_search_relationship_and_requires_its_current_scope() -> Result<()> {
    let repo = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    fs::write(repo.path().join("README.md"),
        "# Search\nCode search searches the code index.\n# Languages\nAll supported code-search languages: Rust.\n")?;
    let db = open_database(
        repo.path(),
        &IndexerConfig {
            cache_dir: Some(cache.path().to_path_buf()),
            ..IndexerConfig::default()
        },
    )?;
    init_schema(&db)?;
    let sources = resolve_sources(repo.path(), &["README.md".into()])?;
    build(
        &db,
        repo.path(),
        &sources,
        &NegativeListFixture,
        "fixture-v1",
    )?;
    let prepared = prepare_search(&db, "Can code search find Lua?", &NegativeListFixture, 20)?;
    let answer = render_answer(&db, &prepared, resolve_prepared(&db, &prepared)?)?;
    assert_eq!(answer.outcome, "no");
    assert_eq!(
        answer.claim_ids.len(),
        2,
        "the list member and search relationship both prove omission"
    );
    assert!(answer.citations.iter().any(|citation| citation
        .text
        .contains("Code search searches the code index")));
    assert!(answer.citations.iter().any(|citation| citation
        .text
        .contains("All supported code-search languages")));

    db.execute(
        "UPDATE graph_claims SET active=0 WHERE predicate='searches_index'",
        [],
    )?;
    assert!(matches!(
        resolve_prepared(&db, &prepared)?,
        super::EvidenceResolution::Unknown { .. }
    ));
    db.execute(
        "UPDATE graph_claims SET active=1, scope='epic' WHERE predicate='searches_index'",
        [],
    )?;
    assert!(matches!(
        resolve_prepared(&db, &prepared)?,
        super::EvidenceResolution::Unknown { .. }
    ));
    Ok(())
}

#[derive(Debug)]
struct EndpointFixture;

impl GraphModel for EndpointFixture {
    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse> {
        let evidence = |needle: &str| {
            request
                .passages
                .iter()
                .find(|p| p.text.contains(needle))
                .map(passage_evidence)
                .ok_or_else(|| anyhow!("missing {needle}"))
        };
        Ok(ExtractionResponse {
            claims: vec![
                claim(
                    entity("index", "ImageIndex"),
                    "uses_parser",
                    entity("parser", "Tree-sitter"),
                    evidence("Image index uses Tree-sitter")?,
                ),
                claim(
                    entity("index", "CodeIndex"),
                    "uses_parser",
                    entity("parser", "OtherParser"),
                    evidence("Code index uses OtherParser")?,
                ),
            ],
            enumerations: vec![],
        })
    }

    fn interpret_question(&self, _: &InterpretationRequest) -> Result<InterpretationResponse> {
        Ok(InterpretationResponse {
            intent: QuestionIntent::DirectRelation,
            entities: vec![
                entity("index", "CodeIndex"),
                entity("parser", "Tree-sitter"),
            ],
            predicates: vec!["uses_parser".into()],
        })
    }

    fn model_name(&self) -> &str {
        "endpoint-fixture"
    }
}

#[test]
fn direct_relation_requires_both_named_endpoints() -> Result<()> {
    let repo = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    fs::write(
        repo.path().join("README.md"),
        "# Image\nImage index uses Tree-sitter.\n# Code\nCode index uses OtherParser.\n",
    )?;
    let db = open_database(
        repo.path(),
        &IndexerConfig {
            cache_dir: Some(cache.path().to_path_buf()),
            ..IndexerConfig::default()
        },
    )?;
    init_schema(&db)?;
    let sources = resolve_sources(repo.path(), &["README.md".into()])?;
    build(&db, repo.path(), &sources, &EndpointFixture, "fixture-v1")?;
    let prepared = prepare_search(
        &db,
        "Does the code index use Tree-sitter?",
        &EndpointFixture,
        20,
    )?;
    assert!(derive_positive(&db, &prepared)?.is_none());
    assert!(matches!(
        resolve_prepared(&db, &prepared)?,
        super::EvidenceResolution::Unknown { .. }
    ));

    let mut matching = prepared.clone();
    matching.resolved_entity_keys = vec!["index:imageindex".into(), "parser:tree-sitter".into()];
    let proof =
        derive_positive(&db, &matching)?.expect("matching endpoints prove the direct relation");
    assert_eq!(proof.claim_ids.len(), 1);
    assert_eq!(proof.evidence_ids.len(), 1);
    matching.resolved_entity_keys = vec!["feature:codesearch".into(), "parser:tree-sitter".into()];
    assert!(derive_positive(&db, &matching)?.is_none());
    Ok(())
}

#[derive(Debug)]
struct ExplicitNegativeFixture;

impl GraphModel for ExplicitNegativeFixture {
    fn extract_claims(&self, request: &ExtractionRequest) -> Result<ExtractionResponse> {
        let evidence = |needle: &str| {
            request
                .passages
                .iter()
                .find(|p| p.text.contains(needle))
                .map(passage_evidence)
                .ok_or_else(|| anyhow!("missing {needle}"))
        };
        let mut image_negative = claim(
            entity("index", "ImageIndex"),
            "indexes_language",
            entity("language", "Python"),
            evidence("Image index excludes Python")?,
        );
        image_negative.polarity = Polarity::Negative;
        let mut code_negative = claim(
            entity("index", "CodeIndex"),
            "indexes_language",
            entity("language", "Python"),
            evidence("Code index excludes Python")?,
        );
        code_negative.polarity = Polarity::Negative;
        Ok(ExtractionResponse {
            claims: vec![
                claim(
                    entity("feature", "CodeSearch"),
                    "searches_index",
                    entity("index", "CodeIndex"),
                    evidence("Code search searches the code index")?,
                ),
                image_negative,
                code_negative,
            ],
            enumerations: vec![],
        })
    }

    fn interpret_question(&self, _: &InterpretationRequest) -> Result<InterpretationResponse> {
        Ok(interpret_code_search_language("Python"))
    }

    fn model_name(&self) -> &str {
        "explicit-negative-fixture"
    }
}

#[test]
fn explicit_language_negative_requires_scoped_search_edge_and_cites_it() -> Result<()> {
    let repo = tempfile::tempdir()?;
    let cache = tempfile::tempdir()?;
    fs::write(repo.path().join("README.md"),
        "# Search\nCode search searches the code index.\n# Image\nImage index excludes Python.\n# Code\nCode index excludes Python.\n")?;
    let db = open_database(
        repo.path(),
        &IndexerConfig {
            cache_dir: Some(cache.path().to_path_buf()),
            ..IndexerConfig::default()
        },
    )?;
    init_schema(&db)?;
    let sources = resolve_sources(repo.path(), &["README.md".into()])?;
    build(
        &db,
        repo.path(),
        &sources,
        &ExplicitNegativeFixture,
        "fixture-v1",
    )?;
    let prepared = prepare_search(
        &db,
        "Can code search find Python?",
        &ExplicitNegativeFixture,
        20,
    )?;
    let answer = render_answer(&db, &prepared, resolve_prepared(&db, &prepared)?)?;
    assert_eq!(answer.outcome, "no");
    assert_eq!(answer.claim_ids.len(), 2);
    assert!(answer
        .citations
        .iter()
        .any(|c| c.text.contains("Code search searches the code index")));
    assert!(answer
        .citations
        .iter()
        .any(|c| c.text.contains("Code index excludes Python")));
    assert!(!answer
        .citations
        .iter()
        .any(|c| c.text.contains("Image index excludes Python")));

    db.execute(
        "UPDATE graph_claims SET active=0 WHERE predicate='indexes_language' AND subject_node_id=(SELECT id FROM graph_nodes WHERE canonical_key='index:codeindex')",
        [],
    )?;
    assert!(matches!(
        resolve_prepared(&db, &prepared)?,
        super::EvidenceResolution::Unknown { .. }
    ));
    db.execute(
        "UPDATE graph_claims SET active=1 WHERE predicate='indexes_language' AND subject_node_id=(SELECT id FROM graph_nodes WHERE canonical_key='index:codeindex')",
        [],
    )?;
    db.execute(
        "UPDATE graph_claims SET active=0 WHERE predicate='searches_index'",
        [],
    )?;
    assert!(matches!(
        resolve_prepared(&db, &prepared)?,
        super::EvidenceResolution::Unknown { .. }
    ));
    Ok(())
}
