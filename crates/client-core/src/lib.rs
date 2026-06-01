//! Shared client-side primitives for Cinch CLI and desktop.
//!
//! Wire DTOs (`proto::cinch::v1::*`) are generated from the canonical
//! `proto/cinch/v1/*.proto` files at build time via `prost-build`; see
//! `build.rs` for the serde attribute injection that keeps wire bytes
//! byte-equal to the Go relay's `encoding/json` `omitempty` output.
//!
//! The optional `specta` feature wires up `specta::Type` derives on wire
//! DTOs that desktop exposes to the frontend. CLI builds without it.

pub mod auth;
pub mod auth_session;
pub mod classify;
pub mod config;
pub mod config_migrate;
pub mod credstore;
pub mod crypto;
pub mod http;
pub mod key_exchange;
pub mod machine;
pub mod media;
pub mod pair_script;
pub mod protocol;
pub mod recovery;
pub mod rest;
pub mod store;
pub mod sync;
pub mod transform;
pub mod transport;
pub mod version;
pub mod ws;

/// Generated Rust message types from `proto/cinch/v1/*.proto`.
///
/// Wire-compatible with the Go relay's hand-written `protocol/*.go` DTOs:
/// snake_case JSON tags, `omitempty` semantics preserved via field-level
/// `skip_serializing_if` predicates injected in `build.rs`.
#[allow(clippy::all)]
pub mod proto {
    pub mod cinch {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/cinch.v1.rs"));
        }
    }
}

/// Helpers used by generated `#[serde(skip_serializing_if = "...")]`
/// attributes to mirror Go's `encoding/json` `omitempty` semantics on
/// scalar fields.
pub mod ser {
    pub fn is_zero_i32(v: &i32) -> bool {
        *v == 0
    }
    pub fn is_zero_i64(v: &i64) -> bool {
        *v == 0
    }
    pub fn is_false(v: &bool) -> bool {
        !*v
    }
    pub fn is_empty_str(v: &str) -> bool {
        v.is_empty()
    }
    pub fn is_empty_vec<T>(v: &[T]) -> bool {
        v.is_empty()
    }
}
