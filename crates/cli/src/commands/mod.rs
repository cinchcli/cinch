pub mod account;
pub mod admin;
pub mod ai;
pub mod auth;
pub mod auth_recovery;
pub mod clip;
pub mod copy;
pub mod device;
pub mod devices;
pub mod fleet;
pub mod get;
pub mod history;
pub mod list;
pub mod mcp;
pub mod pair;
pub mod paste;
pub mod pin;
pub mod plan;
pub mod pull;
pub mod push;
pub mod retention;
pub mod revoke;
pub mod rm;
pub mod search;
pub mod send;
pub mod session;
pub mod shared;
pub mod sources;
pub mod telemetry;
pub mod transform;
pub mod unpin;

/// Print the single-line deprecation note for an old command spelling kept as
/// a hidden alias through the 0.5–0.7 runway (redesign §4b/§4d).
///
/// The format is fixed and asserted by the test matrix — it must be **exactly
/// one** stderr line naming the old spelling, the new spelling, and the 0.8
/// removal. Call it once per deprecated invocation, before delegating to the
/// new handler.
pub fn deprecation_note(old: &str, new: &str) {
    eprintln!("note: `cinch {old}` is now `cinch {new}` (deprecated alias, removed in 0.8)");
}
