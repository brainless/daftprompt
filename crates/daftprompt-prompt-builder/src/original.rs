/// The user's original request, byte-for-byte, before any enrichment.
///
/// Epic 014 Design Decision #4: "The original prompt is immutable." There is
/// deliberately no public way to mutate the wrapped text after construction
/// -- every accessor returns a borrow, never `&mut`, and the formatter
/// (`format::build_enriched_prompt`) copies it into the enriched prompt's
/// `<original-request>` section unmodified (not escaped, not truncated, not
/// re-wrapped). Only retrieved repository text is subject to the untrusted
/// formatting rules in [`crate::format`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OriginalPrompt(String);

impl OriginalPrompt {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for OriginalPrompt {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for OriginalPrompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
