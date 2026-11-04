use daftprompt_core::{BoxFuture, ExtractionModel, ModelError, ModelInput, Scribe, ScribeConfig};
use daftprompt_types::{ContentType, ContentTypeSource, Extraction, ParseError, ParseRequest};

struct Stub(Extraction);
impl ExtractionModel for Stub {
    fn extract(&self, input: ModelInput) -> BoxFuture<'_, Result<Extraction, ModelError>> {
        assert!(input.segments.contains("{b1 "));
        let extraction = self.0.clone();
        Box::pin(async move { Ok(extraction) })
    }
}

async fn parse(
    text: &str,
    proposed: serde_json::Value,
) -> Result<daftprompt_types::ParseResult, ParseError> {
    let extraction = serde_json::from_value(proposed).unwrap();
    Scribe::new(
        ScribeConfig::default().with_max_attempts(1),
        Stub(extraction),
    )
    .parse(ParseRequest::new(text))
    .await
}

#[tokio::test]
async fn block_spans_cover_whole_sections_with_original_bytes() {
    let source = "## Retry\r\n\r\n- Trigger: café e\u{301}.\r\n- Send feedback.\r\n";
    let result = parse(
        source,
        serde_json::json!({
            "spans":[{"id":"s1","block":"b1"}],
            "annotations":[{"kind":"recommendation","span":"s1"}]
        }),
    )
    .await
    .unwrap();
    assert_eq!(result.content_type, ContentType::Markdown);
    assert_eq!(result.content_type_source, ContentTypeSource::Detected);
    assert_eq!(
        result.run.prompt_version.as_deref(),
        Some("scribe-v4+markdown-v1")
    );
    assert_eq!(result.payload.content, source);
    assert_eq!(
        &source[result.spans[0].start_byte..result.spans[0].end_byte],
        source.trim()
    );
}

#[tokio::test]
async fn unknown_blocks_and_parent_child_overlap_are_invalid() {
    for spans in [
        serde_json::json!([{"id":"s1","block":"missing"}]),
        serde_json::json!([{"id":"s1","block":"b1"},{"id":"s2","block":"b2"}]),
    ] {
        let annotations = spans
            .as_array()
            .unwrap()
            .iter()
            .map(|span| serde_json::json!({"kind":"recommendation","span":span["id"]}))
            .collect::<Vec<_>>();
        assert!(matches!(
            parse(
                "# Plan\n\n- Suggest caching.",
                serde_json::json!({"spans":spans,"annotations":annotations})
            )
            .await,
            Err(ParseError::InvalidOutput { .. })
        ));
    }
}

#[tokio::test]
async fn mixed_disjoint_block_and_segment_spans_share_one_extraction() {
    let source = "I suggest caching. This saves requests.\n\nWhat is the budget?";
    let result = parse(source, serde_json::json!({
        "spans":[{"id":"s1","block":"b1"},{"id":"s2","start_segment":3,"end_segment":3,"quote":"What is the budget?"}],
        "annotations":[{"kind":"recommendation","span":"s1"},{"kind":"question","span":"s2"}]
    })).await.unwrap();
    assert_eq!(result.spans.len(), 2);
    assert_eq!(result.spans[0].display_id, 1);
    assert_eq!(result.spans[1].display_id, 2);
    assert_eq!(
        result.run.prompt_version.as_deref(),
        Some("scribe-v4+plain-v1")
    );
}

#[test]
fn block_with_quote_is_rejected_before_validation() {
    assert!(
        serde_json::from_value::<Extraction>(serde_json::json!({
            "spans":[{"id":"s1","block":"b1","quote":"text"}]
        }))
        .is_err()
    );
}

#[tokio::test]
async fn declared_format_and_source_are_recorded() {
    let mut request = ParseRequest::new("# literal plain text");
    request.content_type = Some(ContentType::Plain);
    let result = Scribe::new(ScribeConfig::default(), Stub(Extraction::default()))
        .parse(request)
        .await
        .unwrap();
    assert_eq!(result.content_type, ContentType::Plain);
    assert_eq!(result.content_type_source, ContentTypeSource::Declared);
}

#[test]
fn prompt_assembly_includes_one_format_and_preserves_semantics() {
    use daftprompt_core::scribe::prompt::{render_structure, system_prompt};
    use daftprompt_core::text::structure::structure;
    for (content_type, present, absent) in [
        (
            ContentType::Plain,
            "## Plain format v1",
            "## Markdown format v1",
        ),
        (
            ContentType::Markdown,
            "## Markdown format v1",
            "## Plain format v1",
        ),
    ] {
        let prompt = system_prompt(content_type);
        assert!(prompt.contains(present));
        assert!(!prompt.contains(absent));
        assert!(prompt.find(present).unwrap() < prompt.rfind("## Final checks").unwrap());
        for rule in [
            "smallest block",
            "recommendation",
            "assignee",
            "character for character",
            "No spans overlap",
        ] {
            assert!(prompt.contains(rule), "missing {rule}");
        }
    }
    let text = "# Plan\n\n- One\n- Two";
    assert_eq!(
        render_structure(text, &structure(text, ContentType::Markdown)),
        "{b1 section 1-4}\n[1] # Plan\n[2] \n  {b2 item 3-3}\n[3] - One\n  {b3 item 4-4}\n[4] - Two"
    );
}
