use std::collections::BTreeMap;

use super::{
    FileContent, Listing, ProjectFiles, ProjectToolConfig, Scope, SearchHit, SearchQuery,
    SearchScan, ToolError, check_relative_path, split_lines,
};

/// A filesystem-free [`ProjectFiles`] for tests. It applies the same lexical
/// path rules as the real backend (no absolute paths, `..`, hidden or `.env`
/// components) but has no symlinks, ignore files, or binary files.
#[derive(Debug, Clone, Default)]
pub struct InMemoryProject {
    files: BTreeMap<String, String>,
}

impl InMemoryProject {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file; `path` is root-relative with `/` separators.
    pub fn with_file(mut self, path: &str, text: &str) -> Self {
        self.files
            .insert(super::normalize_path_text(path), text.to_string());
        self
    }

    fn in_scope<'a>(
        &'a self,
        scope: &Scope,
    ) -> Result<impl Iterator<Item = (&'a String, &'a String)>, ToolError> {
        let dir = match &scope.directory {
            Some(d) => check_relative_path(d)?,
            None => String::new(),
        };
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        if !dir.is_empty() && !self.files.keys().any(|p| p.starts_with(&prefix)) {
            return Err(ToolError::NotFound(dir));
        }
        let scope = scope.clone();
        Ok(self.files.iter().filter(move |(path, _)| {
            path.starts_with(&prefix)
                && scope.glob_matches(path)
                && check_relative_path(path).is_ok()
        }))
    }
}

impl ProjectFiles for InMemoryProject {
    fn list(&self, scope: &Scope, config: &ProjectToolConfig) -> Result<Listing, ToolError> {
        let mut paths: Vec<String> = self.in_scope(scope)?.map(|(p, _)| p.clone()).collect();
        let truncated = paths.len() > config.max_list_files;
        paths.truncate(config.max_list_files);
        Ok(Listing { paths, truncated })
    }

    fn search(
        &self,
        query: &SearchQuery,
        scope: &Scope,
        config: &ProjectToolConfig,
    ) -> Result<SearchScan, ToolError> {
        let needle = if query.case_sensitive {
            query.text.clone()
        } else {
            query.text.to_lowercase()
        };
        let mut hits = Vec::new();
        for (scanned, (path, text)) in self.in_scope(scope)?.enumerate() {
            if scanned >= config.max_scan_files {
                return Ok(SearchScan {
                    hits,
                    truncated: true,
                });
            }
            for (i, line) in split_lines(text).into_iter().enumerate() {
                let found = if query.case_sensitive {
                    line.contains(&needle)
                } else {
                    line.to_lowercase().contains(&needle)
                };
                if found {
                    if hits.len() >= config.max_search_matches {
                        return Ok(SearchScan {
                            hits,
                            truncated: true,
                        });
                    }
                    hits.push(SearchHit {
                        path: path.clone(),
                        line: i + 1,
                        text: line.to_string(),
                    });
                }
            }
        }
        Ok(SearchScan {
            hits,
            truncated: false,
        })
    }

    fn read(&self, path: &str, _config: &ProjectToolConfig) -> Result<FileContent, ToolError> {
        let normalized = check_relative_path(path)?;
        match self.files.get(&normalized) {
            Some(text) => Ok(FileContent {
                path: normalized,
                text: text.clone(),
            }),
            None => Err(ToolError::NotFound(normalized)),
        }
    }
}
