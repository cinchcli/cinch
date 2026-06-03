//! 0.5 CLI-redesign behavioral matrix (design §4d / eng-review report §4).
//!
//! Process-level (exit code + captured stderr/stdout). Proves:
//! - every deprecated spelling prints EXACTLY ONE deprecation note naming the
//!   correct new spelling (and still routes to a handler, not a clap "unknown
//!   subcommand" error),
//! - the new spellings print NO note,
//! - generated completions emit ONLY the new names.
//!
//! These run with a throwaway `HOME` and the relay env vars cleared, so they
//! never touch a real `~/.cinch` or contact a relay — the note is printed
//! before the (auth-failing) handler runs, which is exactly the contract.

use std::process::{Command, Stdio};

fn cinch_binary() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_cinch"))
}

const MARKER: &str = "(deprecated alias, removed in 0.8)";

/// Run `cinch <args>` hermetically; return captured stderr.
fn run_stderr(args: &[&str]) -> String {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = Command::new(cinch_binary())
        .args(args)
        .env("HOME", tmp.path())
        .env_remove("CINCH_TOKEN")
        .env_remove("CINCH_RELAY_URL")
        .stdin(Stdio::null())
        .output()
        .expect("run cinch");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn marker_lines(stderr: &str) -> usize {
    stderr.lines().filter(|l| l.contains(MARKER)).count()
}

fn note(old: &str, new: &str) -> String {
    format!("note: `cinch {old}` is now `cinch {new}` {MARKER}")
}

#[test]
fn every_deprecated_alias_prints_exactly_one_correct_note() {
    // (argv, old spelling, new spelling) — the full §4d alias matrix.
    let cases: &[(&[&str], &str, &str)] = &[
        (&["clip", "list"], "clip list", "history list"),
        (&["clip", "search", "q"], "clip search", "history search"),
        (&["clip", "get", "abcd"], "clip get", "history show"),
        (&["clip", "rm", "abcd"], "clip rm", "history rm"),
        (
            &["clip", "transform", "abcd"],
            "clip transform",
            "history transform",
        ),
        (&["device", "list"], "device list", "fleet list"),
        (&["device", "pair", "user@host"], "device pair", "fleet add"),
        (
            &["device", "set-name", "MyMac"],
            "device set-name",
            "fleet rename self",
        ),
        (
            &["device", "nickname", "01JZ", "box"],
            "device nickname",
            "fleet rename <DEVICE>",
        ),
        (
            &["device", "retention"],
            "device retention",
            "fleet retention",
        ),
        (
            &["device", "revoke", "abcd"],
            "device revoke",
            "fleet revoke",
        ),
        (&["device", "sources"], "device sources", "fleet sources"),
        (&["pin", "add", "abcd"], "pin add", "pin"),
        (&["pin", "rm", "abcd"], "pin rm", "unpin"),
        (&["pin", "list"], "pin list", "history list --pinned"),
        (&["auth", "set-name", "MyMac"], "auth set-name", "auth name"),
    ];

    for (argv, old, new) in cases {
        let stderr = run_stderr(argv);
        assert_eq!(
            marker_lines(&stderr),
            1,
            "argv {argv:?} must print EXACTLY ONE deprecation note; stderr:\n{stderr}"
        );
        let expected = note(old, new);
        assert!(
            stderr.contains(&expected),
            "argv {argv:?} note mismatch.\nexpected line: {expected}\ngot stderr:\n{stderr}"
        );
    }
}

#[test]
fn new_spellings_print_no_deprecation_note() {
    for argv in [
        &["history", "list"][..],
        &["fleet", "list"][..],
        &["pin", "abcd"][..],
        &["unpin", "abcd"][..],
        &["auth", "name", "MyMac"][..],
        &["copy"][..],
        &["send"][..],
    ] {
        let stderr = run_stderr(argv);
        assert_eq!(
            marker_lines(&stderr),
            0,
            "new spelling {argv:?} must NOT print a deprecation note; stderr:\n{stderr}"
        );
    }
}

fn completion(shell: &str) -> String {
    let out = Command::new(cinch_binary())
        .args(["completion", shell])
        .output()
        .expect("run cinch completion");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn completions_emit_only_new_names() {
    // bash uses `cinch__subcmd__<name>` mangled identifiers — the most precise
    // surface; it also lets us prove the nested `pin add/rm/list` are gone.
    let bash = completion("bash");
    for dep in [
        "cinch__subcmd__clip",
        "cinch__subcmd__device",
        "cinch__subcmd__push",
        "cinch__subcmd__pin__subcmd__add",
        "cinch__subcmd__pin__subcmd__rm",
        "cinch__subcmd__pin__subcmd__list",
    ] {
        assert!(
            !bash.contains(dep),
            "bash completion leaked deprecated `{dep}`"
        );
    }
    for new in [
        "cinch__subcmd__history",
        "cinch__subcmd__fleet",
        "cinch__subcmd__unpin",
        "cinch__subcmd__send",
        "cinch__subcmd__copy",
        "cinch__subcmd__paste",
    ] {
        assert!(bash.contains(new), "bash completion missing new `{new}`");
    }

    // zsh _describe candidates are `'<name>:<desc>'`.
    let zsh = completion("zsh");
    for dep in ["'clip:", "'device:", "'push:"] {
        assert!(
            !zsh.contains(dep),
            "zsh completion leaked deprecated `{dep}`"
        );
    }
    for new in ["'history:", "'fleet:", "'unpin:"] {
        assert!(zsh.contains(new), "zsh completion missing new `{new}`");
    }

    // fish top-level commands are `... -a "<name>"`.
    let fish = completion("fish");
    for dep in ["-a \"clip\"", "-a \"device\"", "-a \"push\""] {
        assert!(
            !fish.contains(dep),
            "fish completion leaked deprecated `{dep}`"
        );
    }
    for new in ["-a \"history\"", "-a \"fleet\"", "-a \"unpin\""] {
        assert!(fish.contains(new), "fish completion missing new `{new}`");
    }
}
