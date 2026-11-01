//! Scribe's versioned prompt and the segment rendering handed to the model.

use crate::text::segments;

/// Identifier of the prompt revision recorded on every run.
pub const PROMPT_VERSION: &str = "scribe-v3";

/// The complete system instructions for Scribe.
///
/// The file is versioned in the repository and included at compile time, so a
/// build cannot accidentally run with a stale prompt from disk.
pub const SYSTEM_PROMPT: &str = include_str!("../../prompts/scribe-v3.md");

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
        assert_eq!(PROMPT_VERSION, "scribe-v3");
        assert!(SYSTEM_PROMPT.contains("Scribe v3"));
    }
}
