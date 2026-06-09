//! `cinch.yaml` — a repo-committed, version-controlled "project clipboard".
//!
//! Pure parsing + variable resolution. No I/O beyond reading the file and the
//! process environment, and no clipboard/network concerns — the CLI layer wires
//! those in. Editing a clip is a git change; there is no store to reconcile.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

/// The fixed file name discovered by walking up from the working directory.
pub const FILE_NAME: &str = "cinch.yaml";

/// The only schema version this build understands.
pub const SUPPORTED_VERSION: u32 = 1;

/// Canonical `content_type` values allowed in a Clipfile (text-only; `image`
/// from the wire vocabulary is intentionally excluded).
const ALLOWED_CONTENT_TYPES: [&str; 3] = ["text", "code", "url"];

/// A parsed `cinch.yaml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clipfile {
    pub version: u32,
    // `clips` is required (per spec); a file with `version: 1` and no `clips:`
    // key is a parse error rather than an empty Clipfile.
    pub clips: BTreeMap<String, ClipEntry>,
}

/// One named clip declaration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipEntry {
    /// Clip body; may contain `{{var}}` placeholders.
    pub content: String,
    /// Shown in the picker and `--list`.
    #[serde(default)]
    pub description: Option<String>,
    /// Declared template variables.
    #[serde(default)]
    pub vars: BTreeMap<String, VarSpec>,
    /// Optional canonical content type (`text` | `code` | `url`). Auto-detected
    /// by the CLI when omitted.
    #[serde(default)]
    pub content_type: Option<String>,
    /// Optional transform action id applied after interpolation.
    #[serde(default)]
    pub transform: Option<String>,
}

/// How a single variable is resolved.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VarSpec {
    /// Environment variable to read the value from.
    #[serde(default)]
    pub env: Option<String>,
    /// Fallback value when no flag/env is supplied.
    #[serde(default)]
    pub default: Option<String>,
}

/// Everything that can go wrong loading a Clipfile. (Unknown-clip and
/// missing-variable cases are presentation concerns handled in the CLI layer,
/// so they are not modeled here.)
///
/// NOTE: `thiserror` v2 format strings only support `{0}`/`{field}` field
/// references and trailing positional expression args (e.g. `.0.display()`).
/// They do NOT support named args or const references inside the message, so
/// `cinch.yaml`/version literals are written out directly.
#[derive(Debug, Error)]
pub enum ClipfileError {
    #[error("no cinch.yaml found in this project (searched from {} up to the filesystem root)", .0.display())]
    NotFound(PathBuf),
    #[error("failed to read {}: {}", .path.display(), .source)]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cinch.yaml is not valid: {0}")]
    Parse(String),
    #[error("unsupported cinch.yaml version {found} (this cinch supports version 1)")]
    UnsupportedVersion { found: u32 },
    #[error("invalid content_type {0:?} (must be one of: text, code, url)")]
    InvalidContentType(String),
    #[error("unknown transform action {0:?}")]
    UnknownTransform(String),
}

/// Parse + validate a Clipfile from YAML text. Validates schema version,
/// `content_type` values, and `transform` ids up front so errors surface at
/// load time rather than at use time.
pub fn parse(text: &str) -> Result<Clipfile, ClipfileError> {
    let file: Clipfile =
        serde_yaml_ng::from_str(text).map_err(|e| ClipfileError::Parse(e.to_string()))?;

    if file.version != SUPPORTED_VERSION {
        return Err(ClipfileError::UnsupportedVersion {
            found: file.version,
        });
    }

    for entry in file.clips.values() {
        if let Some(ct) = &entry.content_type {
            if !ALLOWED_CONTENT_TYPES.contains(&ct.as_str()) {
                return Err(ClipfileError::InvalidContentType(ct.clone()));
            }
        }
        if let Some(action) = &entry.transform {
            if crate::transform::TransformAction::from_id(action).is_none() {
                return Err(ClipfileError::UnknownTransform(action.clone()));
            }
        }
    }

    Ok(file)
}

/// Walk up from `start` (inclusive) to the filesystem root, returning the path
/// of the nearest `cinch.yaml`. Errors with `NotFound` if none exists.
pub fn find(start: &Path) -> Result<PathBuf, ClipfileError> {
    for dir in start.ancestors() {
        let candidate = dir.join(FILE_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(ClipfileError::NotFound(start.to_path_buf()))
}

/// Discover the nearest `cinch.yaml` from `start`, read it, and parse+validate.
/// Returns the resolved path alongside the parsed Clipfile.
pub fn load_from(start: &Path) -> Result<(PathBuf, Clipfile), ClipfileError> {
    let path = find(start)?;
    let text = std::fs::read_to_string(&path).map_err(|source| ClipfileError::Io {
        path: path.clone(),
        source,
    })?;
    let file = parse(&text)?;
    Ok((path, file))
}

/// Outcome of resolving an entry's declared variables.
#[derive(Debug, Default)]
pub struct Resolution {
    /// Successfully resolved name -> value pairs.
    pub values: BTreeMap<String, String>,
    /// Declared names with no flag, no (non-empty) env, and no default.
    /// The caller decides whether to prompt (TTY) or error (non-TTY).
    ///
    /// Names appear in the same order as iteration over `vars`, which is a
    /// `BTreeMap` — i.e. alphabetical order. The CLI relies on this for its
    /// deterministic error message.
    pub missing: Vec<String>,
}

/// Resolve an entry's declared variables using flags then env then defaults.
/// Empty env values are treated as unset. `get_env` returns the value of an
/// environment variable (injected for testability).
pub fn resolve_vars(
    entry: &ClipEntry,
    flags: &BTreeMap<String, String>,
    get_env: impl Fn(&str) -> Option<String>,
) -> Resolution {
    let mut out = Resolution::default();
    for (name, spec) in &entry.vars {
        if let Some(v) = flags.get(name) {
            out.values.insert(name.clone(), v.clone());
            continue;
        }
        if let Some(env_name) = &spec.env {
            if let Some(v) = get_env(env_name).filter(|v| !v.is_empty()) {
                out.values.insert(name.clone(), v);
                continue;
            }
        }
        if let Some(d) = &spec.default {
            out.values.insert(name.clone(), d.clone());
            continue;
        }
        out.missing.push(name.clone());
    }
    out
}

/// Replace `{{ name }}` runs whose trimmed token is a key in `values`. Any
/// other `{{...}}` (undeclared name, or an unmatched `{{`) is emitted verbatim,
/// in a single left-to-right pass so substituted values are never re-scanned.
///
/// **Loop invariant:** on every successful match the scanner advances `rest`
/// past the entire `{{...}}` span, including the closing `}}`. The output
/// buffer is therefore write-only: substituted values are never re-scanned,
/// which prevents re-substitution and guarantees termination even if a
/// substituted value itself contains `{{...}}` text.
pub fn interpolate(content: &str, values: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        if let Some(close) = after_open.find("}}") {
            let key = after_open[..close].trim();
            if let Some(val) = values.get(key) {
                out.push_str(val);
                rest = &after_open[close + 2..];
                continue;
            }
            // Undeclared: emit the literal "{{" and resume scanning after it so
            // a later valid placeholder in the same run is still handled.
            out.push_str("{{");
            rest = after_open;
        } else {
            // No closing "}}" anywhere: emit the rest verbatim and stop.
            out.push_str("{{");
            out.push_str(after_open);
            return out;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_clipfile() {
        let cf = parse("version: 1\nclips:\n  deploy:\n    content: \"echo hi\"\n").unwrap();
        assert_eq!(cf.version, 1);
        assert_eq!(cf.clips.len(), 1);
        assert_eq!(cf.clips["deploy"].content, "echo hi");
    }

    #[test]
    fn parses_vars_and_optional_fields() {
        let yaml = "version: 1\nclips:\n  api:\n    description: call\n    content: \"{{token}}\"\n    content_type: code\n    vars:\n      token:\n        env: API_TOKEN\n";
        let cf = parse(yaml).unwrap();
        let api = &cf.clips["api"];
        assert_eq!(api.description.as_deref(), Some("call"));
        assert_eq!(api.content_type.as_deref(), Some("code"));
        assert_eq!(api.vars["token"].env.as_deref(), Some("API_TOKEN"));
    }

    #[test]
    fn rejects_unsupported_version() {
        let err = parse("version: 2\nclips: {}\n").unwrap_err();
        assert!(matches!(
            err,
            ClipfileError::UnsupportedVersion { found: 2 }
        ));
    }

    #[test]
    fn rejects_invalid_content_type() {
        let yaml = "version: 1\nclips:\n  x:\n    content: a\n    content_type: image\n";
        let err = parse(yaml).unwrap_err();
        assert!(matches!(err, ClipfileError::InvalidContentType(ct) if ct == "image"));
    }

    #[test]
    fn rejects_unknown_transform() {
        let yaml = "version: 1\nclips:\n  x:\n    content: a\n    transform: bogus\n";
        let err = parse(yaml).unwrap_err();
        assert!(matches!(err, ClipfileError::UnknownTransform(a) if a == "bogus"));
    }

    #[test]
    fn rejects_unknown_field_typo() {
        // deny_unknown_fields catches `contnet:` typos.
        let err = parse("version: 1\nclips:\n  x:\n    contnet: a\n").unwrap_err();
        assert!(matches!(err, ClipfileError::Parse(_)));
    }

    #[test]
    fn find_locates_clipfile_in_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(FILE_NAME), "version: 1\nclips: {}\n").unwrap();
        let nested = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find(&nested).unwrap();
        // Compare canonicalized paths (macOS /var -> /private/var symlink).
        assert_eq!(
            std::fs::canonicalize(&found).unwrap(),
            std::fs::canonicalize(tmp.path().join(FILE_NAME)).unwrap()
        );
    }

    #[test]
    fn find_errors_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let err = find(tmp.path()).unwrap_err();
        let ClipfileError::NotFound(p) = err else {
            panic!("expected NotFound, got {err:?}")
        };
        assert_eq!(p, tmp.path());
    }

    #[test]
    fn load_from_reads_and_parses() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(FILE_NAME),
            "version: 1\nclips:\n  hi:\n    content: hello\n",
        )
        .unwrap();

        let (path, cf) = load_from(tmp.path()).unwrap();
        assert!(path.ends_with(FILE_NAME));
        assert_eq!(cf.clips["hi"].content, "hello");
    }

    fn entry_with_vars(specs: &[(&str, VarSpec)]) -> ClipEntry {
        ClipEntry {
            content: String::new(),
            description: None,
            vars: specs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            content_type: None,
            transform: None,
        }
    }

    #[test]
    fn resolve_precedence_flag_over_env_over_default() {
        let entry = entry_with_vars(&[
            (
                "a",
                VarSpec {
                    env: Some("A_ENV".into()),
                    default: Some("a_def".into()),
                },
            ),
            (
                "b",
                VarSpec {
                    env: Some("B_ENV".into()),
                    default: Some("b_def".into()),
                },
            ),
            (
                "c",
                VarSpec {
                    env: None,
                    default: Some("c_def".into()),
                },
            ),
            (
                "d",
                VarSpec {
                    env: Some("D_ENV".into()),
                    default: None,
                },
            ),
        ]);
        let flags: BTreeMap<String, String> = [("a".to_string(), "a_flag".to_string())]
            .into_iter()
            .collect();
        let env: BTreeMap<&str, &str> = [("B_ENV", "b_env"), ("D_ENV", "")].into_iter().collect();

        let r = resolve_vars(&entry, &flags, |k| env.get(k).map(|s| s.to_string()));

        assert_eq!(r.values["a"], "a_flag"); // flag wins
        assert_eq!(r.values["b"], "b_env"); // env wins over default
        assert_eq!(r.values["c"], "c_def"); // default
        assert!(!r.values.contains_key("d")); // env empty + no default => missing
        assert_eq!(r.missing, vec!["d".to_string()]);
    }

    fn vals(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn interpolate_substitutes_declared_only() {
        let v = vals(&[("token", "abc"), ("env_name", "staging")]);
        assert_eq!(interpolate("Bearer {{token}}", &v), "Bearer abc");
        // tolerant of inner whitespace
        assert_eq!(interpolate("Bearer {{ token }}", &v), "Bearer abc");
        // multiple, adjacent
        assert_eq!(interpolate("{{token}}/{{env_name}}", &v), "abc/staging");
    }

    #[test]
    fn interpolate_leaves_undeclared_verbatim() {
        let v = vals(&[("token", "abc")]);
        // GitHub Actions expression: `secrets.X` is not a declared var.
        assert_eq!(interpolate("${{ secrets.X }}", &v), "${{ secrets.X }}");
        // Unclosed brace pair is left as-is.
        assert_eq!(interpolate("a {{ b", &v), "a {{ b");
        // No vars at all.
        assert_eq!(interpolate("plain text", &BTreeMap::new()), "plain text");
    }

    #[test]
    fn resolve_reports_all_missing_in_order() {
        let entry = entry_with_vars(&[
            (
                "b",
                VarSpec {
                    env: None,
                    default: None,
                },
            ),
            (
                "d",
                VarSpec {
                    env: None,
                    default: None,
                },
            ),
            (
                "a",
                VarSpec {
                    env: None,
                    default: Some("x".into()),
                },
            ),
        ]);
        let r = resolve_vars(&entry, &BTreeMap::new(), |_| None);
        assert_eq!(r.missing, vec!["b".to_string(), "d".to_string()]);
        assert_eq!(r.values["a"], "x");
    }
}
