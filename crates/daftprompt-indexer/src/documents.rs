use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;

/// Files at or below this size (in bytes) produce a single item.
/// Larger files are chunked. Initially 16 KiB.
pub const DOCUMENT_CHUNK_THRESHOLD_BYTES: usize = 16 * 1024;

/// Maximum chunk size in bytes for a single chunk after splitting.
/// Chunks exceeding this are split at paragraph/line boundaries.
const MAX_CHUNK_BYTES: usize = 16 * 1024;

/// Document extensions eligible for indexing.
const DOCUMENT_EXTENSIONS: &[&str] = &["md", "markdown", "txt", "text", "rst", "adoc"];

/// Extensionless filenames eligible for indexing (case-insensitive).
const EXTENSIONLESS_NAMES: &[&str] = &["readme", "license"];

/// A single chunk of a document ready for indexing.
#[derive(Debug, Clone)]
pub struct DocumentChunk {
    /// Repo-relative canonical file path (forward slashes).
    pub file_path: String,
    /// Chunk ordinal (0-based). 0 for unchunked files.
    pub ordinal: usize,
    /// The text content of this chunk.
    pub text: String,
    /// Starting line number (1-based) within the file.
    pub line_start: usize,
    /// Ending line number (1-based) within the file.
    pub line_end: usize,
    /// For Markdown: the heading ancestor path (e.g., "## Section > ### Sub").
    pub heading_path: Option<String>,
}

/// Canonicalize a file path to repo-relative, forward-slash form.
/// Strips the repo prefix, normalizes separators, removes leading `./`.
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

/// Discover eligible document files tracked by git.
///
/// Returns absolute paths. Includes `.md`, `.markdown`, `.txt`, `.text`,
/// `.rst`, `.adoc`, plus extensionless files named `README` or `LICENSE`
/// (case-insensitive). Only git-tracked files are included.
pub fn list_tracked_document_files(repo_path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let repo = gix::discover(repo_path)?;

    let work_dir = match repo.workdir() {
        Some(dir) => dir.to_path_buf(),
        None => return Ok(vec![]),
    };

    let tree_id = repo.head_tree_id_or_empty()?;
    let tree = repo.find_tree(tree_id)?;

    let entries = tree.traverse().breadthfirst.files()?;

    let doc_files: Vec<PathBuf> = entries
        .into_iter()
        .filter(|entry| entry.mode.is_blob())
        .filter_map(|entry| {
            let filepath = entry.filepath.to_str().ok()?;
            let path = Path::new(filepath);
            let filename = path.file_name().and_then(|n| n.to_str())?;

            let eligible = if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                DOCUMENT_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
            } else {
                // Extensionless: check if the filename matches README or LICENSE
                EXTENSIONLESS_NAMES.contains(&filename.to_ascii_lowercase().as_str())
            };

            if eligible {
                Some(work_dir.join(filepath))
            } else {
                None
            }
        })
        .collect();

    Ok(doc_files)
}

/// Extract document chunks from file content.
///
/// For files at or below `DOCUMENT_CHUNK_THRESHOLD_BYTES`, produces a single
/// chunk. For larger files:
/// - Markdown is split on ATX headings, preserving heading ancestry.
/// - Plain text is split at paragraph (blank-line) boundaries.
/// - Over-large sections/paragraphs are split at bounded text windows.
pub fn extract_document_chunks(
    file_path: &str,
    source: &str,
    extension: &str,
) -> Vec<DocumentChunk> {
    let byte_len = source.len();

    if byte_len <= DOCUMENT_CHUNK_THRESHOLD_BYTES {
        let line_count = source.lines().count();
        return vec![DocumentChunk {
            file_path: file_path.to_string(),
            ordinal: 0,
            text: source.to_string(),
            line_start: 1,
            line_end: line_count.max(1),
            heading_path: None,
        }];
    }

    let is_markdown = matches!(extension, "md" | "markdown");

    if is_markdown {
        chunk_markdown(file_path, source)
    } else {
        chunk_plain_text(file_path, source)
    }
}

/// Split Markdown on ATX headings, preserving heading ancestry.
fn chunk_markdown(file_path: &str, source: &str) -> Vec<DocumentChunk> {
    let lines: Vec<&str> = source.lines().collect();
    let mut chunks: Vec<DocumentChunk> = Vec::new();

    // Collect heading hierarchy: (level, text)
    let mut heading_stack: Vec<(usize, String)> = Vec::new();
    // Current section lines
    let mut section_lines: Vec<&str> = Vec::new();
    let mut section_start_line: usize = 1;

    let flush = |chunks: &mut Vec<DocumentChunk>,
                 section_lines: &mut Vec<&str>,
                 heading_stack: &[(usize, String)],
                 start_line: usize,
                 ordinal: &mut usize| {
        if section_lines.is_empty() {
            return;
        }
        let text = section_lines.join("\n");
        let heading_path = if heading_stack.is_empty() {
            None
        } else {
            Some(
                heading_stack
                    .iter()
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" > "),
            )
        };
        let line_end = start_line + section_lines.len() - 1;

        // If the section itself is too large, split at paragraph boundaries
        if text.len() > MAX_CHUNK_BYTES {
            let sub_chunks =
                split_at_paragraphs(file_path, &text, start_line, heading_path.as_deref());
            for mut chunk in sub_chunks {
                chunk.ordinal = *ordinal;
                *ordinal += 1;
                chunks.push(chunk);
            }
        } else {
            chunks.push(DocumentChunk {
                file_path: file_path.to_string(),
                ordinal: *ordinal,
                text,
                line_start: start_line,
                line_end,
                heading_path,
            });
            *ordinal += 1;
        }
        section_lines.clear();
    };

    let mut ordinal = 0usize;

    for (i, line) in lines.iter().enumerate() {
        let line_no = i + 1;

        // Detect ATX headings: lines starting with 1-6 '#' characters
        let heading_level = line.chars().take_while(|&c| c == '#').count();

        if heading_level >= 1 && heading_level <= 6 {
            // Check that there's a space after the hashes (or it's the whole line)
            let after_hashes = &line[heading_level..];
            if after_hashes.is_empty() || after_hashes.starts_with(' ') {
                // Flush previous section
                flush(
                    &mut chunks,
                    &mut section_lines,
                    &heading_stack,
                    section_start_line,
                    &mut ordinal,
                );

                let heading_text = line.to_string();

                // Pop heading stack to the correct level
                while let Some(&(level, _)) = heading_stack.last() {
                    if level >= heading_level {
                        heading_stack.pop();
                    } else {
                        break;
                    }
                }
                heading_stack.push((heading_level, heading_text));

                section_start_line = line_no;
                // Don't include the heading line in the section — it's part of
                // the heading_path. Or include it as the first line of the section.
                section_lines.push(line);
                continue;
            }
        }

        section_lines.push(line);
    }

    // Flush the last section
    flush(
        &mut chunks,
        &mut section_lines,
        &heading_stack,
        section_start_line,
        &mut ordinal,
    );

    chunks
}

/// Split plain text at paragraph (blank-line) boundaries.
fn chunk_plain_text(file_path: &str, source: &str) -> Vec<DocumentChunk> {
    let lines: Vec<&str> = source.lines().collect();
    let mut chunks: Vec<DocumentChunk> = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut current_start: usize = 1;
    let mut ordinal = 0usize;

    for (i, line) in lines.iter().enumerate() {
        let line_no = i + 1;

        if line.trim().is_empty() && !current_lines.is_empty() {
            // Paragraph boundary
            let text = current_lines.join("\n");
            if text.len() > MAX_CHUNK_BYTES {
                let sub_chunks =
                    split_at_bounded_windows(file_path, &text, current_start, &mut ordinal);
                chunks.extend(sub_chunks);
            } else {
                let end_line = current_start + current_lines.len() - 1;
                chunks.push(DocumentChunk {
                    file_path: file_path.to_string(),
                    ordinal,
                    text,
                    line_start: current_start,
                    line_end: end_line,
                    heading_path: None,
                });
                ordinal += 1;
            }
            current_lines.clear();
            current_start = line_no + 1;
        } else {
            if current_lines.is_empty() {
                current_start = line_no;
            }
            current_lines.push(line);
        }
    }

    // Flush remaining
    if !current_lines.is_empty() {
        let text = current_lines.join("\n");
        if text.len() > MAX_CHUNK_BYTES {
            let sub_chunks =
                split_at_bounded_windows(file_path, &text, current_start, &mut ordinal);
            chunks.extend(sub_chunks);
        } else {
            let end_line = current_start + current_lines.len() - 1;
            chunks.push(DocumentChunk {
                file_path: file_path.to_string(),
                ordinal,
                text,
                line_start: current_start,
                line_end: end_line,
                heading_path: None,
            });
        }
    }

    chunks
}

/// Split text at paragraph boundaries, keeping each chunk under MAX_CHUNK_BYTES.
fn split_at_paragraphs(
    file_path: &str,
    source: &str,
    base_line: usize,
    heading_path: Option<&str>,
) -> Vec<DocumentChunk> {
    let mut chunks: Vec<DocumentChunk> = Vec::new();
    let mut current_text = String::new();
    let mut current_start_line = base_line;
    let mut line_offset = 0usize;
    let mut ordinal = 0usize;

    for para in source.split("\n\n") {
        let para_lines: Vec<&str> = para.lines().collect();
        let para_line_count = para_lines.len();

        if !current_text.is_empty() && current_text.len() + para.len() + 2 > MAX_CHUNK_BYTES {
            let end_line = current_start_line + line_offset - 1;
            chunks.push(DocumentChunk {
                file_path: file_path.to_string(),
                ordinal,
                text: current_text.clone(),
                line_start: current_start_line,
                line_end: end_line,
                heading_path: heading_path.map(String::from),
            });
            ordinal += 1;
            current_text.clear();
            current_start_line = base_line + line_offset;
        }

        if !current_text.is_empty() {
            current_text.push_str("\n\n");
        }
        current_text.push_str(para);
        line_offset += para_line_count + 1; // +1 for the blank line
    }

    if !current_text.is_empty() {
        let end_line = current_start_line + line_offset - 1;
        chunks.push(DocumentChunk {
            file_path: file_path.to_string(),
            ordinal,
            text: current_text,
            line_start: current_start_line,
            line_end: end_line,
            heading_path: heading_path.map(String::from),
        });
    }

    chunks
}

/// Split text into bounded windows of ~MAX_CHUNK_BYTES at line boundaries.
fn split_at_bounded_windows(
    file_path: &str,
    source: &str,
    base_line: usize,
    ordinal: &mut usize,
) -> Vec<DocumentChunk> {
    let mut chunks: Vec<DocumentChunk> = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut current_byte_len = 0usize;
    let mut current_start = base_line;

    for (i, line) in lines.iter().enumerate() {
        let line_no = base_line + i;
        let line_bytes = line.len() + 1; // +1 for newline

        if !current_lines.is_empty() && current_byte_len + line_bytes > MAX_CHUNK_BYTES {
            let text = current_lines.join("\n");
            let end_line = current_start + current_lines.len() - 1;
            chunks.push(DocumentChunk {
                file_path: file_path.to_string(),
                ordinal: *ordinal,
                text,
                line_start: current_start,
                line_end: end_line,
                heading_path: None,
            });
            *ordinal += 1;
            current_lines.clear();
            current_byte_len = 0;
            current_start = line_no + 1;
        }

        if current_lines.is_empty() {
            current_start = line_no;
        }
        current_lines.push(line);
        current_byte_len += line_bytes;
    }

    if !current_lines.is_empty() {
        let text = current_lines.join("\n");
        let end_line = current_start + current_lines.len() - 1;
        chunks.push(DocumentChunk {
            file_path: file_path.to_string(),
            ordinal: *ordinal,
            text,
            line_start: current_start,
            line_end: end_line,
            heading_path: None,
        });
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_file_path_works() {
        let repo = std::path::Path::new("/home/user/project");
        let abs = std::path::Path::new("/home/user/project/docs/readme.md");
        assert_eq!(canonicalize_file_path(repo, abs), "docs/readme.md");

        let rel = std::path::Path::new("./docs/guide.md");
        assert_eq!(canonicalize_file_path(repo, rel), "docs/guide.md");

        let nested = std::path::Path::new("crates/foo/docs/api.rst");
        assert_eq!(
            canonicalize_file_path(repo, nested),
            "crates/foo/docs/api.rst"
        );
    }

    #[test]
    fn list_tracked_document_files_returns_results() {
        let repo_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let files = list_tracked_document_files(repo_path).expect("should list files");
        // The repo should have at least README.md and DEVELOP.md
        assert!(
            files.iter().any(|p| p.ends_with("README.md")),
            "should find README.md"
        );
        assert!(
            files.iter().any(|p| p.ends_with("DEVELOP.md")),
            "should find DEVELOP.md"
        );
    }

    #[test]
    fn extract_small_document_single_chunk() {
        let source = "# Hello\n\nThis is a small document.";
        let chunks = extract_document_chunks("docs/test.md", source, "md");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].ordinal, 0);
        assert_eq!(chunks[0].line_start, 1);
        assert_eq!(chunks[0].heading_path, None);
    }

    #[test]
    fn extract_markdown_heading_chunks() {
        // Build a source that exceeds the threshold
        let mut source = String::new();
        source.push_str("# Title\n\n");
        // Fill with enough text to exceed threshold
        let filler = "x".repeat(DOCUMENT_CHUNK_THRESHOLD_BYTES / 3);
        source.push_str(&format!("Intro section\n\n{}\n\n", filler));
        source.push_str("## Section A\n\n");
        source.push_str(&format!("Content A {}\n\n", filler));
        source.push_str("## Section B\n\n");
        source.push_str(&format!("Content B {}\n\n", filler));

        let chunks = extract_document_chunks("docs/big.md", &source, "md");
        assert!(chunks.len() > 1, "should produce multiple chunks");
        // Verify chunks have heading paths
        for chunk in &chunks {
            assert_eq!(chunk.file_path, "docs/big.md");
        }
    }

    #[test]
    fn extract_plain_text_paragraph_chunks() {
        let filler = "y".repeat(DOCUMENT_CHUNK_THRESHOLD_BYTES / 3);
        let source = format!("{}\n\n{}\n\n{}", filler, filler, filler);
        let chunks = extract_document_chunks("docs/big.txt", &source, "txt");
        assert!(chunks.len() > 1, "should produce multiple chunks");
    }

    #[test]
    fn extract_rst_file() {
        let source = "Title\n=====\n\nSome RST content.";
        let chunks = extract_document_chunks("docs/api.rst", source, "rst");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].file_path, "docs/api.rst");
    }

    #[test]
    fn extract_adoc_file() {
        let source = "= Title\n\nSome AsciiDoc content.";
        let chunks = extract_document_chunks("docs/guide.adoc", source, "adoc");
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn extract_txt_file() {
        let source = "Plain text content.\nAnother line.";
        let chunks = extract_document_chunks("notes.txt", source, "txt");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].line_start, 1);
        assert_eq!(chunks[0].line_end, 2);
    }

    #[test]
    fn chunk_ordinals_are_sequential() {
        let filler = "z".repeat(DOCUMENT_CHUNK_THRESHOLD_BYTES / 2);
        let source = format!("{}\n\n{}\n\n{}", filler, filler, filler);
        let chunks = extract_document_chunks("test.txt", &source, "txt");
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.ordinal, i);
        }
    }
}
