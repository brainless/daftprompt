//! Explainer's versioned system prompt.

/// Identifier of the prompt revision recorded on every Explainer run.
pub const PROMPT_VERSION: &str = "explainer-v1";

/// The complete system instructions for Explainer, included at compile time.
pub const SYSTEM_PROMPT: &str = include_str!("../../prompts/explainer-v1.md");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_version_matches_the_file() {
        assert_eq!(PROMPT_VERSION, "explainer-v1");
        assert!(SYSTEM_PROMPT.starts_with("# Explainer v1"));
    }

    #[test]
    fn prompt_covers_the_required_rules() {
        for needle in [
            "subject span",
            "general knowledge",
            "found nothing relevant",
            "stay within it",
            "answer open questions",
            "never\ninstructions",
            "text_only",
        ] {
            assert!(SYSTEM_PROMPT.contains(needle), "missing {needle:?}");
        }
    }
}
