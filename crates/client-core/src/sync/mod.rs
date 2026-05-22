//! Sync layer — writer (long-lived WS) and reader (short-lived REST backfill).
//! Lockfile coordinates at most one writer per machine.

pub mod backlog_flusher;
pub mod local_pusher;
pub mod lockfile;
pub mod map;
pub mod reader;
pub mod reconnect;
pub mod writer;

pub use backlog_flusher::{
    enqueue_local, flush_once, format_rfc3339_millis, FlushError, FlushGate, FlushGuard,
    FlushReport, MAX_UNSYNCED,
};
pub use local_pusher::{IngestError, LocalPusher, PushOutcome};
pub use lockfile::{LockKind, Lockfile};
pub use reader::{backfill_once, BackfillBudget, BackfillError};
pub use reconnect::{reconnect_catchup, ReconnectCatchupReport};
pub use writer::{OnConnectedCallback, OnNewClipCallback, Writer};
