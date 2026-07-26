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
    // ── Epic 010 Task 1 ────────────────────────────────────────────────────
    // JavaScript and JSX. The grammar (`tree-sitter-javascript = 0.25`) covers
    // both `.js` and `.jsx` natively — the scanner accepts JSX text when the
    // grammar requests `JSX_TEXT` — so they share one tree-sitter Language.
    // Two distinct enum variants keep language metadata, source-of-truth
    // identifiers, and tests distinct across the two extensions.
    JavaScript,
    Jsx,
}

impl CodeLanguage {
    /// Canonical lowercase identifier used in stored metadata and external APIs.
    pub fn as_str(&self) -> &'static str {
        match self {
            CodeLanguage::Rust => "rust",
            CodeLanguage::TypeScript => "typescript",
            CodeLanguage::Tsx => "tsx",
            CodeLanguage::JavaScript => "javascript",
            CodeLanguage::Jsx => "jsx",
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
    /// The query is compiled once at construction (Epic 009 Design Decision
    /// #1: queries are compiled at construction, not per file). If the
    /// grammar accepts the query, this function panics: a query that fails
    /// to compile is a deployment-time error, never a per-file error.
    pub fn for_language(language: CodeLanguage) -> Self {
        match language {
            CodeLanguage::Rust => {
                let tree_sitter_language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
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
            CodeLanguage::Tsx => {
                let tree_sitter_language: tree_sitter::Language =
                    tree_sitter_typescript::LANGUAGE_TSX.into();
                let query = Query::new(&tree_sitter_language, TS_QUERY)
                    .expect("tsx tree-sitter query must compile");
                Self {
                    config: LanguageConfig {
                        language,
                        extensions: &["tsx"],
                    },
                    tree_sitter_language,
                    query,
                }
            }
            // Epic 010 Task 1: JavaScript and JSX share the same grammar
            // (the JS scanner parses JSX text via `valid_symbols[JSX_TEXT]`).
            // Both share [`JS_QUERY`] because it is grammar-compatible with
            // both `.js` and `.jsx` source — see the epic's "smoke test"
            // requirement above.
            CodeLanguage::JavaScript => {
                let tree_sitter_language: tree_sitter::Language =
                    tree_sitter_javascript::LANGUAGE.into();
                let query = Query::new(&tree_sitter_language, JS_QUERY)
                    .expect("javascript tree-sitter query must compile");
                Self {
                    config: LanguageConfig {
                        language,
                        extensions: &["js"],
                    },
                    tree_sitter_language,
                    query,
                }
            }
            CodeLanguage::Jsx => {
                let tree_sitter_language: tree_sitter::Language =
                    tree_sitter_javascript::LANGUAGE.into();
                let query = Query::new(&tree_sitter_language, JS_QUERY)
                    .expect("jsx tree-sitter query must compile");
                Self {
                    config: LanguageConfig {
                        language,
                        extensions: &["jsx"],
                    },
                    tree_sitter_language,
                    query,
                }
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
/// Comparison is case-insensitive so callers can pass raw
/// `Path::extension()` output directly.
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
        "js" => Some(CodeLanguage::JavaScript),
        "jsx" => Some(CodeLanguage::Jsx),
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
      name: (property_identifier) @name) @definition.method))

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

(call_expression
  function: (import)
  arguments: (arguments
    (string))) @dynamic_import
"#;

/// JavaScript query — Epic 010 Task 1.
///
/// Compiles against the [`tree_sitter_javascript`] grammar; the same query
/// is reused for both `.js` and `.jsx` since the grammar accepts JSX
/// natively. It intentionally omits the TypeScript-only captures
/// (`interface_declaration`, `type_alias_declaration`, `enum_declaration`,
/// `internal_module`, `function_signature`) that appear in [`TS_QUERY`] —
/// the JS grammar does not expose those node kinds.
///
/// Rejected alternative: reuse [`TS_QUERY`] as-is and rely on tree-sitter
/// error recovery for JS-only files. Rejected because the JS query must
/// compile cleanly when the JS grammar is loaded (Epic 010 acceptance
/// criterion: "no TypeScript-only captures"), and error recovery in a TS
/// query against the JS grammar produces silently empty captures for
/// `lexical_declaration` on `let` / `const`.
const JS_QUERY: &str = r#"
(function_declaration
  name: (identifier) @name) @definition.function

(generator_function_declaration
  name: (identifier) @name) @definition.function

(class_declaration
  name: (identifier) @name) @definition.class

(class_declaration
  name: (identifier) @class_name
  body: (class_body
    (method_definition
      name: (property_identifier) @name) @definition.method))

(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: [(arrow_function) (function_expression)]) @definition.callable_binding)

(variable_declaration
  (variable_declarator
    name: (identifier) @name)) @definition.var_decl

(variable_declarator
  name: (identifier) @name
  value: [(arrow_function) (function_expression)]) @definition.callable_binding

(pair
  key: (property_identifier) @name
  value: [(arrow_function) (function_expression)]) @definition.object_method

(assignment_expression
  left: [
    (identifier) @name
    (member_expression
      property: (property_identifier) @name)
  ]
  right: [(arrow_function) (function_expression)]) @definition.assignment_binding

(lexical_declaration
  (variable_declarator
    name: (identifier) @name)) @definition.lexical_decl

(comment) @comment

(import_statement) @import

(export_statement
  source: (string)) @reexport

(call_expression
  function: (import)
  arguments: (arguments
    (string))) @dynamic_import
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
        CodeLanguage::TypeScript | CodeLanguage::Tsx => {
            extract_typescript_symbols(extractor, repo_path, file_path, source)
        }
        CodeLanguage::JavaScript | CodeLanguage::Jsx => {
            extract_javascript_symbols(extractor, repo_path, file_path, source)
        }
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
    let mut import_nodes: Vec<(usize, usize)> = Vec::new();

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
                    import_nodes.push((cap.node.start_byte(), cap.node.end_byte()));
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
    let mut import_ranges: Vec<(usize, usize)> = Vec::new();

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
                "import" | "reexport" | "dynamic_import" => {
                    import_ranges.push((cap.node.start_byte(), cap.node.end_byte()));
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

        if is_nested_executable_declaration(effective_node)
            && (matches!(kind, SymbolKind::Function)
                || declaration_contains_callable(effective_node))
        {
            continue;
        }

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

    let import_text = collect_imports(source, &import_ranges);
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

/// JavaScript extraction — Epic 010 Task 1 skeleton.
///
/// This is the language-neutral extraction entry point shared by `.js` and
/// `.jsx` files. Task 1 establishes the wiring: the JS query compiles once
/// (see [`CodeExtractor::for_language`]), the production extractor fields
/// exist on [`Indexer`], and indexed metadata carries the canonical
/// `javascript` / `jsx` language string.
///
/// The richer JSDoc attachment, import-source ranges, object-literal
/// method identifiers, prototype assignment handling, JSX-aware body
/// excerpts, and the nested-symbol exclusion contract land in Tasks 2
/// and 3. The skeleton intentionally keeps the symbol set minimal so
/// Task 2 can replace the body without callers noticing a change in
/// insertion shape (each symbol is one row of `items`).
fn extract_javascript_symbols(
    extractor: &CodeExtractor,
    repo_path: &Path,
    file_path: &Path,
    source: &str,
) -> anyhow::Result<Vec<CodeSymbol>> {
    let tree = parse_with_extractor(extractor, file_path, source)?;
    let file_path_str = canonicalize_file_path(repo_path, file_path);
    let mut symbols = Vec::new();
    let mut comment_ranges = Vec::new();
    let mut attached_doc_ranges = Vec::new();
    let mut import_ranges = Vec::new();

    collect_javascript_comments(tree.root_node(), &mut comment_ranges);
    collect_javascript_imports(tree.root_node(), &mut import_ranges);

    let mut cursor = tree.root_node().walk();
    let top_level: Vec<tree_sitter::Node> = tree.root_node().children(&mut cursor).collect();
    for node in top_level {
        process_javascript_module_node(
            node,
            source,
            &file_path_str,
            &mut symbols,
            &mut attached_doc_ranges,
            &mut import_ranges,
        );
    }

    dedup_callable_bindings(&mut symbols);

    let total_lines = source.lines().count();
    let comment_text = collect_standalone_comments(
        source,
        &filter_out_attached_ranges(&comment_ranges, &attached_doc_ranges),
        &[],
    );
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

    let import_text = collect_imports(source, &import_ranges);
    if !import_text.is_empty() {
        symbols.push(CodeSymbol {
            identifier: format!("{}::__imports__", file_path_str),
            text: import_text,
            symbol_kind: SymbolKind::Imports,
            file_path: file_path_str,
            line_start: 1,
            line_end: total_lines,
            embed: false,
        });
    }

    Ok(symbols)
}

fn process_javascript_module_node(
    node: tree_sitter::Node,
    source: &str,
    file_path: &str,
    symbols: &mut Vec<CodeSymbol>,
    attached_doc_ranges: &mut Vec<(usize, usize)>,
    import_ranges: &mut Vec<(usize, usize)>,
) {
    match node.kind() {
        "import_statement" => {}
        "export_statement" => {
            if let Some(declaration) = node.child_by_field_name("declaration") {
                process_javascript_declaration(
                    declaration,
                    node,
                    source,
                    file_path,
                    symbols,
                    attached_doc_ranges,
                    import_ranges,
                );
                if javascript_is_default_export(node, source)
                    && declaration.child_by_field_name("name").is_none()
                    && matches!(
                        declaration.kind(),
                        "function_declaration"
                            | "generator_function_declaration"
                            | "class_declaration"
                    )
                {
                    let kind = if declaration.kind() == "class_declaration" {
                        SymbolKind::Class
                    } else {
                        SymbolKind::Function
                    };
                    push_javascript_symbol(
                        symbols,
                        attached_doc_ranges,
                        &declaration,
                        node,
                        "__default_export",
                        kind,
                        None,
                        source,
                        file_path,
                    );
                    if declaration.kind() == "class_declaration" {
                        process_javascript_class_methods(
                            declaration,
                            "__default_export",
                            source,
                            file_path,
                            symbols,
                            attached_doc_ranges,
                        );
                    }
                }
            } else if let Some(value) = node.child_by_field_name("value") {
                if javascript_is_default_export(node, source)
                    && javascript_is_callable_or_class(value)
                {
                    let kind = if value.kind() == "class" {
                        SymbolKind::Class
                    } else {
                        SymbolKind::Function
                    };
                    push_javascript_symbol(
                        symbols,
                        attached_doc_ranges,
                        &node,
                        node,
                        "__default_export",
                        kind,
                        None,
                        source,
                        file_path,
                    );
                    if value.kind() == "class" {
                        process_javascript_class_methods(
                            value,
                            "__default_export",
                            source,
                            file_path,
                            symbols,
                            attached_doc_ranges,
                        );
                    }
                }
            }
        }
        "function_declaration" | "generator_function_declaration" => {
            process_javascript_function_declaration(
                node,
                node,
                source,
                file_path,
                symbols,
                attached_doc_ranges,
            );
        }
        "class_declaration" => {
            let Some(name) = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            else {
                return;
            };
            push_javascript_symbol(
                symbols,
                attached_doc_ranges,
                &node,
                node,
                name,
                SymbolKind::Class,
                None,
                source,
                file_path,
            );
            process_javascript_class_methods(
                node,
                name,
                source,
                file_path,
                symbols,
                attached_doc_ranges,
            );
        }
        "lexical_declaration" | "variable_declaration" => {
            process_javascript_variable_declaration(
                node,
                node,
                source,
                file_path,
                symbols,
                attached_doc_ranges,
                import_ranges,
            );
        }
        "expression_statement" => {
            let mut cursor = node.walk();
            let Some(expression) = node.named_children(&mut cursor).next() else {
                return;
            };
            if expression.kind() == "assignment_expression" {
                process_javascript_assignment(
                    expression,
                    source,
                    file_path,
                    symbols,
                    attached_doc_ranges,
                );
            }
        }
        _ => {}
    }
}

fn process_javascript_declaration(
    node: tree_sitter::Node,
    anchor: tree_sitter::Node,
    source: &str,
    file_path: &str,
    symbols: &mut Vec<CodeSymbol>,
    attached_doc_ranges: &mut Vec<(usize, usize)>,
    import_ranges: &mut Vec<(usize, usize)>,
) {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" => {
            process_javascript_function_declaration(
                node,
                anchor,
                source,
                file_path,
                symbols,
                attached_doc_ranges,
            );
        }
        "class_declaration" => {
            let Some(name) = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            else {
                return;
            };
            push_javascript_symbol(
                symbols,
                attached_doc_ranges,
                &node,
                anchor,
                name,
                SymbolKind::Class,
                None,
                source,
                file_path,
            );
            process_javascript_class_methods(
                node,
                name,
                source,
                file_path,
                symbols,
                attached_doc_ranges,
            );
        }
        "lexical_declaration" | "variable_declaration" => {
            process_javascript_variable_declaration(
                node,
                anchor,
                source,
                file_path,
                symbols,
                attached_doc_ranges,
                import_ranges,
            );
        }
        _ => {}
    }
}

fn process_javascript_function_declaration(
    node: tree_sitter::Node,
    anchor: tree_sitter::Node,
    source: &str,
    file_path: &str,
    symbols: &mut Vec<CodeSymbol>,
    attached_doc_ranges: &mut Vec<(usize, usize)>,
) {
    let Some(name) = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
    else {
        return;
    };
    push_javascript_symbol(
        symbols,
        attached_doc_ranges,
        &node,
        anchor,
        name,
        SymbolKind::Function,
        None,
        source,
        file_path,
    );
}

fn process_javascript_variable_declaration(
    declaration: tree_sitter::Node,
    anchor: tree_sitter::Node,
    source: &str,
    file_path: &str,
    symbols: &mut Vec<CodeSymbol>,
    attached_doc_ranges: &mut Vec<(usize, usize)>,
    import_ranges: &mut Vec<(usize, usize)>,
) {
    let binding_kind = if declaration.kind() == "lexical_declaration"
        && declaration
            .child_by_field_name("kind")
            .map(|node| node.kind() == "const")
            .unwrap_or(false)
    {
        SymbolKind::Const
    } else {
        SymbolKind::Variable
    };

    let mut cursor = declaration.walk();
    for declarator in declaration
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "variable_declarator")
    {
        let Some(name_node) = declarator.child_by_field_name("name") else {
            continue;
        };
        let Some(_name) = name_node.utf8_text(source.as_bytes()).ok() else {
            continue;
        };

        // Post-Epic 010 review item #2: collect import ranges for literal
        // `require` bindings BEFORE filtering by binding pattern, so
        // destructured declarations like `const { charge } = require(...)`
        // are included in the Imports record.
        if let Some(value) = declarator.child_by_field_name("value") {
            if javascript_is_literal_require(value, source) {
                import_ranges.push((declaration.start_byte(), declaration.end_byte()));
            }
        }

        // Only process symbol-eligible bindings with identifier names.
        if name_node.kind() != "identifier" {
            continue;
        }
        let name = _name;

        if let Some(value) = declarator.child_by_field_name("value") {
            if javascript_is_callable_or_class(value) {
                let kind = if value.kind() == "class" {
                    SymbolKind::Class
                } else {
                    SymbolKind::Function
                };
                push_javascript_symbol(
                    symbols,
                    attached_doc_ranges,
                    &declarator,
                    anchor,
                    name,
                    kind,
                    Some(value),
                    source,
                    file_path,
                );
                if value.kind() == "class" {
                    process_javascript_class_methods(
                        value,
                        name,
                        source,
                        file_path,
                        symbols,
                        attached_doc_ranges,
                    );
                }
                continue;
            }
            if value.kind() == "object" {
                push_javascript_symbol(
                    symbols,
                    attached_doc_ranges,
                    &declarator,
                    anchor,
                    name,
                    binding_kind.clone(),
                    None,
                    source,
                    file_path,
                );
                process_javascript_object_methods(
                    value,
                    name,
                    source,
                    file_path,
                    symbols,
                    attached_doc_ranges,
                );
                continue;
            }
        }

        push_javascript_symbol(
            symbols,
            attached_doc_ranges,
            &declarator,
            declaration,
            name,
            binding_kind.clone(),
            None,
            source,
            file_path,
        );
    }
}

fn process_javascript_class_methods(
    class_node: tree_sitter::Node,
    class_name: &str,
    source: &str,
    file_path: &str,
    symbols: &mut Vec<CodeSymbol>,
    attached_doc_ranges: &mut Vec<(usize, usize)>,
) {
    let Some(body) = class_node.child_by_field_name("body") else {
        return;
    };
    let mut cursor = body.walk();
    for method in body
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "method_definition")
    {
        let Some(name) = method
            .child_by_field_name("name")
            .filter(|node| node.kind() == "property_identifier")
            .and_then(|node| node.utf8_text(source.as_bytes()).ok())
        else {
            continue;
        };
        let identifier = format!("{class_name}::{name}");
        push_javascript_symbol(
            symbols,
            attached_doc_ranges,
            &method,
            method,
            &identifier,
            SymbolKind::Method,
            None,
            source,
            file_path,
        );
    }
}

fn process_javascript_object_methods(
    object: tree_sitter::Node,
    object_name: &str,
    source: &str,
    file_path: &str,
    symbols: &mut Vec<CodeSymbol>,
    attached_doc_ranges: &mut Vec<(usize, usize)>,
) {
    let mut cursor = object.walk();
    for member in object.named_children(&mut cursor) {
        match member.kind() {
            "method_definition" => {
                let Some(name) = member
                    .child_by_field_name("name")
                    .filter(|node| node.kind() == "property_identifier")
                    .and_then(|node| node.utf8_text(source.as_bytes()).ok())
                else {
                    continue;
                };
                let identifier = format!("{object_name}::{name}");
                push_javascript_symbol(
                    symbols,
                    attached_doc_ranges,
                    &member,
                    member,
                    &identifier,
                    SymbolKind::Method,
                    None,
                    source,
                    file_path,
                );
            }
            "pair" => {
                let Some(name) = member
                    .child_by_field_name("key")
                    .filter(|node| node.kind() == "property_identifier")
                    .and_then(|node| node.utf8_text(source.as_bytes()).ok())
                else {
                    continue;
                };
                let Some(value) = member.child_by_field_name("value") else {
                    continue;
                };
                if !javascript_is_callable(value) {
                    continue;
                }
                let identifier = format!("{object_name}::{name}");
                push_javascript_symbol(
                    symbols,
                    attached_doc_ranges,
                    &member,
                    member,
                    &identifier,
                    SymbolKind::Method,
                    Some(value),
                    source,
                    file_path,
                );
            }
            _ => {}
        }
    }
}

fn process_javascript_assignment(
    assignment: tree_sitter::Node,
    source: &str,
    file_path: &str,
    symbols: &mut Vec<CodeSymbol>,
    attached_doc_ranges: &mut Vec<(usize, usize)>,
) {
    let Some(left) = assignment.child_by_field_name("left") else {
        return;
    };
    let Some(right) = assignment.child_by_field_name("right") else {
        return;
    };
    if !javascript_is_callable_or_class(right) {
        return;
    }
    let parts = javascript_member_parts(left, source);
    let Some((name, kind)) = javascript_assignment_symbol(&parts, right.kind()) else {
        return;
    };
    push_javascript_symbol(
        symbols,
        attached_doc_ranges,
        &assignment,
        assignment,
        &name,
        kind,
        Some(right),
        source,
        file_path,
    );
    // Post-Epic 010 review item #3: class-valued CommonJS/ES assignments
    // must also have their methods indexed, just like variable-declarator
    // classes. Without this, `module.exports = class { ... }` and
    // `exports.Gateway = class { ... }` emit the class record but lose
    // all method evidence.
    if right.kind() == "class" {
        process_javascript_class_methods(
            right,
            &name,
            source,
            file_path,
            symbols,
            attached_doc_ranges,
        );
    }
}

fn javascript_assignment_symbol(
    parts: &[String],
    right_kind: &str,
) -> Option<(String, SymbolKind)> {
    let callable_kind = if right_kind == "class" {
        SymbolKind::Class
    } else {
        SymbolKind::Function
    };
    match parts {
        [name] => Some((name.clone(), callable_kind)),
        [receiver, prototype, method] if prototype == "prototype" => {
            Some((format!("{receiver}::{method}"), SymbolKind::Method))
        }
        [exports, name] if exports == "exports" => Some((name.clone(), callable_kind)),
        [module, exports] if module == "module" && exports == "exports" => {
            Some(("__module_export".to_string(), callable_kind))
        }
        [module, exports, name] if module == "module" && exports == "exports" => {
            Some((name.clone(), callable_kind))
        }
        _ => None,
    }
}

fn javascript_member_parts(node: tree_sitter::Node, source: &str) -> Vec<String> {
    if node.kind() == "identifier" {
        return node
            .utf8_text(source.as_bytes())
            .ok()
            .map(|text| vec![text.to_string()])
            .unwrap_or_default();
    }
    if node.kind() != "member_expression" {
        return Vec::new();
    }
    let Some(object) = node.child_by_field_name("object") else {
        return Vec::new();
    };
    let Some(property) = node
        .child_by_field_name("property")
        .filter(|node| node.kind() == "property_identifier")
    else {
        return Vec::new();
    };
    let mut parts = javascript_member_parts(object, source);
    if let Ok(text) = property.utf8_text(source.as_bytes()) {
        parts.push(text.to_string());
    }
    parts
}

fn push_javascript_symbol(
    symbols: &mut Vec<CodeSymbol>,
    attached_doc_ranges: &mut Vec<(usize, usize)>,
    node: &tree_sitter::Node,
    anchor: tree_sitter::Node,
    name: &str,
    kind: SymbolKind,
    callable: Option<tree_sitter::Node>,
    source: &str,
    file_path: &str,
) {
    let (doc_comment, doc_ranges) = extract_javascript_doc_comment(anchor, source);
    attached_doc_ranges.extend(doc_ranges);
    let signature = extract_javascript_signature(*node, callable, source);
    let body_excerpt = callable
        .map(|value| extract_javascript_body_excerpt(value, source))
        .unwrap_or_else(|| extract_javascript_body_excerpt(*node, source));
    let text = compose_symbol_text(&doc_comment, &signature, &body_excerpt);
    symbols.push(CodeSymbol {
        identifier: format!("{file_path}::{name}"),
        text,
        symbol_kind: kind,
        file_path: file_path.to_string(),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
        embed: true,
    });
}

fn extract_javascript_doc_comment(
    anchor: tree_sitter::Node,
    source: &str,
) -> (String, Vec<(usize, usize)>) {
    let mut comments = Vec::new();
    let mut ranges = Vec::new();
    let mut current = anchor.prev_sibling();
    while let Some(sibling) = current {
        if sibling.kind() != "comment" {
            break;
        }
        let text = sibling.utf8_text(source.as_bytes()).unwrap_or("");
        if !text.starts_with("/**") {
            break;
        }
        comments.push(text.to_string());
        ranges.push((sibling.start_position().row, sibling.end_position().row));
        current = sibling.prev_sibling();
    }
    comments.reverse();
    ranges.reverse();
    (comments.join("\n"), ranges)
}

fn extract_javascript_signature(
    node: tree_sitter::Node,
    callable: Option<tree_sitter::Node>,
    source: &str,
) -> String {
    let signature_node = callable.unwrap_or(node);
    let body_start = javascript_body_node(signature_node)
        .map(|body| body.start_byte())
        .unwrap_or(signature_node.end_byte());
    let end = body_start.max(signature_node.start_byte());
    let signature_start = if matches!(node.kind(), "variable_declarator" | "assignment_expression")
        && callable.is_some()
    {
        node.start_byte()
    } else {
        signature_node.start_byte()
    };
    let text = source.get(signature_start..end).unwrap_or("").trim();
    if node.kind() == "variable_declarator" && callable.is_some() {
        let declaration_kind = node
            .parent()
            .and_then(|parent| parent.child_by_field_name("kind"))
            .map(|kind| kind.kind())
            .unwrap_or("var");
        return format!("{declaration_kind} {text}");
    }
    if node.kind() == "assignment_expression" && callable.is_some() {
        return text.trim_end_matches(';').trim().to_string();
    }
    if text.is_empty() {
        bound_declaration_text(node.utf8_text(source.as_bytes()).unwrap_or(""), 2000)
    } else {
        text.to_string()
    }
}

fn extract_javascript_body_excerpt(node: tree_sitter::Node, source: &str) -> String {
    let Some(body) = javascript_body_node(node) else {
        return String::new();
    };
    let text = body.utf8_text(source.as_bytes()).unwrap_or("");
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= 10 {
        text.to_string()
    } else {
        format!("{}\n...", lines[..10].join("\n"))
    }
}

fn javascript_body_node(node: tree_sitter::Node) -> Option<tree_sitter::Node> {
    match node.kind() {
        "function_declaration"
        | "generator_function_declaration"
        | "function_expression"
        | "generator_function"
        | "arrow_function"
        | "method_definition"
        | "class"
        | "class_declaration" => node.child_by_field_name("body"),
        "assignment_expression" | "export_statement" => node
            .child_by_field_name("right")
            .or_else(|| node.child_by_field_name("value"))
            .and_then(javascript_body_node),
        "variable_declarator" => node
            .child_by_field_name("value")
            .and_then(javascript_body_node),
        _ => None,
    }
}

fn javascript_is_callable(node: tree_sitter::Node) -> bool {
    matches!(
        node.kind(),
        "arrow_function" | "function_expression" | "generator_function"
    )
}

fn javascript_is_callable_or_class(node: tree_sitter::Node) -> bool {
    javascript_is_callable(node) || node.kind() == "class"
}

fn javascript_is_literal_require(node: tree_sitter::Node, source: &str) -> bool {
    if node.kind() != "call_expression" {
        return false;
    }
    let Some(function) = node.child_by_field_name("function") else {
        return false;
    };
    if function.kind() != "identifier"
        || function.utf8_text(source.as_bytes()).ok() != Some("require")
    {
        return false;
    }
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return false;
    };
    arguments
        .named_child(0)
        .map(|argument| argument.kind() == "string")
        .unwrap_or(false)
}

fn javascript_is_default_export(node: tree_sitter::Node, source: &str) -> bool {
    node.utf8_text(source.as_bytes())
        .unwrap_or("")
        .trim_start()
        .starts_with("export default")
}

fn collect_javascript_comments(node: tree_sitter::Node, ranges: &mut Vec<(usize, usize)>) {
    if node.kind() == "comment" {
        ranges.push((node.start_position().row, node.end_position().row));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_javascript_comments(child, ranges);
    }
}

fn collect_javascript_imports(node: tree_sitter::Node, ranges: &mut Vec<(usize, usize)>) {
    match node.kind() {
        "import_statement" => ranges.push((node.start_byte(), node.end_byte())),
        "export_statement" if node.child_by_field_name("source").is_some() => {
            ranges.push((node.start_byte(), node.end_byte()))
        }
        "call_expression" => {
            let is_import = node
                .child_by_field_name("function")
                .map(|function| function.kind() == "import")
                .unwrap_or(false);
            let is_static = node
                .child_by_field_name("arguments")
                .and_then(|arguments| arguments.named_child(0))
                .map(|argument| argument.kind() == "string")
                .unwrap_or(false);
            if is_import && is_static {
                ranges.push((node.start_byte(), node.end_byte()));
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_javascript_imports(child, ranges);
    }
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

fn is_nested_executable_declaration(node: tree_sitter::Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(
            parent.kind(),
            "function_declaration" | "function_expression" | "arrow_function" | "method_definition"
        ) {
            return true;
        }
        current = parent.parent();
    }
    false
}

fn declaration_contains_callable(node: tree_sitter::Node) -> bool {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|child| {
        matches!(child.kind(), "arrow_function" | "function_expression")
            || declaration_contains_callable(child)
    });
    found
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
                    attached.push((sibling.start_position().row, sibling.end_position().row));
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

fn collect_attached_doc_walk(node: tree_sitter::Node, source: &str, out: &mut Vec<(usize, usize)>) {
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

fn collect_ts_doc_comments_for(node: tree_sitter::Node, source: &str, out: &mut Vec<String>) {
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
        "variable_declarator" => {
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            if let Some(value) = node.child_by_field_name("value") {
                if matches!(value.kind(), "arrow_function" | "function_expression") {
                    let relative_body_start = value
                        .child_by_field_name("body")
                        .map(|body| body.start_byte().saturating_sub(node.start_byte()))
                        .unwrap_or(text.len());
                    return text[..relative_body_start].trim().to_string();
                }
            }
            text.lines().next().unwrap_or("").trim().to_string()
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
    let executable_node = if node.kind() == "variable_declarator" {
        node.child_by_field_name("value")
            .filter(|value| matches!(value.kind(), "arrow_function" | "function_expression"))
    } else {
        Some(node)
    };
    let body_node = executable_node.and_then(|node| match node.kind() {
        "function_declaration"
        | "function_expression"
        | "method_definition"
        | "arrow_function"
        | "class_declaration"
        | "class_expression"
        | "abstract_class_declaration" => node.child_by_field_name("body").or_else(|| {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .find(|child| matches!(child.kind(), "statement_block" | "class_body"));
            found
        }),
        _ => None,
    });
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
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
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

fn collect_imports(source: &str, import_ranges: &[(usize, usize)]) -> String {
    let mut ranges = import_ranges.to_vec();
    ranges.sort_unstable();
    ranges.dedup();
    ranges
        .into_iter()
        .filter_map(|(start, end)| source.get(start..end))
        .collect::<Vec<_>>()
        .join("\n")
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
/// - Tracked generated/vendor/minified files are filtered centrally via
///   [`is_excluded_generated_path`] (Epic 010 Task 1: "centralized
///   generated/vendor/minified filtering in the shared code-file
///   discovery path"). The exclusion set applies to every language so
///   `.gitignore`-d and tracked-but-generated paths behave consistently.
pub fn list_tracked_code_files(
    repo_path: &Path,
    extensions: &[&str],
) -> anyhow::Result<Vec<PathBuf>> {
    let repo = gix::discover(repo_path)?;
    let tree_id = repo.head_tree_id_or_empty()?.detach();
    let entries = list_tracked_code_file_entries(&repo, tree_id, extensions)?;
    Ok(entries.into_iter().map(|e| e.absolute_path).collect())
}

/// A tracked code file discovered from the Git HEAD tree, carrying both
/// the absolute worktree path and the blob content read from the HEAD tree.
///
/// Returned by [`list_tracked_code_files_with_content`]. Using this struct
/// ensures the indexer reads committed content from the HEAD tree rather
/// than uncommitted worktree changes (post-Epic 010 review item #1).
pub struct TrackedCodeFile {
    /// Absolute worktree path (for display, extension routing, etc.).
    pub absolute_path: PathBuf,
    /// Repo-relative forward-slash path (canonical tracking key).
    pub relative_path: String,
    /// Blob content read from the Git HEAD tree.
    pub content: Vec<u8>,
}

/// Internal entry used during tree traversal before content is read.
struct TrackedCodeFileEntry {
    absolute_path: PathBuf,
    relative_path: String,
    oid: gix::hash::ObjectId,
}

/// Internal: traverse the HEAD tree and collect eligible code-file entries
/// (filtered by extension, `.d.ts`, and generated/vendor policy) without
/// reading blob content yet. Returns entries with their OIDs so the caller
/// can batch-read blobs in a single repo session.
fn list_tracked_code_file_entries(
    repo: &gix::Repository,
    tree_id: gix::hash::ObjectId,
    extensions: &[&str],
) -> anyhow::Result<Vec<TrackedCodeFileEntry>> {
    let work_dir = match repo.workdir() {
        Some(dir) => dir.to_path_buf(),
        None => return Ok(vec![]),
    };

    let tree = repo.find_tree(tree_id)?;

    let entries = tree.traverse().breadthfirst.files()?;

    let mut files: Vec<TrackedCodeFileEntry> = entries
        .into_iter()
        .filter(|entry| entry.mode.is_blob())
        .filter_map(|entry| {
            let rel = entry.filepath.to_str().ok()?.to_string();
            let abs = work_dir.join(Path::new(&rel));
            Some((rel, abs, entry.oid))
        })
        .filter(|(p, _, _)| {
            // Reject `.d.ts` (declaration files) regardless of whether `.ts`
            // was requested. `.d.ts` always sits next to a `.ts` file; skipping
            // it here keeps the policy in one place.
            if p.ends_with(".d.ts") {
                return false;
            }
            // Centralized generated/vendor/minified exclusion (Epic 010
            // Task 1): tracked bundles that ship inside the repo despite
            // being build artifacts must never reach indexing. The check
            // accepts the repo-relative forward-slash path because
            // `gix::Tree::traverse` returns paths in that form.
            if is_excluded_generated_path(p) {
                return false;
            }
            extensions.iter().any(|ext| p.ends_with(ext))
        })
        .map(|(rel, abs, oid)| TrackedCodeFileEntry {
            absolute_path: abs,
            relative_path: rel,
            oid,
        })
        .collect();

    files.sort_by(|a, b| a.absolute_path.cmp(&b.absolute_path));
    Ok(files)
}

/// One-pass tracked-file traversal that reads blob content from the Git HEAD
/// tree. Returns [`TrackedCodeFile`] values carrying both the absolute
/// worktree path and the committed content.
///
/// Also returns the HEAD commit's author timestamp (seconds since UNIX epoch)
/// as a proxy mtime for all returned files, since tree entries do not carry
/// per-file modification times.
///
/// This is the preferred entry point for production indexing because it
/// guarantees discovery, content, and deletion reconciliation all use the
/// same Git HEAD state (post-Epic 010 review item #1).
pub fn list_tracked_code_files_with_content(
    repo_path: &Path,
    extensions: &[&str],
) -> anyhow::Result<(Vec<TrackedCodeFile>, i64)> {
    // One repository handle and one resolved HEAD tree supply discovery,
    // blob contents, and the proxy timestamp. This avoids mixing snapshots
    // if HEAD changes concurrently.
    let repo = gix::discover(repo_path)?;
    let head_commit = repo.head_commit().ok();
    let (tree_id, head_mtime) = match head_commit {
        Some(commit) => {
            let tree_id = commit.tree_id()?.detach();
            let decoded = commit.decode()?;
            let head_mtime = decoded
                .author()
                .ok()
                .and_then(|author| author.time().ok())
                .map(|time| time.seconds)
                .unwrap_or(0);
            (tree_id, head_mtime)
        }
        None => (repo.head_tree_id_or_empty()?.detach(), 0),
    };
    let entries = list_tracked_code_file_entries(&repo, tree_id, extensions)?;

    let mut files = Vec::with_capacity(entries.len());
    for entry in entries {
        // Abort before deletion reconciliation if any discovered blob cannot
        // be read. Silently omitting it would make index_code() mistake a
        // transient object-store failure for a committed file deletion and
        // remove valid existing evidence.
        let blob = repo.find_blob(entry.oid).map_err(|error| {
            anyhow::anyhow!(
                "failed to read HEAD blob for {}: {}",
                entry.relative_path,
                error
            )
        })?;
        files.push(TrackedCodeFile {
            absolute_path: entry.absolute_path,
            relative_path: entry.relative_path,
            content: blob.data.to_vec(),
        });
    }

    Ok((files, head_mtime))
}

/// Repo-relative paths that should never be indexed, regardless of
/// extension. Centralized here (Epic 010 Task 1: "Centralize these policy
/// filters so TypeScript and future languages can share or override
/// them") so `.gitignore`-d, tracked-but-vendored, and minified files
/// behave identically across all five supported languages.
///
/// `path` must be the repo-relative forward-slash path returned by
/// `gix::Tree::traverse`. Comparison is anchored on path segments so a
/// directory named `dist` inside `src/distributor/x.ts` is **not**
/// excluded.
pub fn is_excluded_generated_path(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();

    // `.min.js` is the canonical minified bundle suffix. Filename match
    // rather than segment match so it works on both flat and nested
    // layouts; case-insensitive because some build pipelines emit
    // `.MIN.JS` on Windows.
    if let Some(filename) = segments.last() {
        let lower = filename.to_ascii_lowercase();
        if lower.ends_with(".min.js") {
            return true;
        }
    }

    // Generated/vendor directory prefixes. Match against full path
    // segments (not substrings) so e.g. `src/distributor` survives.
    const EXCLUDED_DIRS: &[&str] = &[
        "node_modules",
        "vendor",
        "dist",
        "build",
        ".next",
        "coverage",
    ];
    if segments.iter().any(|seg| EXCLUDED_DIRS.contains(seg)) {
        return true;
    }

    false
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
        // Epic 010 Task 1: JavaScript / JSX metadata strings must be
        // exactly `javascript` and `jsx` so stored evidence round-trips.
        assert_eq!(super::CodeLanguage::JavaScript.as_str(), "javascript");
        assert_eq!(super::CodeLanguage::Jsx.as_str(), "jsx");
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
        // Epic 010 Task 1: `.js` → JavaScript, `.jsx` → Jsx.
        assert_eq!(
            super::language_for_extension("js"),
            Some(super::CodeLanguage::JavaScript)
        );
        assert_eq!(
            super::language_for_extension("jsx"),
            Some(super::CodeLanguage::Jsx)
        );
        // case-insensitive
        assert_eq!(
            super::language_for_extension("RS"),
            Some(super::CodeLanguage::Rust)
        );
        assert_eq!(
            super::language_for_extension("JS"),
            Some(super::CodeLanguage::JavaScript)
        );
        assert_eq!(
            super::language_for_extension("JSX"),
            Some(super::CodeLanguage::Jsx)
        );
        // mixed-case extensions still route to the correct dialect
        assert_eq!(
            super::language_for_extension("Js"),
            Some(super::CodeLanguage::JavaScript)
        );
        assert_eq!(
            super::language_for_extension("JsX"),
            Some(super::CodeLanguage::Jsx)
        );
        // unsupported — must NOT silently fall back to Rust or TS
        assert_eq!(super::language_for_extension("mjs"), None);
        assert_eq!(super::language_for_extension("cjs"), None);
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

    // ── Epic 010 Task 1: JavaScript / JSX dispatcher + query validation ───

    #[test]
    fn code_extractor_javascript_compiles_query_once() {
        // Epic 010 DD #1: queries are compiled at construction. A
        // compilation failure is a deployment-time error, never a
        // per-file error.
        let extractor = super::CodeExtractor::for_language(super::CodeLanguage::JavaScript);
        assert_eq!(extractor.config.language, super::CodeLanguage::JavaScript);
        assert!(extractor.config.supports("js"));
        assert!(!extractor.config.supports("ts"));
        assert!(!extractor.config.supports("rs"));

        let names: Vec<&str> = extractor.query().capture_names().to_vec();
        // JS-specific captures must be present.
        assert!(names.contains(&"definition.function"));
        assert!(names.contains(&"definition.class"));
        assert!(names.contains(&"definition.method"));
        assert!(names.contains(&"definition.callable_binding"));
        assert!(names.contains(&"definition.object_method"));
        assert!(names.contains(&"definition.assignment_binding"));
        // TS-only captures must NOT appear in the JS query (Epic 010
        // acceptance criterion #4: "JavaScript query contains no
        // TypeScript-only captures").
        for ts_only in [
            "definition.interface",
            "definition.type_alias",
            "definition.enum",
            "definition.module",
            "definition.function_signature",
        ] {
            assert!(
                !names.contains(&ts_only),
                "JS query must not include TypeScript-only capture {ts_only}"
            );
        }
    }

    #[test]
    fn code_extractor_jsx_compiles_query_once() {
        let extractor = super::CodeExtractor::for_language(super::CodeLanguage::Jsx);
        assert_eq!(extractor.config.language, super::CodeLanguage::Jsx);
        assert!(extractor.config.supports("jsx"));
        assert!(!extractor.config.supports("js"));
        // JSX uses the JS grammar — captured query capture names are
        // the JS set; the variant only differs by `extensions` /
        // `language` metadata.
        let names: Vec<&str> = extractor.query().capture_names().to_vec();
        assert!(names.contains(&"definition.function"));
        assert!(names.contains(&"definition.class"));
        assert!(!names.contains(&"definition.interface"));
    }

    #[test]
    fn javascript_jsx_grammars_share_language_object() {
        // The JavaScript grammar natively accepts JSX (its scanner
        // dispatches on `valid_symbols[JSX_TEXT]`), so both `.js` and
        // `.jsx` build `CodeExtractor` instances from the same
        // `tree-sitter` `Language`. That is what lets the epic keep
        // two distinct enum variants while sharing one grammar.
        let js = super::CodeExtractor::for_language(super::CodeLanguage::JavaScript);
        let jsx = super::CodeExtractor::for_language(super::CodeLanguage::Jsx);
        assert_eq!(
            js.tree_sitter_language().abi_version(),
            jsx.tree_sitter_language().abi_version(),
            "js and jsx must share the JavaScript grammar"
        );
    }

    #[test]
    fn javascript_grammar_smoke_test_for_jsx_source() {
        // The JS query must parse a JSX-bearing `.js` fixture without
        // raising a syntax error (Epic 010 DD: "smoke tests prove it
        // compiles and behaves correctly for both").
        let mut parser = tree_sitter::Parser::new();
        let lang: tree_sitter::Language =
            super::CodeExtractor::for_language(super::CodeLanguage::JavaScript)
                .tree_sitter_language()
                .clone();
        parser.set_language(&lang).expect("set JS grammar");

        // Plain JS first: must be error-free so we know the grammar
        // is loaded correctly before adding JSX semantics.
        let plain = "function hello() { return 1; }\n";
        let tree = parser.parse(plain, None).expect("parse plain JS");
        assert!(!tree.root_node().has_error(), "plain JS must parse cleanly");

        // Same parser, JSX inside a `.js` file. The JS grammar
        // accepts JSX natively so this must NOT error.
        let jsx = "const Box = () => <div className=\"x\">hi</div>;\n";
        let tree = parser.parse(jsx, None).expect("parse JSX inside .js");
        assert!(
            !tree.root_node().has_error(),
            "JSX inside a .js file must parse cleanly (got ERROR)"
        );
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

        let rust_only =
            super::list_tracked_code_files(repo_path, &[".rs"]).expect("list .rs files");
        let multi =
            super::list_tracked_code_files(repo_path, &[".rs", ".ts", ".tsx", ".js", ".jsx"])
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
    fn is_excluded_generated_path_filters_documented_directories() {
        // Epic 010 Task 1 acceptance #2: a committed fixture for every
        // configured generated/vendor directory and `*.min.js` is
        // excluded by the shared discovery filter.
        for (path, expected) in [
            ("node_modules/foo/lib.js", true),
            ("packages/app/node_modules/x.ts", true),
            ("vendor/jquery.js", true),
            ("dist/bundle.js", true),
            ("build/output.jsx", true),
            ("packages/app/.next/static/foo.js", true),
            ("coverage/lcov.info", true),
            ("lib.min.js", true),
            ("packages/app/lib.MIN.JS", true),
            ("src/main.rs", false),
            ("src/checkout/session.ts", false),
            ("scripts/build.js", false), // `build.js` itself, not the dir
            ("src/distributor/x.ts", false), // segment-match, not substring
            ("docs/min.js.md", false),   // `.min.js.md`, not `.min.js`
            ("", false),
        ] {
            assert_eq!(
                super::is_excluded_generated_path(path),
                expected,
                "is_excluded_generated_path({path:?}) expected {expected}"
            );
        }
    }

    #[test]
    fn list_tracked_code_files_excludes_tracked_generated_paths() {
        // Build a minimal git repo with one normal .js file and one
        // tracked vendor `.js` file. The vendor one must be filtered
        // out by the shared discovery path even though it is tracked.
        let repo_dir = tempfile::tempdir().expect("repo tempdir");

        let status = std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .args(["init"])
            .status()
            .expect("git init");
        assert!(status.success(), "git init failed");

        std::fs::create_dir_all(repo_dir.path().join("src")).unwrap();
        std::fs::write(
            repo_dir.path().join("src/main.js"),
            "function real() { return 1; }\n",
        )
        .unwrap();
        std::fs::create_dir_all(repo_dir.path().join("node_modules/vendor")).unwrap();
        std::fs::write(
            repo_dir.path().join("node_modules/vendor/lib.js"),
            "function leaked() { return 1; }\n",
        )
        .unwrap();
        std::fs::create_dir_all(repo_dir.path().join("dist")).unwrap();
        std::fs::write(
            repo_dir.path().join("dist/bundle.min.js"),
            "!function(){return 1}();\n",
        )
        .unwrap();

        std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["add", "."])
            .status()
            .expect("git add");
        std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["config", "user.email", "test@test.com"])
            .status()
            .expect("git config email");
        std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["config", "user.name", "Test"])
            .status()
            .expect("git config name");
        std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["commit", "-m", "initial"])
            .status()
            .expect("git commit");

        let files = super::list_tracked_code_files(repo_dir.path(), &[".js"])
            .expect("list tracked code files");
        let rels: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(repo_dir.path())
                    .map(|r| r.to_string_lossy().to_string())
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(
            rels,
            vec!["src/main.js".to_string()],
            "tracked generated files must be excluded, got {rels:?}"
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
            .find(|s| {
                kind_is(s, &super::SymbolKind::Function)
                    && s.identifier.contains("createCheckoutSession")
            })
            .expect("createCheckoutSession Function symbol");
        assert_eq!(
            sym.identifier,
            "src/checkout/session.ts::createCheckoutSession"
        );
        assert!(sym.embed);
        assert!(
            sym.text.contains("Process a checkout session"),
            "JSDoc: {}",
            sym.text
        );
        assert!(
            sym.text.contains("createCheckoutSession"),
            "signature: {}",
            sym.text
        );
    }

    #[test]
    fn ts_extract_supporting_interface_and_type_alias() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let iface = symbols
            .iter()
            .find(|s| {
                kind_is(s, &super::SymbolKind::Interface) && s.identifier.contains("CheckoutConfig")
            })
            .unwrap();
        assert_eq!(iface.identifier, "src/checkout/session.ts::CheckoutConfig");
        let alias = symbols
            .iter()
            .find(|s| {
                kind_is(s, &super::SymbolKind::TypeAlias) && s.identifier.contains("CheckoutResult")
            })
            .unwrap();
        assert_eq!(alias.identifier, "src/checkout/session.ts::CheckoutResult");
    }

    #[test]
    fn ts_extract_configuration_const() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let c = symbols
            .iter()
            .find(|s| kind_is(s, &super::SymbolKind::Const) && s.identifier.contains("MAX_RETRIES"))
            .unwrap();
        assert_eq!(c.identifier, "src/checkout/session.ts::MAX_RETRIES");
    }

    #[test]
    fn ts_extract_class_method_with_class_prefix() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let m = symbols
            .iter()
            .find(|s| kind_is(s, &super::SymbolKind::Method) && s.identifier.contains("charge"))
            .unwrap();
        assert_eq!(
            m.identifier,
            "src/checkout/session.ts::PaymentGateway::charge"
        );
        assert!(m.text.contains("provider.charge"));
        assert!(
            !m.text.contains("get configured"),
            "method evidence leaked the rest of the class: {}",
            m.text
        );
        assert_eq!(m.line_start, 54);
        assert_eq!(m.line_end, 56);
    }

    #[test]
    fn ts_excludes_nested_executable_declarations() {
        let src = r#"
export function checkout(): boolean {
    function validateInternally(): boolean {
        return true;
    }
    const calculateInternally = () => false;
    return validateInternally() && calculateInternally();
}
"#;
        let symbols = extract_ts("src/nested.ts", src);
        assert!(symbols
            .iter()
            .any(|s| s.identifier == "src/nested.ts::checkout"));
        assert!(
            !symbols.iter().any(|s| {
                s.identifier.ends_with("::validateInternally")
                    || s.identifier.ends_with("::calculateInternally")
            }),
            "nested functions leaked into evidence: {:?}",
            symbols.iter().map(|s| &s.identifier).collect::<Vec<_>>()
        );
    }

    #[test]
    fn ts_extract_callable_binding_vs_variable_kind() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let callable = symbols
            .iter()
            .find(|s| {
                kind_is(s, &super::SymbolKind::Function) && s.identifier.contains("chargeCustomer")
            })
            .expect("callable binding");
        assert_eq!(
            callable.identifier,
            "src/checkout/session.ts::chargeCustomer"
        );
        assert!(
            !symbols.iter().any(|s| kind_is(s, &super::SymbolKind::Const)
                && s.identifier.contains("chargeCustomer")),
            "must NOT also be Const"
        );
    }

    #[test]
    fn ts_extract_imports_record_is_fts_only_and_collects_reexports() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let imports: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Imports)
            .collect();
        assert_eq!(imports.len(), 1);
        let rec = &imports[0];
        assert!(!rec.embed);
        assert_eq!(rec.identifier, "src/checkout/session.ts::__imports__");
        assert!(rec.text.contains("PaymentProvider"));
        assert!(rec.text.contains("./legacy"));
    }

    #[test]
    fn ts_import_evidence_preserves_multiline_and_static_dynamic_imports() {
        let src = r#"
import {
    PaymentProvider,
    PaymentResult,
} from "./payment";

export {
    legacyCheckout,
} from "./legacy";

export async function loadCheckout() {
    return import("./checkout");
}
"#;
        let symbols = extract_ts("src/imports.ts", src);
        let imports = symbols
            .iter()
            .find(|s| s.symbol_kind == super::SymbolKind::Imports)
            .expect("imports evidence");
        assert!(imports.text.contains("PaymentResult"));
        assert!(imports.text.contains("from \"./payment\""));
        assert!(imports.text.contains("legacyCheckout"));
        assert!(imports.text.contains("from \"./legacy\""));
        assert!(imports.text.contains("import(\"./checkout\")"));
    }

    #[test]
    fn ts_extract_standalone_limitation_comment() {
        let symbols = extract_ts("src/checkout/session.ts", TS_CHECKOUT_FIXTURE);
        let comments: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Comments)
            .collect();
        assert_eq!(comments.len(), 1);
        let rec = &comments[0];
        assert!(!rec.embed);
        assert!(rec.text.contains("temporary limitation"));
        assert!(rec.text.contains("USD currency"));
        // JSDoc not duplicated.
        let fn_sym = symbols
            .iter()
            .find(|s| {
                kind_is(s, &super::SymbolKind::Function)
                    && s.identifier.contains("createCheckoutSession")
            })
            .unwrap();
        assert!(fn_sym.text.contains("Process a checkout session"));
        assert!(!rec.text.contains("Process a checkout session"));
    }

    #[test]
    fn ts_extract_variable_bindings_let_var() {
        let src = "let count = 0;\nvar legacy = \"x\";\n";
        let symbols = extract_ts("src/vars.ts", src);
        let count = symbols
            .iter()
            .find(|s| s.identifier.contains("count"))
            .unwrap();
        assert_eq!(count.symbol_kind, super::SymbolKind::Variable);
        let legacy = symbols
            .iter()
            .find(|s| s.identifier.contains("legacy"))
            .unwrap();
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
            .filter(|s| {
                kind_is(s, &super::SymbolKind::Function)
                    && s.identifier.contains("foo")
                    && !s.identifier.contains("__")
            })
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
        let default = symbols
            .iter()
            .find(|s| s.identifier.contains("__default_export"))
            .expect("anonymous default export");
        assert_eq!(default.identifier, "src/default.ts::__default_export");
        assert_eq!(default.symbol_kind, super::SymbolKind::Function);
        assert!(default.embed);
    }

    #[test]
    fn ts_extract_nested_namespace_identifier() {
        let src = "export namespace Outer {\n    export namespace Inner {\n        export function nested(): number {\n            return 42;\n        }\n\n        export interface InnerConfig {\n            enabled: boolean;\n        }\n    }\n}\n";
        let symbols = extract_ts("src/namespaces.ts", src);
        let nested = symbols
            .iter()
            .find(|s| s.identifier.contains("nested"))
            .expect("namespaced function");
        assert_eq!(nested.identifier, "src/namespaces.ts::Outer::Inner::nested");
        let iface = symbols
            .iter()
            .find(|s| s.identifier.contains("InnerConfig"))
            .expect("namespaced interface");
        assert_eq!(
            iface.identifier,
            "src/namespaces.ts::Outer::Inner::InnerConfig"
        );
    }

    #[test]
    fn ts_extract_enum() {
        let src = "export enum Color {\n    Red,\n    Green,\n    Blue,\n}\n";
        let symbols = extract_ts("src/enum.ts", src);
        let color = symbols
            .iter()
            .find(|s| s.identifier.contains("Color"))
            .expect("enum Color");
        assert_eq!(color.identifier, "src/enum.ts::Color");
        assert_eq!(color.symbol_kind, super::SymbolKind::Enum);
    }

    #[test]
    fn ts_extract_function_expression_binding() {
        let src = "export const make = function (): string {\n    return \"ok\";\n};\n";
        let symbols = extract_ts("src/binding.ts", src);
        let make = symbols
            .iter()
            .find(|s| s.identifier.contains("make"))
            .expect("function-expression binding");
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
        let long = symbols
            .iter()
            .find(|s| s.identifier.contains("long"))
            .expect("function");
        assert!(
            long.text.contains("..."),
            "body excerpt truncated: {}",
            long.text
        );
    }

    // ── Epic 009 Task 3: TSX extraction tests ───────────────────────────

    /// Product-oriented TSX fixture covering a function component with
    /// typed props, an arrow component, a helper function, attached JSDoc,
    /// and a regular import.
    const TSX_CHECKOUT_FIXTURE: &str = r#"import { PaymentProvider } from "./payment";

/**
 * Props for the checkout button.
 */
export interface CheckoutButtonProps {
    disabled?: boolean;
    amount: number;
    provider: PaymentProvider;
}

/**
 * A button component that starts a checkout flow.
 */
export function CheckoutButton(props: CheckoutButtonProps) {
    return (
        <button disabled={props.disabled} onClick={() => charge(props.amount, props.provider)}>
            Pay {props.amount}
        </button>
    );
}

/** An arrow-function component with destructured props. */
export const CheckoutLabel = ({ amount, provider }: CheckoutLabelProps) => {
    return <span>Charging {amount} via {provider.name}</span>;
};

interface CheckoutLabelProps {
    amount: number;
    provider: PaymentProvider;
}

function charge(amount: number, provider: PaymentProvider): boolean {
    return provider.charge(amount);
}
"#;

    fn extract_tsx(file_path: &str, source: &str) -> Vec<super::CodeSymbol> {
        let extractor = super::CodeExtractor::for_language(super::CodeLanguage::Tsx);
        let path = std::path::Path::new(file_path);
        let repo = std::path::Path::new(".");
        super::extract_symbols_with_extractor(&extractor, repo, path, source)
            .expect("tsx extraction")
    }

    #[test]
    fn tsx_extracts_named_function_component() {
        let symbols = extract_tsx("src/components/CheckoutButton.tsx", TSX_CHECKOUT_FIXTURE);
        let comp = symbols
            .iter()
            .find(|s| {
                kind_is(s, &super::SymbolKind::Function)
                    && s.identifier.contains("CheckoutButton")
                    && !s.identifier.contains("Label")
            })
            .expect("CheckoutButton function component");
        assert_eq!(
            comp.identifier,
            "src/components/CheckoutButton.tsx::CheckoutButton"
        );
        assert!(
            comp.text.contains("Pay"),
            "JSX body should appear in body excerpt: {}",
            comp.text
        );
        // JSDoc attached
        assert!(
            comp.text.contains("A button component"),
            "attached JSDoc: {}",
            comp.text
        );
    }

    #[test]
    fn tsx_extracts_arrow_component_with_typed_props() {
        let symbols = extract_tsx("src/components/CheckoutButton.tsx", TSX_CHECKOUT_FIXTURE);
        let comp = symbols
            .iter()
            .find(|s| {
                kind_is(s, &super::SymbolKind::Function) && s.identifier.contains("CheckoutLabel")
            })
            .expect("CheckoutLabel arrow component");
        assert_eq!(
            comp.identifier,
            "src/components/CheckoutButton.tsx::CheckoutLabel"
        );
        assert!(
            comp.text.contains("CheckoutLabelProps"),
            "typed signature: {}",
            comp.text
        );
        assert!(
            comp.text.contains("<span>"),
            "JSX body excerpt: {}",
            comp.text
        );
    }

    #[test]
    fn tsx_preserves_typed_props_signature() {
        let symbols = extract_tsx("src/components/CheckoutButton.tsx", TSX_CHECKOUT_FIXTURE);
        let iface = symbols
            .iter()
            .find(|s| {
                kind_is(s, &super::SymbolKind::Interface)
                    && s.identifier.contains("CheckoutButtonProps")
            })
            .expect("CheckoutButtonProps interface");
        assert_eq!(
            iface.identifier,
            "src/components/CheckoutButton.tsx::CheckoutButtonProps"
        );
    }

    #[test]
    fn tsx_helper_function_extracted() {
        let symbols = extract_tsx("src/components/CheckoutButton.tsx", TSX_CHECKOUT_FIXTURE);
        let helper = symbols
            .iter()
            .find(|s| kind_is(s, &super::SymbolKind::Function) && s.identifier.contains("::charge"))
            .expect("charge helper function");
        assert_eq!(
            helper.identifier,
            "src/components/CheckoutButton.tsx::charge"
        );
        assert!(helper.text.contains("provider.charge"));
    }

    #[test]
    fn tsx_imports_record_is_fts_only() {
        let symbols = extract_tsx("src/components/CheckoutButton.tsx", TSX_CHECKOUT_FIXTURE);
        let imports: Vec<_> = symbols
            .iter()
            .filter(|s| s.symbol_kind == super::SymbolKind::Imports)
            .collect();
        assert_eq!(imports.len(), 1);
        let rec = &imports[0];
        assert!(!rec.embed);
        assert!(rec.text.contains("PaymentProvider"));
    }

    #[test]
    fn tsx_jsx_does_not_create_top_level_symbols() {
        // JSX tags and embedded expressions should not produce spurious
        // symbols. Verify by counting: only the named declarations +
        // Comments + Imports records.
        let symbols = extract_tsx("src/components/CheckoutButton.tsx", TSX_CHECKOUT_FIXTURE);
        // Expected: CheckoutButtonProps (Interface), CheckoutResult / CheckoutLabelProps
        // (not present), CheckoutButton (Function), CheckoutLabel (Function),
        // charge (Function), __comments__, __imports__.
        // We don't pin the exact count (the fixture is shared with TS tests);
        // we just confirm no `button`, `span`, `JSXElement` kind leaked.
        for s in &symbols {
            assert!(
                !s.identifier.contains("button") && !s.identifier.contains("<"),
                "JSX leaked into identifier: {}",
                s.identifier
            );
        }
    }

    #[test]
    fn tsx_and_ts_dispatch_to_different_grammars() {
        // A .ts file with TSX syntax (e.g. a stray `<T>` generic) should
        // still extract via the TS grammar; a .tsx file with JSX should
        // also extract. Both grammars should accept a basic component
        // declaration in their respective files.
        let ts = r#"
            export function plain(x: number): number { return x; }
        "#;
        let tsx = r#"
            export function Component() { return <div />; }
        "#;
        let ts_symbols = extract_ts("src/only-ts.ts", ts);
        let tsx_symbols = extract_tsx("src/only-tsx.tsx", tsx);

        let ts_fn = ts_symbols
            .iter()
            .find(|s| kind_is(s, &super::SymbolKind::Function) && s.identifier.contains("plain"))
            .expect("plain function in .ts");
        assert_eq!(ts_fn.identifier, "src/only-ts.ts::plain");

        let tsx_fn = tsx_symbols
            .iter()
            .find(|s| {
                kind_is(s, &super::SymbolKind::Function) && s.identifier.contains("Component")
            })
            .expect("Component function in .tsx");
        assert_eq!(tsx_fn.identifier, "src/only-tsx.tsx::Component");
    }

    const JS_TASK2_CHECKOUT_FIXTURE: &str = r#"import {
    PaymentProvider,
    PaymentConfig,
} from "./payment";

export {
    legacyCheckout,
    legacyRefund,
} from "./legacy";

const provider = require("./provider");
const paymentClient = import("./payment-client");
// TODO: temporary limitation — only USD checkout is supported.

/**
 * Creates a checkout session after checkout validation.
 */
export function createCheckoutSession(cart) {
    if (!cart || cart.length === 0) {
        return "empty cart";
    }
    return "checkout session";
}

/** Builds a payment operation with a distinctive charge statement. */
const chargePayment = (amount) => {
    return provider.charge(amount);
};

const DEFAULT_TIMEOUT = 3000;
let runtimeMode = "live";
var legacyMode = "compat";
const destructured = { nested: true };
const { skipped } = destructured;

/** Owns the configured payment provider. */
class PaymentGateway {
    constructor(config) {
        this.config = config;
    }

    static fromConfig(config) {
        return new PaymentGateway(config);
    }

    get configured() {
        return this.config.enabled;
    }

    set configured(value) {
        this.config.enabled = value;
    }

    charge(amount) {
        return this.config.provider.charge(amount);
    }

    sibling() {
        return "sibling";
    }
}

const paymentApi = {
    fetchPayment() {
        return "fetch payment";
    },
    submitPayment: (payload) => {
        return payload.process();
    },
    nested: {
        hiddenMethod() {
            return "hidden";
        },
    },
    [dynamicName]() {
        return "computed";
    },
};

PaymentGateway.prototype.refund = function (amount) {
    return amount;
};
PaymentGateway.prototype[dynamicName] = () => "dynamic";

function outerExecutable() {
    function nestedFunction() {
        return "nested";
    }
    const nestedCallable = () => "nested callable";
    const localValue = 1;
    items.map((item) => item.id);
    return nestedFunction() + nestedCallable() + localValue;
}

export default function () {
    return "default checkout";
}

exports.checkout = function () {
    return "exported checkout";
};
module.exports = function () {
    return "module checkout";
};
"#;

    fn extract_js(file_path: &str, source: &str) -> Vec<super::CodeSymbol> {
        let language = if file_path.ends_with(".jsx") {
            super::CodeLanguage::Jsx
        } else {
            super::CodeLanguage::JavaScript
        };
        let extractor = super::CodeExtractor::for_language(language);
        super::extract_symbols_with_extractor(
            &extractor,
            std::path::Path::new("."),
            std::path::Path::new(file_path),
            source,
        )
        .expect("javascript extraction")
    }

    fn js_symbol<'a>(
        symbols: &'a [super::CodeSymbol],
        kind: &super::SymbolKind,
        identifier: &str,
    ) -> &'a super::CodeSymbol {
        symbols
            .iter()
            .find(|symbol| &symbol.symbol_kind == kind && symbol.identifier == identifier)
            .unwrap_or_else(|| {
                panic!(
                    "missing {identifier}: {:?}",
                    symbols.iter().map(|s| &s.identifier).collect::<Vec<_>>()
                )
            })
    }

    #[test]
    fn javascript_task2_checkout_fixture_extracts_product_evidence() {
        let symbols = extract_js("src/checkout/session.js", JS_TASK2_CHECKOUT_FIXTURE);
        let file = "src/checkout/session.js";
        let checkout = js_symbol(
            &symbols,
            &super::SymbolKind::Function,
            &format!("{file}::createCheckoutSession"),
        );
        assert!(checkout.text.contains("checkout validation"));
        assert!(checkout.text.contains("cart.length"));
        assert!(checkout.embed);

        let provider = js_symbol(
            &symbols,
            &super::SymbolKind::Const,
            &format!("{file}::provider"),
        );
        assert!(provider.text.contains("require"));
        let gateway = js_symbol(
            &symbols,
            &super::SymbolKind::Class,
            &format!("{file}::PaymentGateway"),
        );
        assert!(gateway.text.contains("configured payment provider"));
        let charge = js_symbol(
            &symbols,
            &super::SymbolKind::Method,
            &format!("{file}::PaymentGateway::charge"),
        );
        assert!(charge.text.contains("provider.charge"));
        assert!(!charge.text.contains("sibling"));

        for (name, kind) in [
            ("DEFAULT_TIMEOUT", super::SymbolKind::Const),
            ("runtimeMode", super::SymbolKind::Variable),
            ("legacyMode", super::SymbolKind::Variable),
            ("chargePayment", super::SymbolKind::Function),
        ] {
            assert!(symbols.iter().any(|symbol| {
                symbol.identifier == format!("{file}::{name}") && symbol.symbol_kind == kind
            }));
        }

        for identifier in [
            "paymentApi::fetchPayment",
            "paymentApi::submitPayment",
            "PaymentGateway::fromConfig",
            "PaymentGateway::configured",
            "PaymentGateway::refund",
            "checkout",
        ] {
            assert!(
                symbols
                    .iter()
                    .any(|symbol| symbol.identifier == format!("{file}::{identifier}")),
                "missing {identifier}"
            );
        }
        assert!(symbols.iter().any(|symbol| {
            symbol.identifier == format!("{file}::__default_export")
                && symbol.symbol_kind == super::SymbolKind::Function
        }));
        assert!(symbols.iter().any(|symbol| {
            symbol.identifier == format!("{file}::__module_export")
                && symbol.symbol_kind == super::SymbolKind::Function
        }));
    }

    #[test]
    fn javascript_task2_methods_have_local_ranges_and_text() {
        let source = "class Gateway {\n    first() {\n        return \"first only\";\n    }\n\n    second() {\n        return \"second only\";\n    }\n}\n\nconst api = {\n    alpha() {\n        return \"alpha only\";\n    },\n    beta() {\n        return \"beta only\";\n    },\n};\n";
        let symbols = extract_js("src/methods.js", source);
        let first = js_symbol(
            &symbols,
            &super::SymbolKind::Method,
            "src/methods.js::Gateway::first",
        );
        assert_eq!(first.line_start, 2);
        assert_eq!(first.line_end, 4);
        assert!(first.text.contains("first only"));
        assert!(!first.text.contains("second only"));
        let alpha = js_symbol(
            &symbols,
            &super::SymbolKind::Method,
            "src/methods.js::api::alpha",
        );
        assert_eq!(alpha.line_start, 12);
        assert_eq!(alpha.line_end, 14);
        assert!(alpha.text.contains("alpha only"));
        assert!(!alpha.text.contains("beta only"));
    }

    #[test]
    fn javascript_task2_excludes_nested_executables_and_unsupported_forms() {
        let symbols = extract_js("src/nested.js", JS_TASK2_CHECKOUT_FIXTURE);
        for name in [
            "nestedFunction",
            "nestedCallable",
            "localValue",
            "hiddenMethod",
            "dynamicName",
            "skipped",
        ] {
            assert!(
                !symbols
                    .iter()
                    .any(|symbol| symbol.identifier.ends_with(&format!("::{name}"))),
                "nested or unsupported symbol leaked: {name}"
            );
        }
        assert!(!symbols
            .iter()
            .any(|symbol| symbol.identifier.contains("items")));
    }

    #[test]
    fn javascript_task2_comments_and_imports_are_fts_only_and_complete() {
        let symbols = extract_js("src/imports.js", JS_TASK2_CHECKOUT_FIXTURE);
        let comments = symbols
            .iter()
            .find(|symbol| symbol.symbol_kind == super::SymbolKind::Comments)
            .expect("comments record");
        assert!(!comments.embed);
        assert!(!comments.text.contains("Creates a checkout session"));
        let imports = symbols
            .iter()
            .find(|symbol| symbol.symbol_kind == super::SymbolKind::Imports)
            .expect("imports record");
        assert!(!imports.embed);
        for text in [
            "PaymentProvider",
            "PaymentConfig",
            "legacyRefund",
            "./provider",
            "./payment-client",
        ] {
            assert!(
                imports.text.contains(text),
                "imports missing {text}: {}",
                imports.text
            );
        }
        assert!(imports.text.contains("import(\"./payment-client\")"));
    }

    #[test]
    fn javascript_destructured_require_is_extracted_as_import_evidence() {
        let source = r#"const { charge, refund: issueRefund } = require("./payments");
const [primaryGateway] = require("./gateways");
"#;
        let symbols = extract_js("src/requires.js", source);
        let imports = js_symbol(
            &symbols,
            &super::SymbolKind::Imports,
            "src/requires.js::__imports__",
        );

        assert!(!imports.embed);
        assert_eq!(imports.line_start, 1);
        assert_eq!(imports.line_end, 2);
        assert!(imports
            .text
            .contains(r#"const { charge, refund: issueRefund } = require("./payments");"#));
        assert!(imports
            .text
            .contains(r#"const [primaryGateway] = require("./gateways");"#));
        assert!(
            !symbols.iter().any(|symbol| {
                symbol.identifier.ends_with("::charge")
                    || symbol.identifier.ends_with("::issueRefund")
                    || symbol.identifier.ends_with("::primaryGateway")
            }),
            "destructured require bindings must remain import evidence, not top-level symbols"
        );
    }

    #[test]
    fn javascript_task2_handles_empty_comment_only_and_malformed_sources() {
        let empty = extract_js("src/empty.js", "");
        assert!(empty.is_empty());
        let comment_only = extract_js(
            "src/comments.js",
            "// limitation only\n/* another note */\n",
        );
        assert_eq!(comment_only.len(), 1);
        assert_eq!(comment_only[0].symbol_kind, super::SymbolKind::Comments);
        assert!(!comment_only[0].embed);
        let malformed = extract_js(
            "src/malformed.js",
            "function usable() { return true; }\nconst broken = {\n",
        );
        assert!(malformed
            .iter()
            .any(|symbol| symbol.identifier.ends_with("::usable")));
    }

    #[test]
    fn javascript_task2_handles_destructuring_exports_and_jsdoc() {
        let source = "/** Named export documentation. */\nexport function namedExport() { return 1; }\nexport default class {\n    namedMethod() { return 2; }\n}\nconst { ignored } = { ignored: true };\n";
        let symbols = extract_js("src/exports.js", source);
        let named = js_symbol(
            &symbols,
            &super::SymbolKind::Function,
            "src/exports.js::namedExport",
        );
        assert!(named.text.contains("Named export documentation"));
        assert!(!symbols
            .iter()
            .any(|symbol| symbol.identifier.ends_with("::ignored")));
        assert!(symbols.iter().any(|symbol| {
            symbol.identifier == "src/exports.js::__default_export"
                && symbol.symbol_kind == super::SymbolKind::Class
        }));
        assert!(symbols.iter().any(|symbol| {
            symbol.identifier == "src/exports.js::__default_export::namedMethod"
                && symbol.symbol_kind == super::SymbolKind::Method
        }));
        let comments = symbols
            .iter()
            .find(|symbol| symbol.symbol_kind == super::SymbolKind::Comments);
        assert!(
            comments.is_none()
                || !comments
                    .unwrap()
                    .text
                    .contains("Named export documentation")
        );
    }

    #[test]
    fn jsx_task2_callable_binding_keeps_jsx_body_and_excludes_callbacks() {
        let source = "/** Checkout button documentation. */\nconst CheckoutButton = ({ disabled, label }) => (\n    <button disabled={disabled} onClick={() => alert(label)}>\n        {label}\n    </button>\n);\nfunction helper() { return true; }\n";
        let symbols = extract_js("src/CheckoutButton.jsx", source);
        let component = js_symbol(
            &symbols,
            &super::SymbolKind::Function,
            "src/CheckoutButton.jsx::CheckoutButton",
        );
        assert!(component.text.contains("CheckoutButton"));
        assert!(component.text.contains("<button"));
        assert!(component.text.contains("disabled"));
        assert!(!symbols
            .iter()
            .any(|symbol| symbol.identifier.contains("alert")));
        assert!(!symbols
            .iter()
            .any(|symbol| symbol.identifier.contains("button")));
        assert!(symbols
            .iter()
            .any(|symbol| symbol.identifier.ends_with("::helper")));
    }

    // ── Epic 010 Task 3: JSX component extraction tests ─────────────────

    /// Product-shaped JSX fixture for Task 3. Exercises every component
    /// shape (function, arrow, class), default props, destructured props,
    /// fragments, expression-container callbacks, nested tags, and the
    /// `items.map(item => ...)` callback pattern. Helper function kept
    /// module-scope so its independent searchability is asserted in one of
    /// the focused tests below.
    const JSX_TASK3_FIXTURE: &str = r#"import React from "react";

/**
 * Function component with default props and a product-visible disabled
 * condition. The button stays disabled while checkout validation runs.
 */
export function CheckoutButton({ disabled = false, label = "Pay" }) {
    return (
        <button
            disabled={disabled}
            onClick={() => submitCheckout(label)}
        >
            {disabled ? "Loading..." : label}
        </button>
    );
}

/** Arrow component with destructured props and a click handler call. */
export const CheckoutLabel = ({ amount, provider }) => (
    <span onClick={() => recordClick(amount)}>
        Charging {amount} via {provider.name}
    </span>
);

/** Class component with adjacent render + handler methods. */
export class CheckoutForm extends React.Component {
    render() {
        return (
            <ul>
                {this.props.items.map((item) => (
                    <ItemRow key={item.id} item={item} />
                ))}
            </ul>
        );
    }

    handleClick() {
        return "clicked";
    }

    anotherMethod() {
        return "another only";
    }
}

/** Helper function — must remain independently searchable. */
function helper() { return true; }

const ItemRow = ({ item }) => <li>{item.name}</li>;
"#;

    /// JSX-only fragment fixture for parse-correctness assertions.
    const JSX_TASK3_FRAGMENT_FIXTURE: &str = r#"import React from "react";

/**
 * A component that uses a fragment and an items.map callback.
 */
export function ItemList({ items }) {
    return (
        <>
            <header>
                <h2>only header body</h2>
            </header>
            <ul>
                {items.map((item) => (
                    <ItemRow key={item.id} item={item} />
                ))}
            </ul>
        </>
    );
}
"#;

    /// Default-export fixture: named default function export. Kept in its
    /// own file because valid JS modules allow exactly one `export
    /// default`; combining named and anonymous defaults would be invalid
    /// syntax and trigger dedup ambiguity.
    const JSX_TASK3_NAMED_DEFAULT_FIXTURE: &str = r#"import React from "react";

/** Named default function export. */
export default function NamedDefault(props) {
    return <form>{props.children}</form>;
}
"#;

    /// Default-export fixture: anonymous default class export with
    /// adjacent methods.
    const JSX_TASK3_ANONYMOUS_DEFAULT_FIXTURE: &str = r#"import React from "react";

/** Anonymous default class export with adjacent methods. */
export default class {
    render() {
        return <div>only render body</div>;
    }

    handleClick() {
        return "clicked";
    }
}
"#;

    /// Default-export fixture: anonymous default arrow export.
    const JSX_TASK3_ANONYMOUS_ARROW_DEFAULT_FIXTURE: &str = r#"import React from "react";

/** Anonymous default arrow export. */
export default (props) => <button>{props.label}</button>;
"#;

    #[test]
    fn jsx_task3_function_component_indexed_under_stable_name() {
        let symbols = extract_js(
            "src/components/CheckoutButton.jsx",
            JSX_TASK3_FIXTURE,
        );
        let component = js_symbol(
            &symbols,
            &super::SymbolKind::Function,
            "src/components/CheckoutButton.jsx::CheckoutButton",
        );
        assert_eq!(component.file_path, "src/components/CheckoutButton.jsx");
        assert!(component.embed);
    }

    #[test]
    fn jsx_task3_function_component_retains_default_props_in_signature() {
        let symbols = extract_js(
            "src/components/CheckoutButton.jsx",
            JSX_TASK3_FIXTURE,
        );
        let component = js_symbol(
            &symbols,
            &super::SymbolKind::Function,
            "src/components/CheckoutButton.jsx::CheckoutButton",
        );
        assert!(
            component.text.contains("disabled = false"),
            "default-prop context must appear in signature/text: {}",
            component.text
        );
        assert!(
            component.text.contains("label = \"Pay\""),
            "default-prop context for label must appear: {}",
            component.text
        );
    }

    #[test]
    fn jsx_task3_function_component_body_excerpt_contains_disabled_condition() {
        let symbols = extract_js(
            "src/components/CheckoutButton.jsx",
            JSX_TASK3_FIXTURE,
        );
        let component = js_symbol(
            &symbols,
            &super::SymbolKind::Function,
            "src/components/CheckoutButton.jsx::CheckoutButton",
        );
        assert!(
            component.text.contains("disabled"),
            "body excerpt must contain the `disabled` JSX attribute: {}",
            component.text
        );
        assert!(
            component.text.contains("Loading"),
            "body excerpt must contain the disabled-state copy: {}",
            component.text
        );
        assert!(
            component.text.contains("<button"),
            "body excerpt must include the JSX opening tag: {}",
            component.text
        );
    }

    #[test]
    fn jsx_task3_arrow_component_retains_destructured_props_and_click_handler() {
        let symbols = extract_js(
            "src/components/CheckoutButton.jsx",
            JSX_TASK3_FIXTURE,
        );
        let component = js_symbol(
            &symbols,
            &super::SymbolKind::Function,
            "src/components/CheckoutButton.jsx::CheckoutLabel",
        );
        assert!(
            component.text.contains("amount")
                && component.text.contains("provider"),
            "destructured prop names must appear in the signature/text: {}",
            component.text
        );
        assert!(
            component.text.contains("recordClick"),
            "body excerpt must contain the click handler call: {}",
            component.text
        );
        assert!(
            component.text.contains("<span"),
            "body excerpt must include the JSX opening tag: {}",
            component.text
        );
    }

    #[test]
    fn jsx_task3_class_component_indexed_with_class_kind() {
        let symbols = extract_js(
            "src/components/CheckoutButton.jsx",
            JSX_TASK3_FIXTURE,
        );
        let class_symbol = js_symbol(
            &symbols,
            &super::SymbolKind::Class,
            "src/components/CheckoutButton.jsx::CheckoutForm",
        );
        assert_eq!(class_symbol.file_path, "src/components/CheckoutButton.jsx");
        assert!(class_symbol.embed);
        assert!(
            class_symbol.text.contains("CheckoutForm"),
            "class record must mention the class name: {}",
            class_symbol.text
        );
    }

    #[test]
    fn jsx_task3_class_component_adjacent_methods_have_method_local_evidence() {
        let symbols = extract_js(
            "src/components/CheckoutButton.jsx",
            JSX_TASK3_FIXTURE,
        );

        let render = js_symbol(
            &symbols,
            &super::SymbolKind::Method,
            "src/components/CheckoutButton.jsx::CheckoutForm::render",
        );
        assert!(render.text.contains("render"));
        assert!(
            render.text.contains("<ul"),
            "render body excerpt must contain the JSX ul tag: {}",
            render.text
        );
        assert!(
            render.text.contains("items.map"),
            "render body excerpt must contain the items.map call: {}",
            render.text
        );
        assert!(
            !render.text.contains("clicked"),
            "render text leaked `handleClick` sibling: {}",
            render.text
        );
        assert!(
            !render.text.contains("another only"),
            "render text leaked `anotherMethod` sibling: {}",
            render.text
        );

        let handle = js_symbol(
            &symbols,
            &super::SymbolKind::Method,
            "src/components/CheckoutButton.jsx::CheckoutForm::handleClick",
        );
        assert!(handle.text.contains("handleClick"));
        assert!(handle.text.contains("clicked"));
        assert!(
            !handle.text.contains("<ul"),
            "handleClick text leaked `render` sibling: {}",
            handle.text
        );
        assert!(
            !handle.text.contains("another only"),
            "handleClick text leaked `anotherMethod` sibling: {}",
            handle.text
        );

        let another = js_symbol(
            &symbols,
            &super::SymbolKind::Method,
            "src/components/CheckoutButton.jsx::CheckoutForm::anotherMethod",
        );
        assert!(another.text.contains("anotherMethod"));
        assert!(another.text.contains("another only"));
        assert!(
            !another.text.contains("clicked"),
            "anotherMethod text leaked `handleClick` sibling: {}",
            another.text
        );
        assert!(
            !another.text.contains("<ul"),
            "anotherMethod text leaked `render` sibling: {}",
            another.text
        );

        assert!(render.line_end < handle.line_start);
        assert!(handle.line_end < another.line_start);
    }

    #[test]
    fn jsx_task3_helper_function_remains_independently_searchable() {
        let symbols = extract_js(
            "src/components/CheckoutButton.jsx",
            JSX_TASK3_FIXTURE,
        );
        let helper = js_symbol(
            &symbols,
            &super::SymbolKind::Function,
            "src/components/CheckoutButton.jsx::helper",
        );
        assert!(helper.text.contains("function helper"));
        assert!(helper.embed);
    }

    #[test]
    fn jsx_task3_module_level_arrow_const_component_indexed() {
        let symbols = extract_js(
            "src/components/CheckoutButton.jsx",
            JSX_TASK3_FIXTURE,
        );
        let item_row = js_symbol(
            &symbols,
            &super::SymbolKind::Function,
            "src/components/CheckoutButton.jsx::ItemRow",
        );
        assert!(item_row.text.contains("ItemRow"));
        assert!(item_row.text.contains("<li"));
    }

    #[test]
    fn jsx_task3_jsx_only_syntax_parses_without_error() {
        let mut parser = tree_sitter::Parser::new();
        let lang: tree_sitter::Language =
            super::CodeExtractor::for_language(super::CodeLanguage::Jsx)
                .tree_sitter_language()
                .clone();
        parser.set_language(&lang).expect("set JSX grammar");

        let tree = parser
            .parse(JSX_TASK3_FRAGMENT_FIXTURE, None)
            .expect("parse JSX fragment fixture");
        assert!(
            !tree.root_node().has_error(),
            "JSX fragment + nested tags + items.map must parse cleanly"
        );
    }

    #[test]
    fn jsx_task3_jsx_fragments_nested_tags_and_expression_containers_do_not_become_symbols() {
        let symbols = extract_js(
            "src/components/ItemList.jsx",
            JSX_TASK3_FRAGMENT_FIXTURE,
        );
        for forbidden in ["header", "h2", "ul", "li", "Fragment"] {
            assert!(
                !symbols
                    .iter()
                    .any(|symbol| symbol.identifier
                        .ends_with(&format!("::{forbidden}"))),
                "{forbidden} leaked as a top-level symbol"
            );
        }
        assert!(
            symbols.iter().any(|symbol| symbol.identifier
                == "src/components/ItemList.jsx::ItemList"),
            "ItemList function component must still be indexed"
        );
    }

    #[test]
    fn jsx_task3_items_map_callback_does_not_become_symbol() {
        let symbols = extract_js(
            "src/components/ItemList.jsx",
            JSX_TASK3_FRAGMENT_FIXTURE,
        );
        for forbidden in ["items", "item"] {
            assert!(
                !symbols
                    .iter()
                    .any(|symbol| symbol.identifier
                        .ends_with(&format!("::{forbidden}"))),
                "{forbidden} leaked as a top-level symbol"
            );
        }
        assert!(
            symbols.iter().any(|symbol| symbol.identifier
                == "src/components/ItemList.jsx::ItemList"),
            "ItemList function component must still be indexed"
        );
    }

    #[test]
    fn jsx_task3_named_default_function_export_uses_name_not_default_export() {
        let symbols = extract_js(
            "src/components/Default.jsx",
            JSX_TASK3_NAMED_DEFAULT_FIXTURE,
        );
        let named = js_symbol(
            &symbols,
            &super::SymbolKind::Function,
            "src/components/Default.jsx::NamedDefault",
        );
        assert!(named.text.contains("NamedDefault"));
        assert!(
            !symbols.iter().any(|symbol| {
                symbol.identifier == "src/components/Default.jsx::__default_export"
                    && symbol.symbol_kind == super::SymbolKind::Function
            }),
            "named default export must not collapse to the __default_export identifier"
        );
    }

    #[test]
    fn jsx_task3_anonymous_default_arrow_export_uses_default_export_identifier() {
        let symbols = extract_js(
            "src/components/Default.jsx",
            JSX_TASK3_ANONYMOUS_ARROW_DEFAULT_FIXTURE,
        );
        let default_arrow = js_symbol(
            &symbols,
            &super::SymbolKind::Function,
            "src/components/Default.jsx::__default_export",
        );
        assert!(default_arrow.text.contains("<button"));
        assert!(default_arrow.text.contains("props.label"));
    }

    #[test]
    fn jsx_task3_anonymous_default_class_export_methods_are_namespaced() {
        let symbols = extract_js(
            "src/components/Default.jsx",
            JSX_TASK3_ANONYMOUS_DEFAULT_FIXTURE,
        );
        let class_symbol = js_symbol(
            &symbols,
            &super::SymbolKind::Class,
            "src/components/Default.jsx::__default_export",
        );
        assert!(class_symbol.embed);

        let render = js_symbol(
            &symbols,
            &super::SymbolKind::Method,
            "src/components/Default.jsx::__default_export::render",
        );
        assert!(render.text.contains("only render body"));
        assert!(
            !render.text.contains("clicked"),
            "render leaked handleClick sibling: {}",
            render.text
        );

        let handle = js_symbol(
            &symbols,
            &super::SymbolKind::Method,
            "src/components/Default.jsx::__default_export::handleClick",
        );
        assert!(handle.text.contains("clicked"));
        assert!(
            !handle.text.contains("only render body"),
            "handleClick leaked render sibling: {}",
            handle.text
        );
    }
}
