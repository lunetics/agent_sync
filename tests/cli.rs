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
