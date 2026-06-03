//! Smoke tests for the bare-`cinch` welcome message.

use std::process::Command;

fn cinch_binary() -> std::path::PathBuf {
    // CARGO_BIN_EXE_<name> is set by Cargo for integration tests.
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_cinch"))
}

#[test]
fn bare_cinch_unauthenticated_prints_welcome() {
    let tmp_home = tempfile::tempdir().expect("tempdir");
    let output = Command::new(cinch_binary())
        .env("HOME", tmp_home.path())
        .output()
        .expect("run cinch");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Welcome to Cinch"),
        "expected welcome in stderr, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("cinch auth login"),
        "missing first-command hint:\n{}",
        stderr
    );
    assert!(
        stderr.contains("cinch pair"),
        "welcome should mention cinch pair (Mac→SSH setup hint); got:\n{stderr}"
    );
}

#[test]
fn cinch_help_does_not_print_welcome() {
    let tmp_home = tempfile::tempdir().expect("tempdir");
    let output = Command::new(cinch_binary())
        .arg("-h")
        .env("HOME", tmp_home.path())
        .output()
        .expect("run cinch -h");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        !combined.contains("Welcome to Cinch"),
        "welcome should not appear for -h, got:\n{}",
        combined
    );
}

#[test]
fn push_is_removed_and_hard_errors_with_did_you_mean() {
    // `cinch push` was removed in 0.5 (its meaning changed). Bare `push` must
    // hard-error WITHOUT doing any work — no store open, no stdin save, no
    // network — and point at both replacement verbs. It no longer reaches the
    // auth path, so the old "exit 2 + cinch auth login" contract is gone.
    let tmp_home = tempfile::tempdir().expect("tempdir");
    let output = Command::new(cinch_binary())
        .arg("push")
        .env("HOME", tmp_home.path())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run cinch push");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 (GENERIC_ERROR), got {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cinch copy"),
        "removed-push error must point at `cinch copy`, got:\n{stderr}"
    );
    assert!(
        stderr.contains("cinch send"),
        "removed-push error must point at `cinch send`, got:\n{stderr}"
    );
}
