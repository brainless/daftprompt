use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use ignore::gitignore::Gitignore;

use super::{
    FileContent, Listing, ProjectFiles, ProjectToolConfig, Scope, SearchHit, SearchQuery,
    SearchScan, ToolError, check_components, check_relative_path, split_lines,
};

/// How much of a file is inspected to decide that it is binary or text.
const SNIFF_BYTES: usize = 8192;

/// [`ProjectFiles`] over a real directory.
///
/// Every path is resolved against the canonical root and must stay inside it
/// after symlinks are resolved. `.git`, hidden entries, files ignored by
/// `.gitignore` (root and nested; a git repository is not required), `.env`
/// and `.env.*` are never listed, searched, or read, and neither are binary,
/// non-UTF-8, or oversized files. Symlinks are never followed while walking.
#[derive(Debug, Clone)]
pub struct FsProject {
    root: PathBuf,
}

impl FsProject {
    /// Fails if `root` does not exist or is not a directory.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ToolError> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|e| ToolError::Io(format!("project root: {e}")))?;
        if !root.is_dir() {
            return Err(ToolError::Io("project root is not a directory".into()));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a model-supplied path to a canonical path inside the root and
    /// its normalized root-relative form. The lexical rules run on the input
    /// and again on the canonical result, so a symlink to `.env` or into a
    /// hidden or ignored place is denied too.
    fn resolve(&self, input: &str) -> Result<(PathBuf, String), ToolError> {
        let lexical = check_relative_path(input)?;
        let joined = self.root.join(&lexical);
        let canonical = joined.canonicalize().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ToolError::NotFound(lexical.clone()),
            _ => ToolError::Io(format!("{lexical}: {}", e.kind())),
        })?;
        let rel = canonical.strip_prefix(&self.root).map_err(|_| {
            ToolError::Denied(format!("{input}: resolves outside the project root"))
        })?;
        let rel = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        check_components(input, &rel)?;
        if self.is_ignored(&rel, canonical.is_dir()) {
            return Err(ToolError::Denied(format!("{rel}: ignored by .gitignore")));
        }
        Ok((canonical, rel))
    }

    /// Applies `.gitignore` files from the root down to the path's directory;
    /// a deeper file overrides a shallower one, and an ignored directory
    /// ignores everything below it.
    fn is_ignored(&self, rel: &str, is_dir: bool) -> bool {
        let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
        let mut matchers: Vec<(PathBuf, Gitignore)> = Vec::new();
        let mut dir = self.root.clone();
        for depth in 0..parts.len() {
            let file = dir.join(".gitignore");
            if file.is_file() {
                let (gi, _) = Gitignore::new(&file);
                matchers.push((dir.clone(), gi));
            }
            // Check the prefix ending at `depth`.
            let prefix_is_dir = depth + 1 < parts.len() || is_dir;
            let full = self.root.join(parts[..=depth].join("/"));
            let mut ignored = false;
            for (base, gi) in &matchers {
                if let Ok(local) = full.strip_prefix(base) {
                    let m = gi.matched(local, prefix_is_dir);
                    if m.is_ignore() {
                        ignored = true;
                    } else if m.is_whitelist() {
                        ignored = false;
                    }
                }
            }
            if ignored {
                return true;
            }
            dir = full;
        }
        false
    }

    /// The walk target: a canonical directory inside the root.
    fn scope_dir(&self, scope: &Scope) -> Result<PathBuf, ToolError> {
        match scope.directory.as_deref() {
            None => Ok(self.root.clone()),
            Some(d) => {
                let (canonical, rel) = self.resolve(d)?;
                if !canonical.is_dir() {
                    return Err(ToolError::InvalidArgument(format!(
                        "{rel} is not a directory"
                    )));
                }
                Ok(canonical)
            }
        }
    }

    /// Text files under the target in path order. `visit` gets the relative
    /// path and, when `want_text`, the file text; returning `false` stops.
    fn walk<F>(
        &self,
        target: &Path,
        scope: &Scope,
        config: &ProjectToolConfig,
        want_text: bool,
        mut visit: F,
    ) -> Result<(), ToolError>
    where
        F: FnMut(String, Option<String>) -> bool,
    {
        let target_owned = target.to_path_buf();
        let mut builder = WalkBuilder::new(&self.root);
        builder
            .hidden(true)
            .parents(false)
            .ignore(false)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(false)
            .require_git(false)
            .follow_links(false)
            .max_filesize(Some(config.max_file_bytes))
            .sort_by_file_path(|a, b| a.cmp(b))
            .filter_entry(move |entry| {
                // Walk only the target and the directories leading to it.
                entry.path().starts_with(&target_owned) || target_owned.starts_with(entry.path())
            });
        if scope.shallow {
            let depth = target
                .strip_prefix(&self.root)
                .map(|p| p.components().count())
                .unwrap_or(0);
            builder.max_depth(Some(depth + 1));
        }
        for entry in builder.build() {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_some_and(|t| t.is_file()) || !entry.path().starts_with(target)
            {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&self.root) else {
                continue;
            };
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if check_components(&rel, &rel).is_err() || !scope.glob_matches(&rel) {
                continue;
            }
            let text = if want_text {
                match read_text(entry.path(), &rel, config.max_file_bytes) {
                    Ok(t) => Some(t),
                    Err(_) => continue,
                }
            } else {
                if !sniff_is_text(entry.path()) {
                    continue;
                }
                None
            };
            if !visit(rel, text) {
                break;
            }
        }
        Ok(())
    }
}

/// Whether the head of a file looks like UTF-8 text (no NUL, and valid UTF-8
/// apart from a character cut at the end of the sniffed window).
fn sniff_is_text(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; SNIFF_BYTES];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    buf.truncate(n);
    if buf.contains(&0) {
        return false;
    }
    match std::str::from_utf8(&buf) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && n == SNIFF_BYTES,
    }
}

fn read_text(path: &Path, shown: &str, max_bytes: u64) -> Result<String, ToolError> {
    let file = File::open(path).map_err(|e| ToolError::Io(format!("{shown}: {}", e.kind())))?;
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ToolError::Io(format!("{shown}: {}", e.kind())))?;
    if bytes.len() as u64 > max_bytes {
        return Err(ToolError::TooLarge {
            path: shown.to_string(),
            limit: max_bytes,
        });
    }
    if bytes[..bytes.len().min(SNIFF_BYTES)].contains(&0) {
        return Err(ToolError::Binary(shown.to_string()));
    }
    String::from_utf8(bytes).map_err(|_| ToolError::NotUtf8(shown.to_string()))
}

impl ProjectFiles for FsProject {
    fn list(&self, scope: &Scope, config: &ProjectToolConfig) -> Result<Listing, ToolError> {
        let target = self.scope_dir(scope)?;
        let mut paths = Vec::new();
        let mut truncated = false;
        self.walk(&target, scope, config, false, |rel, _| {
            if paths.len() >= config.max_list_files {
                truncated = true;
                return false;
            }
            paths.push(rel);
            true
        })?;
        Ok(Listing { paths, truncated })
    }

    fn search(
        &self,
        query: &SearchQuery,
        scope: &Scope,
        config: &ProjectToolConfig,
    ) -> Result<SearchScan, ToolError> {
        let target = self.scope_dir(scope)?;
        let needle = if query.case_sensitive {
            query.text.clone()
        } else {
            query.text.to_lowercase()
        };
        let mut hits = Vec::new();
        let mut truncated = false;
        let mut scanned = 0usize;
        self.walk(&target, scope, config, true, |rel, text| {
            if scanned >= config.max_scan_files {
                truncated = true;
                return false;
            }
            scanned += 1;
            let text = text.unwrap_or_default();
            for (i, line) in split_lines(&text).into_iter().enumerate() {
                let found = if query.case_sensitive {
                    line.contains(&needle)
                } else {
                    line.to_lowercase().contains(&needle)
                };
                if found {
                    if hits.len() >= config.max_search_matches {
                        truncated = true;
                        return false;
                    }
                    hits.push(SearchHit {
                        path: rel.clone(),
                        line: i + 1,
                        text: line.to_string(),
                    });
                }
            }
            true
        })?;
        Ok(SearchScan { hits, truncated })
    }

    fn read(&self, path: &str, config: &ProjectToolConfig) -> Result<FileContent, ToolError> {
        let (canonical, rel) = self.resolve(path)?;
        if !canonical.is_file() {
            return Err(ToolError::InvalidArgument(format!("{rel} is not a file")));
        }
        let text = read_text(&canonical, &rel, config.max_file_bytes)?;
        Ok(FileContent { path: rel, text })
    }
}
