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

#[test]
fn list_json_outputs_names() {
    let tmp = tempfile::tempdir().unwrap();
    write_clipfile(
        tmp.path(),
        "version: 1\nclips:\n  a:\n    content: x\n    description: alpha\n  b:\n    content: y\n",
    );
    let (code, stdout, _stderr) = run_use(tmp.path(), &["--list", "--json"]);
    assert_eq!(code, Some(0));
    assert!(stdout.contains("\"name\":\"a\""), "got: {stdout}");
    assert!(
        stdout.contains("\"description\":\"alpha\""),
        "got: {stdout}"
    );
}

#[test]
fn missing_clipfile_errors_with_hint() {
    let tmp = tempfile::tempdir().unwrap();
    let (code, _stdout, stderr) = run_use(tmp.path(), &["anything"]);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("no cinch.yaml"), "got: {stderr}");
}

#[test]
fn unknown_clip_lists_available() {
    let tmp = tempfile::tempdir().unwrap();
    write_clipfile(
        tmp.path(),
        "version: 1\nclips:\n  deploy:\n    content: x\n",
    );
    let (code, _stdout, stderr) = run_use(tmp.path(), &["nope"]);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("not found"), "got: {stderr}");
    assert!(
        stderr.contains("deploy"),
        "fix hint should list available clips, got: {stderr}"
    );
}

#[test]
fn missing_required_var_errors_in_non_tty() {
    let tmp = tempfile::tempdir().unwrap();
    write_clipfile(
        tmp.path(),
        "version: 1\nclips:\n  api:\n    content: \"{{token}}\"\n    vars:\n      token: {}\n",
    );
    // stdin is null (non-TTY) via run_use, so this must error rather than hang.
    let (code, _stdout, stderr) = run_use(tmp.path(), &["api", "--stdout"]);
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("missing required variable"),
        "got: {stderr}"
    );
    assert!(stderr.contains("token"), "got: {stderr}");
}

#[test]
fn transform_pretty_json_is_applied() {
    let tmp = tempfile::tempdir().unwrap();
    // compact JSON content + pretty-json transform => multi-line output.
    write_clipfile(
        tmp.path(),
        "version: 1\nclips:\n  cfg:\n    content: '{\"a\":1}'\n    content_type: code\n    transform: pretty-json\n",
    );
    let (code, stdout, _stderr) = run_use(tmp.path(), &["cfg", "--stdout"]);
    assert_eq!(code, Some(0));
    assert!(
        stdout.contains("\n"),
        "pretty-json should add newlines, got: {stdout:?}"
    );
    assert!(stdout.contains("\"a\": 1"), "got: {stdout:?}");
}

#[test]
fn undeclared_braces_pass_through() {
    let tmp = tempfile::tempdir().unwrap();
    write_clipfile(
        tmp.path(),
        "version: 1\nclips:\n  gha:\n    content: \"token ${{ secrets.X }}\"\n",
    );
    let (code, stdout, _stderr) = run_use(tmp.path(), &["gha", "--stdout"]);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim_end(), "token ${{ secrets.X }}");
}

#[test]
fn var_value_may_contain_equals() {
    let tmp = tempfile::tempdir().unwrap();
    write_clipfile(
        tmp.path(),
        "version: 1\nclips:\n  kv:\n    content: \"{{pair}}\"\n    vars:\n      pair: {}\n",
    );
    let (code, stdout, _stderr) = run_use(tmp.path(), &["kv", "--stdout", "--var", "pair=a=b"]);
    assert_eq!(code, Some(0));
    assert_eq!(stdout.trim_end(), "a=b");
}
