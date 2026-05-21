use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use crate::auth::AuthState;
use crate::commands::clips::LocalClip;

#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct AuthStateChanged(pub AuthState);

/// Fired when the desktop's background updater refreshes the cached
/// latest-release tags. React's `useLatestVersions` hook subscribes to
/// this so version badges re-render the moment the new tag lands,
/// without waiting for the next mount.
#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct LatestVersionsUpdated(pub crate::update_check::LatestVersions);

#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct WsStatus(pub String);

#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct ClipReceived(pub LocalClip);

/// Fired when a clip arrives from another device via the relay WebSocket
/// (i.e., delivered by `client_core::sync::Writer`'s on_new_clip callback).
/// Distinct from `ClipReceived` (which fires for locally-captured clips on
/// this device) so the React layer can trigger per-source desktop alerts
/// without a local-vs-remote heuristic.
#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct RemoteClipReceived(pub LocalClip);

#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct ClipDeleted(pub String);

#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct NewSourceDetected(pub String);

#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct ImageDownloadFailed(pub String);

#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct ImageDownloadComplete(pub String);

/// Fired when the desktop notices that credentials on disk came from the CLI
/// (the FS watcher observed a credential_version bump while the desktop was
/// in LocalOnly). The frontend uses this to surface a one-shot toast so the
/// user knows their CLI sign-in carried over.
#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct AuthAdoptedFromCli {
    pub user_short: String,
}

/// Fired when the desktop receives a `cinch://login?relay=…&from=cli` deep
/// link from the CLI's handoff. The React layer responds by opening the
/// AddRelayDialog with the relay URL pre-filled.
#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct CliHandoffRequested {
    pub relay_url: String,
}

/// Fired by `pair_via_ssh` when the remote machine's `cinch auth login
/// --headless` emits the device-code marker. The frontend opens `url`
/// in a browser so the user can complete OAuth; the command keeps running
/// until the SSH process exits.
#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct SshPairMarkerFound {
    pub url: String,
}

/// Fired when queued offline clips are dropped because the encryption key
/// is missing. The frontend shows a toast prompting the user to sign in again.
#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct OfflineQueueDropped {
    pub count: u32,
}

/// Fired when a received clip cannot be decrypted (wrong or missing AES key).
/// The desktop fires retry_key_bundle automatically; this event lets the UI
/// show a hint so the user knows recovery is in progress.
#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct ClipDecryptFailed {
    pub clip_id: String,
    /// "missing_key" or "tag_failed: <detail>"
    pub reason: String,
}

/// Fired when a clip's pin state changes (from relay WS broadcast or local pin/unpin).
/// React listens to refresh the pinned-clips list on any device.
#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct ClipPinned {
    pub clip_id: String,
    pub is_pinned: bool,
    pub pin_note: Option<String>,
}

/// Fired when the relay pushes a `device_code_pending` WebSocket message, indicating
/// that a CLI `cinch auth login --user EMAIL` has started and is awaiting desktop
/// approval. The React layer uses this event to surface the approval UI.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct DeviceCodePending(pub crate::auth::state::PendingDeviceCode);

/// Streamed to the `snap-overlay` window during a panel drag. All
/// coordinates are physical pixels relative to the global desktop space;
/// the overlay subtracts the monitor origin and divides by `scale` to get
/// CSS-logical coordinates. `visible:false` tells the overlay to clear.
#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct SnapGuideUpdate {
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_w: u32,
    pub monitor_h: u32,
    pub scale: f64,
    /// Anchor top-left (physical).
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub win_w: u32,
    pub win_h: u32,
    pub within_snap: bool,
    pub visible: bool,
}
