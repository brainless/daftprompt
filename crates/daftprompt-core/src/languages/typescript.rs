//! TypeScript and JavaScript: `package.json`, `tsconfig.json`, and the
//! lockfile, which decides the package manager.

use std::collections::BTreeMap;

use daftprompt_types::{CommandKind, LanguageDetection, LanguageId, LanguageProject};
use serde_json::Value;

use super::scan::{self, Entry};
use super::{CommandCandidate, LanguageSupport, ProjectFiles};

pub(super) struct TypeScript;

/// Lockfile name and the package manager it identifies.
const LOCKFILES: &[(&str, &str)] = &[
    ("pnpm-lock.yaml", "pnpm"),
    ("yarn.lock", "yarn"),
    ("bun.lock", "bun"),
    ("bun.lockb", "bun"),
    ("package-lock.json", "npm"),
    ("npm-shrinkwrap.json", "npm"),
];

impl LanguageSupport for TypeScript {
    fn context_suggestions(&self, files: &dyn ProjectFiles) -> Vec<super::ContextSuggestion> {
        scan::context_suggestions(
            files,
            "{package.json,tsconfig*.json,pnpm-workspace.yaml,turbo.json,nx.json}",
            "{index.ts,index.tsx,main.ts,main.tsx,layout.tsx,page.tsx,App.tsx}",
            &[
                "src",
                "app",
                "pages",
                "src/app",
                "src/pages",
                "test",
                "tests",
            ],
        )
    }

    fn id(&self) -> LanguageId {
        LanguageId::TypeScript
    }

    fn detect(&self, files: &dyn ProjectFiles) -> Option<LanguageDetection> {
        let (manifests, truncated) = scan::find(files, "package.json");
        if manifests.is_empty() {
            return None;
        }
        let (extra, extra_truncated) = scan::find(
            files,
            "{tsconfig.json,pnpm-lock.yaml,yarn.lock,bun.lock,bun.lockb,package-lock.json,\
             npm-shrinkwrap.json}",
        );
        // Marker files by directory.
        let mut by_dir: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for path in extra {
            by_dir.entry(scan::dir_of(&path)).or_default().push(path);
        }

        let entries = manifests
            .into_iter()
            .map(|path| {
                let dir = scan::dir_of(&path);
                let parsed: Value = scan::read(files, &path)
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .unwrap_or(Value::Null);
                let name = parsed
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let patterns = workspace_patterns(&parsed);
                let here = by_dir.get(&dir).cloned().unwrap_or_default();

                let lock_manager = package_manager_of(&here);
                let field_manager = parsed
                    .get("packageManager")
                    .and_then(Value::as_str)
                    .and_then(|field| field.split('@').next())
                    .filter(|name| matches!(*name, "npm" | "pnpm" | "yarn" | "bun"))
                    .map(str::to_owned);
                // Lockfile first; then the `packageManager` field; then the
                // nearest ancestor's lockfile, which governs a monorepo.
                let package_manager = lock_manager
                    .or(field_manager)
                    .or_else(|| ancestor_manager(&by_dir, &dir));

                let has_tsconfig = here
                    .iter()
                    .any(|file| file.rsplit('/').next() == Some("tsconfig.json"));
                let typescript = has_tsconfig
                    || ["dependencies", "devDependencies"].iter().any(|section| {
                        parsed
                            .get(section)
                            .and_then(Value::as_object)
                            .is_some_and(|deps| deps.contains_key("typescript"))
                    });

                let mut project_files = vec![path];
                project_files.extend(here);
                project_files.sort();
                Entry {
                    members: scan::members(&patterns),
                    project: LanguageProject {
                        dir,
                        name,
                        package_manager,
                        toolchain: Some(
                            if typescript {
                                "typescript"
                            } else {
                                "javascript"
                            }
                            .to_owned(),
                        ),
                        workspace: !patterns.is_empty(),
                        packages: Vec::new(),
                        files: project_files,
                    },
                }
            })
            .collect();
        let (projects, truncated) = scan::cap(scan::fold(entries), truncated || extra_truncated);
        Some(LanguageDetection {
            language: LanguageId::TypeScript,
            projects,
            truncated,
        })
    }

    fn key_files(&self, detection: &LanguageDetection) -> Vec<String> {
        scan::key_files(detection)
    }

    fn command_candidates(
        &self,
        files: &dyn ProjectFiles,
        detection: &LanguageDetection,
    ) -> Vec<CommandCandidate> {
        let mut out = Vec::new();
        let (mut manifests, _) = scan::find(files, "package.json");
        // Shallow manifests first, so a root's commands lead.
        manifests.sort_by_key(|path| (path.matches('/').count(), path.clone()));
        for path in manifests {
            let dir = scan::dir_of(&path);
            let Some(owner) = scan::owner(detection, &dir) else {
                continue;
            };
            let manager = owner.package_manager.as_deref().unwrap_or("npm");
            let Some(text) = scan::read(files, &path) else {
                continue;
            };
            let Ok(Value::Object(manifest)) = serde_json::from_str::<Value>(&text) else {
                continue;
            };

            // Install once per project root; workspace members share it.
            if dir == owner.dir {
                let line = scan::json_key_line(&text, "name", "name")
                    .or_else(|| scan::line_where(&text, 1, |l| l.contains('{')));
                out.push(scan::candidate(
                    LanguageId::TypeScript,
                    &dir,
                    format!("{manager} install"),
                    CommandKind::Setup,
                    "install",
                    scan::evidence(&path, &text, line),
                ));
            }

            let Some(Value::Object(scripts)) = manifest.get("scripts") else {
                continue;
            };
            let mut found: Vec<CommandCandidate> = Vec::new();
            for (name, body) in scripts {
                if !body.is_string() || !scan::is_plain_word(name) {
                    continue;
                }
                let line = scan::json_key_line(&text, "scripts", name);
                found.push(scan::candidate(
                    LanguageId::TypeScript,
                    &dir,
                    script_command(manager, name),
                    script_kind(name),
                    name,
                    scan::evidence(&path, &text, line),
                ));
            }
            // Manifest order, not the map's.
            found.sort_by_key(|c| c.evidence.start_line);
            out.extend(found);
        }
        out.truncate(scan::MAX_CANDIDATES);
        out
    }
}

/// How `manager` runs the package script `name`.
fn script_command(manager: &str, name: &str) -> String {
    match manager {
        "pnpm" => format!("pnpm {name}"),
        "yarn" => format!("yarn {name}"),
        "bun" => format!("bun run {name}"),
        _ => format!("npm run {name}"),
    }
}

/// The kind a script name suggests, from its words: `dev`, `start`, `build`,
/// `test`, `lint`, `format`, type checking (a kind of lint), else other. A
/// `pre`/`post` hook is not the script itself and stays `other`.
fn script_kind(name: &str) -> CommandKind {
    let lower = name.to_ascii_lowercase();
    if lower.contains("typecheck") || lower.contains("type-check") || lower == "tsc" {
        return CommandKind::Lint;
    }
    let first = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .find(|word| !word.is_empty())
        .unwrap_or("");
    match first {
        "dev" | "serve" | "watch" => CommandKind::Dev,
        "start" => CommandKind::Run,
        "build" | "compile" => CommandKind::Build,
        "test" | "tests" | "e2e" | "jest" | "vitest" => CommandKind::Test,
        "lint" | "eslint" | "check" => CommandKind::Lint,
        "format" | "fmt" | "prettier" => CommandKind::Format,
        _ => CommandKind::Other,
    }
}

/// `workspaces` as an array, or as `{ "packages": [...] }` (yarn).
fn workspace_patterns(manifest: &Value) -> Vec<String> {
    let list = match manifest.get("workspaces") {
        Some(Value::Array(list)) => Some(list),
        Some(Value::Object(object)) => object.get("packages").and_then(Value::as_array),
        _ => None,
    };
    list.map(|list| {
        list.iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect()
    })
    .unwrap_or_default()
}

/// The package manager the lockfiles among `files` identify. When several
/// exist the order of [`LOCKFILES`] decides.
fn package_manager_of(files: &[String]) -> Option<String> {
    LOCKFILES.iter().find_map(|(lock, manager)| {
        files
            .iter()
            .any(|file| file.rsplit('/').next() == Some(*lock))
            .then(|| (*manager).to_owned())
    })
}

/// The package manager of the nearest ancestor directory that has a lockfile.
fn ancestor_manager(by_dir: &BTreeMap<String, Vec<String>>, dir: &str) -> Option<String> {
    let mut current = dir;
    while !current.is_empty() {
        current = current
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        if let Some(manager) = by_dir
            .get(current)
            .and_then(|files| package_manager_of(files))
        {
            return Some(manager);
        }
    }
    None
}
