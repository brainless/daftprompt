use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;

pub fn list_tracked_rust_files(repo_path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let repo = gix::discover(repo_path)?;

    let work_dir = match repo.workdir() {
        Some(dir) => dir.to_path_buf(),
        None => return Ok(vec![]),
    };

    let tree_id = repo.head_tree_id_or_empty()?;
    let tree = repo.find_tree(tree_id)?;

    let entries = tree.traverse().breadthfirst.files()?;

    let rust_files: Vec<PathBuf> = entries
        .into_iter()
        .filter(|entry| {
            entry.mode.is_blob()
                && entry
                    .filepath
                    .to_str()
                    .map_or(false, |p| p.ends_with(".rs"))
        })
        .filter_map(|entry| {
            entry.filepath.to_str().ok().map(|p| work_dir.join(p))
        })
        .collect();

    Ok(rust_files)
}

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

    #[test]
    fn list_tracked_rust_files_returns_results() {
        let repo_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let files = super::list_tracked_rust_files(repo_path).expect("should list files");
        assert!(!files.is_empty(), "should find at least one .rs file");
        assert!(
            files.iter().any(|p| p.ends_with("code.rs")),
            "should find code.rs in the results"
        );
    }
}
