use assert_cmd::Command;
use predicates::prelude::*;

fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}

fn agentsync() -> Command {
    Command::new(env!("CARGO_BIN_EXE_agentsync"))
}

#[test]
fn version_prints_the_engine_version() {
    agentsync()
        .arg("version")
        .assert()
        .success()
        .stdout(format!("agentsync v{}\n", engine_version()));
}

#[test]
fn version_flags_match_the_bash_cli() {
    for flag in ["--version", "-v"] {
        agentsync()
            .arg(flag)
            .assert()
            .success()
            .stdout(format!("agentsync v{}\n", engine_version()));
    }
}

#[test]
fn a_stale_binary_refuses_to_run() {
    agentsync()
        .env("AGENTSYNC_ENGINE_VERSION", "0.0.1")
        .arg("version")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("cargo build --release"));
}

#[test]
fn a_matching_engine_version_is_accepted() {
    agentsync()
        .env("AGENTSYNC_ENGINE_VERSION", engine_version())
        .arg("version")
        .assert()
        .success();
}

#[test]
fn list_works_without_a_project_config() {
    let dir = tempfile::tempdir().unwrap();
    agentsync()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("  AgentSync Tools\n"))
        .stdout(predicate::str::contains("Claude Code"))
        .stdout(predicate::str::contains("  0 of 13 enabled\n"))
        .stdout(predicate::str::contains("Enable a tool:"));
}

#[test]
fn ls_is_an_alias_for_list() {
    let dir = tempfile::tempdir().unwrap();
    agentsync()
        .current_dir(dir.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("  AgentSync Tools\n"));
}

#[test]
fn list_counts_configured_tools_and_honours_the_repo_root_variable() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
    std::fs::write(
        dir.path().join(".ai/agent_sync.yaml"),
        "tools:\n  enabled:\n    - claude\n",
    )
    .unwrap();
    agentsync()
        .env("AGENTSYNC_REPO_ROOT", dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("  1 of 13 enabled\n"))
        .stdout(predicate::str::contains("Customize a tool:"))
        .stdout(predicate::str::contains("Enable a tool:").not());
}
