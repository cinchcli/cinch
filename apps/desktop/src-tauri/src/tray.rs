use log::info;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

pub struct TrayMenuItems {
    pub pending: MenuItem<tauri::Wry>,
    // Kept alive so the system tray icon isn't removed when this scope ends.
    #[allow(dead_code)]
    pub tray: tauri::tray::TrayIcon<tauri::Wry>,
}

pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let open = MenuItem::with_id(app, "open", "Open Dashboard", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Cinch", true, None::<&str>)?;
    // Initially empty and disabled; set_pending_count enables it when codes arrive.
    let pending = MenuItem::with_id(app, "pending", "", false, None::<&str>)?;
    let sep1 = tauri::menu::PredefinedMenuItem::separator(app)?;
    let sep2 = tauri::menu::PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(app, &[&open, &sep1, &pending, &sep2, &quit])?;

    let tray_img = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;
    let tray_icon = TrayIconBuilder::new()
        .icon(tray_img)
        .icon_as_template(true)
        .menu(&menu)
        .tooltip("Cinch — Clipboard Sync")
        .on_menu_event(|app: &AppHandle, event| match event.id().as_ref() {
            "open" => crate::show_on_active_monitor(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::show_on_active_monitor(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(TrayMenuItems {
        pending,
        tray: tray_icon,
    });

    info!("tray icon created");
    Ok(())
}

/// Update the tray menu item to reflect pending device-code count.
/// Called from the WS handler when a `device_code_pending` frame arrives,
/// and from the TTL sweeper (Task 3.6) after expiry.
pub fn set_pending_count(app: &AppHandle, count: usize) {
    let label = if count == 0 {
        String::new()
    } else if count == 1 {
        "1 pending login request".to_string()
    } else {
        format!("{} pending login requests", count)
    };
    if let Some(items) = app.try_state::<TrayMenuItems>() {
        let _ = items.pending.set_text(&label);
        let _ = items.pending.set_enabled(count > 0);
    }
    // TODO(future): swap to a badged tray icon when count > 0.
    // Requires `icons/tray-badge.png` asset.
}
