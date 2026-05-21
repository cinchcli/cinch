//! `cinch_cli` — Cinch CLI library entrypoint.
//!
//! The standalone `cinch` binary is a thin wrapper around [`run`]. The
//! desktop bundle re-uses this entrypoint via the `builtin-cli` feature so
//! macOS/Windows users get both the CLI and the desktop app from a single
//! install.

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

pub mod client_info;
pub mod commands;
pub mod desktop_handoff;
pub mod exit;
pub mod fmt;
pub mod key_state;
pub mod runtime;
pub mod telemetry;
pub mod update;
pub mod update_check;

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
    /// Manage authentication.
    Auth(commands::auth::Args),
    /// Set up cinch on a remote machine via SSH.
    Pair(commands::pair::Args),
    /// Manage this device (e.g. set its display name).
    Device(commands::device::Args),
    /// List paired devices for this account.
    Devices(commands::devices::Args),
    /// Print a single clip's content by ID prefix.
    Get(commands::get::Args),
    /// List recent clips.
    List(commands::list::Args),
    /// Full-text search across the local clip store.
    Search(commands::search::Args),
    /// Pin a clip by ID prefix.
    Pin(commands::pin::Args),
    /// List pinned clips.
    Pinned(commands::pinned::Args),
    /// Unpin a clip by ID prefix.
    Unpin(commands::unpin::Args),
    /// Set or clear a device's nickname.
    Nickname(commands::nickname::Args),
    /// List distinct source machines that have pushed clips.
    Sources(commands::sources::Args),
    /// View or set per-device clip retention.
    Retention(commands::retention::Args),
    /// Revoke a paired device's token (asks for confirmation).
    Revoke(commands::revoke::Args),
    /// Administer this relay (self-host operators only).
    Admin(commands::admin::Args),
    /// Delete a clip by ID prefix (with TTY confirm unless --force).
    Rm(commands::rm::Args),
    /// View or change anonymous usage telemetry state.
    Telemetry(commands::telemetry::Args),
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
// Teaches the shell to complete device-name values (`pull --from`,
// `push --to`) with `cinch devices --names`.

const ZSH_FROM_OVERRIDE: &str = r#"
# cinch device-name dynamic completion
_cinch_devices_names() {
  local -a devs
  devs=( ${(f)"$(cinch devices --names 2>/dev/null)"} )
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
  if [[ ${words[2]} == push ]] && \
     [[ ${words[CURRENT-1]} == --to || ${words[CURRENT]} == --to=* ]]; then
    compset -P '--to='
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
  mapfile -t COMPREPLY < <(cinch devices --names 2>/dev/null | grep -- "^${word}")
}
_cinch_with_from() {
  local cur prev prev2
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"
  prev2="${COMP_WORDS[COMP_CWORD-2]:-}"
  if [[ "$prev" == "--from" || "$prev" == "--to" ]]; then
    _cinch_devices_names
    return
  fi
  if [[ "$prev" == "=" && ( "$prev2" == "--from" || "$prev2" == "--to" ) ]]; then
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
  -a '(cinch devices --names 2>/dev/null)'
complete -c cinch -n '__fish_seen_subcommand_from push' -l to -f \
  -d 'Device nickname or hostname' \
  -a '(cinch devices --names 2>/dev/null)'
"#;

fn command_name(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Push(_) => "push",
        Cmd::Pull(_) => "pull",
        Cmd::Auth(_) => "auth",
        Cmd::Pair(_) => "pair",
        Cmd::Device(_) => "device",
        Cmd::Devices(_) => "devices",
        Cmd::Get(_) => "get",
        Cmd::List(_) => "list",
        Cmd::Search(_) => "search",
        Cmd::Pin(_) => "pin",
        Cmd::Pinned(_) => "pinned",
        Cmd::Unpin(_) => "unpin",
        Cmd::Nickname(_) => "nickname",
        Cmd::Sources(_) => "sources",
        Cmd::Retention(_) => "retention",
        Cmd::Revoke(_) => "revoke",
        Cmd::Admin(_) => "admin",
        Cmd::Rm(_) => "rm",
        Cmd::Telemetry(_) => "telemetry",
        Cmd::Completion { .. } => "completion",
        Cmd::SelfUpdate(_) => "self-update",
    }
}

/// Library entrypoint. Returns the process exit code: `0` on success,
/// the `ExitError::code` on failure. The standalone `cinch` binary and
/// the desktop binary (when invoked as `cinch`) both call this.
pub fn run() -> i32 {
    let cli = Cli::parse();

    if let Cmd::Completion { shell } = cli.cmd {
        let mut cmd = Cli::command();
        let bin_name = cmd.get_name().to_string();
        clap_complete::generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
        print_completion_override(shell);
        return 0;
    }

    // Skip telemetry init for the `cinch telemetry` meta-command so that
    // inspecting/toggling state does not itself create the distinct_id file
    // or print the first-run notice.
    let instrument = !matches!(cli.cmd, Cmd::Telemetry(_));
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
        if let Ok(ctx) = crate::runtime::open_ctx() {
            crate::runtime::spawn_session_flush(&ctx);
        }
        let cmd_result = match cli.cmd {
            Cmd::Push(args) => commands::push::run(args).await,
            Cmd::Pull(args) => commands::pull::run(args).await,
            Cmd::Auth(args) => commands::auth::run(args).await,
            Cmd::Pair(args) => commands::pair::run(args).await,
            Cmd::Device(args) => commands::device::run(args).await,
            Cmd::Devices(args) => commands::devices::run(args).await,
            Cmd::Get(args) => commands::get::run(args).await,
            Cmd::List(args) => commands::list::run(args).await,
            Cmd::Search(args) => commands::search::run(args).await,
            Cmd::Pin(args) => commands::pin::run(args).await,
            Cmd::Pinned(args) => commands::pinned::run(args).await,
            Cmd::Unpin(args) => commands::unpin::run(args).await,
            Cmd::Nickname(args) => commands::nickname::run(args).await,
            Cmd::Sources(args) => commands::sources::run(args).await,
            Cmd::Retention(args) => commands::retention::run(args).await,
            Cmd::Revoke(args) => commands::revoke::run(args).await,
            Cmd::Admin(args) => commands::admin::run(args).await,
            Cmd::Rm(args) => commands::rm::run(args).await,
            Cmd::Telemetry(args) => commands::telemetry::run(args).await,
            Cmd::SelfUpdate(args) => update::run_self_update(args).await,
            Cmd::Completion { .. } => unreachable!(),
        };
        // Best-effort update notifier: never delays user-facing output by >300ms,
        // never affects exit status, never surfaces its errors. Replaces the
        // older `update_check::check_self_outdated` polling path.
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            update::notifier::maybe_notify(),
        )
        .await;
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
