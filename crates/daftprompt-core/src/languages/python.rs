//! Python: `pyproject.toml`, `uv.lock` or `poetry.lock`, `requirements*.txt`,
//! `setup.py`, `setup.cfg` and `manage.py`. The tool is uv, poetry or pip.

use std::collections::BTreeMap;

use daftprompt_types::{CommandKind, LanguageDetection, LanguageId, LanguageProject};

use super::scan::{self, Entry};
use super::{CommandCandidate, LanguageSupport, ProjectFiles};

pub(super) struct Python;

impl LanguageSupport for Python {
    fn id(&self) -> LanguageId {
        LanguageId::Python
    }

    fn detect(&self, files: &dyn ProjectFiles) -> Option<LanguageDetection> {
        let (markers, truncated) = scan::find(
            files,
            "{pyproject.toml,setup.py,setup.cfg,manage.py,uv.lock,poetry.lock,requirements*.txt}",
        );
        if markers.is_empty() {
            return None;
        }
        let mut by_dir: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for path in markers {
            by_dir.entry(scan::dir_of(&path)).or_default().push(path);
        }

        let entries = by_dir
            .into_iter()
            .map(|(dir, mut project_files)| {
                let has = |name: &str| {
                    project_files
                        .iter()
                        .any(|file| file.rsplit('/').next() == Some(name))
                };
                let pyproject = project_files
                    .iter()
                    .find(|file| file.rsplit('/').next() == Some("pyproject.toml"))
                    .and_then(|path| scan::read(files, path))
                    .and_then(|text| text.parse::<toml::Table>().ok());

                let tool = pyproject
                    .as_ref()
                    .and_then(|table| table.get("tool"))
                    .and_then(|tool| tool.as_table());
                let uv = tool
                    .and_then(|tool| tool.get("uv"))
                    .and_then(|uv| uv.as_table());
                let poetry_table = tool.and_then(|tool| tool.get("poetry"));
                let backend_is_poetry = pyproject
                    .as_ref()
                    .and_then(|table| table.get("build-system"))
                    .and_then(|build| build.get("build-backend"))
                    .and_then(|backend| backend.as_str())
                    .is_some_and(|backend| backend.starts_with("poetry"));

                let package_manager = if has("uv.lock") || uv.is_some() {
                    Some("uv")
                } else if has("poetry.lock") || poetry_table.is_some() || backend_is_poetry {
                    Some("poetry")
                } else if has("pyproject.toml")
                    || has("setup.py")
                    || has("setup.cfg")
                    || project_files.iter().any(|file| {
                        file.rsplit('/')
                            .next()
                            .is_some_and(|n| n.starts_with("requirements"))
                    })
                {
                    Some("pip")
                } else {
                    None
                };

                let name = pyproject
                    .as_ref()
                    .and_then(|table| {
                        table
                            .get("project")
                            .and_then(|project| project.get("name"))
                            .or_else(|| poetry_table.and_then(|poetry| poetry.get("name")))
                    })
                    .and_then(|name| name.as_str())
                    .map(str::to_owned);
                let toolchain = pyproject
                    .as_ref()
                    .and_then(|table| table.get("project"))
                    .and_then(|project| project.get("requires-python"))
                    .and_then(|version| version.as_str())
                    .map(str::to_owned);

                let patterns: Vec<String> = uv
                    .and_then(|uv| uv.get("workspace"))
                    .and_then(|workspace| workspace.get("members"))
                    .and_then(|members| members.as_array())
                    .map(|members| {
                        members
                            .iter()
                            .filter_map(|member| member.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();

                project_files.sort();
                Entry {
                    members: scan::members(&patterns),
                    project: LanguageProject {
                        dir,
                        name,
                        package_manager: package_manager.map(str::to_owned),
                        toolchain,
                        workspace: uv.is_some_and(|uv| uv.contains_key("workspace")),
                        packages: Vec::new(),
                        files: project_files,
                    },
                }
            })
            .collect();
        let (projects, truncated) = scan::cap(scan::fold(entries), truncated);
        Some(LanguageDetection {
            language: LanguageId::Python,
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
            project_candidates(files, project, &mut out);
        }
        out.truncate(scan::MAX_CANDIDATES);
        out
    }
}

fn file_named<'a>(project: &'a LanguageProject, name: &str) -> Option<&'a String> {
    project
        .files
        .iter()
        .find(|file| file.rsplit('/').next() == Some(name))
}

/// How the project's tool runs an installed program: a prefix for `uv`,
/// `poetry`, a virtual environment's `bin` for pip, nothing known otherwise.
fn runner(manager: Option<&str>) -> &'static str {
    match manager {
        Some("uv") => "uv run ",
        Some("poetry") => "poetry run ",
        Some("pip") => ".venv/bin/",
        _ => "",
    }
}

fn project_candidates(
    files: &dyn ProjectFiles,
    project: &LanguageProject,
    out: &mut Vec<CommandCandidate>,
) {
    let dir = project.dir.as_str();
    let manager = project.package_manager.as_deref();
    let lang = LanguageId::Python;
    let pyproject_path = file_named(project, "pyproject.toml");
    let pyproject_text = pyproject_path.and_then(|path| scan::read(files, path));
    let table = pyproject_text
        .as_deref()
        .and_then(|text| text.parse::<toml::Table>().ok());

    // Setup: each step is its own command.
    match manager {
        Some("uv") | Some("poetry") => {
            let command = if manager == Some("uv") {
                "uv sync"
            } else {
                "poetry install"
            };
            let marker = pyproject_path
                .cloned()
                .or_else(|| project.files.first().cloned());
            if let Some(path) = marker {
                let text = scan::read(files, &path).unwrap_or_default();
                let line = scan::toml_section_line(&text, "project")
                    .or_else(|| scan::toml_section_line(&text, "tool.poetry"));
                out.push(scan::candidate(
                    lang,
                    dir,
                    command,
                    CommandKind::Setup,
                    if manager == Some("uv") {
                        "sync"
                    } else {
                        "install"
                    },
                    scan::evidence(&path, &text, line),
                ));
            }
        }
        Some("pip") => {
            let requirements = file_named(project, "requirements.txt").or_else(|| {
                project.files.iter().find(|file| {
                    file.rsplit('/')
                        .next()
                        .is_some_and(|n| n.starts_with("requirements") && n.ends_with(".txt"))
                })
            });
            let (install, source) = match (
                requirements,
                pyproject_path,
                file_named(project, "setup.py"),
            ) {
                (Some(req), _, _) => {
                    let name = req.rsplit('/').next().unwrap_or(req);
                    (
                        format!(".venv/bin/pip install -r {name}"),
                        Some(req.clone()),
                    )
                }
                (None, Some(path), _) | (None, None, Some(path)) => {
                    (".venv/bin/pip install -e .".to_owned(), Some(path.clone()))
                }
                _ => (String::new(), None),
            };
            if let Some(path) = source {
                let text = scan::read(files, &path).unwrap_or_default();
                let line = if path.ends_with("pyproject.toml") {
                    scan::toml_section_line(&text, "project")
                } else {
                    None
                };
                let evidence = scan::evidence(&path, &text, line);
                out.push(scan::candidate(
                    lang,
                    dir,
                    "python3 -m venv .venv",
                    CommandKind::Setup,
                    "venv",
                    evidence.clone(),
                ));
                out.push(scan::candidate(
                    lang,
                    dir,
                    install,
                    CommandKind::Setup,
                    "pip install",
                    evidence,
                ));
            }
        }
        _ => {}
    }

    // Console scripts, run through the tool.
    if let (Some(path), Some(text), Some(table)) = (pyproject_path, &pyproject_text, &table) {
        let sections = [
            (
                "project.scripts",
                table.get("project").and_then(|p| p.get("scripts")),
            ),
            (
                "tool.poetry.scripts",
                table
                    .get("tool")
                    .and_then(|t| t.get("poetry"))
                    .and_then(|p| p.get("scripts")),
            ),
        ];
        for (section, scripts) in sections {
            let Some(scripts) = scripts.and_then(|s| s.as_table()) else {
                continue;
            };
            let mut found = Vec::new();
            for name in scripts.keys().filter(|name| scan::is_plain_word(name)) {
                let line = scan::toml_key_line(text, section, name)
                    .or_else(|| scan::toml_section_line(text, section));
                found.push(scan::candidate(
                    lang,
                    dir,
                    format!("{}{name}", runner(manager)),
                    CommandKind::Run,
                    name,
                    scan::evidence(path, text, line),
                ));
            }
            found.sort_by_key(|c| c.evidence.start_line);
            out.extend(found);
        }
    }

    // Django.
    if let Some(path) = file_named(project, "manage.py") {
        let text = scan::read(files, path).unwrap_or_default();
        let line = scan::line_where(&text, 1, |l| l.contains("DJANGO_SETTINGS_MODULE"));
        let evidence = scan::evidence(path, &text, line);
        let python = match manager {
            Some("pip") => ".venv/bin/python".to_owned(),
            Some(_) => format!("{}python", runner(manager)),
            None => "python3".to_owned(),
        };
        out.push(scan::candidate(
            lang,
            dir,
            format!("{python} manage.py runserver"),
            CommandKind::Dev,
            "runserver",
            evidence.clone(),
        ));
        out.push(scan::candidate(
            lang,
            dir,
            format!("{python} manage.py test"),
            CommandKind::Test,
            "django test",
            evidence,
        ));
    }

    // pytest, as a dependency or in configuration.
    let uses_pytest = |text: &str| {
        scan::line_where(text, 1, |line| {
            !line.trim_start().starts_with('#') && line.to_ascii_lowercase().contains("pytest")
        })
    };
    let mut pytest = pyproject_path
        .zip(pyproject_text.as_deref())
        .and_then(|(path, text)| uses_pytest(text).map(|l| scan::evidence(path, text, Some(l))));
    if pytest.is_none() {
        pytest = project
            .files
            .iter()
            .filter(|file| {
                file.rsplit('/')
                    .next()
                    .is_some_and(|n| n.starts_with("requirements") && n.ends_with(".txt"))
            })
            .find_map(|path| {
                let text = scan::read(files, path)?;
                let line = uses_pytest(&text)?;
                Some(scan::evidence(path, &text, Some(line)))
            });
    }
    if let Some(evidence) = pytest {
        out.push(scan::candidate(
            lang,
            dir,
            format!("{}pytest", runner(manager)),
            CommandKind::Test,
            "pytest",
            evidence,
        ));
    }
}
