//! `cinch unpin <REF>` — unpin a clip (redesign §2; was `pin rm`).
//!
//! Cross-plane by default (eng-review D2): unpins on the fleet (relay) AND
//! locally; `--local` scopes it to the local store only. Shares the pin/unpin
//! core with [`crate::commands::pin::set_pin`].

use crate::exit::ExitError;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Clip reference (id prefix, min 4 chars) to unpin. Cross-plane (fleet +
    /// local) unless `--local`.
    pub reference: String,
    /// Unpin locally only — do not touch the fleet (no relay call, no auth).
    #[arg(long)]
    pub local: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    crate::commands::pin::set_pin(&args.reference, args.local, false).await
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
    fn unpin_ref_parses() {
        let cli = TestCli::try_parse_from(["test", "abcd"]).expect("unpin <REF> parses");
        assert_eq!(cli.args.reference, "abcd");
        assert!(!cli.args.local);
    }

    #[test]
    fn unpin_ref_local_parses() {
        let cli = TestCli::try_parse_from(["test", "abcd", "--local"]).expect("unpin --local");
        assert!(cli.args.local);
    }
}
