//! `cinch session copy` — pull answer(s) out of an agent coding session into a
//! cinch clip + the system clipboard.
//!
//! An agent session (Claude Code today) lives as a noisy JSONL transcript under
//! `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`. This command resolves the
//! session, selects one or more answers (interactive picker / `--last` /
//! `--all`), renders them to clean Markdown via [`client_core::session`], and
//! then both saves the result as a syncing clip and copies it to the clipboard.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use client_core::machine::hostname_or_unknown;
use client_core::rest::ContentType;
use client_core::session::source::SessionSelector;
use client_core::session::{
    answer_is_empty, markdown, Answer, ClaudeSource, RenderOpts, Session, SessionSource,
};
use client_core::store::models::{StoredClip, SyncState};
use client_core::store::{self, queries, Store};
use skim::prelude::*;

use crate::exit::{ExitError, GENERIC_ERROR};
use crate::io::{copy_text_to_clipboard, write_to_stdout};

/// Tool-result render budget (chars) before truncation. Mirrors the session
/// renderer default; kept explicit here so the CLI owns the policy.
const TOOL_RESULT_MAX: usize = 800;

/// Upper bound for an auto-derived clip label.
const LABEL_MAX: usize = 80;

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// Copy answer(s) from an agent session to a clip + the clipboard.
    Copy(CopyArgs),
}

#[derive(Debug, clap::Args)]
pub struct CopyArgs {
    /// Session id prefix, or "latest" (default).
    pub session: Option<String>,

    /// Session source. Default and only value now: claude.
    #[arg(long, default_value = "claude")]
    pub from: String,

    /// Interactively choose the session too (not just the answer).
    #[arg(long)]
    pub pick: bool,

    /// Last N answers (default N=1). Non-interactive path.
    #[arg(long, num_args = 0..=1, default_missing_value = "1")]
    pub last: Option<usize>,

    /// Whole session (every answer, in order).
    #[arg(long, conflicts_with = "last")]
    pub all: bool,

    /// Include the eliciting user prompt above each answer.
    #[arg(long)]
    pub with_prompt: bool,

    /// Include assistant thinking blocks (default off).
    #[arg(long)]
    pub include_thinking: bool,

    /// Exclude tool calls/results (default: include, results truncated).
    #[arg(long)]
    pub no_tools: bool,

    /// Write Markdown to stdout instead of saving a clip.
    #[arg(long)]
    pub stdout: bool,

    /// Skip the system-clipboard copy.
    #[arg(long)]
    pub no_copy: bool,

    /// Clip label (default: derived session/answer title).
    #[arg(short = 'l', long)]
    pub label: Option<String>,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::Copy(a) => run_copy(a).await,
    }
}

async fn run_copy(args: CopyArgs) -> Result<(), ExitError> {
    // 1. Validate source. Only `claude` ships in this cut; the trait is here
    //    so codex / gemini sources can slot in later without a flag change.
    if args.from != "claude" {
        return Err(ExitError::new(
            GENERIC_ERROR,
            format!("Unknown session source: {}", args.from),
            "Only --from claude is supported in this version.",
        ));
    }
    let source = ClaudeSource::new();

    // 2. Current working directory drives the Claude project-dir lookup.
    let cwd = std::env::current_dir()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Cannot read cwd: {e}"), ""))?;

    // 3. Resolve which session to load.
    let selector = if args.pick {
        pick_session(&source, &cwd)?
    } else if let Some(s) = args.session.as_deref().filter(|s| *s != "latest") {
        SessionSelector::IdPrefix(s.to_string())
    } else {
        SessionSelector::Latest
    };

    // 4. Load + fully parse the session.
    let session: Session = source.load(&cwd, &selector).map_err(map_session_err)?;

    // 5. A session with no real user prompts has nothing to copy.
    if session.answers.is_empty() {
        return Err(ExitError::new(GENERIC_ERROR, "Session has no answers.", ""));
    }

    // 6. Render options — built up front so the picker preview and the
    //    empty-answer filter below both reflect exactly what will be copied.
    let opts = RenderOpts {
        with_prompt: args.with_prompt,
        include_thinking: args.include_thinking,
        include_tools: !args.no_tools,
        tool_result_max: TOOL_RESULT_MAX,
    };

    // 7. Keep only answers that produce content under these options. An
    //    in-progress trailing turn (a user prompt with no assistant reply) or a
    //    thinking-only answer renders empty; copying it would save a useless
    //    clip, so such answers are never offered or selected.
    let renderable: Vec<Answer> = session
        .answers
        .iter()
        .filter(|a| !answer_is_empty(a, opts))
        .cloned()
        .collect();
    if renderable.is_empty() {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "This session has no answers with copyable content.",
            "The latest turn may still be in progress. Try --with-prompt, or pick another session.",
        ));
    }

    // 8. Select which answer(s) to copy, in ascending session order.
    let owned: Vec<Answer> = if args.all {
        renderable
    } else if let Some(n) = args.last {
        let n = n.max(1).min(renderable.len());
        renderable[renderable.len() - n..].to_vec()
    } else {
        // Interactive default — requires a TTY (skim draws a full-screen UI).
        if !std::io::stdin().is_terminal() {
            return Err(ExitError::new(
                GENERIC_ERROR,
                "No answer selection and stdin is not a TTY.",
                "Pass --last [N] or --all for non-interactive use.",
            ));
        }
        let indices = pick_answers(&renderable, opts)?;
        indices.iter().map(|&i| renderable[i].clone()).collect()
    };

    // 9. Render the selection to Markdown.
    let md = markdown(&owned, opts);

    // 8. Output: stdout-only, or clip (+ clipboard).
    if args.stdout {
        write_to_stdout(md.as_bytes())?;
        if !md.ends_with('\n') {
            write_to_stdout(b"\n")?;
        }
        return Ok(());
    }

    let clip_id = save_clip(&md, args.label, &session)?;

    if !args.no_copy {
        // Best-effort: a clipboard failure must never fail the command, the
        // clip is already persisted.
        copy_text_to_clipboard(&md);
    }

    eprintln!(
        "\u{2713} Saved session answer(s) to clip {} \u{00B7} {} answer(s)",
        clip_id,
        owned.len()
    );
    Ok(())
}

/// Persist the rendered Markdown as a syncing text clip, mirroring the
/// `cinch push` storage path. Unlike push (which keeps clips `Local`), the
/// spec requires `SyncState::Pending` here so the clip syncs across devices —
/// that cross-device hop is the whole point of the command.
fn save_clip(md: &str, label: Option<String>, session: &Session) -> Result<String, ExitError> {
    let store_path = store::default_db_path().map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Could not determine local store path: {e}"),
            "",
        )
    })?;
    let store = Store::open(&store_path).map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Could not open local store: {e}"),
            "",
        )
    })?;

    let label = label.unwrap_or_else(|| derive_label(session));
    let data = md.as_bytes().to_vec();
    let byte_size = data.len() as i64;

    let clip_id = ulid::Ulid::new().to_string();
    let stored = StoredClip {
        id: clip_id.clone(),
        source: format!("remote:{}", hostname_or_unknown()),
        label: Some(label),
        content_type: ContentType::Text.as_wire().to_string(),
        content: Some(data),
        byte_size,
        created_at: chrono::Utc::now().timestamp_millis(),
        // Deliberate deviation from `cinch push`: Pending so the backlog
        // flusher syncs this clip to other devices.
        sync_state: SyncState::Pending,
        ..Default::default()
    };

    queries::insert_clip(&store, &stored)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Local store write failed: {e}"), ""))?;

    // Wake the background flusher (identical to the push path).
    let signal_path = store_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join("local_push.signal");
    let _ = std::fs::write(&signal_path, b"1");

    Ok(clip_id)
}

/// Default clip label: the session title, else its id, capped at [`LABEL_MAX`].
fn derive_label(session: &Session) -> String {
    let raw = session.title.clone().unwrap_or_else(|| session.id.clone());
    if raw.chars().count() <= LABEL_MAX {
        raw
    } else {
        raw.chars().take(LABEL_MAX).collect()
    }
}

/// One session row in the `--pick` list. The preview pane shows a parsed
/// overview (title + per-answer prompt table) so a session is recognizable.
struct SessionItem {
    path: PathBuf,
    label: String,
}

impl SkimItem for SessionItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.label)
    }
    fn preview(&self, _ctx: PreviewContext) -> ItemPreview {
        ItemPreview::Text(session_overview(&self.path))
    }
}

/// One answer row in the picker. The preview pane shows the exact Markdown that
/// would be copied for this answer (current render flags applied).
struct AnswerItem {
    index: usize,
    label: String,
    preview_md: String,
}

impl SkimItem for AnswerItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.label)
    }
    fn preview(&self, _ctx: PreviewContext) -> ItemPreview {
        ItemPreview::Text(self.preview_md.clone())
    }
}

/// Render a one-screen overview of a session for the `--pick` preview pane:
/// the title, id, answer count, and a numbered table of each answer's prompt.
fn session_overview(path: &Path) -> String {
    let source = ClaudeSource::new();
    // `SessionSelector::Path` ignores the cwd, so any path works here.
    match source.load(Path::new(""), &SessionSelector::Path(path.to_path_buf())) {
        Ok(s) => {
            let title = s.title.clone().unwrap_or_else(|| s.id.clone());
            let mut out = format!("{title}\n{}\n\n{} answer(s)\n\n", s.id, s.answers.len());
            for (i, a) in s.answers.iter().enumerate() {
                out.push_str(&format!("{:>3}. {}\n", i + 1, a.preview()));
            }
            out
        }
        Err(e) => format!("(failed to load session: {e})"),
    }
}

/// Run skim over `items` with a right-side preview pane and return the selected
/// ones. `multi` enables TAB multi-select. Errors on abort / empty selection.
fn run_skim(
    items: Vec<Arc<dyn SkimItem>>,
    multi: bool,
    prompt: &str,
) -> Result<Vec<Arc<dyn SkimItem>>, ExitError> {
    let options = SkimOptionsBuilder::default()
        .multi(multi)
        .prompt(prompt.to_string())
        .height("100%".to_string())
        // A non-None `preview` enables the preview pane; each item supplies its
        // own content via `SkimItem::preview`, so this command string is unused.
        .preview(Some(String::new()))
        .preview_window("right:60%:wrap".to_string())
        .build()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Picker init failed: {e}"), ""))?;

    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
    for item in items {
        let _ = tx.send(item);
    }
    drop(tx);

    let out = Skim::run_with(&options, Some(rx))
        .ok_or_else(|| ExitError::new(GENERIC_ERROR, "Picker failed to start.", ""))?;
    if out.is_abort {
        return Err(ExitError::new(GENERIC_ERROR, "Selection cancelled.", ""));
    }
    if out.selected_items.is_empty() {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "Nothing selected.",
            "Press Enter to pick (TAB toggles multi-select).",
        ));
    }
    Ok(out.selected_items)
}

/// Interactive session picker (skim, TTY-only). Returns a resolved
/// [`SessionSelector::Path`].
fn pick_session(source: &ClaudeSource, cwd: &Path) -> Result<SessionSelector, ExitError> {
    if !std::io::stdin().is_terminal() {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "--pick requires an interactive terminal.",
            "Pass a SESSION id prefix instead of --pick for non-interactive use.",
        ));
    }

    let sessions = source.list_sessions(cwd).map_err(map_session_err)?;
    if sessions.is_empty() {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "No sessions found for this directory.",
            "Run this inside a directory that has a Claude Code session.",
        ));
    }

    let items: Vec<Arc<dyn SkimItem>> = sessions
        .iter()
        .map(|s| {
            let title = s.title.clone().unwrap_or_else(|| s.id.clone());
            let id_prefix: String = s.id.chars().take(8).collect();
            Arc::new(SessionItem {
                path: s.path.clone(),
                label: format!("{title}  \u{00B7}  {id_prefix}"),
            }) as Arc<dyn SkimItem>
        })
        .collect();

    let selected = run_skim(items, false, "session> ")?;
    let item = selected[0]
        .as_any()
        .downcast_ref::<SessionItem>()
        .ok_or_else(|| ExitError::new(GENERIC_ERROR, "Picker returned an unexpected item.", ""))?;
    Ok(SessionSelector::Path(item.path.clone()))
}

/// Interactive answer picker (skim, multi-select, TTY-only). Returns deduped,
/// ascending 0-based indices. `opts` drives the live preview rendering so the
/// preview pane matches what would be copied.
fn pick_answers(answers: &[Answer], opts: RenderOpts) -> Result<Vec<usize>, ExitError> {
    let items: Vec<Arc<dyn SkimItem>> = answers
        .iter()
        .enumerate()
        .map(|(i, a)| {
            Arc::new(AnswerItem {
                index: i,
                label: format!("[#{}] {}", i + 1, a.preview()),
                preview_md: markdown(std::slice::from_ref(a), opts),
            }) as Arc<dyn SkimItem>
        })
        .collect();

    let selected = run_skim(items, true, "answer> ")?;
    let mut indices: Vec<usize> = selected
        .iter()
        .filter_map(|it| it.as_any().downcast_ref::<AnswerItem>().map(|ai| ai.index))
        .collect();
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}

/// Map a [`client_core::session::SessionError`] onto a CLI [`ExitError`] with a
/// helpful fix hint.
fn map_session_err(e: client_core::session::SessionError) -> ExitError {
    use client_core::session::SessionError;
    match e {
        SessionError::NoSessions(_) => ExitError::new(
            GENERIC_ERROR,
            e.to_string(),
            "Run this inside a directory that has a Claude Code session, or pass --pick.",
        ),
        SessionError::NotFound(_) => ExitError::new(
            GENERIC_ERROR,
            e.to_string(),
            "No session matches that id prefix.",
        ),
        SessionError::NoHome | SessionError::Io(_) | SessionError::Json(_) => {
            ExitError::new(GENERIC_ERROR, e.to_string(), "")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use client_core::session::{AnswerPart, Prompt};

    #[derive(Debug, Parser)]
    #[command(no_binary_name = true)]
    struct SessionHarness {
        #[command(flatten)]
        args: Args,
    }

    fn copy_args(harness: SessionHarness) -> CopyArgs {
        let Cmd::Copy(a) = harness.args.cmd;
        a
    }

    #[test]
    fn parses_last_with_explicit_count() {
        let h = SessionHarness::try_parse_from(["copy", "--last", "3"]).expect("parse ok");
        assert_eq!(copy_args(h).last, Some(3));
    }

    #[test]
    fn parses_bare_last_as_one() {
        // `--last` with no value resolves to N=1 via default_missing_value.
        let h = SessionHarness::try_parse_from(["copy", "--last"]).expect("parse ok");
        assert_eq!(copy_args(h).last, Some(1));
    }

    #[test]
    fn parses_all_stdout_no_tools_flags() {
        let h = SessionHarness::try_parse_from(["copy", "--all", "--stdout", "--no-tools"])
            .expect("parse ok");
        let a = copy_args(h);
        assert!(a.all);
        assert!(a.stdout);
        assert!(a.no_tools);
        assert!(a.last.is_none());
    }

    #[test]
    fn all_conflicts_with_last() {
        let result = SessionHarness::try_parse_from(["copy", "--all", "--last", "2"]);
        assert!(result.is_err(), "expected --all/--last conflict to error");
    }

    #[test]
    fn from_defaults_to_claude() {
        let h = SessionHarness::try_parse_from(["copy"]).expect("parse ok");
        assert_eq!(copy_args(h).from, "claude");
    }

    // --- derive_label -------------------------------------------------------

    fn session_with(title: Option<&str>, id: &str) -> Session {
        Session {
            id: id.to_string(),
            title: title.map(String::from),
            path: std::path::PathBuf::from("/tmp/x.jsonl"),
            answers: Vec::new(),
        }
    }

    #[test]
    fn derive_label_prefers_title() {
        let s = session_with(Some("Fix the auth bug"), "abc-123");
        assert_eq!(derive_label(&s), "Fix the auth bug");
    }

    #[test]
    fn derive_label_falls_back_to_id() {
        let s = session_with(None, "abc-123");
        assert_eq!(derive_label(&s), "abc-123");
    }

    #[test]
    fn derive_label_caps_length() {
        let long = "x".repeat(200);
        let s = session_with(Some(&long), "id");
        assert_eq!(derive_label(&s).chars().count(), LABEL_MAX);
    }

    // --- save_clip ----------------------------------------------------------

    #[test]
    fn save_clip_writes_pending_text_clip() {
        // Use an in-memory store directly (save_clip itself opens the default
        // on-disk store, so exercise the storage shape via queries here).
        let store = Store::open(Path::new(":memory:")).unwrap();
        let md = "## Assistant\n\nhello\n";
        let data = md.as_bytes().to_vec();
        let stored = StoredClip {
            id: ulid::Ulid::new().to_string(),
            source: format!("remote:{}", hostname_or_unknown()),
            label: Some("title".to_string()),
            content_type: ContentType::Text.as_wire().to_string(),
            content: Some(data.clone()),
            byte_size: data.len() as i64,
            created_at: chrono::Utc::now().timestamp_millis(),
            sync_state: SyncState::Pending,
            ..Default::default()
        };
        queries::insert_clip(&store, &stored).unwrap();

        let fetched = queries::get_clip(&store, &stored.id).unwrap().unwrap();
        assert_eq!(fetched.content_type, "text");
        assert_eq!(fetched.sync_state, SyncState::Pending);
        assert_eq!(fetched.content.as_deref(), Some(md.as_bytes()));
    }

    // --- map_session_err ----------------------------------------------------

    #[test]
    fn map_session_err_no_sessions_hints_pick() {
        use client_core::session::SessionError;
        let err = map_session_err(SessionError::NoSessions("/tmp".to_string()));
        assert_eq!(err.code, GENERIC_ERROR);
        assert!(err.fix.contains("--pick"));
    }

    // Touch AnswerPart / Prompt so the renderer-facing imports stay exercised
    // and the test module compiles against the public session surface.
    #[test]
    fn answer_preview_uses_prompt() {
        let a = Answer {
            index: 0,
            prompt: Some(Prompt {
                text: "do the thing".to_string(),
            }),
            parts: vec![AnswerPart::Text("done".to_string())],
        };
        assert!(a.preview().contains("do the thing"));
    }

    #[test]
    fn answer_item_exposes_text_and_preview() {
        use skim::prelude::{ItemPreview, PreviewContext, SkimItem};
        let item = AnswerItem {
            index: 2,
            label: "[#3] do the thing".to_string(),
            preview_md: "## Assistant\n\ndone\n".to_string(),
        };
        assert_eq!(item.text().as_ref(), "[#3] do the thing");
        let ctx = PreviewContext {
            query: "",
            cmd_query: "",
            width: 80,
            height: 24,
            current_index: 0,
            current_selection: "",
            selected_indices: &[],
            selections: &[],
        };
        match item.preview(ctx) {
            ItemPreview::Text(body) => assert!(body.contains("done")),
            _ => panic!("expected a text preview"),
        }
    }
}
