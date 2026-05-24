//! `cinch mcp` — read-only Model Context Protocol server over the local
//! clipboard store. Runs on a quiet path (no auth, no network, no telemetry,
//! no async runtime) so the stdio JSON-RPC stream is never corrupted.

mod mapping;
mod protocol;
mod query;

use crate::exit::{ExitError, GENERIC_ERROR};
use client_core::store::{default_db_path, Store};

#[derive(Debug, clap::Args)]
pub struct Args {}

/// Open the local store directly (no auth / RestClient), read-only usage.
fn open_store() -> Result<Store, ExitError> {
    let path = default_db_path().map_err(|e| {
        ExitError::new(GENERIC_ERROR, format!("cannot resolve store path: {e}"), "")
    })?;
    Store::open(&path)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("cannot open store: {e}"), ""))
}

pub fn run(_args: Args) -> Result<(), ExitError> {
    let store = open_store()?;
    protocol::serve_stdio(&store)
}
