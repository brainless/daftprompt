//! Scribe's versioned prompt and the segment rendering handed to the model.

use crate::text::segments;

/// Identifier of the prompt revision recorded on every run.
pub const PROMPT_VERSION: &str = "scribe-v4";

/// The complete system instructions for Scribe.
///
/// The file is versioned in the repository and included at compile time, so a
/// build cannot accidentally run with a stale prompt from disk.
pub const SYSTEM_PROMPT: &str = include_str!("../../prompts/scribe-v4.md");

/// Combined prompt version recorded on a run.
pub fn prompt_version(content_type: daftprompt_types::ContentType) -> &'static str {
    match content_type {
        daftprompt_types::ContentType::Plain => "scribe-v4+plain-v1",
        daftprompt_types::ContentType::Markdown => "scribe-v4+markdown-v1",
    }
}

/// Assemble the shared semantic rules and exactly one format section.
pub fn system_prompt(content_type: daftprompt_types::ContentType) -> String {
    let format = match content_type {
        daftprompt_types::ContentType::Plain => {
            include_str!("../../prompts/scribe-format-plain-v1.md")
        }
        daftprompt_types::ContentType::Markdown => {
            include_str!("../../prompts/scribe-format-markdown-v1.md")
        }
    };
    // Keep the shared final semantic checks last so format/granularity examples
    // cannot overshadow the unchanged label rules.
    let (core, checks) = SYSTEM_PROMPT
        .split_once("## Final checks")
        .expect("the compile-time Scribe core has final checks");
    format!("{core}\n{format}\n## Final checks{checks}")
}

/// Interleave unnumbered block markers immediately before their first segment.
pub fn render_structure(source: &str, tree: &crate::text::structure::Structure) -> String {
    let mut output = Vec::new();
    for segment in segments(source) {
        for block in tree
            .blocks
            .iter()
            .filter(|block| block.segments.start == segment.number)
        {
            output.push(format!(
                "{}{{{} {} {}-{}}}",
                "  ".repeat(block.depth),
                block.id,
                block.kind.as_str(),
                block.segments.start,
                block.segments.end
            ));
        }
        output.push(format!("[{}] {}", segment.number, segment.text(source)));
    }
    output.join("\n")
}

/// Render the text as the numbered `[n] text` segments the model refers to.
///
/// Empty segments render as `[n] ` so their numbering stays visible. CRLF
/// terminators are not part of segment text, matching how segmentation works.
pub fn render_segments(source: &str) -> String {
    segments(source)
        .into_iter()
        .map(|segment| format!("[{}] {}", segment.number, segment.text(source)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_numbered_segments() {
        assert_eq!(
            render_segments("first\n\nthird"),
            "[1] first\n[2] \n[3] third"
        );
    }

    #[test]
    fn crlf_is_not_part_of_the_rendered_segment() {
        assert_eq!(render_segments("a\r\nb\r\n"), "[1] a\n[2] b\n[3] ");
    }

    #[test]
    fn prompt_version_matches_the_file_name() {
        assert_eq!(PROMPT_VERSION, "scribe-v4");
        assert!(SYSTEM_PROMPT.contains("Scribe v4"));
    }
}
