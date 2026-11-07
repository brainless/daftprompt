//! Deterministic command candidates over in-memory projects. No filesystem,
//! credentials or network.

use daftprompt_core::languages::{self, CommandCandidate};
use daftprompt_core::project_tools::InMemoryProject;
use daftprompt_types::{CommandKind, LanguageId};

fn candidates(project: &InMemoryProject) -> Vec<CommandCandidate> {
    let profile = languages::profile(project);
    languages::command_candidates(project, &profile)
}

/// `(working_dir, command)` pairs in order.
fn pairs(list: &[CommandCandidate]) -> Vec<(&str, &str)> {
    list.iter()
        .map(|c| (c.working_dir.as_str(), c.command.as_str()))
        .collect()
}

fn find<'a>(list: &'a [CommandCandidate], command: &str) -> &'a CommandCandidate {
    list.iter()
        .find(|c| c.command == command)
        .unwrap_or_else(|| panic!("no candidate {command:?} in {list:#?}"))
}

/// The text of the cited lines.
fn cited(project: &InMemoryProject, c: &CommandCandidate) -> String {
    use daftprompt_core::project_tools::{ProjectFiles, ProjectToolConfig};
    let text = project
        .read(&c.evidence.path, &ProjectToolConfig::default())
        .unwrap()
        .text;
    text.lines()
        .skip(c.evidence.start_line as usize - 1)
        .take((c.evidence.end_line - c.evidence.start_line + 1) as usize)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_rust_workspace_gets_workspace_commands_and_a_run_per_binary_package() {
    let project = InMemoryProject::new()
        .with_file(
            "Cargo.toml",
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\"]\n",
        )
        .with_file(
            "crates/app/Cargo.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
        )
        .with_file("crates/app/src/main.rs", "fn main() {}\n")
        .with_file("crates/lib/Cargo.toml", "[package]\nname = \"lib\"\n")
        .with_file("crates/lib/src/lib.rs", "")
        .with_file(
            "crates/tools/Cargo.toml",
            "[package]\nname = \"tools\"\n\n[[bin]]\nname = \"gen\"\npath = \"src/gen.rs\"\n\n[[bin]]\nname = \"check\"\npath = \"src/check.rs\"\n",
        );
    let list = candidates(&project);
    assert_eq!(
        pairs(&list),
        [
            ("", "cargo build --workspace"),
            ("", "cargo test --workspace"),
            ("", "cargo clippy --workspace --all-targets"),
            ("", "cargo fmt --all"),
            ("", "cargo run -p app"),
            ("", "cargo run -p tools --bin gen"),
            ("", "cargo run -p tools --bin check"),
        ]
    );
    assert!(list.iter().all(|c| c.language == LanguageId::Rust));
    assert_eq!(
        find(&list, "cargo test --workspace").kind,
        CommandKind::Test
    );
    assert_eq!(
        find(&list, "cargo clippy --workspace --all-targets").kind,
        CommandKind::Lint
    );
    assert_eq!(find(&list, "cargo fmt --all").kind, CommandKind::Format);
    assert_eq!(find(&list, "cargo run -p app").kind, CommandKind::Run);

    // Evidence: the workspace table, the member's package name, the [[bin]].
    let build = find(&list, "cargo build --workspace");
    assert_eq!(build.evidence.path, "Cargo.toml");
    assert_eq!((build.evidence.start_line, build.evidence.end_line), (1, 1));
    assert_eq!(cited(&project, build), "[workspace]");
    let run = find(&list, "cargo run -p app");
    assert_eq!(run.evidence.path, "crates/app/Cargo.toml");
    assert_eq!(run.evidence.start_line, 2);
    assert_eq!(cited(&project, run), "name = \"app\"");
    let check = find(&list, "cargo run -p tools --bin check");
    assert_eq!(check.evidence.start_line, 9);
    assert_eq!(cited(&project, check), "name = \"check\"");
    assert!(list.iter().all(|c| c.label.chars().count() <= 24));
}

#[test]
fn a_single_rust_package_uses_plain_commands() {
    let project = InMemoryProject::new()
        .with_file("Cargo.toml", "# demo\n[package]\nname = \"demo\"\n")
        .with_file("src/main.rs", "fn main() {}\n");
    let list = candidates(&project);
    assert_eq!(
        pairs(&list),
        [
            ("", "cargo build"),
            ("", "cargo test"),
            ("", "cargo clippy --all-targets"),
            ("", "cargo fmt --all"),
            ("", "cargo run -p demo"),
        ]
    );
    assert_eq!(find(&list, "cargo build").evidence.start_line, 2);
}

#[test]
fn javascript_scripts_use_the_detected_package_manager() {
    for (lock, install, script, run_line) in [
        ("package-lock.json", "npm install", "npm run dev", 4),
        ("pnpm-lock.yaml", "pnpm install", "pnpm dev", 4),
        ("yarn.lock", "yarn install", "yarn dev", 4),
        ("bun.lock", "bun install", "bun run dev", 4),
        ("", "npm install", "npm run dev", 4),
    ] {
        let mut project = InMemoryProject::new().with_file(
            "package.json",
            "{\n  \"name\": \"web\",\n  \"scripts\": {\n    \"dev\": \"vite\",\n    \"build\": \"vite build\"\n  }\n}\n",
        );
        if !lock.is_empty() {
            project = project.with_file(lock, "");
        }
        let list = candidates(&project);
        assert_eq!(list[0].command, install, "{lock}");
        assert_eq!(list[0].kind, CommandKind::Setup);
        assert_eq!(list[0].evidence.start_line, 2);
        let dev = find(&list, script);
        assert_eq!(dev.kind, CommandKind::Dev);
        assert_eq!(dev.label, "dev");
        assert_eq!(dev.evidence.start_line, run_line, "{lock}");
        assert_eq!(cited(&project, dev), "    \"dev\": \"vite\",");
        assert_eq!(list.len(), 3, "{list:#?}");
    }
}

#[test]
fn script_names_map_to_kinds() {
    let scripts = [
        ("dev", CommandKind::Dev),
        ("dev:server", CommandKind::Dev),
        ("start", CommandKind::Run),
        ("build", CommandKind::Build),
        ("build:watch", CommandKind::Build),
        ("test", CommandKind::Test),
        ("test:unit", CommandKind::Test),
        ("lint", CommandKind::Lint),
        ("typecheck", CommandKind::Lint),
        ("type-check", CommandKind::Lint),
        ("format", CommandKind::Format),
        ("prebuild", CommandKind::Other),
        ("deploy", CommandKind::Other),
    ];
    let body: Vec<String> = scripts
        .iter()
        .map(|(name, _)| format!("\"{name}\": \"x\""))
        .collect();
    let project = InMemoryProject::new()
        .with_file(
            "package.json",
            &format!("{{\"scripts\": {{{}}}}}", body.join(", ")),
        )
        .with_file("pnpm-lock.yaml", "");
    let list = candidates(&project);
    for (name, kind) in scripts {
        assert_eq!(find(&list, &format!("pnpm {name}")).kind, kind, "{name}");
    }
    // One line of JSON: every script cites that line.
    assert!(list.iter().skip(1).all(|c| c.evidence.start_line == 1));
}

#[test]
fn scripts_with_unsafe_names_or_non_string_bodies_are_skipped() {
    let project = InMemoryProject::new().with_file(
        "package.json",
        "{\"scripts\": {\"ok\": \"x\", \"a b\": \"x\", \"$(rm)\": \"x\", \"-x\": \"x\", \"n\": 3}}",
    );
    let list = candidates(&project);
    assert_eq!(pairs(&list), [("", "npm install"), ("", "npm run ok")]);
}

#[test]
fn python_uv_and_poetry_projects_set_up_with_their_tool() {
    let uv = InMemoryProject::new()
        .with_file(
            "pyproject.toml",
            "[project]\nname = \"svc\"\n\n[project.scripts]\nserve = \"svc:main\"\nmigrate = \"svc:migrate\"\n",
        )
        .with_file("uv.lock", "");
    let list = candidates(&uv);
    assert_eq!(
        pairs(&list),
        [
            ("", "uv sync"),
            ("", "uv run serve"),
            ("", "uv run migrate"),
        ]
    );
    assert_eq!(list[0].kind, CommandKind::Setup);
    assert_eq!(list[0].evidence.start_line, 1);
    assert_eq!(cited(&uv, &list[1]), "serve = \"svc:main\"");
    assert_eq!(list[1].evidence.start_line, 5);
    assert_eq!(list[1].kind, CommandKind::Run);

    let poetry = InMemoryProject::new()
        .with_file(
            "pyproject.toml",
            "[tool.poetry]\nname = \"svc\"\n\n[tool.poetry.scripts]\nserve = \"svc:main\"\n\n[tool.poetry.group.dev.dependencies]\npytest = \"^8\"\n",
        )
        .with_file("poetry.lock", "");
    let list = candidates(&poetry);
    assert_eq!(
        pairs(&list),
        [
            ("", "poetry install"),
            ("", "poetry run serve"),
            ("", "poetry run pytest"),
        ]
    );
    assert_eq!(find(&list, "poetry run pytest").evidence.start_line, 8);
    assert_eq!(find(&list, "poetry run pytest").kind, CommandKind::Test);
}

#[test]
fn python_pip_creates_a_venv_and_installs_as_separate_commands() {
    let req = InMemoryProject::new().with_file("requirements.txt", "# deps\nrequests\npytest>=8\n");
    let list = candidates(&req);
    assert_eq!(
        pairs(&list),
        [
            ("", "python3 -m venv .venv"),
            ("", ".venv/bin/pip install -r requirements.txt"),
            ("", ".venv/bin/pytest"),
        ]
    );
    assert!(list.iter().all(|c| !c.command.contains("&&")));
    assert_eq!(list[1].evidence.path, "requirements.txt");
    assert_eq!(
        (list[1].evidence.start_line, list[1].evidence.end_line),
        (1, 3)
    );
    assert_eq!(list[2].evidence.start_line, 3);

    let editable = InMemoryProject::new().with_file(
        "pyproject.toml",
        "[build-system]\nrequires = [\"setuptools\"]\n\n[project]\nname = \"lib\"\n",
    );
    let list = candidates(&editable);
    assert_eq!(
        pairs(&list),
        [
            ("", "python3 -m venv .venv"),
            ("", ".venv/bin/pip install -e ."),
        ]
    );
    assert_eq!(list[0].evidence.start_line, 4);
}

#[test]
fn django_projects_get_runserver_and_test() {
    let project = InMemoryProject::new()
        .with_file("requirements.txt", "django\n")
        .with_file(
            "manage.py",
            "import os\n\ndef main():\n    os.environ.setdefault('DJANGO_SETTINGS_MODULE', 'x.settings')\n",
        );
    let list = candidates(&project);
    let run = find(&list, ".venv/bin/python manage.py runserver");
    assert_eq!(run.kind, CommandKind::Dev);
    assert_eq!(run.evidence.path, "manage.py");
    assert_eq!(run.evidence.start_line, 4);
    assert_eq!(
        find(&list, ".venv/bin/python manage.py test").kind,
        CommandKind::Test
    );

    let uv = InMemoryProject::new()
        .with_file("manage.py", "print()\n")
        .with_file("pyproject.toml", "[tool.uv]\n")
        .with_file("uv.lock", "");
    assert!(
        candidates(&uv)
            .iter()
            .any(|c| c.command == "uv run python manage.py runserver")
    );
    let bare = InMemoryProject::new().with_file("manage.py", "print()\n");
    assert!(
        candidates(&bare)
            .iter()
            .any(|c| c.command == "python3 manage.py runserver")
    );
}

#[test]
fn a_monorepo_reports_working_directories() {
    let project = InMemoryProject::new()
        .with_file("Cargo.toml", "[package]\nname = \"server\"\n")
        .with_file("src/main.rs", "")
        .with_file("web/package.json", "{\"scripts\":{\"dev\":\"vite\"}}")
        .with_file("web/pnpm-lock.yaml", "")
        .with_file("tools/requirements.txt", "requests\n");
    let list = candidates(&project);
    let dirs = |language| -> Vec<&str> {
        list.iter()
            .filter(|c| c.language == language)
            .map(|c| c.working_dir.as_str())
            .collect()
    };
    assert!(dirs(LanguageId::Rust).iter().all(|d| d.is_empty()));
    assert_eq!(dirs(LanguageId::TypeScript), ["web", "web"]);
    assert_eq!(dirs(LanguageId::Python), ["tools", "tools"]);
    assert_eq!(find(&list, "pnpm dev").evidence.path, "web/package.json");
}

#[test]
fn workspace_members_scripts_run_in_the_members_directory_with_the_roots_manager() {
    let project = InMemoryProject::new()
        .with_file(
            "package.json",
            "{\"workspaces\": [\"apps/*\"], \"scripts\": {\"build\": \"turbo build\"}}",
        )
        .with_file("pnpm-lock.yaml", "")
        .with_file(
            "apps/web/package.json",
            "{\"scripts\": {\"dev\": \"next dev\"}}",
        );
    let list = candidates(&project);
    assert_eq!(
        pairs(&list),
        [
            ("", "pnpm install"),
            ("", "pnpm build"),
            ("apps/web", "pnpm dev"),
        ]
    );
}

#[test]
fn malformed_manifests_do_not_panic_and_yield_few_candidates() {
    let project = InMemoryProject::new()
        .with_file("Cargo.toml", "[package\nname = = broken")
        .with_file("src/main.rs", "")
        .with_file("package.json", "{ not json")
        .with_file("sub/package.json", "[1,2]")
        .with_file("pyproject.toml", "[[[ nope")
        .with_file("manage.py", "");
    let list = candidates(&project);
    // The Cargo project still gets its commands; no run (no package name).
    assert!(list.iter().any(|c| c.command == "cargo build"));
    assert!(!list.iter().any(|c| c.command.starts_with("cargo run")));
    assert!(!list.iter().any(|c| c.command.starts_with("npm run")));
    for c in &list {
        assert!(c.evidence.start_line >= 1 && c.evidence.end_line >= c.evidence.start_line);
    }
    // An empty folder has none.
    assert!(candidates(&InMemoryProject::new()).is_empty());
}

#[test]
fn evidence_always_lies_within_the_manifest() {
    let project = InMemoryProject::new()
        .with_file("Cargo.toml", "[workspace]\nmembers = []\n")
        .with_file("package.json", "{\"scripts\":{}}")
        .with_file("pyproject.toml", "[project]\nname = \"x\"\n");
    for c in candidates(&project) {
        use daftprompt_core::project_tools::{ProjectFiles, ProjectToolConfig};
        let text = project
            .read(&c.evidence.path, &ProjectToolConfig::default())
            .unwrap()
            .text;
        assert!(
            c.evidence.end_line as usize <= text.lines().count().max(1),
            "{c:?}"
        );
    }
}
