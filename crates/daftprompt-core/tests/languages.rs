//! Language detection and key files over in-memory projects. No filesystem,
//! credentials or network.

use daftprompt_core::languages::{self, registry};
use daftprompt_core::project_tools::InMemoryProject;
use daftprompt_types::{LanguageDetection, LanguageId, LanguageProject, ProjectProfile};

fn language(profile: &ProjectProfile, id: LanguageId) -> Option<&LanguageDetection> {
    profile.languages.iter().find(|l| l.language == id)
}

fn only(detection: &LanguageDetection) -> &LanguageProject {
    assert_eq!(detection.projects.len(), 1, "{detection:?}");
    &detection.projects[0]
}

#[test]
fn an_empty_folder_has_no_languages_and_no_key_files() {
    let profile = languages::profile(&InMemoryProject::new());
    assert!(profile.languages.is_empty());
    assert!(profile.key_files.is_empty());
}

#[test]
fn a_rust_workspace_reports_members_names_and_the_toolchain() {
    let project = InMemoryProject::new()
        .with_file(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\n[workspace.package]\nversion = \"0.1.0\"\n",
        )
        .with_file(
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"nightly-2026-01-01\"\n",
        )
        .with_file(
            "crates/core/Cargo.toml",
            "[package]\nname = \"demo-core\"\n",
        )
        .with_file("crates/api/Cargo.toml", "[package]\nname = \"demo-api\"\n")
        .with_file("crates/api/src/main.rs", "fn main() {}\n")
        .with_file("README.md", "# demo\n");
    let profile = languages::profile(&project);

    assert_eq!(profile.languages.len(), 1);
    let rust = language(&profile, LanguageId::Rust).unwrap();
    let root = only(rust);
    assert_eq!(root.dir, "");
    assert!(root.workspace);
    assert_eq!(root.package_manager.as_deref(), Some("cargo"));
    assert_eq!(root.toolchain.as_deref(), Some("nightly-2026-01-01"));
    assert_eq!(root.packages, ["demo-api", "demo-core"]);
    assert_eq!(root.files, ["Cargo.toml", "rust-toolchain.toml"]);
    assert_eq!(
        profile.key_files,
        ["README.md", "Cargo.toml", "rust-toolchain.toml"]
    );
}

#[test]
fn a_single_rust_package_and_nested_non_members_are_separate_projects() {
    let project = InMemoryProject::new()
        .with_file("Cargo.toml", "[package]\nname = \"app\"\n")
        .with_file("tools/gen/Cargo.toml", "[package]\nname = \"gen\"\n");
    let profile = languages::profile(&project);
    let rust = language(&profile, LanguageId::Rust).unwrap();
    assert_eq!(rust.projects.len(), 2);
    assert_eq!(rust.projects[0].name.as_deref(), Some("app"));
    assert!(!rust.projects[0].workspace);
    assert_eq!(rust.projects[1].dir, "tools/gen");
}

#[test]
fn the_lockfile_decides_the_package_manager() {
    let cases = [
        ("package-lock.json", "npm"),
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
    ];
    for (lockfile, manager) in cases {
        let project = InMemoryProject::new()
            .with_file(
                "package.json",
                "{\"name\":\"web\",\"scripts\":{\"dev\":\"vite\"}}",
            )
            .with_file("tsconfig.json", "{}")
            .with_file(lockfile, "lock\n");
        let profile = languages::profile(&project);
        let ts = language(&profile, LanguageId::TypeScript).expect(lockfile);
        let found = only(ts);
        assert_eq!(
            found.package_manager.as_deref(),
            Some(manager),
            "{lockfile}"
        );
        assert_eq!(found.toolchain.as_deref(), Some("typescript"));
        assert_eq!(found.name.as_deref(), Some("web"));
        assert!(found.files.contains(&lockfile.to_owned()));
        // Lockfiles are evidence, not files worth reading.
        assert!(!profile.key_files.contains(&lockfile.to_owned()));
        assert!(profile.key_files.contains(&"package.json".to_owned()));
        assert!(profile.key_files.contains(&"tsconfig.json".to_owned()));
    }
}

#[test]
fn javascript_without_a_lockfile_uses_the_package_manager_field_or_none() {
    let project =
        InMemoryProject::new().with_file("package.json", "{\"packageManager\":\"pnpm@9.1.0\"}");
    let profile = languages::profile(&project);
    let found = only(language(&profile, LanguageId::TypeScript).unwrap()).clone();
    assert_eq!(found.package_manager.as_deref(), Some("pnpm"));
    assert_eq!(found.toolchain.as_deref(), Some("javascript"));

    let project = InMemoryProject::new().with_file("package.json", "{\"name\":\"x\"}");
    let profile = languages::profile(&project);
    assert_eq!(
        only(language(&profile, LanguageId::TypeScript).unwrap()).package_manager,
        None
    );
}

#[test]
fn a_typescript_dependency_marks_typescript_and_workspaces_fold_members() {
    let project = InMemoryProject::new()
        .with_file(
            "package.json",
            "{\"name\":\"root\",\"workspaces\":[\"packages/*\"],\"devDependencies\":{\"typescript\":\"5\"}}",
        )
        .with_file("pnpm-lock.yaml", "")
        .with_file("packages/ui/package.json", "{\"name\":\"@acme/ui\"}")
        .with_file("packages/app/package.json", "{\"name\":\"@acme/app\"}");
    let profile = languages::profile(&project);
    let root = only(language(&profile, LanguageId::TypeScript).unwrap()).clone();
    assert!(root.workspace);
    assert_eq!(root.toolchain.as_deref(), Some("typescript"));
    assert_eq!(root.packages, ["@acme/app", "@acme/ui"]);
}

#[test]
fn nested_packages_inherit_the_monorepo_lockfile() {
    let project = InMemoryProject::new()
        .with_file("yarn.lock", "")
        .with_file("apps/site/package.json", "{\"name\":\"site\"}");
    let profile = languages::profile(&project);
    let found = only(language(&profile, LanguageId::TypeScript).unwrap()).clone();
    assert_eq!(found.dir, "apps/site");
    assert_eq!(found.package_manager.as_deref(), Some("yarn"));
}

#[test]
fn python_tools_are_told_apart() {
    let uv = InMemoryProject::new()
        .with_file(
            "pyproject.toml",
            "[project]\nname = \"svc\"\nrequires-python = \">=3.12\"\n",
        )
        .with_file("uv.lock", "");
    let poetry = InMemoryProject::new()
        .with_file("pyproject.toml", "[tool.poetry]\nname = \"svc\"\n")
        .with_file("poetry.lock", "");
    let poetry_backend_only = InMemoryProject::new().with_file(
        "pyproject.toml",
        "[build-system]\nbuild-backend = \"poetry.core.masonry.api\"\n",
    );
    let plain = InMemoryProject::new()
        .with_file("requirements.txt", "flask\n")
        .with_file("requirements-dev.txt", "pytest\n")
        .with_file("manage.py", "");
    let setup = InMemoryProject::new().with_file("setup.py", "");

    let manager = |project: &InMemoryProject| {
        let profile = languages::profile(project);
        only(language(&profile, LanguageId::Python).unwrap())
            .package_manager
            .clone()
    };
    assert_eq!(manager(&uv).as_deref(), Some("uv"));
    assert_eq!(manager(&poetry).as_deref(), Some("poetry"));
    assert_eq!(manager(&poetry_backend_only).as_deref(), Some("poetry"));
    assert_eq!(manager(&plain).as_deref(), Some("pip"));
    assert_eq!(manager(&setup).as_deref(), Some("pip"));

    let profile = languages::profile(&uv);
    let found = only(language(&profile, LanguageId::Python).unwrap()).clone();
    assert_eq!(found.name.as_deref(), Some("svc"));
    assert_eq!(found.toolchain.as_deref(), Some(">=3.12"));
    assert_eq!(profile.key_files, ["pyproject.toml"]);

    let profile = languages::profile(&plain);
    let found = only(language(&profile, LanguageId::Python).unwrap()).clone();
    assert_eq!(
        found.files,
        ["manage.py", "requirements-dev.txt", "requirements.txt"]
    );
}

#[test]
fn a_mixed_monorepo_reports_each_language_in_its_directory() {
    let project = InMemoryProject::new()
        .with_file("Cargo.toml", "[package]\nname = \"server\"\n")
        .with_file("web/package.json", "{\"name\":\"web\"}")
        .with_file("web/tsconfig.json", "{}")
        .with_file("web/pnpm-lock.yaml", "")
        .with_file("scripts/requirements.txt", "requests\n")
        .with_file("README.md", "# mono\n")
        .with_file("docs/guide.md", "guide\n")
        .with_file("docs/logo.png", "not text")
        .with_file("node_modules/dep/package.json", "{\"name\":\"dep\"}")
        .with_file("target/debug/Cargo.toml", "[package]\nname = \"build\"\n");
    let profile = languages::profile(&project);

    let ids: Vec<LanguageId> = profile.languages.iter().map(|l| l.language).collect();
    assert_eq!(
        ids,
        [LanguageId::Rust, LanguageId::TypeScript, LanguageId::Python]
    );
    assert_eq!(only(language(&profile, LanguageId::Rust).unwrap()).dir, "");
    let web = only(language(&profile, LanguageId::TypeScript).unwrap());
    assert_eq!(web.dir, "web");
    assert_eq!(web.package_manager.as_deref(), Some("pnpm"));
    assert_eq!(
        only(language(&profile, LanguageId::Python).unwrap()).dir,
        "scripts"
    );
    // Vendored and build directories are never projects.
    assert!(
        !profile
            .key_files
            .iter()
            .any(|f| f.contains("node_modules") || f.starts_with("target"))
    );
    assert_eq!(
        profile.key_files,
        [
            "README.md",
            "docs/guide.md",
            "Cargo.toml",
            "web/package.json",
            "web/tsconfig.json",
            "scripts/requirements.txt"
        ]
    );
}

#[test]
fn malformed_manifests_still_detect_the_language_with_less_detail() {
    let project = InMemoryProject::new()
        .with_file("Cargo.toml", "[package\nname = = broken")
        .with_file("package.json", "{ not json")
        .with_file("pyproject.toml", "[[[ nope")
        .with_file("rust-toolchain.toml", "[toolchain\n");
    let profile = languages::profile(&project);
    assert_eq!(profile.languages.len(), 3);

    let rust = only(language(&profile, LanguageId::Rust).unwrap());
    assert_eq!(rust.name, None);
    assert!(!rust.workspace);
    assert_eq!(rust.package_manager.as_deref(), Some("cargo"));
    assert!(rust.files.contains(&"Cargo.toml".to_owned()));

    let ts = only(language(&profile, LanguageId::TypeScript).unwrap());
    assert_eq!(ts.name, None);
    assert_eq!(ts.toolchain.as_deref(), Some("javascript"));

    let py = only(language(&profile, LanguageId::Python).unwrap());
    assert_eq!(py.name, None);
    assert_eq!(py.package_manager.as_deref(), Some("pip"));
}

#[test]
fn key_files_cover_the_generic_documents_in_a_stable_order() {
    let project = InMemoryProject::new()
        .with_file("CLAUDE.md", "")
        .with_file("AGENTS.md", "")
        .with_file("ARCHITECTURE.md", "")
        .with_file("CONTRIBUTING.md", "")
        .with_file("DEVELOPMENT.md", "")
        .with_file("readme.rst", "")
        .with_file("src/README.md", "nested, not a root doc")
        .with_file("docs/a.md", "")
        .with_file("docs/sub/b.mdx", "")
        .with_file("main.c", "");
    let profile = languages::profile(&project);
    assert_eq!(
        profile.key_files,
        [
            "readme.rst",
            "CONTRIBUTING.md",
            "DEVELOPMENT.md",
            "ARCHITECTURE.md",
            "AGENTS.md",
            "CLAUDE.md",
            "docs/a.md",
            "docs/sub/b.mdx",
        ]
    );
}

#[test]
fn the_registry_lists_every_language_once() {
    let ids: Vec<LanguageId> = registry().iter().map(|l| l.id()).collect();
    assert_eq!(
        ids,
        [LanguageId::Rust, LanguageId::TypeScript, LanguageId::Python]
    );
}

#[test]
fn the_profile_serializes_with_snake_case_language_ids() {
    let project = InMemoryProject::new().with_file("web/package.json", "{}");
    let json = serde_json::to_value(languages::profile(&project)).unwrap();
    assert_eq!(json["languages"][0]["language"], "typescript");
}

#[test]
fn nested_docs_come_after_manifests_and_never_crowd_out_direct_docs() {
    let mut project = InMemoryProject::new()
        .with_file("README.md", "")
        .with_file("docs/PRD.md", "")
        .with_file("docs/data-model.md", "")
        .with_file("Cargo.toml", "[package]\nname = \"p\"\n");
    for i in 0..30 {
        project = project.with_file(&format!("docs/evaluations/e{i:02}.md"), "");
    }
    let profile = languages::profile(&project);
    assert_eq!(
        &profile.key_files[..4],
        [
            "README.md",
            "docs/PRD.md",
            "docs/data-model.md",
            "Cargo.toml"
        ]
    );
    // 20 files from docs/ in all: 2 direct and 18 nested.
    let nested: Vec<&String> = profile.key_files[4..].iter().collect();
    assert_eq!(nested.len(), 18);
    assert!(nested.iter().all(|f| f.starts_with("docs/evaluations/")));
}

#[test]
fn nested_docs_yield_to_the_key_file_cap() {
    let mut project = InMemoryProject::new().with_file("README.md", "");
    for i in 0..30 {
        project = project.with_file(&format!("docs/d{i:02}.md"), "");
    }
    for i in 0..30 {
        project = project.with_file(&format!("svc{i:02}/Cargo.toml"), "[package]\nname=\"x\"\n");
    }
    for i in 0..10 {
        project = project.with_file(&format!("docs/sub/n{i}.md"), "");
    }
    let profile = languages::profile(&project);
    assert_eq!(profile.key_files.len(), 40);
    assert!(!profile.key_files.iter().any(|f| f.starts_with("docs/sub/")));
    assert!(profile.key_files.iter().any(|f| f == "svc00/Cargo.toml"));
}
