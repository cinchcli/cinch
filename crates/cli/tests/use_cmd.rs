use std::path::Path;
use std::process::Command;

fn cinch_binary() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_cinch"))
}

fn write_clipfile(dir: &Path, body: &str) {
    std::fs::write(dir.join("cinch.yaml"), body).expect("write cinch.yaml");
}

/// Run `cinch use <args>` in `dir` with an isolated HOME; return (exit_code, stdout, stderr).
fn run_use(dir: &Path, args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(cinch_binary())
        .arg("use")
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env_remove("CINCH_TOKEN")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run cinch use");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn stdout_interpolates_flag_var() {
    let tmp = tempfile::tempdir().unwrap();
    write_clipfile(
        tmp.path(),
        "version: 1\nclips:\n  greet:\n    content: \"hi {{name}}\"\n    vars:\n      name: {}\n",
    );
    let (code, stdout, _stderr) = run_use(tmp.path(), &["greet", "--stdout", "--var", "name=bob"]);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim_end(), "hi bob");
}

#[test]
fn stdout_uses_default_var() {
    let tmp = tempfile::tempdir().unwrap();
    write_clipfile(
        tmp.path(),
        "version: 1\nclips:\n  greet:\n    content: \"hi {{name}}\"\n    vars:\n      name:\n        default: world\n",
    );
    let (code, stdout, _stderr) = run_use(tmp.path(), &["greet", "--stdout"]);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim_end(), "hi world");
}
