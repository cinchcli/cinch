//! `cinch push` — REMOVED in 0.5 (hard error, did-you-mean).
//!
//! `push`'s meaning changed in the 0.5 CLI redesign. To prevent a silent
//! leak or a silent local save, bare `cinch push` now hard-errors and does
//! NOTHING: it opens no store, consumes/persists no stdin, and makes no
//! network call. It tells the user the two replacement verbs:
//!   • Save locally  → cinch copy
//!   • Send to fleet → cinch send
//!
//! The variant is hidden from help/completions (`#[command(hide = true)]` in
//! `lib.rs`) but still parses and routes here, so `cinch push`, `echo x |
//! cinch push`, and `cinch push --token X --relay Y` all land on this error.

use crate::exit::{ExitError, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Catch-all so ANY arguments/flags (e.g. `--token X --relay Y`, or a
    /// piped stdin payload) still route to the hard error rather than a clap
    /// parse failure. Swallowed and never inspected — `push` does nothing.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub _rest: Vec<String>,
}

/// Hard error. Performs NO work: no store open, no stdin consume that
/// persists, no network. This is the leak-prevention guarantee — bare `push`
/// must NEVER save or send.
pub async fn run(_args: Args) -> Result<(), ExitError> {
    Err(ExitError::new(
        GENERIC_ERROR,
        "`cinch push` was removed in 0.5. Its meaning changed.",
        "  • Save locally  → cinch copy\n  • Send to fleet → cinch send",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_with(rest: &[&str]) -> Args {
        Args {
            _rest: rest.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[tokio::test]
    async fn push_hard_errors_with_nonzero_code() {
        let res = run(args_with(&[])).await;
        let err = res.expect_err("push must hard-error");
        assert_ne!(err.code, crate::exit::SUCCESS, "exit code must be non-zero");
        assert_eq!(err.code, GENERIC_ERROR);
    }

    #[tokio::test]
    async fn push_error_mentions_both_copy_and_send() {
        // The did-you-mean (redesign §4d) must name BOTH replacement verbs.
        let err = run(args_with(&[])).await.expect_err("hard error");
        let combined = format!("{} {}", err.message, err.fix);
        assert!(
            combined.contains("cinch copy"),
            "must point at `cinch copy`, got: {combined}"
        );
        assert!(
            combined.contains("cinch send"),
            "must point at `cinch send`, got: {combined}"
        );
    }

    #[tokio::test]
    async fn push_with_old_inert_flags_still_errors() {
        // The old `--token X --relay Y` form must resurrect nothing — it lands
        // on the same hard error. (The catch-all swallows these as positionals
        // in the unit harness; the safety property is that run() refuses.)
        let err = run(args_with(&["--token", "X", "--relay", "Y"]))
            .await
            .expect_err("flags must not resurrect push");
        assert_eq!(err.code, GENERIC_ERROR);
    }

    #[tokio::test]
    async fn push_with_piped_payload_form_still_errors() {
        // Simulates `echo x | cinch push` argument-wise: even with trailing
        // content captured, run() does nothing and errors.
        let err = run(args_with(&["payload"]))
            .await
            .expect_err("piped form must not save/send");
        assert_eq!(err.code, GENERIC_ERROR);
    }
}
