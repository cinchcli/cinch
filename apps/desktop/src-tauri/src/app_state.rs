//! Shared Tauri-managed state types and helpers used across the desktop
//! binary. Split out of `lib.rs` so the entry module stays focused on
//! orchestration.

use std::sync::{Arc, Mutex};

use crate::protocol;

/// Handle to the shared `client_core` store (new Phase 4 store).
/// Commands that need to read/write the new store access this via Tauri state.
pub type SharedStore = Arc<client_core::store::Store>;

/// The long-lived sync writer started at startup. Wrapped in `Mutex<Option<…>>`
/// so the shutdown path can `take()` it and call `Writer::shutdown`.
pub type WriterHandle = Mutex<Option<client_core::sync::Writer>>;

/// Local-clip ingest pipeline (encrypt + push to relay + write-through to
/// shared store). Lives independently of `Writer` so reader-mode desktops
/// (lock held by another process) can still publish locally-detected clips.
/// Wrapped so `restart_writer` can swap it on credential change.
pub type LocalPusherHandle = Arc<Mutex<Option<client_core::sync::LocalPusher>>>;

pub type PreviousAppPid = Arc<Mutex<Option<i32>>>;

/// Sender side of the channel that forwards remote `NewClip` notifications
/// from `client_core::sync::Writer`'s `on_new_clip` callback into Tauri's
/// event bus. Stored in Tauri state so `restart_writer` can rebuild the
/// callback with the same delivery target after a credential swap.
pub(crate) struct ClipNotifierTx(
    pub(crate) tokio::sync::mpsc::UnboundedSender<client_core::protocol::Clip>,
);

/// Fires on WS (re)connect so the consumer in lib.rs setup() can emit a
/// DevicesChanged Tauri event without needing an AppHandle inside
/// client-core's `on_connected` callback.
#[derive(Clone)]
pub(crate) struct DevicesChangedTx(pub(crate) tokio::sync::mpsc::UnboundedSender<()>);

/// Builds the `ClientInfo` block that identifies this desktop binary to
/// `cinch-core`'s REST + WS clients. Cinch-core attaches it as HTTP
/// headers and as the WS `client_hello` payload, so the relay can
/// persist the per-device version row used by `cinch device list` and the
/// desktop's version badges.
pub fn build_client_info() -> client_core::version::ClientInfo {
    client_core::version::ClientInfo {
        client_type: client_core::version::ClientType::Desktop,
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Build a `LocalPusherHandle` for the pre-login / RestClient-failed paths.
///
/// The pusher is constructed with `enc_key = None`, so `push_text` /
/// `push_image_png` short-circuit to `backlog_flusher::enqueue_local` and
/// never touch the network. A stub `RestClient` is required by the
/// `LocalPusher::new` signature; we use a known-bad URL when no relay is
/// configured so any accidental network call fails fast and loudly. As
/// soon as credentials propagate, `restart_writer` swaps in a fully-wired
/// pusher with the real RestClient + encryption key.
pub(crate) fn build_offline_pusher_handle(
    shared_store: &SharedStore,
    config: &protocol::Config,
) -> LocalPusherHandle {
    let stub_url = if !config.relay_url.is_empty() {
        config.relay_url.clone()
    } else {
        // Known-bad sentinel — never actually contacted because the
        // no-key path skips push_clip_json entirely.
        "http://127.0.0.1:0".to_string()
    };
    let stub_token = config.token.clone();
    match client_core::http::RestClient::new(stub_url, stub_token, build_client_info()) {
        Ok(rest_client) => {
            let pusher = client_core::sync::LocalPusher::new(
                shared_store.clone(),
                Arc::new(rest_client),
                None,
            );
            Arc::new(Mutex::new(Some(pusher)))
        }
        Err(e) => {
            log::warn!(
                "cannot build stub RestClient for offline pusher (non-fatal): {}",
                e
            );
            Arc::new(Mutex::new(None))
        }
    }
}
