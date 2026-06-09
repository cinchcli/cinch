//! `cinch use <name>` — resolve a clip declared in the project's `cinch.yaml`
//! and place it on the system clipboard (or stdout). The repo file is the
//! single source of truth; nothing is synced or stored.

use std::collections::BTreeMap;
use std::io::IsTerminal;

use client_core::clipfile::{self, ClipEntry, Clipfile, ClipfileError};
use serde::Serialize;

use crate::exit::{ExitError, GENERIC_ERROR};
use crate::io::{copy_text_to_clipboard, write_to_stdout};

// skim powers the interactive picker; it is a Unix-only dependency (its tuikit
// backend does not build on Windows). Mirrors crates/cli/src/commands/session.rs.
#[cfg(not(target_os = "windows"))]
use skim::prelude::*;
#[cfg(not(target_os = "windows"))]
use std::sync::Arc;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Name of the clip to use (from cinch.yaml). Omit to pick interactively.
    pub name: Option<String>,
    /// Print to stdout instead of copying to the system clipboard.
    #[arg(long)]
    pub stdout: bool,
    /// Supply or override a template variable. Repeatable. Format: NAME=VALUE.
    #[arg(long = "var", value_name = "NAME=VALUE")]
    pub vars: Vec<String>,
    /// List available clips and exit (does not copy anything).
    #[arg(long)]
    pub list: bool,
    /// With --list, emit JSON instead of a table.
    #[arg(long)]
    pub json: bool,
}

#[derive(Serialize)]
struct ClipInfo {
    name: String,
    description: Option<String>,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    let cwd = std::env::current_dir().map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("cannot read current directory: {e}"),
            "",
        )
    })?;
    let (_path, cf) = clipfile::load_from(&cwd).map_err(to_exit)?;

    if args.list {
        return print_list(&cf, args.json);
    }

    let name = match &args.name {
        Some(n) => n.clone(),
        None => {
            if !std::io::stdin().is_terminal() {
                return Err(ExitError::new(
                    GENERIC_ERROR,
                    "no clip name given",
                    "Run `cinch use --list` to see available clips, or `cinch use <name>`.",
                ));
            }
            pick_clip(&cf)?
        }
    };

    let entry = cf
        .clips
        .get(&name)
        .ok_or_else(|| unknown_clip(&name, &cf))?;

    let resolved = resolve_entry(entry, &args.vars)?;
    let mut content = clipfile::interpolate(&entry.content, &resolved);

    if let Some(action_id) = &entry.transform {
        content = apply_transform(action_id, &content, entry)?;
    }

    output(&name, &content, args.stdout)
}

/// Parse `--var NAME=VALUE` flags, resolve declared variables, and prompt for
/// any still-missing ones when attached to a TTY (else error).
fn resolve_entry(
    entry: &ClipEntry,
    raw_vars: &[String],
) -> Result<BTreeMap<String, String>, ExitError> {
    let flags = parse_var_flags(raw_vars)?;
    let mut resolution = clipfile::resolve_vars(entry, &flags, |k| std::env::var(k).ok());

    if !resolution.missing.is_empty() {
        if std::io::stdin().is_terminal() {
            for name in &resolution.missing {
                let desc = entry.vars.get(name).and_then(|v| v.env.as_deref());
                let value = prompt_var(name, desc)?;
                resolution.values.insert(name.clone(), value);
            }
        } else {
            return Err(ExitError::new(
                GENERIC_ERROR,
                format!(
                    "missing required variable(s): {}",
                    resolution.missing.join(", ")
                ),
                "Provide them with --var NAME=VALUE or set the declared env var(s).",
            ));
        }
    }
    Ok(resolution.values)
}

fn parse_var_flags(raw: &[String]) -> Result<BTreeMap<String, String>, ExitError> {
    let mut map = BTreeMap::new();
    for item in raw {
        match item.split_once('=') {
            Some((k, v)) => {
                map.insert(k.trim().to_string(), v.to_string());
            }
            None => {
                return Err(ExitError::new(
                    GENERIC_ERROR,
                    format!("--var must be NAME=VALUE, got: {item}"),
                    "Example: --var token=abc123",
                ))
            }
        }
    }
    Ok(map)
}

fn prompt_var(name: &str, env_hint: Option<&str>) -> Result<String, ExitError> {
    use std::io::Write;
    match env_hint {
        Some(env) => eprint!("{name} (or set ${env}): "),
        None => eprint!("{name}: "),
    }
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("failed to read input: {e}"), ""))?;
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

fn apply_transform(action_id: &str, content: &str, entry: &ClipEntry) -> Result<String, ExitError> {
    let action = client_core::transform::TransformAction::from_id(action_id).ok_or_else(|| {
        ExitError::new(
            GENERIC_ERROR,
            format!("unknown transform action: {action_id}"),
            "",
        )
    })?;
    let content_type = entry.content_type.clone().unwrap_or_else(|| {
        client_core::classify::detect(content.as_bytes())
            .as_wire()
            .to_string()
    });
    client_core::transform::apply_transform(action, content, &content_type)
        .map_err(|e| ExitError::new(GENERIC_ERROR, e.to_string(), ""))
}

fn output(name: &str, content: &str, to_stdout: bool) -> Result<(), ExitError> {
    if to_stdout {
        return write_to_stdout(content.as_bytes());
    }
    // Best-effort clipboard; never lose the content if the clipboard is absent
    // (e.g. headless CI) — fall back to stdout.
    if copy_text_to_clipboard(content) {
        eprintln!("Copied \"{name}\" to clipboard.");
        Ok(())
    } else {
        write_to_stdout(content.as_bytes())
    }
}

fn print_list(cf: &Clipfile, json: bool) -> Result<(), ExitError> {
    let infos: Vec<ClipInfo> = cf
        .clips
        .iter()
        .map(|(name, e)| ClipInfo {
            name: name.clone(),
            description: e.description.clone(),
        })
        .collect();

    if json || !std::io::stdout().is_terminal() {
        let s = serde_json::to_string(&infos)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("serialize failed: {e}"), ""))?;
        println!("{s}");
    } else if infos.is_empty() {
        eprintln!("No clips defined in cinch.yaml.");
    } else {
        for info in &infos {
            match &info.description {
                Some(d) => println!("{}  —  {}", info.name, d),
                None => println!("{}", info.name),
            }
        }
    }
    Ok(())
}

fn unknown_clip(name: &str, cf: &Clipfile) -> ExitError {
    let available: Vec<&str> = cf.clips.keys().map(String::as_str).collect();
    let fix = if available.is_empty() {
        "cinch.yaml defines no clips.".to_string()
    } else {
        format!("Available clips: {}", available.join(", "))
    };
    ExitError::new(
        GENERIC_ERROR,
        format!("clip \"{name}\" not found in cinch.yaml"),
        fix,
    )
}

/// Map a clipfile-layer error to a CLI ExitError, adding a fix hint for the
/// common "no file here" case.
fn to_exit(err: ClipfileError) -> ExitError {
    let fix = match &err {
        ClipfileError::NotFound(_) => {
            "Create a cinch.yaml at your project root (see `cinch use --help`)."
        }
        _ => "",
    };
    ExitError::new(GENERIC_ERROR, err.to_string(), fix)
}

#[cfg(not(target_os = "windows"))]
struct ClipItem {
    name: String,
    description: String,
}

#[cfg(not(target_os = "windows"))]
impl SkimItem for ClipItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.name)
    }
    fn preview(&self, _ctx: PreviewContext) -> ItemPreview {
        ItemPreview::Text(self.description.clone())
    }
}

#[cfg(not(target_os = "windows"))]
fn pick_clip(cf: &Clipfile) -> Result<String, ExitError> {
    if cf.clips.is_empty() {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "cinch.yaml defines no clips.",
            "",
        ));
    }
    let items: Vec<Arc<dyn SkimItem>> = cf
        .clips
        .iter()
        .map(|(name, e)| {
            Arc::new(ClipItem {
                name: name.clone(),
                description: e.description.clone().unwrap_or_default(),
            }) as Arc<dyn SkimItem>
        })
        .collect();

    let options = SkimOptionsBuilder::default()
        .prompt("clip> ".to_string())
        .height("50%".to_string())
        .preview(Some(String::new()))
        .preview_window("right:50%:wrap".to_string())
        .build()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("picker init failed: {e}"), ""))?;

    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
    for item in items {
        let _ = tx.send(item);
    }
    drop(tx);

    let out = Skim::run_with(&options, Some(rx))
        .ok_or_else(|| ExitError::new(GENERIC_ERROR, "picker failed to start.", ""))?;
    if out.is_abort {
        return Err(ExitError::new(GENERIC_ERROR, "selection cancelled.", ""));
    }
    let selected = out.selected_items.first().ok_or_else(|| {
        ExitError::new(GENERIC_ERROR, "nothing selected.", "Press Enter to pick.")
    })?;
    Ok(selected.text().into_owned())
}

#[cfg(target_os = "windows")]
fn pick_clip(_cf: &Clipfile) -> Result<String, ExitError> {
    Err(ExitError::new(
        GENERIC_ERROR,
        "interactive clip selection is not available on Windows.",
        "Pass a clip name: `cinch use <name>` (see `cinch use --list`).",
    ))
}
