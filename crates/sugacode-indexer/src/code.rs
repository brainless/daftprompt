#[cfg(test)]
mod tests {
    use tree_sitter::Parser;

    #[test]
    fn parse_rust_source_file() {
        let mut parser = Parser::new();
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
        parser
            .set_language(&language)
            .expect("Error loading Rust grammar");

        let source = "fn main() {}";
        let tree = parser.parse(source, None).expect("Failed to parse source");
        let root = tree.root_node();

        assert_eq!(root.kind(), "source_file");
        assert!(!root.has_error());
    }

    #[test]
    fn xxh3_stable_hash() {
        use xxhash_rust::xxh3::xxh3_64;

        let hash = xxh3_64(b"hello");
        assert_eq!(hash, 10760762337991515389);
    }
}
