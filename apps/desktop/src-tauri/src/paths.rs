//! Centralized, panic-free filesystem path resolution for the desktop app.
//!
//! Several sites used to duplicate the expression
//! `dirs::data_dir().unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/share"))`.
//! The inner `.unwrap()` crashed the whole app on startup whenever neither the
//! platform data directory nor the home directory could be resolved (rare, but
//! fatal — and entirely avoidable). These helpers fold that resolution into one
//! place that can never panic.

use std::path::PathBuf;

/// Path to the cross-process writer lockfile. Delegates to
/// `client_core::store::lock_path()` and folds its error into the same
/// `/tmp/cinch.lock` fallback both writer-start sites previously inlined,
/// so the lock semantics stay byte-identical across startup and restart.
pub fn lock_path() -> PathBuf {
    client_core::store::lock_path().unwrap_or_else(|_| PathBuf::from("/tmp/cinch.lock"))
}
