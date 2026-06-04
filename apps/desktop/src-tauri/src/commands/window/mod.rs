//! Window snap-guide orchestration: a transparent click-through overlay
//! window shows a full-screen "H" while the user drags the panel, and the
//! panel snaps to a single per-monitor anchor (or stays free) on release.
//!
//! The pieces live in focused submodules:
//! - [`geometry`] — monitor/window geometry helpers.
//! - [`overlay`] — the snap-guide overlay window + per-frame guide emission.
//! - [`copy_toast`] — the detached "Copied" confirmation window.
//! - [`drag`] — the drag lifecycle (start / move / drop) and placement persistence.

pub mod copy_toast;
pub mod drag;
pub mod geometry;
pub mod overlay;

pub(crate) use geometry::to_box;

pub use copy_toast::{ensure_copy_toast, CopyToastState};
pub use drag::{load_placement, on_window_moved};
pub use overlay::{ensure_overlay, SnapState};
