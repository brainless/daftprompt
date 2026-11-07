//! Rust: `Cargo.toml` (with workspace members and package names) and
//! `rust-toolchain.toml`.

use std::collections::BTreeMap;

use daftprompt_types::{CommandKind, LanguageDetection, LanguageId, LanguageProject};

use super::scan::{self, Entry};
use super::{CommandCandidate, LanguageSupport, ProjectFiles};

pub(super) struct Rust;

impl LanguageSupport for Rust {
    fn id(&self) -> LanguageId {
        LanguageId::Rust
    }

    fn detect(&self, files: &dyn ProjectFiles) -> Option<LanguageDetection> {
        let (manifests, truncated) = scan::find(files, "Cargo.toml");
        if manifests.is_empty() {
            return None;
        }
        let (toolchain_files, _) = scan::find(files, "{rust-toolchain.toml,rust-toolchain}");
        // The channel of each toolchain file, by directory. It applies to the
        // directory and everything below it.
        let toolchains: BTreeMap<String, (String, Option<String>)> = toolchain_files
            .into_iter()
            .map(|path| {
                let channel = scan::read(files, &path).and_then(|text| channel(&text));
                (scan::dir_of(&path), (path, channel))
            })
            .collect();

        let entries = manifests
            .into_iter()
            .map(|path| {
                let dir = scan::dir_of(&path);
                let parsed = scan::read(files, &path)
                    .and_then(|text| text.parse::<toml::Table>().ok())
                    .unwrap_or_default();
                let name = parsed
                    .get("package")
                    .and_then(|package| package.get("name"))
                    .and_then(|name| name.as_str())
                    .map(str::to_owned);
                let workspace = parsed.get("workspace").and_then(|w| w.as_table());
                let patterns: Vec<String> = workspace
                    .and_then(|w| w.get("members"))
                    .and_then(|m| m.as_array())
                    .map(|members| {
                        members
                            .iter()
                            .filter_map(|m| m.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();

                let mut project_files = vec![path];
                let mut toolchain = None;
                // The nearest toolchain file at or above this directory.
                let mut candidate = Some(dir.clone());
                while let Some(current) = candidate {
                    if let Some((file, channel)) = toolchains.get(&current) {
                        project_files.push(file.clone());
                        toolchain = channel.clone();
                        break;
                    }
                    candidate = current
                        .rsplit_once('/')
                        .map(|(parent, _)| parent.to_owned());
                    if candidate.is_none() && !current.is_empty() {
                        candidate = Some(String::new());
                    }
                }
                project_files.sort();
                project_files.dedup();
                Entry {
                    members: scan::members(&patterns),
                    project: LanguageProject {
                        dir,
                        name,
                        package_manager: Some("cargo".to_owned()),
                        toolchain,
                        workspace: workspace.is_some(),
                        packages: Vec::new(),
                        files: project_files,
                    },
                }
            })
            .collect();
        let (projects, truncated) = scan::cap(scan::fold(entries), truncated);
        Some(LanguageDetection {
            language: LanguageId::Rust,
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
        for project in &detection.projects {
            let path = scan::join(&project.dir, "Cargo.toml");
            let Some(text) = scan::read(files, &path) else {
                continue;
            };
            let header = if project.workspace {
                "workspace"
            } else {
                "package"
            };
            let evidence = scan::evidence(&path, &text, scan::toml_section_line(&text, header));
            let dir = project.dir.as_str();
            let (build, test, clippy, fmt) = if project.workspace {
                (
                    "cargo build --workspace",
                    "cargo test --workspace",
                    "cargo clippy --workspace --all-targets",
                    "cargo fmt --all",
                )
            } else {
                (
                    "cargo build",
                    "cargo test",
                    "cargo clippy --all-targets",
                    "cargo fmt --all",
                )
            };
            for (command, kind, label) in [
                (build, CommandKind::Build, "build"),
                (test, CommandKind::Test, "test"),
                (clippy, CommandKind::Lint, "clippy"),
                (fmt, CommandKind::Format, "fmt"),
            ] {
                out.push(scan::candidate(
                    LanguageId::Rust,
                    dir,
                    command,
                    kind,
                    label,
                    evidence.clone(),
                ));
            }
        }

        // Binaries: every manifest, including workspace members, run from the
        // directory of the project that owns it.
        let (manifests, _) = scan::find(files, "Cargo.toml");
        for path in manifests {
            let Some(owner) = scan::owner(detection, &scan::dir_of(&path)) else {
                continue;
            };
            let Some(text) = scan::read(files, &path) else {
                continue;
            };
            let Some(table) = text.parse::<toml::Table>().ok() else {
                continue;
            };
            let Some(package) = table
                .get("package")
                .and_then(|package| package.get("name"))
                .and_then(|name| name.as_str())
                .filter(|name| scan::is_plain_word(name))
            else {
                continue;
            };
            let dir = scan::dir_of(&path);
            let mut bins: Vec<(String, Option<u32>)> = Vec::new();
            if scan::read(files, &scan::join(&dir, "src/main.rs")).is_some() {
                let line = scan::toml_key_line(&text, "package", "name");
                bins.push((package.to_owned(), line));
            }
            let first_bin = scan::line_where(&text, 1, |line| line.trim() == "[[bin]]");
            for bin in table
                .get("bin")
                .and_then(|bin| bin.as_array())
                .into_iter()
                .flatten()
            {
                let Some(name) = bin
                    .get("name")
                    .and_then(|name| name.as_str())
                    .filter(|name| scan::is_plain_word(name))
                else {
                    continue;
                };
                if bins.iter().any(|(existing, _)| existing == name) {
                    continue;
                }
                let needle = format!("\"{name}\"");
                let line = first_bin
                    .and_then(|from| scan::line_where(&text, from, |l| l.contains(&needle)));
                bins.push((name.to_owned(), line.or(first_bin)));
            }
            for (bin, line) in bins {
                let command = if bin == package {
                    format!("cargo run -p {package}")
                } else {
                    format!("cargo run -p {package} --bin {bin}")
                };
                out.push(scan::candidate(
                    LanguageId::Rust,
                    &owner.dir,
                    command,
                    CommandKind::Run,
                    &format!("run {bin}"),
                    scan::evidence(&path, &text, line),
                ));
            }
        }
        out.truncate(scan::MAX_CANDIDATES);
        out
    }
}

/// `toolchain.channel` of a `rust-toolchain.toml`, or the bare channel name
/// of a legacy one-line `rust-toolchain`.
fn channel(text: &str) -> Option<String> {
    match text.parse::<toml::Table>() {
        Ok(table) => table
            .get("toolchain")?
            .get("channel")?
            .as_str()
            .map(str::to_owned),
        Err(_) => {
            let line = text.trim();
            (!line.is_empty() && !line.contains(['=', '[', '\n'])).then(|| line.to_owned())
        }
    }
}
