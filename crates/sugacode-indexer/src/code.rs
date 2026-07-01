use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    ImplMethod,
    TraitMethod,
    TypeAlias,
    Const,
    Static,
    Module,
    Macro,
    Comments,
    Imports,
}

#[derive(Debug, Clone)]
pub struct CodeSymbol {
    pub identifier: String,
    pub text: String,
    pub symbol_kind: SymbolKind,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub embed: bool,
}

const RUST_QUERY: &str = r#"
(function_item
  name: (identifier) @name) @definition.function

(struct_item
  name: (type_identifier) @name) @definition.struct

(enum_item
  name: (type_identifier) @name) @definition.enum

(trait_item
  name: (type_identifier) @name) @definition.trait

(impl_item
  type: (type_identifier) @impl_type
  body: (declaration_list
    (function_item
      name: (identifier) @name) @definition.impl_method))

(trait_item
  name: (type_identifier) @trait_type
  body: (declaration_list
    (function_item
      name: (identifier) @name) @definition.trait_method))

(type_item
  name: (type_identifier) @name) @definition.type_alias

(const_item
  name: (identifier) @name) @definition.const

(static_item
  name: (identifier) @name) @definition.static

(mod_item
  name: (identifier) @name) @definition.module

(macro_definition
  name: (identifier) @name) @definition.macro

(line_comment) @comment
(block_comment) @comment

(use_declaration) @import
"#;

pub fn extract_symbols(file_path: &Path, source: &str) -> anyhow::Result<Vec<CodeSymbol>> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    parser.set_language(&language)?;
    let tree = parser.parse(source, None).ok_or_else(|| {
        anyhow::anyhow!("Failed to parse source file: {}", file_path.display())
    })?;

    let query = Query::new(&language, RUST_QUERY)?;
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());

    let capture_names = query.capture_names();
    let mut symbols: Vec<CodeSymbol> = Vec::new();
    let mut doc_comment_nodes: Vec<usize> = Vec::new();
    let mut standalone_comment_ranges: Vec<(usize, usize)> = Vec::new();
    let mut import_nodes: Vec<usize> = Vec::new();

    let file_path_str = canonicalize_file_path(
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        file_path,
    );

    while let Some(m) = matches.next() {
        let captures = m.captures;

        let mut symbol_name: Option<String> = None;
        let mut definition_node: Option<tree_sitter::Node> = None;
        let mut definition_kind: Option<&str> = None;
        let mut impl_type: Option<String> = None;
        let mut trait_type: Option<String> = None;

        for cap in captures {
            let name = capture_names[cap.index as usize];
            match name {
                "name" => {
                    symbol_name = Some(
                        cap.node
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .to_string(),
                    );
                }
                "impl_type" => {
                    impl_type = Some(
                        cap.node
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .to_string(),
                    );
                }
                "trait_type" => {
                    trait_type = Some(
                        cap.node
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .to_string(),
                    );
                }
                "comment" => {
                    let start = cap.node.start_position().row;
                    let end = cap.node.end_position().row;
                    standalone_comment_ranges.push((start, end));
                }
                "import" => {
                    import_nodes.push(cap.node.start_position().row);
                }
                _ => {
                    if name.starts_with("definition.") {
                        definition_node = Some(cap.node);
                        definition_kind = Some(name);
                    }
                }
            }
        }

        let symbol_kind = definition_kind.and_then(|kind| match kind {
            "definition.function" => Some(SymbolKind::Function),
            "definition.struct" => Some(SymbolKind::Struct),
            "definition.enum" => Some(SymbolKind::Enum),
            "definition.trait" => Some(SymbolKind::Trait),
            "definition.impl_method" => Some(SymbolKind::ImplMethod),
            "definition.trait_method" => Some(SymbolKind::TraitMethod),
            "definition.type_alias" => Some(SymbolKind::TypeAlias),
            "definition.const" => Some(SymbolKind::Const),
            "definition.static" => Some(SymbolKind::Static),
            "definition.module" => Some(SymbolKind::Module),
            "definition.macro" => Some(SymbolKind::Macro),
            _ => None,
        });

        if let (Some(kind), Some(name), Some(node)) = (symbol_kind, symbol_name, definition_node) {
            // Skip generic function_item matches for nodes inside impl/trait blocks,
            // since those are also matched by the more specific impl_method/trait_method patterns.
            if kind == SymbolKind::Function && is_inside_impl_or_trait(node) {
                continue;
            }
            let doc_comment = extract_doc_comment(node, source);
            let signature = extract_signature(node, source);
            let body_excerpt = extract_body_excerpt(node, source);
            let text = compose_symbol_text(&doc_comment, &signature, &body_excerpt);

            let identifier = build_identifier(&file_path_str, node, &name, impl_type.as_deref(), trait_type.as_deref(), source);

            let line_start = node.start_position().row + 1;
            let line_end = node.end_position().row + 1;

            let start_row = node.start_position().row;
            let end_row = node.end_position().row;
            doc_comment_nodes.extend(start_row..=end_row);

            symbols.push(CodeSymbol {
                identifier,
                text,
                symbol_kind: kind,
                file_path: file_path_str.clone(),
                line_start,
                line_end,
                embed: true,
            });
        }
    }

    let comment_text = collect_standalone_comments(source, &standalone_comment_ranges, &doc_comment_nodes);
    if !comment_text.is_empty() {
        symbols.push(CodeSymbol {
            identifier: format!("{}::__comments__", file_path_str),
            text: comment_text,
            symbol_kind: SymbolKind::Comments,
            file_path: file_path_str.clone(),
            line_start: 1,
            line_end: source.lines().count(),
            embed: false,
        });
    }

    let import_text = collect_imports(source, &import_nodes);
    if !import_text.is_empty() {
        symbols.push(CodeSymbol {
            identifier: format!("{}::__imports__", file_path_str),
            text: import_text,
            symbol_kind: SymbolKind::Imports,
            file_path: file_path_str.clone(),
            line_start: 1,
            line_end: source.lines().count(),
            embed: false,
        });
    }

    Ok(symbols)
}

fn build_identifier(
    file_path: &str,
    node: tree_sitter::Node,
    name: &str,
    impl_type: Option<&str>,
    trait_type: Option<&str>,
    source: &str,
) -> String {
    let mut mod_path: Vec<String> = Vec::new();
    // For impl methods, detect if this is a trait impl by checking if the
    // impl_item ancestor has a 'trait' child.
    let mut impl_trait_name: Option<String> = None;
    let mut current = node.parent();

    while let Some(parent) = current {
        if parent.kind() == "mod_item" {
            if let Some(mod_name) = parent.child_by_field_name("name") {
                if let Ok(text) = mod_name.utf8_text(source.as_bytes()) {
                    mod_path.push(text.to_string());
                }
            }
        }
        if parent.kind() == "impl_item" && impl_trait_name.is_none() {
            // Check if this impl has a trait: impl Trait for Type
            if let Some(trait_node) = parent.child_by_field_name("trait") {
                if let Ok(text) = trait_node.utf8_text(source.as_bytes()) {
                    impl_trait_name = Some(text.to_string());
                }
            }
        }
        current = parent.parent();
    }

    mod_path.reverse();

    let mut parts = vec![file_path.to_string()];
    parts.extend(mod_path);

    // For trait impls: file_path::TraitName::method
    // For inherent impls: file_path::ImplType::method
    // For trait default methods: file_path::TraitName::method
    if let (Some(impl_type), Some(ref impl_trait)) = (impl_type, &impl_trait_name) {
        parts.push(format!("{}<{}>", impl_trait, impl_type));
    } else if let Some(impl_type) = impl_type {
        parts.push(impl_type.to_string());
    } else if let Some(trait_type) = trait_type {
        parts.push(trait_type.to_string());
    }

    parts.push(name.to_string());
    parts.join("::")
}

fn extract_doc_comment(node: tree_sitter::Node, source: &str) -> String {
    let mut comments: Vec<String> = Vec::new();
    let mut current = node.prev_sibling();

    while let Some(sibling) = current {
        let kind = sibling.kind();
        if kind == "line_comment" || kind == "block_comment" {
            let text = sibling
                .utf8_text(source.as_bytes())
                .unwrap_or("")
                .to_string();
            if text.starts_with("///") || text.starts_with("//!") || text.starts_with("/**") {
                comments.push(text);
                current = sibling.prev_sibling();
                continue;
            }
        }
        break;
    }

    comments.reverse();
    comments.join("\n")
}

fn extract_signature(node: tree_sitter::Node, source: &str) -> String {
    match node.kind() {
        "function_item" => {
            // Find the block (body) and take everything before it
            let mut block_start = node.end_byte();
            for i in 0..node.child_count() {
                let child = node.child(i).unwrap();
                if child.kind() == "block" {
                    block_start = child.start_byte();
                    break;
                }
            }
            let sig_bytes = &source.as_bytes()[node.start_byte()..block_start];
            String::from_utf8_lossy(sig_bytes).trim().to_string()
        }
        "struct_item" | "enum_item" | "trait_item" | "type_item" | "const_item" | "static_item" | "macro_definition" => {
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            text.lines().next().unwrap_or("").to_string()
        }
        "mod_item" => {
            let name = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .unwrap_or("");
            format!("mod {name}")
        }
        _ => {
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            text.lines().next().unwrap_or("").to_string()
        }
    }
}

fn extract_body_excerpt(node: tree_sitter::Node, source: &str) -> String {
    let body_node = match node.kind() {
        "function_item" => {
            let mut body = None;
            for i in 0..node.child_count() {
                let child = node.child(i).unwrap();
                if child.kind() == "block" {
                    body = Some(child);
                    break;
                }
            }
            body
        }
        _ => None,
    };

    match body_node {
        Some(body) => {
            let text = body.utf8_text(source.as_bytes()).unwrap_or("");
            let lines: Vec<&str> = text.lines().collect();
            if lines.len() <= 10 {
                text.to_string()
            } else {
                let excerpt: String = lines[..10].join("\n");
                format!("{excerpt}\n...")
            }
        }
        None => String::new(),
    }
}

pub fn compose_symbol_text(doc_comment: &str, signature: &str, body_excerpt: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !doc_comment.is_empty() {
        parts.push(doc_comment.to_string());
    }
    if !signature.is_empty() {
        parts.push(signature.to_string());
    }
    if !body_excerpt.is_empty() {
        parts.push(body_excerpt.to_string());
    }
    parts.join("\n")
}

fn collect_standalone_comments(
    source: &str,
    comment_ranges: &[(usize, usize)],
    symbol_line_ranges: &[usize],
) -> String {
    let mut result = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    for &(start, end) in comment_ranges {
        if symbol_line_ranges.contains(&start) {
            continue;
        }
        for line_no in start..=end {
            if let Some(line) = lines.get(line_no) {
                result.push(line.to_string());
            }
        }
    }

    result.join("\n")
}

fn collect_imports(source: &str, import_rows: &[usize]) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut result = Vec::new();
    for &row in import_rows {
        if let Some(line) = lines.get(row) {
            result.push(line.to_string());
        }
    }
    result.join("\n")
}

fn is_inside_impl_or_trait(node: tree_sitter::Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "impl_item" || parent.kind() == "trait_item" {
            return true;
        }
        current = parent.parent();
    }
    false
}

pub fn canonicalize_file_path(repo_path: &Path, file_path: &Path) -> String {
    let canonical = if file_path.is_absolute() {
        match file_path.strip_prefix(repo_path) {
            Ok(rel) => rel.to_path_buf(),
            Err(_) => file_path.to_path_buf(),
        }
    } else {
        file_path.to_path_buf()
    };

    let s = canonical.to_string_lossy().to_string();
    let s = s.replace('\\', "/");
    let s = s.strip_prefix("./").unwrap_or(&s);
    s.to_string()
}

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

    #[test]
    fn extract_symbols_basic() {
        let source = r#"
fn hello() {
    println!("hello");
}

struct Point {
    x: i32,
    y: i32,
}

enum Color {
    Red,
    Green,
}
"#;
        let file_path = std::path::Path::new("src/lib.rs");
        let symbols = super::extract_symbols(file_path, source).expect("should extract symbols");

        let fns: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Function)
            .collect();
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].identifier, "src/lib.rs::hello");

        let structs: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Struct)
            .collect();
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].identifier, "src/lib.rs::Point");

        let enums: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Enum)
            .collect();
        assert_eq!(enums.len(), 1);
        assert_eq!(enums[0].identifier, "src/lib.rs::Color");
    }

    #[test]
    fn extract_symbols_doc_comments() {
        let source = r#"
/// Adds two numbers together.
/// Returns the sum.
fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;
        let file_path = std::path::Path::new("src/math.rs");
        let symbols = super::extract_symbols(file_path, source).expect("should extract symbols");

        let fns: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Function)
            .collect();
        assert_eq!(fns.len(), 1);
        assert!(fns[0].text.contains("/// Adds two numbers together."));
        assert!(fns[0].text.contains("fn add(a: i32, b: i32) -> i32"));
    }

    #[test]
    fn extract_symbols_impl_method() {
        let source = r#"
struct Foo;

impl Foo {
    fn bar(&self) -> i32 {
        42
    }
}
"#;
        let file_path = std::path::Path::new("src/foo.rs");
        let symbols = super::extract_symbols(file_path, source).expect("should extract symbols");

        let methods: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::ImplMethod)
            .collect();
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].identifier, "src/foo.rs::Foo::bar");
    }

    #[test]
    fn extract_symbols_nested_module() {
        let source = r#"
mod utils {
    pub fn helper() -> bool {
        true
    }
}
"#;
        let file_path = std::path::Path::new("src/lib.rs");
        let symbols = super::extract_symbols(file_path, source).expect("should extract symbols");

        let fns: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Function)
            .collect();
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].identifier, "src/lib.rs::utils::helper");
    }

    #[test]
    fn extract_symbols_comments_and_imports_not_embedded() {
        let source = r#"
use std::collections::HashMap;

// This is a standalone comment

fn do_work() {
    // inline comment
    println!("work");
}
"#;
        let file_path = std::path::Path::new("src/main.rs");
        let symbols = super::extract_symbols(file_path, source).expect("should extract symbols");

        let comments: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Comments)
            .collect();
        assert_eq!(comments.len(), 1);
        assert!(!comments[0].embed);

        let imports: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Imports)
            .collect();
        assert_eq!(imports.len(), 1);
        assert!(!imports[0].embed);
    }

    #[test]
    fn compose_symbol_text_joins_parts() {
        let text = super::compose_symbol_text("/// doc", "fn foo()", "{ body }");
        assert_eq!(text, "/// doc\nfn foo()\n{ body }");

        let text = super::compose_symbol_text("", "fn bar()", "");
        assert_eq!(text, "fn bar()");

        let text = super::compose_symbol_text("/// only", "", "");
        assert_eq!(text, "/// only");
    }

    #[test]
    fn canonicalize_file_path_works() {
        let repo = std::path::Path::new("/home/user/project");
        let abs = std::path::Path::new("/home/user/project/src/lib.rs");
        assert_eq!(super::canonicalize_file_path(repo, abs), "src/lib.rs");

        let rel = std::path::Path::new("./src/main.rs");
        assert_eq!(super::canonicalize_file_path(repo, rel), "src/main.rs");

        let nested = std::path::Path::new("crates/foo/src/bar.rs");
        assert_eq!(
            super::canonicalize_file_path(repo, nested),
            "crates/foo/src/bar.rs"
        );
    }

    #[test]
    fn extract_symbols_trait_method() {
        let source = r#"
trait Greeter {
    fn greet(&self) -> String {
        "hello".to_string()
    }
}
"#;
        let file_path = std::path::Path::new("src/greet.rs");
        let symbols = super::extract_symbols(file_path, source).expect("should extract symbols");

        let methods: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::TraitMethod)
            .collect();
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].identifier, "src/greet.rs::Greeter::greet");
    }

    #[test]
    fn extract_symbols_body_excerpt_truncation() {
        let mut body_lines = String::from("fn long_function() {\n");
        for i in 0..20 {
            body_lines.push_str(&format!("    let x{i} = {i};\n"));
        }
        body_lines.push('}');

        let file_path = std::path::Path::new("src/lib.rs");
        let symbols =
            super::extract_symbols(file_path, &body_lines).expect("should extract symbols");

        let fns: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Function)
            .collect();
        assert_eq!(fns.len(), 1);
        assert!(
            fns[0].text.contains("..."),
            "body excerpt should be truncated"
        );
    }
}
