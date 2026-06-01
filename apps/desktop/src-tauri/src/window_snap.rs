//! Pure window-placement math and persistence types for the Raycast-style
//! snap guides. No Tauri imports — fully unit-testable.

use serde::{Deserialize, Serialize};

/// A monitor described in physical pixels (Tauri `Monitor` is converted into
/// this at the integration boundary).
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorBox {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub name: Option<String>,
    /// HiDPI scale factor for physical↔logical pixel conversion at the Tauri
    /// integration boundary; not consumed by `anchor_for`, which works in
    /// physical pixels throughout.
    pub scale: f64,
}

/// Window outer size in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WinSize {
    pub w: i32,
    pub h: i32,
}

/// Vertical anchor fraction: top gap = (monitor_h - win_h) * K.
/// Smaller = higher on screen. 0.5 would be exact center.
pub const K: f64 = 0.32;

/// Top-left physical position where the panel should sit when anchored on
/// `m`: horizontally centered, vertically `K` of the free space from the top.
pub fn anchor_for(m: &MonitorBox, win: WinSize) -> (i32, i32) {
    let x = m.x + (m.w - win.w) / 2;
    let y = m.y + (((m.h - win.h) as f64) * K).round() as i32;
    (x, y)
}

/// Max distance (physical px) between the dragged panel's center and the
/// anchor center for the drop to snap.
pub const SNAP_THRESHOLD_PX: f64 = 80.0;

/// Decide where the panel lands. `drop_center`/`anchor_center` are physical
/// pixels. Returns the panel **top-left** and whether it snapped.
pub fn resolve_drop(
    drop_center: (i32, i32),
    anchor_center: (i32, i32),
    win: WinSize,
    threshold: f64,
) -> ((i32, i32), bool) {
    let dx = (drop_center.0 - anchor_center.0) as f64;
    let dy = (drop_center.1 - anchor_center.1) as f64;
    let dist = (dx * dx + dy * dy).sqrt();
    let snapped = dist <= threshold;
    let center = if snapped { anchor_center } else { drop_center };
    let top_left = (center.0 - win.w / 2, center.1 - win.h / 2);
    (top_left, snapped)
}

/// Stable identity for a monitor across reconnects: prefer the OS name,
/// otherwise the geometry.
pub fn monitor_fingerprint(m: &MonitorBox) -> String {
    match &m.name {
        Some(n) if !n.is_empty() => format!("name:{n}"),
        _ => format!("geo:{},{},{},{}", m.x, m.y, m.w, m.h),
    }
}

/// Persisted per-monitor placement of the panel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub monitor: String,
    pub x: i32,
    pub y: i32,
    pub anchored: bool,
}

fn monitor_at(monitors: &[MonitorBox], px: f64, py: f64) -> Option<&MonitorBox> {
    monitors.iter().find(|m| {
        px >= m.x as f64 && px < (m.x + m.w) as f64 && py >= m.y as f64 && py < (m.y + m.h) as f64
    })
}

fn clamp_into(m: &MonitorBox, win: WinSize, x: i32, y: i32) -> (i32, i32) {
    let max_x = m.x + (m.w - win.w).max(0);
    let max_y = m.y + (m.h - win.h).max(0);
    (x.clamp(m.x, max_x), y.clamp(m.y, max_y))
}

/// Decide the panel top-left for a summon:
/// 1. saved + its monitor present → restore (anchored → recompute anchor;
///    free → clamp saved x/y into that monitor),
/// 2. else → anchor on the monitor under the cursor,
/// 3. else (no monitor under cursor / empty list) → anchor on first monitor,
/// 4. else → (0, 0) as a last resort.
pub fn choose_placement(
    saved: Option<&Placement>,
    monitors: &[MonitorBox],
    cursor: (f64, f64),
    win: WinSize,
) -> (i32, i32) {
    if let Some(p) = saved {
        if let Some(m) = monitors
            .iter()
            .find(|m| monitor_fingerprint(m) == p.monitor)
        {
            return if p.anchored {
                anchor_for(m, win)
            } else {
                clamp_into(m, win, p.x, p.y)
            };
        }
    }
    if let Some(m) = monitor_at(monitors, cursor.0, cursor.1) {
        return anchor_for(m, win);
    }
    if let Some(m) = monitors.first() {
        return anchor_for(m, win);
    }
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_is_centered_x_and_above_center_y() {
        let m = MonitorBox {
            x: 0,
            y: 0,
            w: 1920,
            h: 1200,
            name: None,
            scale: 2.0,
        };
        let (x, y) = anchor_for(&m, WinSize { w: 960, h: 600 });
        assert_eq!(x, 480); // (1920-960)/2
                            // free space = 600; 600*0.32 = 192
        assert_eq!(y, 192);
        // above true center (true center top would be 300)
        assert!(y < 300);
    }

    #[test]
    fn anchor_respects_monitor_origin_offset() {
        let m = MonitorBox {
            x: 1920,
            y: -200,
            w: 1920,
            h: 1080,
            name: None,
            scale: 1.0,
        };
        let (x, y) = anchor_for(&m, WinSize { w: 960, h: 600 });
        assert_eq!(x, 1920 + 480);
        assert_eq!(y, -46); // -200 + round(480 * 0.32) = -200 + 154
    }

    #[test]
    fn resolve_drop_snaps_inside_threshold() {
        // anchor center at (100,100); drop center 50px away → snap
        let (pos, anchored) = resolve_drop(
            (130, 140),
            (100, 100),
            WinSize { w: 60, h: 40 },
            SNAP_THRESHOLD_PX,
        );
        assert!(anchored);
        // returned top-left = anchor center - win/2 = (100-30, 100-20)
        assert_eq!(pos, (70, 80));
    }

    #[test]
    fn resolve_drop_keeps_free_outside_threshold() {
        let (pos, anchored) = resolve_drop(
            (400, 400),
            (100, 100),
            WinSize { w: 60, h: 40 },
            SNAP_THRESHOLD_PX,
        );
        assert!(!anchored);
        // free top-left = drop center - win/2
        assert_eq!(pos, (370, 380));
    }

    #[test]
    fn resolve_drop_boundary_is_inclusive_snaps_at_exact_threshold() {
        // dist exactly == SNAP_THRESHOLD_PX (80.0): horizontal offset of 80px.
        // `<=` is intentional → exact-threshold drop must snap.
        let (pos, anchored) = resolve_drop(
            (180, 100),
            (100, 100),
            WinSize { w: 60, h: 40 },
            SNAP_THRESHOLD_PX,
        );
        assert!(
            anchored,
            "drop exactly at the threshold must snap (inclusive <=)"
        );
        assert_eq!(pos, (70, 80)); // snapped → anchor_center(100,100) - win/2(30,20)
    }

    #[test]
    fn fingerprint_prefers_name_then_falls_back_to_geometry() {
        let named = MonitorBox {
            x: 0,
            y: 0,
            w: 100,
            h: 100,
            name: Some("DELL U2720".into()),
            scale: 1.0,
        };
        assert_eq!(monitor_fingerprint(&named), "name:DELL U2720");
        let anon = MonitorBox {
            x: 10,
            y: 20,
            w: 1920,
            h: 1080,
            name: None,
            scale: 2.0,
        };
        assert_eq!(monitor_fingerprint(&anon), "geo:10,20,1920,1080");
    }

    #[test]
    fn placement_json_round_trips() {
        let p = Placement {
            monitor: "name:DELL".into(),
            x: 12,
            y: -7,
            anchored: true,
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: Placement = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn choose_placement_restores_anchored_recomputed_when_monitor_present() {
        let m = MonitorBox {
            x: 0,
            y: 0,
            w: 1920,
            h: 1200,
            name: Some("A".into()),
            scale: 1.0,
        };
        let saved = Placement {
            monitor: "name:A".into(),
            x: 999,
            y: 999,
            anchored: true,
        };
        let win = WinSize { w: 960, h: 600 };
        let got = choose_placement(Some(&saved), std::slice::from_ref(&m), (10.0, 10.0), win);
        // anchored:true → recompute anchor, ignore stale saved x/y
        assert_eq!(got, anchor_for(&m, win));
    }

    #[test]
    fn choose_placement_restores_free_clamped_into_monitor() {
        let m = MonitorBox {
            x: 0,
            y: 0,
            w: 1000,
            h: 800,
            name: Some("A".into()),
            scale: 1.0,
        };
        let win = WinSize { w: 960, h: 600 };
        // saved free position partly off-screen → clamp so window stays on monitor
        let saved = Placement {
            monitor: "name:A".into(),
            x: 900,
            y: 700,
            anchored: false,
        };
        let got = choose_placement(Some(&saved), &[m], (0.0, 0.0), win);
        assert_eq!(got, (1000 - 960, 800 - 600)); // clamped to (40,200)
    }

    #[test]
    fn choose_placement_falls_back_to_cursor_monitor_anchor_when_saved_monitor_gone() {
        let cur = MonitorBox {
            x: 2000,
            y: 0,
            w: 1920,
            h: 1080,
            name: Some("B".into()),
            scale: 1.0,
        };
        let win = WinSize { w: 960, h: 600 };
        let saved = Placement {
            monitor: "name:GONE".into(),
            x: 1,
            y: 1,
            anchored: false,
        };
        // cursor at (2500,300) → inside monitor B
        let got = choose_placement(
            Some(&saved),
            std::slice::from_ref(&cur),
            (2500.0, 300.0),
            win,
        );
        assert_eq!(got, anchor_for(&cur, win));
    }

    #[test]
    fn choose_placement_no_saved_uses_cursor_monitor_anchor() {
        let cur = MonitorBox {
            x: 0,
            y: 0,
            w: 1440,
            h: 900,
            name: None,
            scale: 2.0,
        };
        let win = WinSize { w: 960, h: 600 };
        let got = choose_placement(None, std::slice::from_ref(&cur), (100.0, 100.0), win);
        assert_eq!(got, anchor_for(&cur, win));
    }

    #[test]
    fn choose_placement_uses_first_monitor_when_saved_gone_and_cursor_off_all_monitors() {
        // saved monitor absent; cursor (-9999,-9999) is on no monitor →
        // step 3 fallback: anchor on the first monitor in the list.
        let first = MonitorBox {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
            name: Some("A".into()),
            scale: 1.0,
        };
        let other = MonitorBox {
            x: 1920,
            y: 0,
            w: 1280,
            h: 1024,
            name: Some("B".into()),
            scale: 1.0,
        };
        let win = WinSize { w: 960, h: 600 };
        let saved = Placement {
            monitor: "name:GONE".into(),
            x: 5,
            y: 5,
            anchored: true,
        };
        let got = choose_placement(
            Some(&saved),
            &[first.clone(), other],
            (-9999.0, -9999.0),
            win,
        );
        assert_eq!(got, anchor_for(&first, win));
    }

    #[test]
    fn choose_placement_last_resort_origin_when_no_saved_and_no_monitors() {
        // no saved placement and an empty monitor list → (0,0) last resort.
        let win = WinSize { w: 960, h: 600 };
        let got = choose_placement(None, &[], (100.0, 100.0), win);
        assert_eq!(got, (0, 0));
    }
}
