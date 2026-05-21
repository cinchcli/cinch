//! `cinch pinned` — alias of `cinch list --pinned`.

use crate::exit::ExitError;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Max number of clips to return. Hard cap is 200.
    #[arg(long, default_value_t = 50)]
    pub limit: u32,
    /// Force JSON output (default when stdout is not a TTY).
    #[arg(long)]
    pub json: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    let list_args = crate::commands::list::Args {
        limit: args.limit,
        from: None,
        text_only: false,
        exclude_self: false,
        json: args.json,
        remote: false,
        pinned: true,
    };
    crate::commands::list::run(list_args).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_pinned_args_translates_to_list_args() {
        // Compile-time test: ensure the explicit field-by-field construction
        // of list::Args in run() stays in sync with the list::Args struct.
        // If a new required field is added to list::Args, this must be updated.
        let _ = crate::commands::list::Args {
            limit: 10,
            from: None,
            text_only: false,
            exclude_self: false,
            json: false,
            remote: false,
            pinned: true,
        };
    }
}
