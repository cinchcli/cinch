//! Re-export of shared credential storage from `client-core`.
//!
//! Implementation lives in `crates/client-core/src/auth.rs` so the CLI
//! shares the same `~/.cinch/config.json` schema and 0600 permission
//! discipline. Existing `crate::auth::credential::*` call sites continue
//! to work via this re-export.

pub use client_core::auth::*;
