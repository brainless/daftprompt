//! Epic 014 Task 5 CLI-level golden check: the `helper` subcommand with no
//! `--scripted-fixture` (the always-available disabled path) must produce
//! output byte-identical to the `prompt` subcommand for the same inputs
//! (Task 5 acceptance criterion: "the deterministic baseline remains fully
//! usable when helper execution is disabled").

#[test]
fn cli_helper_disabled_matches_cli_prompt() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let bin = env!("CARGO_BIN_EXE_task_zero_lab");
    let request = "continue closing the Task 0 blockers";

    let prompt_out = std::process::Command::new(bin)
        .args(["prompt", "--repo", repo_root.to_str().unwrap(), "--rev", "HEAD", "--epics", "014", "--request", request])
        .output()
        .unwrap();
    let helper_out = std::process::Command::new(bin)
        .args(["helper", "--repo", repo_root.to_str().unwrap(), "--rev", "HEAD", "--epics", "014", "--request", request])
        .output()
        .unwrap();

    assert!(prompt_out.status.success(), "prompt subcommand failed: {}", String::from_utf8_lossy(&prompt_out.stderr));
    assert!(helper_out.status.success(), "helper subcommand failed: {}", String::from_utf8_lossy(&helper_out.stderr));
    assert_eq!(prompt_out.stdout, helper_out.stdout);
}
