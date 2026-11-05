use std::fs;
use std::path::Path;
use std::sync::Arc;

use daftprompt_core::project_tools::{
    InMemoryProject, LIST_FILES, ProjectToolConfig, ProjectToolbox, READ_FILE, SEARCH, rig_tools,
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn write(root: &Path, rel: &str, bytes: impl AsRef<[u8]>) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

fn project() -> (TempDir, ProjectToolbox) {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "src/main.rs",
        "fn main() {\n    println!(\"hi\");\n}\n",
    );
    write(
        dir.path(),
        "src/lib.rs",
        "pub fn Token() {}\n// token bucket\n",
    );
    write(dir.path(), "README.md", "# Title\nsee token\n");
    let tb = ProjectToolbox::for_root(dir.path(), ProjectToolConfig::default()).unwrap();
    (dir, tb)
}

fn run(tb: &ProjectToolbox, name: &str, args: Value) -> String {
    tb.call(name, args).text
}

fn is_denied_or_missing(text: &str) -> bool {
    text.starts_with("error: access denied") || text.starts_with("error: not found")
}

#[test]
fn lists_reads_and_searches() {
    let (_d, tb) = project();
    let list = run(&tb, LIST_FILES, json!({}));
    assert_eq!(list, "README.md\nsrc/lib.rs\nsrc/main.rs\n");
    let list = run(&tb, LIST_FILES, json!({"directory": "src", "glob": "*.rs"}));
    assert_eq!(list, "src/lib.rs\nsrc/main.rs\n");
    let hits = run(&tb, SEARCH, json!({"query": "token"}));
    assert_eq!(
        hits,
        "README.md:2: see token\nsrc/lib.rs:1: pub fn Token() {}\nsrc/lib.rs:2: // token bucket\n"
    );
    let hits = run(
        &tb,
        SEARCH,
        json!({"query": "token", "case_sensitive": true}),
    );
    assert_eq!(
        hits,
        "README.md:2: see token\nsrc/lib.rs:2: // token bucket\n"
    );
    let read = run(
        &tb,
        READ_FILE,
        json!({"path": "./src//main.rs", "start_line": 2, "end_line": 3}),
    );
    assert_eq!(
        read,
        "src/main.rs (lines 2-3 of 3)\n2:     println!(\"hi\");\n3: }\n"
    );
    assert_eq!(run(&tb, SEARCH, json!({"query": "zzz"})), "(no matches)\n");
}

#[test]
fn rejects_traversal_and_absolute_paths() {
    let (d, tb) = project();
    let outside = TempDir::new().unwrap();
    write(outside.path(), "secret.txt", "secret");
    for path in [
        "../x".to_string(),
        "src/../../x".to_string(),
        "src/../README.md/../../x".to_string(),
        "/etc/passwd".to_string(),
        outside.path().join("secret.txt").display().to_string(),
        d.path().join("README.md").display().to_string(),
    ] {
        let out = run(&tb, READ_FILE, json!({"path": path}));
        assert!(out.starts_with("error: access denied"), "{path}: {out}");
        assert!(!out.contains(&d.path().display().to_string()) || path.starts_with('/'));
    }
    assert!(run(&tb, LIST_FILES, json!({"directory": ".."})).starts_with("error: access denied"));
    assert!(
        run(&tb, SEARCH, json!({"query": "a", "directory": "/"}))
            .starts_with("error: access denied")
    );
    assert!(run(&tb, READ_FILE, json!({"path": "nope.txt"})).starts_with("error: not found"));
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_denied_and_inside_links_are_not_walked() {
    use std::os::unix::fs::symlink;
    let (d, tb) = project();
    let outside = TempDir::new().unwrap();
    write(outside.path(), "secret.txt", "needle outside");
    symlink(outside.path().join("secret.txt"), d.path().join("link.txt")).unwrap();
    symlink(outside.path(), d.path().join("linkdir")).unwrap();
    symlink(d.path().join("README.md"), d.path().join("inner.md")).unwrap();

    let out = run(&tb, READ_FILE, json!({"path": "link.txt"}));
    assert!(out.contains("resolves outside"), "{out}");
    assert!(
        run(&tb, READ_FILE, json!({"path": "linkdir/secret.txt"}))
            .starts_with("error: access denied")
    );
    assert!(
        run(&tb, LIST_FILES, json!({"directory": "linkdir"})).starts_with("error: access denied")
    );
    // Walks never follow or list symlinks.
    assert_eq!(
        run(&tb, LIST_FILES, json!({})),
        "README.md\nsrc/lib.rs\nsrc/main.rs\n"
    );
    assert_eq!(
        run(&tb, SEARCH, json!({"query": "needle"})),
        "(no matches)\n"
    );
    // A link that stays inside the root reads as its target.
    let out = run(&tb, READ_FILE, json!({"path": "inner.md"}));
    assert!(out.starts_with("README.md (lines 1-2 of 2)"), "{out}");
    // A link to .env resolves to .env and is denied.
    write(d.path(), ".env", "KEY=1");
    symlink(d.path().join(".env"), d.path().join("plain.txt")).unwrap();
    assert!(run(&tb, READ_FILE, json!({"path": "plain.txt"})).contains("environment files"));
}

#[test]
fn env_hidden_and_git_are_denied_and_never_listed() {
    let (d, tb) = project();
    write(d.path(), ".env", "SECRET=needle");
    write(d.path(), ".env.local", "SECRET=needle");
    write(d.path(), "sub/.env", "SECRET=needle");
    write(d.path(), "sub/.env.production", "SECRET=needle");
    write(d.path(), ".hidden/notes.txt", "needle");
    write(d.path(), ".git/config", "needle");
    write(d.path(), "src/.private.rs", "needle");
    for path in [".env", ".env.local", "sub/.env", "sub/.env.production"] {
        let out = run(&tb, READ_FILE, json!({"path": path}));
        assert!(out.contains("environment files"), "{path}: {out}");
    }
    for path in [
        ".hidden/notes.txt",
        ".git/config",
        "src/.private.rs",
        ".gitignore",
    ] {
        let out = run(&tb, READ_FILE, json!({"path": path}));
        assert!(out.starts_with("error: access denied"), "{path}: {out}");
    }
    assert!(run(&tb, LIST_FILES, json!({"directory": ".git"})).starts_with("error: access denied"));
    let list = run(&tb, LIST_FILES, json!({}));
    assert!(
        !list.contains(".env") && !list.contains(".git") && !list.contains(".hidden"),
        "{list}"
    );
    assert_eq!(
        run(&tb, SEARCH, json!({"query": "needle"})),
        "(no matches)\n"
    );
}

#[test]
fn gitignore_is_honoured_without_a_git_repo() {
    let (d, tb) = project();
    write(d.path(), ".gitignore", "target/\n*.log\n!keep.log\n");
    write(d.path(), "target/out.txt", "needle");
    write(d.path(), "debug.log", "needle");
    write(d.path(), "keep.log", "needle");
    write(d.path(), "src/.gitignore", "gen/\n/local.rs\n");
    write(d.path(), "src/gen/a.rs", "needle");
    write(d.path(), "src/local.rs", "needle");
    write(d.path(), "other/local.rs", "needle");

    assert_eq!(
        run(&tb, LIST_FILES, json!({})),
        "README.md\nkeep.log\nother/local.rs\nsrc/lib.rs\nsrc/main.rs\n"
    );
    assert_eq!(
        run(&tb, SEARCH, json!({"query": "needle"})),
        "keep.log:1: needle\nother/local.rs:1: needle\n"
    );
    for path in [
        "target/out.txt",
        "debug.log",
        "src/gen/a.rs",
        "src/local.rs",
    ] {
        let out = run(&tb, READ_FILE, json!({"path": path}));
        assert!(out.contains("ignored by .gitignore"), "{path}: {out}");
    }
    assert!(run(&tb, LIST_FILES, json!({"directory": "target"})).contains("ignored by .gitignore"));
    assert!(run(&tb, READ_FILE, json!({"path": "keep.log"})).contains("1: needle"));
    assert!(run(&tb, READ_FILE, json!({"path": "other/local.rs"})).contains("1: needle"));
}

#[test]
fn binary_non_utf8_and_oversized_files_are_skipped() {
    let (d, _) = project();
    write(d.path(), "bin.dat", b"abc\0needle");
    write(d.path(), "latin1.txt", b"caf\xe9 needle\n");
    write(d.path(), "big.txt", "needle\n".repeat(100));
    let config = ProjectToolConfig {
        max_file_bytes: 200,
        ..Default::default()
    };
    let tb = ProjectToolbox::for_root(d.path(), config).unwrap();
    assert!(run(&tb, READ_FILE, json!({"path": "bin.dat"})).contains("binary"));
    assert!(run(&tb, READ_FILE, json!({"path": "latin1.txt"})).contains("not valid UTF-8"));
    assert!(run(&tb, READ_FILE, json!({"path": "big.txt"})).contains("larger than 200 bytes"));
    let list = run(&tb, LIST_FILES, json!({}));
    assert!(
        !list.contains("bin.dat") && !list.contains("big.txt"),
        "{list}"
    );
    assert_eq!(
        run(&tb, SEARCH, json!({"query": "needle"})),
        "(no matches)\n"
    );
}

#[test]
fn list_and_search_caps_mark_truncation() {
    let d = TempDir::new().unwrap();
    for i in 0..10 {
        write(d.path(), &format!("f{i}.txt"), "hit\nhit\nhit\n");
    }
    let config = ProjectToolConfig {
        max_list_files: 4,
        max_search_matches: 5,
        ..Default::default()
    };
    let tb = ProjectToolbox::for_root(d.path(), config).unwrap();
    let list = run(&tb, LIST_FILES, json!({}));
    assert_eq!(list.lines().count(), 5, "{list}");
    assert!(list.contains("[truncated: showing 4 files"), "{list}");
    let hits = run(&tb, SEARCH, json!({"query": "hit"}));
    assert_eq!(hits.lines().filter(|l| l.contains(": hit")).count(), 5);
    assert!(hits.contains("[truncated: showing 5 matches"), "{hits}");
    let calls = tb.recorder().calls();
    assert!(calls.iter().all(|c| c.truncated));

    // Exactly at the cap is not truncated.
    let config = ProjectToolConfig {
        max_list_files: 10,
        ..Default::default()
    };
    let tb = ProjectToolbox::for_root(d.path(), config).unwrap();
    assert!(!run(&tb, LIST_FILES, json!({})).contains("truncated"));

    // The scanned-file cap also marks truncation.
    let config = ProjectToolConfig {
        max_scan_files: 2,
        ..Default::default()
    };
    let tb = ProjectToolbox::for_root(d.path(), config).unwrap();
    let hits = run(&tb, SEARCH, json!({"query": "hit"}));
    assert!(hits.contains("[truncated"), "{hits}");
}

#[test]
fn long_lines_and_output_bytes_are_capped() {
    let d = TempDir::new().unwrap();
    write(
        d.path(),
        "wide.txt",
        format!("{}\nshort\n", "é".repeat(100)),
    );
    write(d.path(), "many.txt", "0123456789\n".repeat(100));
    let config = ProjectToolConfig {
        max_line_bytes: 11,
        max_output_bytes: 100,
        ..Default::default()
    };
    let tb = ProjectToolbox::for_root(d.path(), config).unwrap();
    let out = run(&tb, READ_FILE, json!({"path": "wide.txt"}));
    assert!(
        out.contains("1: ééééé… [line truncated]\n2: short"),
        "{out}"
    );
    let hits = run(&tb, SEARCH, json!({"query": "é"}));
    assert!(
        hits.contains("wide.txt:1: ééééé… [line truncated]"),
        "{hits}"
    );
    let out = run(&tb, READ_FILE, json!({"path": "many.txt"}));
    assert!(
        out.contains("[truncated:") && out.contains("start_line="),
        "{out}"
    );
    let read = tb.recorder().calls().pop().unwrap().read.unwrap();
    assert!(read.end_line < 100 && read.end_line >= 1);
    assert!(tb.recorder().covers("many.txt", 1, read.end_line));
    assert!(!tb.recorder().covers("many.txt", 1, read.end_line + 1));
}

#[test]
fn read_file_line_ranges_and_edge_cases() {
    let d = TempDir::new().unwrap();
    write(d.path(), "crlf.txt", "one\r\ntwo\r\nthree\r\n");
    write(d.path(), "nonl.txt", "a\nb");
    write(d.path(), "empty.txt", "");
    write(d.path(), "blank.txt", "\n");
    write(
        d.path(),
        "long.txt",
        (1..=10).map(|i| format!("l{i}\n")).collect::<String>(),
    );
    let config = ProjectToolConfig {
        max_read_lines: 4,
        ..Default::default()
    };
    let tb = ProjectToolbox::for_root(d.path(), config).unwrap();

    assert_eq!(
        run(&tb, READ_FILE, json!({"path": "crlf.txt"})),
        "crlf.txt (lines 1-3 of 3)\n1: one\n2: two\n3: three\n"
    );
    assert_eq!(
        run(&tb, READ_FILE, json!({"path": "nonl.txt"})),
        "nonl.txt (lines 1-2 of 2)\n1: a\n2: b\n"
    );
    assert_eq!(
        run(&tb, READ_FILE, json!({"path": "blank.txt"})),
        "blank.txt (lines 1-1 of 1)\n1: \n"
    );
    assert_eq!(
        run(&tb, READ_FILE, json!({"path": "empty.txt"})),
        "empty.txt (empty file, 0 lines)\n"
    );
    // End past the file is clamped; start past it is an error.
    assert_eq!(
        run(
            &tb,
            READ_FILE,
            json!({"path": "nonl.txt", "start_line": 2, "end_line": 99})
        ),
        "nonl.txt (lines 2-2 of 2)\n2: b\n"
    );
    let past = run(&tb, READ_FILE, json!({"path": "nonl.txt", "start_line": 3}));
    assert!(
        past.starts_with("error: invalid arguments: start_line 3 is past the end"),
        "{past}"
    );
    assert!(
        run(&tb, READ_FILE, json!({"path": "nonl.txt", "start_line": 0}))
            .starts_with("error: invalid")
    );
    assert!(
        run(
            &tb,
            READ_FILE,
            json!({"path": "nonl.txt", "start_line": 2, "end_line": 1})
        )
        .starts_with("error: invalid")
    );
    assert!(
        run(&tb, READ_FILE, json!({"path": "."}))
            .starts_with("error: invalid arguments: . is not a file")
            || run(&tb, READ_FILE, json!({"path": "."})).contains("not a file")
    );
    assert!(run(&tb, READ_FILE, json!({})).starts_with("error: invalid arguments"));

    // The line cap truncates with a continuation hint.
    let out = run(&tb, READ_FILE, json!({"path": "long.txt"}));
    assert!(
        out.starts_with("long.txt (lines 1-4 of 10)\n1: l1\n"),
        "{out}"
    );
    assert!(out.contains("[truncated: showing lines 1-4 of the 10 requested; call read_file with start_line=5 to continue]"), "{out}");
}

#[test]
fn recorder_tracks_calls_and_line_coverage() {
    let (_d, tb) = project();
    run(&tb, LIST_FILES, json!({}));
    run(
        &tb,
        READ_FILE,
        json!({"path": "src/lib.rs", "start_line": 1, "end_line": 1}),
    );
    run(&tb, READ_FILE, json!({"path": "nope"}));
    run(
        &tb,
        READ_FILE,
        json!({"path": "src/lib.rs", "start_line": 2, "end_line": 2}),
    );
    let rec = tb.recorder();
    let calls = rec.calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[1].name, "read_file");
    assert_eq!(
        calls[1].args,
        json!({"path": "src/lib.rs", "start_line": 1, "end_line": 1})
    );
    assert!(calls[1].output.contains("1: pub fn Token() {}"));
    assert!(calls[2].is_error && calls[2].read.is_none());
    assert_eq!(
        calls.iter().map(|c| c.index).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    // Adjacent reads combine.
    assert!(rec.covers("src/lib.rs", 1, 2));
    assert!(rec.covers("./src/lib.rs", 2, 2));
    assert!(!rec.covers("src/lib.rs", 1, 3));
    assert!(!rec.covers("src/main.rs", 1, 1));
    assert!(!rec.covers("src/lib.rs", 0, 1));
    assert!(!rec.covers("src/lib.rs", 2, 1));
    assert_eq!(rec.reads().len(), 2);
    assert_eq!(rec.file_sha256("src/lib.rs").unwrap().len(), 64);
    assert!(rec.file_sha256("src/main.rs").is_none());
}

#[test]
fn call_cap_returns_errors_as_results_and_records_them() {
    let (_d, tb) = project();
    let config = ProjectToolConfig {
        max_tool_calls: 2,
        ..Default::default()
    };
    let tb2 = ProjectToolbox::for_root(_d.path(), config).unwrap();
    let _ = tb;
    assert!(!tb2.call(LIST_FILES, json!({})).is_error);
    assert!(!tb2.call(LIST_FILES, json!(null)).is_error);
    let third = tb2.call(LIST_FILES, json!({}));
    assert!(third.is_error);
    assert!(
        third.text.contains("tool call limit reached (2 calls)"),
        "{}",
        third.text
    );
    assert_eq!(tb2.recorder().call_count(), 3);
    let bad = ProjectToolbox::for_root(_d.path(), ProjectToolConfig::default()).unwrap();
    assert!(
        bad.call("rm_rf", json!({}))
            .text
            .starts_with("error: invalid arguments: unknown tool")
    );
}

#[test]
fn in_memory_stub_needs_no_filesystem() {
    let files = InMemoryProject::new()
        .with_file("docs/a.md", "alpha\r\nbeta token\n")
        .with_file(".env", "X=1")
        .with_file("src/x.rs", "token\n");
    let tb = ProjectToolbox::new(Arc::new(files), ProjectToolConfig::default());
    assert_eq!(run(&tb, LIST_FILES, json!({})), "docs/a.md\nsrc/x.rs\n");
    assert_eq!(run(&tb, LIST_FILES, json!({"glob": "*.md"})), "docs/a.md\n");
    assert_eq!(
        run(&tb, SEARCH, json!({"query": "TOKEN"})),
        "docs/a.md:2: beta token\nsrc/x.rs:1: token\n"
    );
    assert!(run(&tb, READ_FILE, json!({"path": "../x"})).starts_with("error: access denied"));
    assert!(run(&tb, READ_FILE, json!({"path": ".env"})).starts_with("error: access denied"));
    assert!(is_denied_or_missing(&run(
        &tb,
        READ_FILE,
        json!({"path": "/etc/passwd"})
    )));
    assert!(
        run(&tb, READ_FILE, json!({"path": "docs/a.md", "end_line": 1})).contains("1: alpha\n")
    );
    assert!(tb.recorder().covers("docs/a.md", 1, 1));
}

#[tokio::test]
async fn rig_adapters_share_the_toolbox() {
    use rig::tool::{PortableTool, tool_definition};
    let (_d, tb) = project();
    let tb = Arc::new(tb);
    let tools = rig_tools(&tb);
    assert_eq!(tool_definition(&tools.read_file).name, "read_file");
    assert_eq!(tool_definition(&tools.search).name, "search");
    assert_eq!(tool_definition(&tools.list_files).name, "list_files");
    let out = PortableTool::call(&tools.read_file, json!({"path": "README.md"}))
        .await
        .unwrap();
    assert!(out.starts_with("README.md (lines 1-2 of 2)"));
    let out = PortableTool::call(&tools.read_file, json!({"path": "../x"}))
        .await
        .unwrap();
    assert!(out.starts_with("error: access denied"));
    assert_eq!(tb.recorder().call_count(), 2);
}
