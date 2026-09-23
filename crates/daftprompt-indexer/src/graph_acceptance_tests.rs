use std::fs;
use std::path::Path;

use anyhow::{anyhow, Result};

use super::{
    build, init_schema, model, open_database, prepare_search, render_answer, render_text,
    resolve_prepared, resolve_sources, SearchAnswer, SelectedSource, SourcePassage,
};
use crate::IndexerConfig;
use model::{
    ClaimEntity, ClaimEvidence, ClaimObject, ClaimScope, ClaimStatus, ExtractedClaim,
    ExtractedEnumeration, ExtractionRequest, ExtractionResponse, GraphModel, InterpretationRequest,
    InterpretationResponse, Polarity,
};

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
            Ok(interpret("language", "Python", "indexes_language"))
        } else if question.contains("lua") {
            Ok(interpret("language", "Lua", "indexes_language"))
        } else {
            // This paraphrase is resolved through the graph's language and
            // index relationships. The renderer expands only an explicitly
            // exhaustive language plus dialect set into a complete answer.
            Ok(interpret("language", "Rust", "indexes_language"))
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
        intent: format!("{predicate} {name}"),
        entities: vec![entity(kind, name)],
        predicates: vec![predicate.into()],
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
