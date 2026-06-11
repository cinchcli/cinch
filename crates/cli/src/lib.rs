//! `cinch_cli` — Cinch CLI library entrypoint.
//!
//! The standalone `cinch` binary is a thin wrapper around [`run`]. The
//! desktop bundle re-uses this entrypoint via the `builtin-cli` feature so
//! macOS/Windows users get both the CLI and the desktop app from a single
//! install.

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

mod auth_state;
mod client_info;
mod commands;
mod desktop_handoff;
mod exit;
mod fmt;
mod io;
mod key_state;
#[cfg(target_os = "macos")]
mod macos_pasteboard;
mod runtime;
mod telemetry;
mod update;
mod update_check;

#[derive(Parser)]
#[command(
    name = "cinch",
    about = "Pipe remote output to your local clipboard. Instantly.",
    version,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Save stdin (or INPUT) to your LOCAL clip history. Never contacts the relay. (pbcopy-shaped)
    Copy(commands::copy::Args),
    /// Print a LOCAL clip to stdout (latest by default). (pbpaste-shaped)
    Paste(commands::paste::Args),
    /// Send stdin to your fleet via the relay — E2EE, broadcast to all your devices.
    Send(commands::send::Args),
    /// Pull clipboard content to stdout.
    Pull(commands::pull::Args),
    /// Edit a LOCAL clip's text in $EDITOR and save the result as a new clip.
    Edit(commands::edit::Args),
    /// AI workflows over explicit terminal or clipboard context.
    Ai(commands::ai::Args),
    /// Browse, search, and manage your LOCAL clip history (list/search/show/rm/transform).
    History(commands::history::Args),
    /// Copy answer(s) from an agent coding session to a clip + the clipboard.
    Session(commands::session::Args),
    /// Pin a clip (fleet + local). --local to pin locally only.
    Pin(commands::pin::Args),
    /// Unpin a clip (fleet + local). --local to unpin locally only.
    Unpin(commands::unpin::Args),
    /// Manage the machines paired to your account = your fleet.
    Fleet(commands::fleet::Args),
    /// Manage authentication.
    Auth(commands::auth::Args),
    /// Account-level commands: plan tier + telemetry preference.
    Account(commands::account::Args),
    /// Administer this relay (self-host operators only).
    Admin(commands::admin::Args),
    /// Print a shell completion script to stdout.
    ///
    /// Example: cinch completion zsh > ~/.zsh/completions/_cinch
    Completion {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Check for a newer release and, with confirmation, install it.
    Update(update::UpdateArgs),
    /// Run a read-only MCP server over your local clipboard (stdio).
    Mcp(commands::mcp::Args),
    /// Resolve a named clip from the project's cinch.yaml onto your clipboard.
    #[command(name = "use")]
    Use(commands::use_::Args),
    // --- hidden deprecated aliases (0.5–0.7 runway, removed in 0.8) ---
    /// (deprecated) `clip *` → `history *`. Hidden; prints one note and routes
    /// to the new handler.
    #[command(hide = true)]
    Clip(commands::clip::Args),
    /// (deprecated) `device *` → `fleet *`. Hidden; prints one note and routes
    /// to the new handler.
    #[command(hide = true)]
    Device(commands::device::Args),
    /// REMOVED in 0.5. Bare `cinch push` changed meaning, so it now hard-errors
    /// with a did-you-mean (copy/send) and never silently saves or sends. Kept
    /// as a hidden variant so old invocations route to that error, not a clap
    /// "unknown subcommand".
    #[command(hide = true)]
    Push(commands::push::Args),
    /// REMOVED: renamed to `cinch update`. Hidden; routes to a hard error so
    /// old `cinch self-update` invocations get a clear redirect, not a clap
    /// "unknown subcommand".
    #[command(hide = true)]
    SelfUpdate(update::RemovedArgs),
}

/// The basename the binary was invoked under, normalized to one of our two
/// supported command names. Invoked via the `ci` symlink → `"ci"`; anything
/// else (including `cinch` and odd argv[0]s) → `"cinch"`.
fn invoked_bin_name() -> &'static str {
    let arg0 = std::env::args_os().next();
    let base = arg0
        .as_deref()
        .map(std::path::Path::new)
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .map(|s| s.strip_suffix(".exe").unwrap_or(s));
    match base {
        Some("ci") => "ci",
        _ => "cinch",
    }
}

/// The hand-written device-name completion override, keyed to `name`
/// (`cinch` or `ci`) so the two completion files never collide when sourced.
fn completion_override(shell: Shell, name: &str) -> String {
    let template = match shell {
        Shell::Zsh => ZSH_FROM_OVERRIDE,
        Shell::Bash => BASH_FROM_OVERRIDE,
        Shell::Fish => FISH_FROM_OVERRIDE,
        _ => return String::new(),
    };
    template.replace("{bin}", name)
}

/// Render the full completion script for `name`: clap's AOT output (hidden
/// subcommands stripped) plus the dynamic override, both keyed to `name`.
fn render_completion(shell: Shell, name: &str) -> String {
    let full = Cli::command();
    // AOT generators don't skip `hide = true` subcommands; rebuild the tree
    // without them so deprecated aliases never appear as candidates (§4d).
    let mut cmd = strip_hidden_subcommands(&full).version(env!("CARGO_PKG_VERSION"));
    let mut buf: Vec<u8> = Vec::new();
    clap_complete::generate(shell, &mut cmd, name, &mut buf);
    let mut out = String::from_utf8(buf).expect("clap completion output is valid UTF-8");
    out.push_str(&completion_override(shell, name));
    out
}

/// Rebuild `cmd` keeping only **non-hidden** subcommands, recursively.
///
/// clap_complete's AOT generators (bash/zsh/fish) iterate `get_subcommands()`
/// without honoring `hide = true` (only the dynamic engine filters hidden), so
/// the deprecated aliases (`clip`/`device`/`push`, `pin add/rm/list`, `auth
/// set-name`) would otherwise leak into generated completions. clap exposes no
/// subcommand-removal API, but `Arg`/`Command` are `Clone`, so we reconstruct
/// the tree minus hidden children. Completions therefore emit ONLY the new
/// names from 0.5 (redesign §4b/§4d).
///
/// Args are cloned (preserving value hints / possible-values, so e.g.
/// `completion <shell>` still completes shell names). The auto `help`/`version`
/// args are skipped so clap re-adds them on `build()` rather than panicking on
/// a duplicate.
fn strip_hidden_subcommands(cmd: &clap::Command) -> clap::Command {
    // `Command::new` wants `Into<Str>`; `Str: From<&'static str>` is the one
    // conversion guaranteed across clap_builder versions. Completion generation
    // runs once and the process exits immediately after, so leaking the handful
    // of small command-name strings is free.
    let name: &'static str = Box::leak(cmd.get_name().to_string().into_boxed_str());
    let mut rebuilt = clap::Command::new(name);
    if let Some(about) = cmd.get_about() {
        rebuilt = rebuilt.about(about.clone());
    }
    for arg in cmd.get_arguments() {
        let id = arg.get_id().as_str();
        if id == "help" || id == "version" {
            continue;
        }
        rebuilt = rebuilt.arg(arg.clone());
    }
    for sub in cmd.get_subcommands().filter(|s| !s.is_hide_set()) {
        rebuilt = rebuilt.subcommand(strip_hidden_subcommands(sub));
    }
    rebuilt
}

// Appended after clap_complete's static output.
// Teaches the shell to complete device-name values (`pull --from`)
// with `{bin} fleet list --names` (the post-0.5 name; `device` is a hidden
// deprecated alias and must not appear in generated completions, §4d).
// `{bin}` is substituted at runtime with the invoked binary name (cinch or ci).

const ZSH_FROM_OVERRIDE: &str = r#"
# {bin} device-name dynamic completion
_{bin}_devices_names() {
  local -a devs
  devs=( ${(f)"$({bin} fleet list --names 2>/dev/null)"} )
  _describe 'device' devs
}
# clap_complete inlines subcommands inside _{bin}, so we rename it
# and wrap with a version that intercepts device-name completion.
# `compset -P '--flag='` strips the `--flag=` prefix from $PREFIX so
# candidates match against the value portion; it's a no-op when the
# current word has no `=`, so it works for both `--from <tab>` and
# `--from=<tab>`.
functions[_{bin}_generated]=$functions[_{bin}]
_{bin}() {
  if [[ ${words[2]} == pull ]] && \
     [[ ${words[CURRENT-1]} == --from || ${words[CURRENT]} == --from=* ]]; then
    compset -P '--from='
    _{bin}_devices_names
    return
  fi
  _{bin}_generated "$@"
}
"#;

const BASH_FROM_OVERRIDE: &str = r#"
# {bin} device-name dynamic completion.
# Bash's default COMP_WORDBREAKS contains `=`, so `--from=foo` is split
# into three tokens (`--from`, `=`, `foo`). Detect both forms by also
# inspecting the token two slots back when the previous token is `=`.
_{bin}_devices_names() {
  local word="${COMP_WORDS[COMP_CWORD]}"
  mapfile -t COMPREPLY < <({bin} fleet list --names 2>/dev/null | grep -- "^${word}")
}
_{bin}_with_from() {
  local cur prev prev2
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"
  prev2="${COMP_WORDS[COMP_CWORD-2]:-}"
  if [[ "$prev" == "--from" ]]; then
    _{bin}_devices_names
    return
  fi
  if [[ "$prev" == "=" && "$prev2" == "--from" ]]; then
    _{bin}_devices_names
    return
  fi
  _{bin}
}
complete -F _{bin}_with_from {bin}
"#;

const FISH_FROM_OVERRIDE: &str = r#"
# {bin} device-name dynamic completion
complete -c {bin} -n '__fish_seen_subcommand_from pull' -l from -f \
  -d 'Device nickname or hostname' \
  -a '({bin} fleet list --names 2>/dev/null)'
"#;

fn command_name(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Copy(_) => "copy",
        Cmd::Paste(_) => "paste",
        Cmd::Send(_) => "send",
        Cmd::Push(_) => "push",
        Cmd::Pull(_) => "pull",
        Cmd::Edit(_) => "edit",
        Cmd::Ai(_) => "ai",
        Cmd::History(_) => "history",
        Cmd::Clip(_) => "clip",
        Cmd::Session(_) => "session",
        Cmd::Pin(_) => "pin",
        Cmd::Unpin(_) => "unpin",
        Cmd::Fleet(_) => "fleet",
        Cmd::Device(_) => "device",
        Cmd::Auth(_) => "auth",
        Cmd::Account(_) => "account",
        Cmd::Admin(_) => "admin",
        Cmd::Completion { .. } => "completion",
        Cmd::Update(_) => "update",
        Cmd::SelfUpdate(_) => "self-update",
        Cmd::Mcp(_) => "mcp",
        Cmd::Use(_) => "use",
    }
}

fn is_ai_cmd(cmd: &Cmd) -> bool {
    matches!(cmd, Cmd::Ai(_))
}

/// Returns true when the invocation is `cinch account telemetry ...`. Used to
/// suppress telemetry initialization for the meta-command that inspects /
/// toggles telemetry itself — otherwise running `cinch account telemetry
/// status` would create the distinct_id file as a side effect.
fn is_telemetry_cmd(cmd: &Cmd) -> bool {
    matches!(
        cmd,
        Cmd::Account(args) if matches!(args.cmd, commands::account::Cmd::Telemetry(_))
    )
}

fn print_first_run_welcome() {
    eprintln!("Welcome to Cinch — pipe clipboard between machines.");
    eprintln!();
    eprintln!("Get started:");
    eprintln!("  cinch auth login              Sign in via browser");
    eprintln!("  echo \"hello\" | cinch copy     Save to local history");
    eprintln!("  echo \"hello\" | cinch send     Send to your fleet (E2EE)");
    eprintln!("  cinch pull                    Receive the latest fleet clip");
    eprintln!();
    eprintln!("Setting up a remote machine? From your laptop, run:");
    eprintln!("  cinch pair user@host");
    eprintln!();
    eprintln!("Docs: https://cinchcli.com/docs/");
    eprintln!();
}

/// Library entrypoint. Returns the process exit code: `0` on success,
/// the `ExitError::code` on failure. The standalone `cinch` binary and
/// the desktop binary (when invoked as `cinch`) both call this.
pub fn run() -> i32 {
    if std::env::args().len() == 1 && !auth_state::is_authenticated() {
        print_first_run_welcome();
        // Fall through — clap's `arg_required_else_help = true` will
        // print the usage block and exit with code 2.
    }
    let cli = Cli::parse();

    if let Cmd::Completion { shell } = cli.cmd {
        // Key completions to the name we were invoked as, so the `ci` symlink
        // gets a working `_ci` script while `cinch` is byte-for-byte unchanged.
        print!("{}", render_completion(shell, invoked_bin_name()));
        return 0;
    }

    // Quiet path: MCP is a read-only stdio server. It must run before
    // telemetry/update-check/session-flush and without the tokio runtime,
    // or stray stdout/stderr would corrupt the JSON-RPC stream.
    if matches!(cli.cmd, Cmd::Mcp(_)) {
        let Cmd::Mcp(args) = cli.cmd else {
            unreachable!()
        };
        return match commands::mcp::run(args) {
            Ok(()) => exit::SUCCESS,
            Err(e) => {
                e.print_stderr();
                e.code
            }
        };
    }

    // Skip telemetry init for the `cinch account telemetry` meta-command so
    // that inspecting/toggling state does not itself create the distinct_id
    // file or print the first-run notice.
    //
    // Also keep `cinch ai` free of background network side effects. AI commands
    // have their own explicit provider boundary; `--no-send` must not trigger
    // telemetry, update checks, relay backfills, or any other network call.
    let ai_cmd = is_ai_cmd(&cli.cmd);
    let instrument = !is_telemetry_cmd(&cli.cmd) && !ai_cmd;
    if instrument {
        telemetry::init();
    }
    let cmd_label = command_name(&cli.cmd);
    let started_at = std::time::Instant::now();
    if instrument {
        telemetry::capture(telemetry::Event::new("cli.command.invoked").with("command", cmd_label));
        // Drain any MCP session counter files (§7 item 2): the quiet MCP path
        // can't emit telemetry itself, so the next ordinary CLI invocation
        // emits `mcp.session.completed` for it. Best-effort; never blocks.
        commands::mcp::metrics::drain_and_emit();
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("could not start tokio runtime");
    let result = rt.block_on(async {
        // T1: best-effort backlog flush on every CLI session start.
        // No-op if not authenticated, missing encryption key, or another
        // flush is in flight. Detached — never delays the user's command.
        if !ai_cmd {
            if let Ok(ctx) = crate::runtime::open_ctx() {
                crate::runtime::spawn_session_flush(&ctx);
            }
        }
        let cmd_result = match cli.cmd {
            Cmd::Copy(args) => commands::copy::run(args).await,
            Cmd::Paste(args) => commands::paste::run(args).await,
            Cmd::Send(args) => commands::send::run(args).await,
            Cmd::Push(args) => commands::push::run(args).await,
            Cmd::Pull(args) => commands::pull::run(args).await,
            Cmd::Edit(args) => commands::edit::run(args).await,
            Cmd::Ai(args) => commands::ai::run(args).await,
            Cmd::History(args) => commands::history::run(args).await,
            Cmd::Clip(args) => commands::clip::run(args).await,
            Cmd::Session(args) => commands::session::run(args).await,
            Cmd::Pin(args) => commands::pin::run(args).await,
            Cmd::Unpin(args) => commands::unpin::run(args).await,
            Cmd::Fleet(args) => commands::fleet::run(args).await,
            Cmd::Device(args) => commands::device::run(args).await,
            Cmd::Auth(args) => commands::auth::run(args).await,
            Cmd::Account(args) => commands::account::run(args).await,
            Cmd::Admin(args) => commands::admin::run(args).await,
            Cmd::Update(args) => update::run_update(args).await,
            Cmd::SelfUpdate(_) => update::run_removed_self_update().await,
            Cmd::Use(args) => commands::use_::run(args).await,
            Cmd::Completion { .. } => unreachable!(),
            Cmd::Mcp(_) => unreachable!(),
        };
        // Best-effort update notifier: never delays user-facing output by >300ms,
        // never affects exit status, never surfaces its errors. Replaces the
        // older `update_check::check_self_outdated` polling path.
        if !ai_cmd {
            let _ = tokio::time::timeout(
                std::time::Duration::from_millis(300),
                update::notifier::maybe_notify(),
            )
            .await;
        }
        if instrument {
            let duration_ms = started_at.elapsed().as_millis() as u64;
            let (success, exit_code) = match &cmd_result {
                Ok(_) => (true, 0_i32),
                Err(err) => (false, err.code),
            };
            telemetry::capture(
                telemetry::Event::new("cli.command.completed")
                    .with("command", cmd_label)
                    .with("success", success)
                    .with("duration_ms", duration_ms)
                    .with("exit_code", exit_code),
            );
            telemetry::flush(std::time::Duration::from_secs(2)).await;
        }
        cmd_result
    });
    match result {
        Ok(_) => 0,
        Err(err) => {
            err.print_stderr();
            err.code
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{completion_override, render_completion};
    use clap_complete::Shell;

    #[test]
    fn cinch_completion_keeps_existing_tokens() {
        let z = render_completion(Shell::Zsh, "cinch");
        assert!(z.contains("_cinch_generated"));
        assert!(z.contains("_cinch_devices_names"));
        let b = render_completion(Shell::Bash, "cinch");
        assert!(b.contains("complete -F _cinch_with_from cinch"));
        let f = render_completion(Shell::Fish, "cinch");
        assert!(f.contains("complete -c cinch"));
    }

    #[test]
    fn ci_completion_is_namespaced_to_ci() {
        let z = render_completion(Shell::Zsh, "ci");
        assert!(z.contains("_ci_generated"));
        assert!(z.contains("_ci_devices_names"));
        // The cinch-keyed override fns must NOT leak, or sourcing both the
        // _cinch and _ci files would collide.
        assert!(!z.contains("_cinch_generated"));
        let b = render_completion(Shell::Bash, "ci");
        assert!(b.contains("complete -F _ci_with_from ci"));
        assert!(!b.contains("_cinch_with_from"));
        let f = render_completion(Shell::Fish, "ci");
        assert!(f.contains("complete -c ci"));
        assert!(!f.contains("complete -c cinch"));
    }

    #[test]
    fn override_is_empty_for_unsupported_shell() {
        assert_eq!(completion_override(Shell::Elvish, "ci"), "");
    }
}

#[cfg(test)]
mod redesign_tests {
    //! 0.5 command-surface redesign — top-level routing/parse matrix (§4d).
    //! Behavioral assertions (the one-shot deprecation note, push hard-error,
    //! completions) live in the process-level `tests/deprecation.rs`; these
    //! prove every old spelling still *parses* to its (hidden) alias variant
    //! and that the two collapsing merges keep their flags.
    use super::*;

    fn parse(argv: &[&str]) -> Cmd {
        Cli::try_parse_from(argv).expect("argv should parse").cmd
    }

    // --- new spellings parse ------------------------------------------------

    #[test]
    fn new_top_level_verbs_parse() {
        assert!(matches!(
            parse(&["cinch", "history", "list"]),
            Cmd::History(_)
        ));
        assert!(matches!(parse(&["cinch", "fleet", "list"]), Cmd::Fleet(_)));
        assert!(matches!(parse(&["cinch", "unpin", "abcd"]), Cmd::Unpin(_)));
        // New `pin <REF>` (no legacy subcommand).
        match parse(&["cinch", "pin", "abcd"]) {
            Cmd::Pin(a) => {
                assert!(a.legacy.is_none());
                assert_eq!(a.reference.as_deref(), Some("abcd"));
            }
            _ => panic!("expected Pin"),
        }
    }

    #[test]
    fn bare_history_and_fleet_parse_without_subcommand() {
        assert!(matches!(parse(&["cinch", "history"]), Cmd::History(_)));
        assert!(matches!(parse(&["cinch", "fleet"]), Cmd::Fleet(_)));
    }

    // --- the ~16 deprecated spellings still PARSE (route to hidden aliases) --

    #[test]
    fn deprecated_clip_group_parses() {
        for sub in [
            &["cinch", "clip", "list"][..],
            &["cinch", "clip", "search", "q"][..],
            &["cinch", "clip", "get", "abcd"][..],
            &["cinch", "clip", "rm", "abcd"][..],
            &["cinch", "clip", "transform", "abcd"][..],
        ] {
            assert!(matches!(parse(sub), Cmd::Clip(_)), "argv {sub:?}");
        }
    }

    #[test]
    fn deprecated_device_group_parses() {
        for sub in [
            &["cinch", "device", "list"][..],
            &["cinch", "device", "pair", "user@host"][..],
            &["cinch", "device", "set-name", "MyMac"][..],
            &["cinch", "device", "nickname", "01J", "box"][..],
            &["cinch", "device", "retention"][..],
            &["cinch", "device", "revoke", "abcd"][..],
            &["cinch", "device", "sources"][..],
        ] {
            assert!(matches!(parse(sub), Cmd::Device(_)), "argv {sub:?}");
        }
    }

    #[test]
    fn deprecated_pin_subforms_parse() {
        match parse(&["cinch", "pin", "add", "abcd"]) {
            Cmd::Pin(a) => assert!(matches!(a.legacy, Some(commands::pin::Legacy::Add(_)))),
            _ => panic!("expected Pin/Add"),
        }
        match parse(&["cinch", "pin", "rm", "abcd"]) {
            Cmd::Pin(a) => assert!(matches!(a.legacy, Some(commands::pin::Legacy::Rm(_)))),
            _ => panic!("expected Pin/Rm"),
        }
        match parse(&["cinch", "pin", "list"]) {
            Cmd::Pin(a) => assert!(matches!(a.legacy, Some(commands::pin::Legacy::List(_)))),
            _ => panic!("expected Pin/List"),
        }
    }

    #[test]
    fn deprecated_auth_set_name_parses() {
        match parse(&["cinch", "auth", "set-name", "MyMac"]) {
            Cmd::Auth(a) => assert!(matches!(a.cmd, commands::auth::Cmd::SetName { .. })),
            _ => panic!("expected Auth/SetName"),
        }
    }

    #[test]
    fn update_parses_with_flags() {
        assert!(matches!(parse(&["cinch", "update"]), Cmd::Update(_)));
        assert!(matches!(
            parse(&["cinch", "update", "--check"]),
            Cmd::Update(_)
        ));
        match parse(&["cinch", "update", "-y", "--force"]) {
            Cmd::Update(a) => {
                assert!(a.yes && a.force && !a.check);
            }
            _ => panic!("expected Update"),
        }
    }

    #[test]
    fn removed_self_update_still_parses_to_hidden_variant() {
        assert!(matches!(
            parse(&["cinch", "self-update"]),
            Cmd::SelfUpdate(_)
        ));
        assert!(matches!(
            parse(&["cinch", "self-update", "--check"]),
            Cmd::SelfUpdate(_)
        ));
    }

    #[test]
    fn removed_push_still_routes_to_hidden_variant() {
        // Hard-error happens at run-time; here we prove `push` parses to the
        // hidden variant rather than clap rejecting it as unknown.
        assert!(matches!(parse(&["cinch", "push"]), Cmd::Push(_)));
    }

    // --- the two collapsing-merge cases keep their flags (§4d) --------------

    #[test]
    fn merge_clip_get_meta_preserves_meta_flag() {
        // `clip get --meta` → `history show --meta`: the --meta flag survives
        // because the alias reuses the same `get::Args`.
        match parse(&["cinch", "clip", "get", "abcd", "--meta"]) {
            Cmd::Clip(a) => match a.cmd {
                commands::clip::Cmd::Get(g) => {
                    assert_eq!(g.id_or_index, "abcd");
                    assert!(g.meta, "--meta must survive the clip→history alias");
                }
                _ => panic!("expected clip get"),
            },
            _ => panic!("expected Clip"),
        }
    }

    #[test]
    fn merge_device_set_name_parses_self_targeting_form() {
        // `device set-name <NAME>` carries NO device positional (it targets
        // THIS machine); the `self` target is injected at dispatch
        // (device::run → fleet::run_rename("self", ...)).
        match parse(&["cinch", "device", "set-name", "MyMac"]) {
            Cmd::Device(a) => match a.cmd {
                commands::device::Cmd::SetName { name, clear } => {
                    assert_eq!(name.as_deref(), Some("MyMac"));
                    assert!(!clear);
                }
                _ => panic!("expected device set-name"),
            },
            _ => panic!("expected Device"),
        }
    }
}
