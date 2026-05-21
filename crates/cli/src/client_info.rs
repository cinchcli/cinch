//! Builds the CLI's `ClientInfo` for cinch-core's RestClient/WsConfig.
//!
//! Critical: `version` must be `env!("CARGO_PKG_VERSION")` of the
//! `cinch-cli` binary crate — NOT of `cinchcli-core` (which would
//! report the library version, not what the user is actually running).

use client_core::version::{ClientInfo, ClientType};

pub fn for_cli() -> ClientInfo {
    ClientInfo {
        client_type: ClientType::Cli,
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}
