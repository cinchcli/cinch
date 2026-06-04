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
    /// Push stdin to your local clipboard.
    Push(commands::push::Args),
    /// Pull clipboard content to stdout.
    Pull(commands::pull::Args),
    /// Send stdin to all your devices (relay broadcast + local + clipboard).
    Send(commands::send::Args),
    /// AI workflows over explicit terminal or clipboard context.
    Ai(commands::ai::Args),
    /// Operate on clips: list, search, get, rm.
    Clip(commands::clip::Args),
    /// Pin / unpin clips and list pinned clips.
    Pin(commands::pin::Args),
    /// Manage paired devices on this account.
    Device(commands::device::Args),
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
    /// Download and install the latest release (for manually-placed binaries).
    SelfUpdate(update::SelfUpdateArgs),
    /// Run a read-only MCP server over your local clipboard (stdio).
    Mcp(commands::mcp::Args),
}

fn print_completion_override(shell: Shell) {
    match shell {
        Shell::Zsh => print!("{}", ZSH_FROM_OVERRIDE),
        Shell::Bash => print!("{}", BASH_FROM_OVERRIDE),
        Shell::Fish => print!("{}", FISH_FROM_OVERRIDE),
        _ => {}
    }
}

// Appended after clap_complete's static output.
// Teaches the shell to complete device-name values (`pull --from`)
// with `cinch device list --names`.

const ZSH_FROM_OVERRIDE: &str = r#"
# cinch device-name dynamic completion
_cinch_devices_names() {
  local -a devs
  devs=( ${(f)"$(cinch device list --names 2>/dev/null)"} )
  _describe 'device' devs
}
# clap_complete inlines subcommands inside _cinch, so we rename it
# and wrap with a version that intercepts device-name completion.
# `compset -P '--flag='` strips the `--flag=` prefix from $PREFIX so
# candidates match against the value portion; it's a no-op when the
# current word has no `=`, so it works for both `--from <tab>` and
# `--from=<tab>`.
functions[_cinch_generated]=$functions[_cinch]
_cinch() {
  if [[ ${words[2]} == pull ]] && \
     [[ ${words[CURRENT-1]} == --from || ${words[CURRENT]} == --from=* ]]; then
    compset -P '--from='
    _cinch_devices_names
    return
  fi
  _cinch_generated "$@"
}
"#;

const BASH_FROM_OVERRIDE: &str = r#"
# cinch device-name dynamic completion.
# Bash's default COMP_WORDBREAKS contains `=`, so `--from=foo` is split
# into three tokens (`--from`, `=`, `foo`). Detect both forms by also
# inspecting the token two slots back when the previous token is `=`.
_cinch_devices_names() {
  local word="${COMP_WORDS[COMP_CWORD]}"
  mapfile -t COMPREPLY < <(cinch device list --names 2>/dev/null | grep -- "^${word}")
}
_cinch_with_from() {
  local cur prev prev2
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"
  prev2="${COMP_WORDS[COMP_CWORD-2]:-}"
  if [[ "$prev" == "--from" ]]; then
    _cinch_devices_names
    return
  fi
  if [[ "$prev" == "=" && "$prev2" == "--from" ]]; then
    _cinch_devices_names
    return
  fi
  _cinch
}
complete -F _cinch_with_from cinch
"#;

const FISH_FROM_OVERRIDE: &str = r#"
# cinch device-name dynamic completion
complete -c cinch -n '__fish_seen_subcommand_from pull' -l from -f \
  -d 'Device nickname or hostname' \
  -a '(cinch device list --names 2>/dev/null)'
"#;

fn command_name(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Push(_) => "push",
        Cmd::Pull(_) => "pull",
        Cmd::Send(_) => "send",
        Cmd::Ai(_) => "ai",
        Cmd::Clip(_) => "clip",
        Cmd::Pin(_) => "pin",
        Cmd::Device(_) => "device",
        Cmd::Auth(_) => "auth",
        Cmd::Account(_) => "account",
        Cmd::Admin(_) => "admin",
        Cmd::Completion { .. } => "completion",
        Cmd::SelfUpdate(_) => "self-update",
        Cmd::Mcp(_) => "mcp",
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
    eprintln!("  echo \"hello\" | cinch push     Send your clipboard");
    eprintln!("  cinch pull                    Receive the latest clip");
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
        let mut cmd = Cli::command();
        let bin_name = cmd.get_name().to_string();
        clap_complete::generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
        print_completion_override(shell);
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
            Cmd::Push(args) => commands::push::run(args).await,
            Cmd::Pull(args) => commands::pull::run(args).await,
            Cmd::Send(args) => commands::send::run(args).await,
            Cmd::Ai(args) => commands::ai::run(args).await,
            Cmd::Clip(args) => commands::clip::run(args).await,
            Cmd::Pin(args) => commands::pin::run(args).await,
            Cmd::Device(args) => commands::device::run(args).await,
            Cmd::Auth(args) => commands::auth::run(args).await,
            Cmd::Account(args) => commands::account::run(args).await,
            Cmd::Admin(args) => commands::admin::run(args).await,
            Cmd::SelfUpdate(args) => update::run_self_update(args).await,
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
