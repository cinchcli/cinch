//! `cinch device sources` — distinct source machines that have ever pushed clips.

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// One name per line, no header.
    #[arg(long)]
    pub names: bool,
    /// `text` (default) or `json`.
    #[arg(long, default_value = "text")]
    pub format: String,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    let ctx = crate::runtime::open_ctx().map_err(|_| {
        ExitError::new(
            AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login",
        )
    })?;
    crate::runtime::opportunistic_backfill(&ctx).await;

    let rows = client_core::store::queries::list_sources(&ctx.store)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;

    if args.names {
        for r in &rows {
            println!("{}", r.source);
        }
        return Ok(());
    }

    if args.format == "json" {
        let s = serde_json::to_string(&rows)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("serialize: {e}"), ""))?;
        println!("{s}");
        return Ok(());
    }

    println!("  {:<24}  {:>6}  LAST SEEN", "SOURCE", "CLIPS");
    for r in &rows {
        let last_seen = crate::fmt::fmt_last_seen(r.last_seen);
        println!("  {:<24}  {:>6}  {}", r.source, r.clip_count, last_seen);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn default_format_is_text() {
        // No flags → human-readable table output. The default literal lives
        // in the `default_value = "text"` attribute, so this test pins it.
        let cli = TestCli::try_parse_from(["test"]).expect("bare invocation parses");
        assert_eq!(cli.args.format, "text");
        assert!(!cli.args.names);
    }

    #[test]
    fn names_flag_sets_names_true() {
        let cli = TestCli::try_parse_from(["test", "--names"]).expect("--names parses");
        assert!(cli.args.names);
        // `--names` does NOT touch the format field — the `--names` branch
        // in run() short-circuits before the format match, so both can be
        // set together without conflict.
        assert_eq!(cli.args.format, "text");
    }

    #[test]
    fn format_json_parses() {
        let cli =
            TestCli::try_parse_from(["test", "--format", "json"]).expect("--format json parses");
        assert_eq!(cli.args.format, "json");
        assert!(!cli.args.names);
    }

    #[test]
    fn format_accepts_arbitrary_strings_clap_side() {
        // clap doesn't validate against an enum here — `run()` does the
        // "is it json?" check at runtime, falling through to the text
        // table for anything else. Pin that lenient-parsing contract so a
        // future "tighten to enum" change is a deliberate decision.
        let cli = TestCli::try_parse_from(["test", "--format", "yaml"])
            .expect("clap accepts unknown format strings");
        assert_eq!(cli.args.format, "yaml");
    }
}
