//! Re-export of shared crypto primitives from `client-core`.
//!
//! The implementation moved to `crates/client-core/src/crypto.rs` so the
//! CLI can share the exact wire format. Existing `crate::crypto::*` call
//! sites in this crate continue to work unchanged via this re-export.

pub use client_core::crypto::*;
