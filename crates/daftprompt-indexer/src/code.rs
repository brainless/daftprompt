use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

/// Supported source languages for code indexing.
///
/// Languages are addressed by canonical lowercase metadata string (see
/// [`CodeLanguage::as_str`]). New dialects extend this enum and register a
/// [`LanguageConfig`] via [`CodeExtractor::for_language`] (Task 2/3) — see
/// the `REGISTRY` comment for the construction site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeLanguage {
    Rust,
    TypeScript,
    Tsx,
}

impl CodeLanguage {
    /// Canonical lowercase identifier used in stored metadata and external APIs.
    pub fn as_str(&self) -> &'static str {
        match self {
            CodeLanguage::Rust => "rust",
            CodeLanguage::TypeScript => "typescript",
            CodeLanguage::Tsx => "tsx",
        }
    }
}

/// Symbol kinds stored in metadata.
///
/// Display formatting (`{:?}`) is intentionally NOT the storage format —
/// [`SymbolKind::as_str`] / [`SymbolKind::from_str_key`] are the explicit
/// (de)serialization surface so unknown values do not silently round-trip
/// through `Function` (Epic 009 Design Decision #3).
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
    // ── Epic 009 Task 1 ────────────────────────────────────────────────────
    // TypeScript/TSX evidence shapes. Wired into the Rust symbol search
    // path as well so callers never see a parse failure for these names.
    Class,
    Interface,
    Method,
    Variable,
    /// Stored kind was not recognized during deserialization. Diagnostic-only
    /// for the lifetime of a single read; never produced by Rust extraction.
    Unknown(String),
}

impl SymbolKind {
    /// Explicit lowercase storage key.
    pub fn as_str(&self) -> &str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::ImplMethod => "implmethod",
            SymbolKind::TraitMethod => "traitmethod",
            SymbolKind::TypeAlias => "typealias",
            SymbolKind::Const => "const",
            SymbolKind::Static => "static",
            SymbolKind::Module => "module",
            SymbolKind::Macro => "macro",
            SymbolKind::Comments => "comments",
            SymbolKind::Imports => "imports",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Method => "method",
            SymbolKind::Variable => "variable",
            SymbolKind::Unknown(_) => "unknown",
        }
    }

    /// Parse a stored kind key.
    ///
    /// Unknown strings return [`SymbolKind::Unknown`] with the original
    /// value preserved for diagnostics; they no longer collapse to
    /// `Function` (Epic 009 Design Decision #3).
    pub fn from_str_key(s: &str) -> SymbolKind {
        match s {
            "function" => SymbolKind::Function,
            "struct" => SymbolKind::Struct,
            "enum" => SymbolKind::Enum,
            "trait" => SymbolKind::Trait,
            "implmethod" => SymbolKind::ImplMethod,
            "traitmethod" => SymbolKind::TraitMethod,
            "typealias" => SymbolKind::TypeAlias,
            "const" => SymbolKind::Const,
            "static" => SymbolKind::Static,
            "module" => SymbolKind::Module,
            "macro" => SymbolKind::Macro,
            "comments" => SymbolKind::Comments,
            "imports" => SymbolKind::Imports,
            "class" => SymbolKind::Class,
            "interface" => SymbolKind::Interface,
            "method" => SymbolKind::Method,
            "variable" => SymbolKind::Variable,
            other => SymbolKind::Unknown(other.to_string()),
        }
    }
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

/// Per-dialect configuration: tree-sitter language, canonical extensions,
/// and the metadata identifier. The compiled [`Query`] lives on the owning
/// [`CodeExtractor`] so it is built once per process (Epic 009 Design
/// Decision #1: queries are compiled at construction, not per file).
#[derive(Debug, Clone, Copy)]
pub struct LanguageConfig {
    pub language: CodeLanguage,
    /// Canonical (lowercase, leading-dot) file extensions routed to this dialect.
    pub extensions: &'static [&'static str],
}

impl LanguageConfig {
    /// Returns true if `extension` (e.g. `"ts"`, `"tsx"`, `"rs"`) is
    /// supported by this language config. Comparison is case-insensitive.
    pub fn supports(&self, extension: &str) -> bool {
        self.extensions
            .iter()
            .any(|e| e.eq_ignore_ascii_case(extension))
    }
}

/// Built-once extractor for a single dialect.
///
/// Constructed at indexer construction time so the tree-sitter query is
/// compiled once and reused for every file of that dialect. This is the
/// Epic 009 invariant that supersedes the per-file `Query::new` previously
/// sitting inside `extract_symbols_in_repo`.
pub struct CodeExtractor {
    pub config: LanguageConfig,
    tree_sitter_language: tree_sitter::Language,
    query: Query,
}

impl CodeExtractor {
    /// Construct a Rust extractor with the canonical query.
    pub fn rust() -> Self {
        Self::for_language(CodeLanguage::Rust)
    }

    /// Construct an extractor for any registered dialect.
    ///
    /// Task 2/3 register the TypeScript and TSX dialects here. For now only
    /// Rust is wired (the grammar dependencies for the others land in
    /// Task 2 per the Epic plan). Unsupported languages return an error so
    /// callers cannot accidentally fall back to Rust parsing.
    pub fn for_language(language: CodeLanguage) -> Self {
        match language {
            CodeLanguage::Rust => {
                let tree_sitter_language: tree_sitter::Language =
                    tree_sitter_rust::LANGUAGE.into();
                let query = Query::new(&tree_sitter_language, RUST_QUERY)
                    .expect("rust tree-sitter query must compile");
                Self {
                    config: LanguageConfig {
                        language,
                        extensions: &["rs"],
                    },
                    tree_sitter_language,
                    query,
                }
            }
            CodeLanguage::TypeScript => {
                let tree_sitter_language: tree_sitter::Language =
                    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
                let query = Query::new(&tree_sitter_language, TS_QUERY)
                    .expect("typescript tree-sitter query must compile");
                Self {
                    config: LanguageConfig {
                        language,
                        extensions: &["ts"],
                    },
                    tree_sitter_language,
                    query,
                }
            }
            // TSX extractor arrives in Task 3.
            CodeLanguage::Tsx => {
                panic!("Tsx extractor not yet wired (introduced in Task 3)")
            }
        }
    }

    /// Tree-sitter language (passed to `Parser::set_language`).
    pub fn tree_sitter_language(&self) -> &tree_sitter::Language {
        &self.tree_sitter_language
    }

    /// Compiled query (one allocation per extractor, reused per file).
    pub fn query(&self) -> &Query {
        &self.query
    }
}

/// Map a canonical lowercase extension to its [`CodeLanguage`].
///
/// Returns `None` for unsupported extensions so the caller can refuse to
/// dispatch rather than silently fall back to Rust (Epic 009 Design
/// Decision #1: "report unsupported extensions without falling back to Rust").
///
/// `.d.ts` is intentionally not recognized as a code source in this epic
/// (Epic 009 Design Decision #2: ".d.ts declaration files are skipped
/// initially"). Callers that have already filtered via
/// [`list_tracked_code_files`] need not check again; this map simply has
/// no entry for `.d.ts`.
pub fn language_for_extension(extension: &str) -> Option<CodeLanguage> {
    match extension.to_ascii_lowercase().as_str() {
        "rs" => Some(CodeLanguage::Rust),
        "ts" => Some(CodeLanguage::TypeScript),
        "tsx" => Some(CodeLanguage::Tsx),
        _ => None,
    }
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
  type: (_) @impl_type
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

/// TypeScript query — Epic 009 Task 2.
const TS_QUERY: &str = r#"
(function_declaration
  name: (identifier) @name) @definition.function

(class_declaration
  name: (type_identifier) @name) @definition.class

(class_declaration
  name: (type_identifier) @class_name
  body: (class_body
    (method_definition
      name: (property_identifier) @name))) @definition.method

(interface_declaration
  name: (type_identifier) @name) @definition.interface

(type_alias_declaration
  name: (type_identifier) @name) @definition.type_alias

(enum_declaration
  name: (identifier) @name) @definition.enum

(internal_module
  name: (identifier) @name) @definition.module

(lexical_declaration
  (variable_declarator
    name: (identifier) @name)) @definition.lexical_decl

(variable_declaration
  (variable_declarator
    name: (identifier) @name)) @definition.var_decl

(variable_declarator
  name: (identifier) @name
  value: (arrow_function)) @definition.callable_binding

(variable_declarator
  name: (identifier) @name
  value: (function_expression)) @definition.callable_binding

(function_signature
  name: (identifier) @name) @definition.function_signature

(comment) @comment

(import_statement) @import

(export_statement
  source: (string)) @reexport
"#;

pub fn extract_symbols(file_path: &Path, source: &str) -> anyhow::Result<Vec<CodeSymbol>> {
    let repo_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    extract_symbols_in_repo(&repo_path, file_path, source)
}

/// Extract symbols with identifiers relative to `repo_path`.
///
/// Rust compatibility wrapper — delegates to [`extract_symbols_with_extractor`]
/// using a freshly-constructed Rust extractor. Production callers should
/// hold a single [`CodeExtractor`] at indexer construction time and dispatch
/// via [`extract_symbols_with_extractor`] instead of re-allocating per call.
pub fn extract_symbols_in_repo(
    repo_path: &Path,
    file_path: &Path,
    source: &str,
) -> anyhow::Result<Vec<CodeSymbol>> {
    let extractor = CodeExtractor::rust();
    extract_symbols_with_extractor(&extractor, repo_path, file_path, source)
}

/// Extract symbols using a pre-built extractor.
///
/// This is the language-neutral extraction entry point: the tree-sitter
/// query is already compiled on `extractor`, so per-file overhead is just
/// parsing and tree-sitter cursor work (Epic 009 Design Decision #1).
pub fn extract_symbols_with_extractor(
    extractor: &CodeExtractor,
    repo_path: &Path,
    file_path: &Path,
    source: &str,
) -> anyhow::Result<Vec<CodeSymbol>> {
    match extractor.config.language {
        CodeLanguage::Rust => extract_rust_symbols(extractor, repo_path, file_path, source),
        CodeLanguage::TypeScript => {
            extract_typescript_symbols(extractor, repo_path, file_path, source)
        }
        CodeLanguage::Tsx => Err(anyhow::anyhow!(
            "Tsx extraction not yet wired (introduced in Task 3)"
        )),
    }
}

fn parse_with_extractor(
    extractor: &CodeExtractor,
    file_path: &Path,
    source: &str,
) -> anyhow::Result<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser.set_language(extractor.tree_sitter_language())?;
    parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse source file: {}", file_path.display()))
}

fn extract_rust_symbols(
    extractor: &CodeExtractor,
    repo_path: &Path,
    file_path: &Path,
    source: &str,
) -> anyhow::Result<Vec<CodeSymbol>> {
    let tree = parse_with_extractor(extractor, file_path, source)?;

    let query = extractor.query();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());

    let capture_names = query.capture_names();
    let mut symbols: Vec<CodeSymbol> = Vec::new();
    let mut doc_comment_nodes: Vec<usize> = Vec::new();
    let mut standalone_comment_ranges: Vec<(usize, usize)> = Vec::new();
    let mut import_nodes: Vec<usize> = Vec::new();

    let file_path_str = canonicalize_file_path(repo_path, file_path);

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

            let identifier = build_identifier(
                &file_path_str,
                node,
                &name,
                impl_type.as_deref(),
                trait_type.as_deref(),
                source,
            );

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

    let comment_text =
        collect_standalone_comments(source, &standalone_comment_ranges, &doc_comment_nodes);
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

/// TypeScript extraction — Epic 009 Task 2.
fn extract_typescript_symbols(
    extractor: &CodeExtractor,
    repo_path: &Path,
    file_path: &Path,
    source: &str,
) -> anyhow::Result<Vec<CodeSymbol>> {
    let tree = parse_with_extractor(extractor, file_path, source)?;

    let query = extractor.query();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());

    let capture_names = query.capture_names();
    let mut symbols: Vec<CodeSymbol> = Vec::new();
    let mut standalone_comment_ranges: Vec<(usize, usize)> = Vec::new();
    let mut import_nodes: Vec<usize> = Vec::new();

    let file_path_str = canonicalize_file_path(repo_path, file_path);

    let mut function_signature_nodes: Vec<tree_sitter::Node> = Vec::new();
    let mut attached_doc_ranges: Vec<(usize, usize)> = Vec::new();

    while let Some(m) = matches.next() {
        let captures = m.captures;

        let mut symbol_name: Option<String> = None;
        let mut definition_node: Option<tree_sitter::Node> = None;
        let mut definition_kind: Option<&str> = None;
        let mut class_name: Option<String> = None;

        for cap in captures {
            let cname = capture_names[cap.index as usize];
            match cname {
                "name" => {
                    symbol_name = Some(
                        cap.node
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .to_string(),
                    );
                }
                "class_name" => {
                    class_name = Some(
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
                "import" | "reexport" => {
                    import_nodes.push(cap.node.start_position().row);
                }
                _ => {
                    if cname.starts_with("definition.") {
                        definition_node = Some(cap.node);
                        definition_kind = Some(cname);
                    }
                }
            }
        }

        let resolved = resolve_typescript_match(
            definition_kind,
            definition_node,
            symbol_name.as_deref(),
            class_name.as_deref(),
        );
        let Some((kind, effective_node, effective_name)) = resolved else {
            continue;
        };

        if is_typescript_signature_only(effective_node) {
            function_signature_nodes.push(effective_node);
            continue;
        }

        let identifier = build_ts_identifier(
            &file_path_str,
            effective_node,
            &effective_name,
            class_name.as_deref(),
            source,
        );

        let doc_comment = extract_ts_doc_comment(effective_node, source);
        let signature = extract_ts_signature(effective_node, source);
        let body_excerpt = extract_ts_body_excerpt(effective_node, source);
        let text = compose_symbol_text(&doc_comment, &signature, &body_excerpt);

        let line_start = effective_node.start_position().row + 1;
        let line_end = effective_node.end_position().row + 1;

        collect_attached_doc_ranges(effective_node, source, &mut attached_doc_ranges);

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

    dedup_callable_bindings(&mut symbols);
    fold_overloads_into_implementations(&mut symbols, source, &function_signature_nodes);

    let (default_symbol, default_attached) =
        extract_anonymous_default_export(source, &file_path_str, &tree);
    if let Some(sym) = default_symbol {
        attached_doc_ranges.extend(default_attached);
        symbols.push(sym);
    }

    let total_lines = source.lines().count();
    let filtered_comment_ranges =
        filter_out_attached_ranges(&standalone_comment_ranges, &attached_doc_ranges);

    let comment_text = collect_standalone_comments(source, &filtered_comment_ranges, &[]);
    if !comment_text.is_empty() {
        symbols.push(CodeSymbol {
            identifier: format!("{}::__comments__", file_path_str),
            text: comment_text,
            symbol_kind: SymbolKind::Comments,
            file_path: file_path_str.clone(),
            line_start: 1,
            line_end: total_lines,
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
            line_end: total_lines,
            embed: false,
        });
    }

    Ok(symbols)
}

fn resolve_typescript_match<'a>(
    definition_kind: Option<&str>,
    definition_node: Option<tree_sitter::Node<'a>>,
    symbol_name: Option<&str>,
    class_name: Option<&str>,
) -> Option<(SymbolKind, tree_sitter::Node<'a>, String)> {
    let kind_str = definition_kind?;
    let node = definition_node?;
    let name = symbol_name?.to_string();
    match kind_str {
        "definition.function" => Some((SymbolKind::Function, node, name)),
        "definition.class" => Some((SymbolKind::Class, node, name)),
        "definition.method" => {
            let effective = match class_name {
                Some(cn) => format!("{cn}::{name}"),
                None => name.clone(),
            };
            Some((SymbolKind::Method, node, effective))
        }
        "definition.interface" => Some((SymbolKind::Interface, node, name)),
        "definition.type_alias" => Some((SymbolKind::TypeAlias, node, name)),
        "definition.enum" => Some((SymbolKind::Enum, node, name)),
        "definition.module" => Some((SymbolKind::Module, node, name)),
        "definition.lexical_decl" => {
            let lexical_kind = node.child_by_field_name("kind").map(|k| k.kind());
            match lexical_kind {
                Some("const") => Some((SymbolKind::Const, node, name)),
                _ => Some((SymbolKind::Variable, node, name)),
            }
        }
        "definition.var_decl" => Some((SymbolKind::Variable, node, name)),
        "definition.callable_binding" => Some((SymbolKind::Function, node, name)),
        "definition.function_signature" => Some((SymbolKind::Function, node, name)),
        _ => None,
    }
}

fn is_typescript_signature_only(node: tree_sitter::Node) -> bool {
    if node.kind() == "function_signature" {
        return true;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_signature" {
            return true;
        }
    }
    false
}

fn fold_overloads_into_implementations(
    symbols: &mut Vec<CodeSymbol>,
    source: &str,
    signatures: &[tree_sitter::Node],
) {
    if signatures.is_empty() {
        return;
    }

    let mut by_name: std::collections::HashMap<String, Vec<tree_sitter::Node>> =
        std::collections::HashMap::new();
    for sig in signatures {
        let name_node = sig.child_by_field_name("name").or_else(|| {
            let mut cursor = sig.walk();
            for child in sig.children(&mut cursor) {
                if child.kind() == "identifier" {
                    return Some(child);
                }
            }
            None
        });
        if let Some(nn) = name_node {
            if let Ok(name) = nn.utf8_text(source.as_bytes()) {
                by_name.entry(name.to_string()).or_default().push(*sig);
            }
        }
    }
    if by_name.is_empty() {
        return;
    }

    for (name, sigs) in &by_name {
        // Snapshot the implementation's original start row before any
        // folds so the second/third signature comparison still works.
        let candidate_indices: Vec<(usize, usize)> = symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                matches!(s.symbol_kind, SymbolKind::Function)
                    && s.identifier.rsplit("::").next() == Some(name.as_str())
            })
            .map(|(i, s)| (i, s.line_start))
            .collect();
        if candidate_indices.is_empty() {
            continue;
        }

        for sig in sigs {
            let sig_row = sig.start_position().row;
            let Ok(sig_text) = sig.utf8_text(source.as_bytes()) else {
                continue;
            };
            let sig_text = sig_text.trim();
            if sig_text.is_empty() {
                continue;
            }
            // Use the snapshot of the original line_start, not the live
            // (potentially shrunk) value, so multiple signatures fold
            // against the same implementation index.
            if let Some(&(idx, impl_line_start)) = candidate_indices
                .iter()
                .filter(|(_, ls)| *ls >= sig_row + 1)
                .min_by_key(|(_, ls)| *ls)
            {
                let existing = symbols[idx].text.clone();
                let new_text = format!("{sig_text};\n{existing}");
                symbols[idx].text = new_text;

                let new_start = sig.start_position().row + 1;
                if new_start < impl_line_start {
                    symbols[idx].line_start = new_start;
                }
            }
        }
    }
}

fn extract_anonymous_default_export(
    source: &str,
    file_path_str: &str,
    tree: &tree_sitter::Tree,
) -> (Option<CodeSymbol>, Vec<(usize, usize)>) {
    fn walk<'a>(
        node: tree_sitter::Node<'a>,
        source: &str,
        file_path_str: &str,
    ) -> Option<(CodeSymbol, Vec<(usize, usize)>)> {
        if node.kind() != "export_statement" {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if let Some(found) = walk(child, source, file_path_str) {
                    return Some(found);
                }
            }
            return None;
        }
        if !has_default_keyword(node, source) {
            return None;
        }

        let mut cursor = node.walk();
        let mut body: Option<tree_sitter::Node> = None;
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_declaration" | "class_declaration" => {
                    if child.child_by_field_name("name").is_none() {
                        body = Some(child);
                        break;
                    }
                }
                "function_expression" | "class_expression" | "arrow_function" => {
                    body = Some(child);
                    break;
                }
                _ => {}
            }
        }
        let body = body?;

        let doc_comment = extract_ts_doc_comment(body, source);
        let signature = extract_ts_signature(body, source);
        let body_excerpt = extract_ts_body_excerpt(body, source);
        let text = compose_symbol_text(&doc_comment, &signature, &body_excerpt);

        let kind = match body.kind() {
            "class_declaration" | "class_expression" => SymbolKind::Class,
            _ => SymbolKind::Function,
        };

        let mut attached: Vec<(usize, usize)> = Vec::new();
        let mut current = body.prev_sibling();
        while let Some(sibling) = current {
            if sibling.kind() == "comment" {
                let text = sibling.utf8_text(source.as_bytes()).unwrap_or("");
                if text.starts_with("/**") || text.starts_with("///") {
                    attached.push((
                        sibling.start_position().row,
                        sibling.end_position().row,
                    ));
                    current = sibling.prev_sibling();
                    continue;
                }
            }
            break;
        }

        let sym = CodeSymbol {
            identifier: format!("{file_path_str}::__default_export"),
            text,
            symbol_kind: kind,
            file_path: file_path_str.to_string(),
            line_start: body.start_position().row + 1,
            line_end: body.end_position().row + 1,
            embed: true,
        };
        Some((sym, attached))
    }

    match walk(tree.root_node(), source, file_path_str) {
        Some((sym, attached)) => (Some(sym), attached),
        None => (None, Vec::new()),
    }
}

fn has_default_keyword(node: tree_sitter::Node, source: &str) -> bool {
    let text = node.utf8_text(source.as_bytes()).unwrap_or("");
    text.trim_start().starts_with("export default")
}

fn collect_attached_doc_ranges(
    node: tree_sitter::Node,
    source: &str,
    out: &mut Vec<(usize, usize)>,
) {
    collect_attached_doc_walk(node, source, out);
    let mut cur = node.parent();
    while let Some(parent) = cur {
        collect_attached_doc_walk(parent, source, out);
        cur = parent.parent();
    }
}

fn collect_attached_doc_walk(
    node: tree_sitter::Node,
    source: &str,
    out: &mut Vec<(usize, usize)>,
) {
    let mut current = node.prev_sibling();
    while let Some(sibling) = current {
        if sibling.kind() == "comment" {
            let text = sibling.utf8_text(source.as_bytes()).unwrap_or("");
            if text.starts_with("/**") || text.starts_with("///") {
                let start = sibling.start_position().row;
                let end = sibling.end_position().row;
                out.push((start, end));
                current = sibling.prev_sibling();
                continue;
            }
        }
        break;
    }
}

fn filter_out_attached_ranges(
    comment_ranges: &[(usize, usize)],
    attached: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    comment_ranges
        .iter()
        .filter(|(s, _)| !attached.iter().any(|(a, _)| a == s))
        .copied()
        .collect()
}

fn build_ts_identifier(
    file_path: &str,
    node: tree_sitter::Node,
    name: &str,
    class_name: Option<&str>,
    source: &str,
) -> String {
    let mut namespace_path: Vec<String> = Vec::new();
    let mut current = node.parent();

    while let Some(parent) = current {
        match parent.kind() {
            "internal_module" => {
                if let Some(name_node) = parent.child_by_field_name("name") {
                    if let Ok(text) = name_node.utf8_text(source.as_bytes()) {
                        namespace_path.push(text.to_string());
                    }
                }
            }
            "class_declaration" | "abstract_class_declaration" => {
                if class_name.is_none() {
                    if let Some(class_name_node) = parent.child_by_field_name("name") {
                        if let Ok(text) = class_name_node.utf8_text(source.as_bytes()) {
                            namespace_path.push(text.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
        current = parent.parent();
    }

    namespace_path.reverse();

    let mut parts = vec![file_path.to_string()];
    parts.extend(namespace_path);
    parts.push(name.to_string());
    parts.join("::")
}

fn extract_ts_doc_comment(node: tree_sitter::Node, source: &str) -> String {
    let mut comments: Vec<String> = Vec::new();
    collect_ts_doc_comments_for(node, source, &mut comments);
    let mut cur = node.parent();
    while let Some(parent) = cur {
        collect_ts_doc_comments_for(parent, source, &mut comments);
        cur = parent.parent();
    }
    comments.reverse();
    comments.join("\n")
}

fn collect_ts_doc_comments_for(
    node: tree_sitter::Node,
    source: &str,
    out: &mut Vec<String>,
) {
    let mut current = node.prev_sibling();
    while let Some(sibling) = current {
        if sibling.kind() == "comment" {
            let text = sibling.utf8_text(source.as_bytes()).unwrap_or("");
            if text.starts_with("/**") || text.starts_with("///") {
                out.push(text.to_string());
                current = sibling.prev_sibling();
                continue;
            }
        }
        break;
    }
}

fn extract_ts_signature(node: tree_sitter::Node, source: &str) -> String {
    match node.kind() {
        "function_declaration" | "function_expression" | "method_definition" => {
            let mut body_start = node.end_byte();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "statement_block" || child.kind() == "class_body" {
                    body_start = child.start_byte();
                    break;
                }
            }
            let sig_bytes = &source.as_bytes()[node.start_byte()..body_start];
            String::from_utf8_lossy(sig_bytes).trim().to_string()
        }
        "arrow_function" => {
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            if let Some(idx) = text.find("=>") {
                text[..idx + 2].trim().to_string()
            } else {
                text.lines().next().unwrap_or("").trim().to_string()
            }
        }
        "class_declaration" | "class_expression" | "abstract_class_declaration" => {
            let mut body_start = node.end_byte();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "class_body" {
                    body_start = child.start_byte();
                    break;
                }
            }
            let sig_bytes = &source.as_bytes()[node.start_byte()..body_start];
            String::from_utf8_lossy(sig_bytes).trim().to_string()
        }
        "interface_declaration" | "type_alias_declaration" | "enum_declaration" => {
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            bound_declaration_text(text, 2000)
        }
        "lexical_declaration" | "variable_declaration" => {
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            text.lines().next().unwrap_or("").trim().to_string()
        }
        _ => {
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            text.lines().next().unwrap_or("").trim().to_string()
        }
    }
}

fn bound_declaration_text(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        text.trim().to_string()
    } else {
        let mut end = limit;
        while !text.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{} ...", &text[..end])
    }
}

fn extract_ts_body_excerpt(node: tree_sitter::Node, source: &str) -> String {
    let body_node = match node.kind() {
        "function_declaration" | "function_expression" | "method_definition"
        | "arrow_function" | "class_declaration" | "class_expression"
        | "abstract_class_declaration" => {
            let mut cursor = node.walk();
            let mut found = None;
            for child in node.children(&mut cursor) {
                if child.kind() == "statement_block" || child.kind() == "class_body" {
                    found = Some(child);
                    break;
                }
            }
            found
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

/// After the TS query has produced the initial symbol list, collapse any
/// duplicate identifiers. The most common case is `const make = function()
/// {}` matching both the `lexical_declaration` (→ Const) and the
/// `variable_declarator` with `value: (function_expression)` (→
/// `callable_binding`, Function) patterns. The query match order is not
/// deterministic, so we apply precedence rules in a post-pass:
/// Function/Method > Class > Interface/TypeAlias/Enum > others.
fn dedup_callable_bindings(symbols: &mut Vec<CodeSymbol>) {
    let precedence = |k: &SymbolKind| -> u8 {
        match k {
            SymbolKind::Function | SymbolKind::Method => 3,
            SymbolKind::Class => 2,
            SymbolKind::Interface | SymbolKind::TypeAlias | SymbolKind::Enum => 1,
            _ => 0,
        }
    };
    let mut seen: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut to_remove: Vec<usize> = Vec::new();
    for (idx, sym) in symbols.iter().enumerate() {
        let key = sym.identifier.clone();
        if let Some(&prev_idx) = seen.get(&key) {
            let prev = &symbols[prev_idx];
            if precedence(&sym.symbol_kind) > precedence(&prev.symbol_kind) {
                to_remove.push(prev_idx);
                seen.insert(key, idx);
            } else {
                to_remove.push(idx);
            }
        } else {
            seen.insert(key, idx);
        }
    }
    to_remove.sort_unstable();
    to_remove.dedup();
    for &i in to_remove.iter().rev() {
        symbols.remove(i);
    }
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
        "struct_item" | "enum_item" | "trait_item" | "type_item" | "const_item" | "static_item"
        | "macro_definition" => {
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
    // Rust compatibility wrapper — production callers should use
    // `list_tracked_code_files` with an extension set, but this remains
    // stable for tests and any direct Rust-only callers.
    list_tracked_code_files(repo_path, &[".rs"])
}

/// One-pass tracked-file traversal for a configurable set of supported
/// extensions (Epic 009 Design Decision #2).
///
/// - Paths returned are absolute and rooted at the repo workdir.
/// - Untracked and `.gitignore`-d entries are excluded by virtue of being
///   outside `HEAD`'s tree.
/// - `.d.ts` declaration files are explicitly filtered out in this epic
///   (Epic 009 Design Decision #2: "skip .d.ts declaration files initially").
/// - `extensions` are compared against the full path suffix, so callers
///   should pass canonical forms including the leading dot (e.g. `.rs`).
pub fn list_tracked_code_files(
    repo_path: &Path,
    extensions: &[&str],
) -> anyhow::Result<Vec<PathBuf>> {
    let repo = gix::discover(repo_path)?;

    let work_dir = match repo.workdir() {
        Some(dir) => dir.to_path_buf(),
        None => return Ok(vec![]),
    };

    let tree_id = repo.head_tree_id_or_empty()?;
    let tree = repo.find_tree(tree_id)?;

    let entries = tree.traverse().breadthfirst.files()?;

    let mut files: Vec<PathBuf> = entries
        .into_iter()
        .filter(|entry| entry.mode.is_blob())
        .filter_map(|entry| {
            entry
                .filepath
                .to_str()
                .ok()
                .map(|p| (p.to_string(), work_dir.join(p)))
        })
        .filter(|(p, _)| {
            // Reject `.d.ts` (declaration files) regardless of whether `.ts`
            // was requested. `.d.ts` always sits next to a `.ts` file; skipping
            // it here keeps the policy in one place.
            if p.ends_with(".d.ts") {
                return false;
            }
            extensions.iter().any(|ext| p.ends_with(ext))
        })
        .map(|(_, abs)| abs)
        .collect();

    files.sort();
    Ok(files)
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

    // ── Epic 009 Task 1: language-neutral foundation tests ─────────────────

    #[test]
    fn code_language_as_str_matches_metadata() {
        assert_eq!(super::CodeLanguage::Rust.as_str(), "rust");
        assert_eq!(super::CodeLanguage::TypeScript.as_str(), "typescript");
        assert_eq!(super::CodeLanguage::Tsx.as_str(), "tsx");
    }

    #[test]
    fn language_for_extension_routes_canonical_extensions() {
        assert_eq!(
            super::language_for_extension("rs"),
            Some(super::CodeLanguage::Rust)
        );
        assert_eq!(
            super::language_for_extension("ts"),
            Some(super::CodeLanguage::TypeScript)
        );
        assert_eq!(
            super::language_for_extension("tsx"),
            Some(super::CodeLanguage::Tsx)
        );
        // case-insensitive
        assert_eq!(
            super::language_for_extension("RS"),
            Some(super::CodeLanguage::Rust)
        );
        // unsupported — must NOT silently fall back to Rust
        assert_eq!(super::language_for_extension("js"), None);
        assert_eq!(super::language_for_extension("d.ts"), None);
        assert_eq!(super::language_for_extension(""), None);
    }

    #[test]
    fn code_extractor_rust_compiles_query_once() {
        let extractor = super::CodeExtractor::rust();
        assert_eq!(extractor.config.language, super::CodeLanguage::Rust);
        assert!(extractor.config.supports("rs"));
        assert!(!extractor.config.supports("ts"));
        // Query is compiled: capture names must include the Rust kinds.
        let names: Vec<&str> = extractor.query().capture_names().to_vec();
        assert!(names.contains(&"definition.function"));
        assert!(names.contains(&"definition.struct"));
        assert!(names.contains(&"definition.trait"));
    }

    #[test]
    fn symbol_kind_serde_round_trip() {
        for kind in [
            super::SymbolKind::Function,
            super::SymbolKind::Struct,
            super::SymbolKind::Enum,
            super::SymbolKind::Trait,
            super::SymbolKind::ImplMethod,
            super::SymbolKind::TraitMethod,
            super::SymbolKind::TypeAlias,
            super::SymbolKind::Const,
            super::SymbolKind::Static,
            super::SymbolKind::Module,
            super::SymbolKind::Macro,
            super::SymbolKind::Comments,
            super::SymbolKind::Imports,
        ] {
            let key = kind.as_str();
            let parsed = super::SymbolKind::from_str_key(key);
            assert_eq!(parsed, kind, "round-trip failed for {kind:?} ({key})");
        }
    }

    #[test]
    fn symbol_kind_serde_new_variants_round_trip() {
        // Epic 009 Task 1: Class / Interface / Method / Variable serialize
        // and parse back to the same variant.
        for kind in [
            super::SymbolKind::Class,
            super::SymbolKind::Interface,
            super::SymbolKind::Method,
            super::SymbolKind::Variable,
        ] {
            let parsed = super::SymbolKind::from_str_key(kind.as_str());
            assert_eq!(parsed, kind, "round-trip failed for {kind:?}");
        }
    }

    #[test]
    fn symbol_kind_unknown_does_not_silently_become_function() {
        // Epic 009 Design Decision #3: unknown stored kinds must NOT
        // collapse to Function.
        let parsed = super::SymbolKind::from_str_key("decorator");
        assert!(
            matches!(parsed, super::SymbolKind::Unknown(ref s) if s == "decorator"),
            "expected Unknown(\"decorator\"), got {parsed:?}"
        );
    }

    #[test]
    fn list_tracked_code_files_supports_multi_extension_set() {
        // The Rust wrapper and the multi-extension API must agree on the
        // Rust-only result set against the daftprompt repo.
        let repo_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();

        let rust_only = super::list_tracked_code_files(repo_path, &[".rs"])
            .expect("list .rs files");
        let multi = super::list_tracked_code_files(repo_path, &[".rs", ".ts", ".tsx"])
            .expect("list multi-ext files");

        // Multi must contain every Rust-only file (the repo has no .ts/.tsx).
        for path in &rust_only {
            assert!(
                multi.contains(path),
                "multi-ext result set should include {path:?}"
            );
        }

        // None of the entries should be a `.d.ts` declaration file.
        for path in &multi {
            assert!(
                !path.to_string_lossy().ends_with(".d.ts"),
                "multi-ext result must not include .d.ts; got {path:?}"
            );
        }
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

    // ── Epic 008 Task 1: Evidence fixtures and extraction assertions ─────────

    /// Small but realistic "checkout" feature covering all extraction paths:
    /// product capability function, supporting type, nested module, impl method,
    /// standalone comment, doc comments, and imports.
    const CHECKOUT_FIXTURE: &str = r#"use std::collections::HashMap;

/// Process a checkout session for the given cart.
///
/// Validates the cart contents, applies any active discounts,
/// and delegates to the payment provider for charging.
fn create_checkout_session(cart: &HashMap<String, i32>) -> Result<String, String> {
    if cart.is_empty() {
        return Err("cart is empty".into());
    }
    let total: i32 = cart.values().sum();
    if total <= 0 {
        return Err("total must be positive".into());
    }
    Ok(format!("session_{}", total))
}

mod payments {
    pub fn helper() -> bool {
        true
    }
}

struct PaymentGateway;

impl PaymentGateway {
    fn process_payment(&self) -> bool {
        true
    }
}

const MAX_RETRIES: u32 = 3;

// TODO: temporary limitation — only USD currency is supported right now
use std::sync::Arc;
"#;

    fn find_symbol<'a>(
        symbols: &'a [super::CodeSymbol],
        kind: &super::SymbolKind,
        name_part: &str,
    ) -> &'a super::CodeSymbol {
        symbols
            .iter()
            .find(|s| &s.symbol_kind == kind && s.identifier.contains(name_part))
            .unwrap_or_else(|| {
                panic!(
                    "expected {kind:?} symbol containing '{name_part}'; got {:?}",
                    symbols
                        .iter()
                        .map(|s| (&s.symbol_kind, &s.identifier))
                        .collect::<Vec<_>>()
                )
            })
    }

    #[test]
    fn extract_symbols_product_capability() {
        let file_path = std::path::Path::new("src/checkout.rs");
        let symbols = super::extract_symbols(file_path, CHECKOUT_FIXTURE)
            .expect("should extract symbols from checkout fixture");

        let sym = find_symbol(
            &symbols,
            &super::SymbolKind::Function,
            "create_checkout_session",
        );

        assert_eq!(sym.identifier, "src/checkout.rs::create_checkout_session");
        assert_eq!(sym.file_path, "src/checkout.rs");
        assert_eq!(sym.line_start, 7);
        assert_eq!(sym.line_end, 16);
        assert!(sym.embed);

        // doc comment in symbol text
        assert!(
            sym.text.contains("/// Process a checkout session"),
            "text should contain doc comment; got:\n{}",
            sym.text
        );

        // signature in symbol text
        assert!(
            sym.text.contains(
                "fn create_checkout_session(cart: &HashMap<String, i32>) -> Result<String, String>"
            ),
            "text should contain signature; got:\n{}",
            sym.text
        );

        // body excerpt contains validation logic
        assert!(
            sym.text.contains("cart.is_empty()"),
            "text should contain body excerpt with validation; got:\n{}",
            sym.text
        );
    }

    #[test]
    fn extract_symbols_supporting_type() {
        let file_path = std::path::Path::new("src/checkout.rs");
        let symbols =
            super::extract_symbols(file_path, CHECKOUT_FIXTURE).expect("should extract symbols");

        let gw = find_symbol(&symbols, &super::SymbolKind::Struct, "PaymentGateway");
        assert_eq!(gw.identifier, "src/checkout.rs::PaymentGateway");
        assert_eq!(gw.file_path, "src/checkout.rs");
        assert_eq!(gw.line_start, 24);
        assert_eq!(gw.line_end, 24);
        assert!(gw.embed);

        let retries = find_symbol(&symbols, &super::SymbolKind::Const, "MAX_RETRIES");
        assert_eq!(retries.identifier, "src/checkout.rs::MAX_RETRIES");
        assert!(retries.text.contains("const MAX_RETRIES: u32 = 3"));
        assert!(retries.embed);
    }

    #[test]
    fn extract_symbols_nested_module_and_impl() {
        let file_path = std::path::Path::new("src/checkout.rs");
        let symbols =
            super::extract_symbols(file_path, CHECKOUT_FIXTURE).expect("should extract symbols");

        let helper = find_symbol(&symbols, &super::SymbolKind::Function, "payments::helper");
        assert_eq!(helper.identifier, "src/checkout.rs::payments::helper");
        assert_eq!(helper.line_start, 19);
        assert_eq!(helper.line_end, 21);
        assert!(helper.embed);

        let method = find_symbol(&symbols, &super::SymbolKind::ImplMethod, "process_payment");
        assert_eq!(
            method.identifier,
            "src/checkout.rs::PaymentGateway::process_payment"
        );
        assert_eq!(method.line_start, 27);
        assert_eq!(method.line_end, 29);
        assert!(method.embed);
    }

    #[test]
    fn extract_symbols_standalone_comments_contract() {
        let file_path = std::path::Path::new("src/checkout.rs");
        let symbols =
            super::extract_symbols(file_path, CHECKOUT_FIXTURE).expect("should extract symbols");

        let comments: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Comments)
            .collect();
        assert_eq!(comments.len(), 1, "expected exactly one Comments record");

        let rec = &comments[0];
        assert!(!rec.embed, "Comments record must have embed=false");
        assert_eq!(rec.identifier, "src/checkout.rs::__comments__");
        assert!(
            rec.text.contains("TODO: temporary limitation"),
            "comments text should contain TODO; got:\n{}",
            rec.text
        );
        assert!(
            rec.text.contains("only USD currency"),
            "comments text should contain the limitation note; got:\n{}",
            rec.text
        );
    }

    #[test]
    fn extract_symbols_imports_contract() {
        let file_path = std::path::Path::new("src/checkout.rs");
        let symbols =
            super::extract_symbols(file_path, CHECKOUT_FIXTURE).expect("should extract symbols");

        let imports: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Imports)
            .collect();
        assert_eq!(imports.len(), 1, "expected exactly one Imports record");

        let rec = &imports[0];
        assert!(!rec.embed, "Imports record must have embed=false");
        assert_eq!(rec.identifier, "src/checkout.rs::__imports__");
        assert!(
            rec.text.contains("use std::collections::HashMap"),
            "imports should contain HashMap import; got:\n{}",
            rec.text
        );
        assert!(
            rec.text.contains("use std::sync::Arc"),
            "imports should contain Arc import; got:\n{}",
            rec.text
        );
    }

    // ── Epic 009 Task 2: TypeScript extraction tests ─────────────────────

    const TS_CHECKOUT_FIXTURE: &str = r#"import { PaymentProvider } from "./payment";

/**
 * Configuration for the checkout service.
 *
 * The default timeout applies unless the caller passes an override.
 */
export interface CheckoutConfig {
    timeoutMs: number;
    provider: "stripe" | "manual";
}

/**
 * A read-only DTO returned by the checkout API.
 */
export type CheckoutResult = {
    sessionId: string;
    status: "ok" | "failed";
};

/** Maximum retries before the checkout fails. */
export const MAX_RETRIES = 3;

/**
 * Process a checkout session for the given cart.
 *
 * Validates the cart contents, applies any active discounts,
 * and delegates to the payment provider for charging.
 */
export function createCheckoutSession(
    cart: { sku: string; qty: number }[],
    config: CheckoutConfig
): Promise<CheckoutResult> {
    if (cart.length === 0) {
        throw new Error("cart is empty");
    }
    return Promise.resolve({
        sessionId: "session_" + cart.length,
        status: "ok",
    });
}

/** A small helper that delegates to the payment provider. */
export const chargeCustomer = (amount: number, provider: PaymentProvider): boolean =>
    provider.charge(amount);

/**
 * The gateway that owns payment-provider orchestration.
 */
export class PaymentGateway {
    constructor(private readonly provider: PaymentProvider) {}

    /** Charge the configured amount and return true on success. */
    charge(amount: number): boolean {
        return this.provider.charge(amount);
    }

    get configured(): boolean {
        return this.provider.isConfigured();
    }
}

export { default as legacy } from "./legacy";
// TODO: temporary limitation — only USD currency is supported right now
export function helper() { return 1; }
"#;

    fn extract_ts(file_path: &str, source: &str) -> Vec<super::CodeSymbol> {
        let extractor = super::CodeExtractor::for_language(super::CodeLanguage::TypeScript);
        let path = std::path::Path::new(file_path);
        let repo = std::path::Path::new(".");
        super::extract_symbols_with_extractor(&extractor, repo, path, source)
            .expect("typescript extraction")
    }

    fn kind_is(s: &super::CodeSymbol, k: &super::SymbolKind) -> bool {
        std::mem::discriminant(&s.symbol_kind) == std::mem::discriminant(k)
    }

    #[test]
    fn ts_extract_public_function_with_jsdoc() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let sym = symbols
            .iter()
            .find(|s| kind_is(s, &super::SymbolKind::Function) && s.identifier.contains("createCheckoutSession"))
            .expect("createCheckoutSession Function symbol");
        assert_eq!(sym.identifier, "src/checkout/session.ts::createCheckoutSession");
        assert!(sym.embed);
        assert!(sym.text.contains("Process a checkout session"), "JSDoc: {}", sym.text);
        assert!(sym.text.contains("createCheckoutSession"), "signature: {}", sym.text);
    }

    #[test]
    fn ts_extract_supporting_interface_and_type_alias() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let iface = symbols.iter().find(|s| kind_is(s, &super::SymbolKind::Interface) && s.identifier.contains("CheckoutConfig")).unwrap();
        assert_eq!(iface.identifier, "src/checkout/session.ts::CheckoutConfig");
        let alias = symbols.iter().find(|s| kind_is(s, &super::SymbolKind::TypeAlias) && s.identifier.contains("CheckoutResult")).unwrap();
        assert_eq!(alias.identifier, "src/checkout/session.ts::CheckoutResult");
    }

    #[test]
    fn ts_extract_configuration_const() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let c = symbols.iter().find(|s| kind_is(s, &super::SymbolKind::Const) && s.identifier.contains("MAX_RETRIES")).unwrap();
        assert_eq!(c.identifier, "src/checkout/session.ts::MAX_RETRIES");
    }

    #[test]
    fn ts_extract_class_method_with_class_prefix() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let m = symbols.iter().find(|s| kind_is(s, &super::SymbolKind::Method) && s.identifier.contains("charge")).unwrap();
        assert_eq!(m.identifier, "src/checkout/session.ts::PaymentGateway::charge");
        assert!(m.text.contains("provider.charge"));
    }

    #[test]
    fn ts_extract_callable_binding_vs_variable_kind() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let callable = symbols.iter().find(|s| kind_is(s, &super::SymbolKind::Function) && s.identifier.contains("chargeCustomer")).expect("callable binding");
        assert_eq!(callable.identifier, "src/checkout/session.ts::chargeCustomer");
        assert!(!symbols.iter().any(|s| kind_is(s, &super::SymbolKind::Const) && s.identifier.contains("chargeCustomer")), "must NOT also be Const");
    }

    #[test]
    fn ts_extract_imports_record_is_fts_only_and_collects_reexports() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let imports: Vec<_> = symbols.iter().filter(|s| s.symbol_kind == super::SymbolKind::Imports).collect();
        assert_eq!(imports.len(), 1);
        let rec = &imports[0];
        assert!(!rec.embed);
        assert_eq!(rec.identifier, "src/checkout/session.ts::__imports__");
        assert!(rec.text.contains("PaymentProvider"));
        assert!(rec.text.contains("./legacy"));
    }

    #[test]
    fn ts_extract_standalone_limitation_comment() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let comments: Vec<_> = symbols.iter().filter(|s| s.symbol_kind == super::SymbolKind::Comments).collect();
        assert_eq!(comments.len(), 1);
        let rec = &comments[0];
        assert!(!rec.embed);
        assert!(rec.text.contains("temporary limitation"));
        assert!(rec.text.contains("USD currency"));
        // JSDoc not duplicated.
        let fn_sym = symbols.iter().find(|s| kind_is(s, &super::SymbolKind::Function) && s.identifier.contains("createCheckoutSession")).unwrap();
        assert!(fn_sym.text.contains("Process a checkout session"));
        assert!(!rec.text.contains("Process a checkout session"));
    }

    #[test]
    fn ts_extract_variable_bindings_let_var() {
        let src = "let count = 0;\nvar legacy = \"x\";\n";
        let symbols = extract_ts("src/vars.ts", src);
        let count = symbols.iter().find(|s| s.identifier.contains("count")).unwrap();
        assert_eq!(count.symbol_kind, super::SymbolKind::Variable);
        let legacy = symbols.iter().find(|s| s.identifier.contains("legacy")).unwrap();
        assert_eq!(legacy.symbol_kind, super::SymbolKind::Variable);
    }

    #[test]
    fn ts_extract_empty_file_emits_no_symbols() {
        let symbols = extract_ts("src/empty.ts", "");
        assert!(symbols.is_empty());
    }

    #[test]
    fn ts_extract_comment_only_file_emits_comments_only() {
        let src = "// nothing implemented yet\n// still nothing\n";
        let symbols = extract_ts("src/comments-only.ts", src);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].symbol_kind, super::SymbolKind::Comments);
        assert!(symbols[0].text.contains("nothing implemented yet"));
    }

    #[test]
    fn ts_extract_syntax_error_file_partial_extraction() {
        // Two parseable declarations followed by a trailing token that
        // doesn't close, exercising error-recovery without wrapping the
        // whole file in a single ERROR node.
        let src = "\
export function broken(a: number): number {\n    return a;\n}\n\
\nexport interface Half {\n    name: string;\n}\n\
\nexport const dangling = {\n    x: 1,\n";
        let symbols = extract_ts("src/broken.ts", src);

        // The function and interface parse cleanly.
        let broken = symbols
            .iter()
            .find(|s| kind_is(s, &super::SymbolKind::Function) && s.identifier.contains("broken"))
            .expect("partial extraction should still capture the function");
        assert_eq!(broken.identifier, "src/broken.ts::broken");
        let half = symbols
            .iter()
            .find(|s| kind_is(s, &super::SymbolKind::Interface) && s.identifier.contains("Half"))
            .expect("partial extraction should still capture interface Half");
        assert_eq!(half.identifier, "src/broken.ts::Half");

        // No panic, no error from the caller — extraction succeeded.
    }

    #[test]
    fn ts_extract_function_overloads_folded_into_implementation() {
        let src = "export function foo(x: number): number;\nexport function foo(x: string): string;\nexport function foo(x: any): any {\n    return x;\n}\n";
        let symbols = extract_ts("src/overloads.ts", src);
        let foo_records: Vec<_> = symbols
            .iter()
            .filter(|s| kind_is(s, &super::SymbolKind::Function) && s.identifier.contains("foo") && !s.identifier.contains("__"))
            .collect();
        assert_eq!(
            foo_records.len(),
            1,
            "expected one folded record; got: {:?}",
            foo_records
                .iter()
                .map(|s| (&s.identifier, &s.text))
                .collect::<Vec<_>>()
        );
        let foo = foo_records[0];
        assert_eq!(foo.identifier, "src/overloads.ts::foo");
        assert!(
            foo.text.contains("function foo(x: number): number"),
            "first overload: {}",
            foo.text
        );
        assert!(
            foo.text.contains("function foo(x: string): string"),
            "second overload: {}",
            foo.text
        );
        assert!(foo.text.contains("return x;"));
    }

    #[test]
    fn ts_extract_anonymous_default_export_uses_stable_suffix() {
        let src = "export default function () {\n    return \"default\";\n}\n";
        let symbols = extract_ts("src/default.ts", src);
        let default = symbols.iter().find(|s| s.identifier.contains("__default_export"))
            .expect("anonymous default export");
        assert_eq!(default.identifier, "src/default.ts::__default_export");
        assert_eq!(default.symbol_kind, super::SymbolKind::Function);
        assert!(default.embed);
    }

    #[test]
    fn ts_extract_nested_namespace_identifier() {
        let src = "export namespace Outer {\n    export namespace Inner {\n        export function nested(): number {\n            return 42;\n        }\n\n        export interface InnerConfig {\n            enabled: boolean;\n        }\n    }\n}\n";
        let symbols = extract_ts("src/namespaces.ts", src);
        let nested = symbols.iter().find(|s| s.identifier.contains("nested")).expect("namespaced function");
        assert_eq!(nested.identifier, "src/namespaces.ts::Outer::Inner::nested");
        let iface = symbols.iter().find(|s| s.identifier.contains("InnerConfig")).expect("namespaced interface");
        assert_eq!(iface.identifier, "src/namespaces.ts::Outer::Inner::InnerConfig");
    }

    #[test]
    fn ts_extract_enum() {
        let src = "export enum Color {\n    Red,\n    Green,\n    Blue,\n}\n";
        let symbols = extract_ts("src/enum.ts", src);
        let color = symbols.iter().find(|s| s.identifier.contains("Color")).expect("enum Color");
        assert_eq!(color.identifier, "src/enum.ts::Color");
        assert_eq!(color.symbol_kind, super::SymbolKind::Enum);
    }

    #[test]
    fn ts_extract_function_expression_binding() {
        let src = "export const make = function (): string {\n    return \"ok\";\n};\n";
        let symbols = extract_ts("src/binding.ts", src);
        let make = symbols.iter().find(|s| s.identifier.contains("make")).expect("function-expression binding");
        assert_eq!(make.symbol_kind, super::SymbolKind::Function);
        assert_eq!(make.identifier, "src/binding.ts::make");
    }

    #[test]
    fn ts_extract_body_excerpt_truncation() {
        let mut src = String::from("export function long(): void {\n");
        for i in 0..20 {
            src.push_str(&format!("    const x{i} = {i};\n"));
        }
        src.push_str("}\n");
        let symbols = extract_ts("src/long.ts", &src);
        let long = symbols.iter().find(|s| s.identifier.contains("long")).expect("function");
        assert!(long.text.contains("..."), "body excerpt truncated: {}", long.text);
    }
}
