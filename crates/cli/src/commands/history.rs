//! `cinch history` — browse/search/manage the LOCAL clip store (redesign §2;
//! was `cinch clip`).
//!
//! Read/browse verbs (`list`, `search`, `show`, `transform`) are local-only.
//! `rm` is cross-plane `[R+L]` by default (eng-review D2) — it deletes on the
//! fleet AND locally and says so; `--local` scopes it down. Bare `cinch
//! history` is `cinch history list`.
//!
//! Each subcommand reuses an existing module's `Args`, so behavior is shared
//! with the deprecated `clip` aliases (which route to the same handlers).

use crate::exit::ExitError;

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// Recent clips with previews.
    List(crate::commands::list::Args),
    /// FTS5 full-text search of local history.
    Search(crate::commands::search::Args),
    /// Show one local clip (content, or --meta). (was `clip get`)
    Show(crate::commands::get::Args),
    /// Delete clip(s) on the fleet AND locally (cross-plane; --local to scope down).
    Rm(crate::commands::rm::Args),
    /// Transform a clip's text (pretty-json, redact-secrets, ...).
    Transform(crate::commands::transform::Args),
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        // Bare `cinch history` lists recent clips.
        None => crate::commands::list::run(default_list_args()).await,
        Some(Cmd::List(a)) => crate::commands::list::run(a).await,
        Some(Cmd::Search(a)) => crate::commands::search::run(a).await,
        Some(Cmd::Show(a)) => crate::commands::get::run(a).await,
        Some(Cmd::Rm(a)) => crate::commands::rm::run(a).await,
        Some(Cmd::Transform(a)) => crate::commands::transform::run(a).await,
    }
}

/// Defaults for bare `cinch history` (mirror `list`'s clap defaults).
fn default_list_args() -> crate::commands::list::Args {
    crate::commands::list::Args {
        limit: 50,
        from: None,
        text_only: false,
        exclude_self: false,
        json: false,
        remote: false,
        pinned: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(clap::Parser)]
    struct TestCli {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn bare_history_has_no_subcommand() {
        let cli = TestCli::try_parse_from(["test"]).expect("bare history parses");
        assert!(cli.args.cmd.is_none());
    }

    #[test]
    fn history_show_meta_survives() {
        // The `--meta` flag (clip get --meta → history show --meta merge case)
        // must reach the `show` (get) handler.
        let cli = TestCli::try_parse_from(["test", "show", "abcd", "--meta"])
            .expect("history show --meta parses");
        match cli.args.cmd {
            Some(Cmd::Show(a)) => {
                assert_eq!(a.id_or_index, "abcd");
                assert!(a.meta);
            }
            _ => panic!("expected Show"),
        }
    }

    #[test]
    fn history_rm_variadic_parses() {
        let cli =
            TestCli::try_parse_from(["test", "rm", "aaaa", "bbbb"]).expect("history rm parses");
        match cli.args.cmd {
            Some(Cmd::Rm(a)) => assert_eq!(a.ids, vec!["aaaa", "bbbb"]),
            _ => panic!("expected Rm"),
        }
    }
}
