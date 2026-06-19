mod app_menu;
pub mod app_state;
pub mod auth;
mod auth_bootstrap;
mod clipboard;
mod commands;
pub mod crypto;
mod deep_link;
pub mod events;
pub mod media;
mod paths;
pub mod protocol;
mod retention;
mod startup;
mod sync_status;
pub mod telemetry;
mod tray;
pub mod update_check;
mod validate;
mod window_manage;
mod window_snap;
mod writer_restart;
mod writer_setup;

#[cfg(test)]
mod tests;

use log::info;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri_specta::{collect_commands, collect_events, Builder, Event};

use auth::state::PendingCodesHandle;
use auth::{AuthState, AuthStateHandle};
use commands::clips::{DeviceCache, DeviceCacheHandle};
use protocol::MultiConfigHandle;

pub(crate) use app_state::ClipNotifierTx;
pub(crate) use app_state::DevicesChangedTx;
pub use app_state::{
    build_client_info, LocalPusherHandle, PreviousAppPid, SharedStore, WriterHandle,
};
pub(crate) use validate::{validate_auth_callback, validate_relay_url};
#[cfg(target_os = "macos")]
pub(crate) use window_manage::activate_app_by_pid;
pub(crate) use window_manage::show_on_active_monitor;
pub(crate) use writer_restart::restart_writer;

pub fn make_specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::clips::list_clips,
            commands::clips::list_pinned_clips,
            commands::clips::pin_clip,
            commands::clips::unpin_clip,
            commands::clips::search_clips,
            commands::clips::get_sources,
            commands::clips::list_source_apps,
            commands::clips::delete_clip,
            commands::clips::send_clip,
            commands::clips::send_current_clipboard,
            commands::clips::get_clip_count,
            commands::clips::get_config_info,
            commands::clips::get_source_auto_copy,
            commands::clips::set_source_auto_copy,
            commands::clips::get_all_source_settings,
            commands::clips::get_source_alert_enabled,
            commands::clips::set_source_alert_enabled,
            commands::clips::get_all_source_alert_settings,
            commands::clips::copy_clip_to_clipboard,
            commands::clips::edit_clip,
            commands::clips::copy_image_to_clipboard,
            commands::clips::save_image_to_file,
            commands::clips::focus_previous_app,
            commands::clips::list_devices,
            commands::clips::set_device_nickname,
            commands::clips::revoke_device,
            commands::clips::get_excluded_apps,
            commands::clips::set_excluded_apps,
            commands::clips::get_retention_config,
            commands::clips::set_retention_config,
            commands::clips::preview_retention_change,
            commands::clips::clear_local_history,
            commands::clips::save_config,
            commands::clips::get_ws_status,
            commands::clips::get_global_shortcut,
            commands::clips::set_global_shortcut,
            commands::clips::get_send_shortcut,
            commands::clips::set_send_shortcut,
            commands::clips::get_action_shortcuts,
            commands::clips::set_action_shortcuts,
            commands::clips::reset_action_shortcuts,
            commands::clips::get_agent_resume_config,
            commands::clips::set_agent_resume_enabled,
            commands::auth::get_auth_state,
            commands::auth::get_user_profile,
            commands::auth::set_display_name,
            commands::auth::list_auth_providers,
            commands::auth::sign_in,
            commands::auth::sign_out,
            commands::auth::retry_auth,
            commands::auth::handle_deeplink,
            commands::auth::pair_via_ssh,
            commands::auth::list_ssh_hosts,
            commands::auth::approve_remote_login,
            commands::auth::deny_remote_login,
            commands::relays::pair_with_token,
            commands::updater::get_latest_versions,
            commands::updater::get_device_version_status,
            commands::updater::run_self_update,
            commands::window::drag::snap_drag_start,
            commands::window::copy_toast::show_copy_toast,
        ])
        .events(collect_events![
            events::AuthStateChanged,
            events::WsStatus,
            events::TrayOpenSettings,
            events::DevicesChanged,
            events::ClipReceived,
            events::RemoteClipReceived,
            events::ClipDeleted,
            events::NewSourceDetected,
            events::ImageDownloadFailed,
            events::ImageDownloadComplete,
            events::AuthAdoptedFromCli,
            events::CliHandoffRequested,
            events::SshPairMarkerFound,
            events::OfflineQueueDropped,
            events::ClipDecryptFailed,
            events::ClipPinned,
            events::DeviceCodePending,
            events::LatestVersionsUpdated,
            events::SnapGuideUpdate,
            events::CopyToastRequested,
            events::ClipSent,
        ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    telemetry::init();

    // Load MultiConfig from ~/.cinch/config.json (migrates legacy single-Config format)
    let multi_config = protocol::MultiConfig::load();
    let config = multi_config.to_active_config();
    if let Some(p) = multi_config.active_profile() {
        info!("config loaded: relay={}, user={}", p.relay_url, p.user_id);
    } else {
        info!("config not found, starting in setup mode");
    }
    let is_configured = config.is_configured();
    if is_configured && !config.user_id.is_empty() {
        telemetry::identify(&config.user_id);
    }
    telemetry::capture(
        telemetry::Event::new("desktop.app.opened").with("is_configured", is_configured),
    );
    let active_relay_id_seed = multi_config.active_relay_id.clone().unwrap_or_default();

    let ws_relay_url = config.relay_url.clone();
    let ws_token = config.token.clone();
    let config_for_auth_seed = config.clone();

    // ── Shared client-core Store at ~/.cinch/store.db ───────────────────────
    // The single store shared between the desktop and the CLI writer. The
    // desktop now runs entirely on this store; the legacy per-app SQLite DB
    // and its store::db module have been removed.
    let shared_store: SharedStore = startup::open_shared_store();

    // Build the NewClip notifier channel before Writer::start so the initial
    // writer — spawned synchronously below, before Tauri's AppHandle exists —
    // can deliver remote clip arrivals to a consumer task that we'll spawn
    // inside `.setup()` once `app.handle()` is available.
    let (clip_notif_tx, clip_notif_rx) =
        tokio::sync::mpsc::unbounded_channel::<client_core::protocol::Clip>();

    let (devices_changed_tx, devices_changed_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

    let (writer_handle, local_pusher_handle): (WriterHandle, LocalPusherHandle) =
        startup::build_initial_writer_and_pusher(
            &config,
            is_configured,
            &shared_store,
            clip_notif_tx.clone(),
            devices_changed_tx.clone(),
        );

    let multi_config_handle: MultiConfigHandle = Arc::new(Mutex::new(multi_config));
    let device_cache_handle: DeviceCacheHandle = Arc::new(DeviceCache::new());
    let ws_abort_handle = Arc::new(sync_status::WsAbortHandle::new());
    let pending_relay_add = Arc::new(commands::relays::PendingRelayAdd::new());
    let pending_auth_relay = Arc::new(commands::relays::PendingAuthRelay::new());
    let previous_app_pid: PreviousAppPid = Arc::new(Mutex::new(None));

    // Single clipboard service shared by monitor, ws client, and Tauri commands.
    let clipboard_service = Arc::new(clipboard::ClipboardService::new_platform_default());

    let ws_status = Arc::new(sync_status::WsStatus::new());

    // Shared relay connectivity flag for offline queue logic
    let relay_connected = Arc::new(AtomicBool::new(false));

    // AuthStateHandle — canonical shared AuthState (CONTEXT.md D-12/D-13).
    // Created here so the FS watcher (spawn_credential_watcher) has a handle to funnel
    // `transition()` calls through. Plan 03 Task 1 will extend the initial state setup.
    let auth_state_handle: AuthStateHandle = Arc::new(Mutex::new(AuthState::default()));

    // PendingCodesHandle — in-memory list of pending device-code approval requests
    // forwarded from the relay via `device_code_pending` WS messages (Task 3.3).
    // Registered as Tauri state so approve/deny commands (Task 3.4) can access it.
    let pending_codes_handle: PendingCodesHandle = Arc::new(Mutex::new(Vec::new()));

    let specta_builder = make_specta_builder();

    #[cfg(debug_assertions)]
    specta_builder
        .export(
            specta_typescript::Typescript::default(),
            "../src/bindings.ts",
        )
        .expect("Failed to export TypeScript bindings");

    // Build a `cinch://` HTTP response, attaching an immutable `Cache-Control`
    // for cacheable (200) results so a revisit hits the webview cache instead
    // of re-reading the BLOB. A plain (non-capturing) fn so it can be called
    // both inline and from the worker thread below.
    fn build_media_response(r: crate::media::MediaResponse) -> tauri::http::Response<Vec<u8>> {
        let mut builder = tauri::http::Response::builder()
            .status(r.status)
            .header("Content-Type", r.content_type);
        if let Some(cc) = crate::media::media_cache_control(r.status) {
            builder = builder.header("Cache-Control", cc);
        }
        builder.body(r.body).unwrap()
    }

    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("cinch", move |ctx, request, responder| {
            let uri = request.uri().to_string();
            if let Some(clip_id) = uri
                .strip_prefix("cinch://media/")
                .or_else(|| uri.strip_prefix("cinch://media\\"))
            {
                // Image BLOBs can be several MB. This handler is invoked on the
                // webview's main thread (macOS WKWebView), so reading the BLOB
                // inline blocks the UI on every navigation to an image clip.
                // Clone the shared store handle and serve from a worker thread,
                // responding once the read completes.
                let clip_id = clip_id.to_string();
                let store: crate::SharedStore = ctx
                    .app_handle()
                    .state::<crate::SharedStore>()
                    .inner()
                    .clone();
                std::thread::spawn(move || {
                    let r = crate::media::serve_clip_image(&store, &clip_id);
                    responder.respond(build_media_response(r));
                });
            } else {
                // app-icon / unknown: serve inline on the calling (main) thread.
                // Unlike the cinch://media path (which only reads SQLite and so
                // is cheap to background), the icon lookup is AppKit: it creates
                // autoreleased NSWorkspace/NSImage/NSData objects that the main
                // run loop's autorelease pool reclaims each iteration. A bare
                // worker thread has no pool and would leak one per icon, so keep
                // it here — it's cheap now anyway (~1ms; it renders a small
                // CGImage rather than the full-resolution icon).
                let r = if let Some(bundle_id) = uri
                    .strip_prefix("cinch://app-icon/")
                    .or_else(|| uri.strip_prefix("cinch://app-icon\\"))
                {
                    crate::media::serve_app_icon(bundle_id)
                } else {
                    crate::media::MediaResponse {
                        status: 404,
                        content_type: "application/octet-stream",
                        body: Vec::new(),
                    }
                };
                responder.respond(build_media_response(r));
            }
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(multi_config_handle.clone())
        .manage(device_cache_handle.clone())
        .manage(ws_abort_handle.clone())
        .manage(pending_relay_add.clone())
        .manage(pending_auth_relay.clone())
        .manage(clipboard_service.clone())
        .manage(ws_status.clone())
        .manage(relay_connected.clone())
        .manage(auth_state_handle.clone())
        .manage(pending_codes_handle.clone())
        .manage(previous_app_pid.clone())
        .manage(commands::window::SnapState::new())
        .manage(commands::window::CopyToastState::new())
        // Phase 4: shared client-core Store, sync Writer, and LocalPusher
        .manage(shared_store)
        .manage(writer_handle)
        .manage(ClipNotifierTx(clip_notif_tx.clone()))
        .manage(DevicesChangedTx(devices_changed_tx.clone()))
        .manage(local_pusher_handle.clone())
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
                if window.label() == "main" {
                    crate::window_manage::set_dock_visible(window.app_handle(), false);
                }
            }
            tauri::WindowEvent::Moved(_) if window.label() == "main" => {
                commands::window::on_window_moved(window.app_handle());
            }
            _ => {}
        })
        .menu(app_menu::build_menu)
        .on_menu_event(app_menu::handle_menu_event)
        .invoke_handler(specta_builder.invoke_handler())
        .setup(move |app| {
            specta_builder.mount_events(app);

            let handle = app.handle();

            // Drain the NewClip notifier channel into Tauri's event bus. Both
            // the initial Writer (built before `tauri::Builder`) and any
            // Writer built by `restart_writer` push wire clips here; we map
            // them to a stub `LocalClip` payload so the React side can look
            // up alert settings by source and trigger an OS notification.
            {
                let app_for_consumer = handle.clone();
                let mut rx = clip_notif_rx;
                let clipboard_for_paste = clipboard_service.clone();
                let store_for_paste: crate::SharedStore =
                    app.state::<crate::SharedStore>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    while let Some(clip) = rx.recv().await {
                        let payload = clipboard::monitor::clip_received_stub(
                            &clip.clip_id,
                            &clip.source,
                            None,
                            None,
                            None,
                            clip.byte_size,
                            &clip.content_type,
                        );
                        let _ = crate::events::RemoteClipReceived(payload).emit(&app_for_consumer);

                        // Sync image clips into the local OS pasteboard so the
                        // user can paste into any app without clicking Copy in
                        // the in-app history UI. Text/code/url stay manual so
                        // browsing history does not stomp the local clipboard.
                        crate::clipboard::auto_paste::paste_incoming_image(
                            &clipboard_for_paste,
                            &store_for_paste,
                            &clip.clip_id,
                            &clip.content_type,
                        );
                    }
                });
            }

            // Consumer for the WS-connect → DevicesChanged path. Each `()` on
            // the channel becomes one DevicesChanged event. The mutation
            // commands emit DevicesChanged directly via their AppHandle, so
            // this consumer covers only the WS-side producer.
            {
                let app_for_devices = handle.clone();
                let mut rx = devices_changed_rx;
                tauri::async_runtime::spawn(async move {
                    while let Some(()) = rx.recv().await {
                        if let Err(e) = crate::events::DevicesChanged.emit(&app_for_devices) {
                            log::warn!("DevicesChanged emit failed: {}", e);
                        }
                    }
                });
            }

            // Periodic GitHub Releases refresh. Drives the per-device
            // version badge: the first iteration fires on launch and
            // every 6 hours after, so a long-running session always has
            // a current cache without the user clicking anything.
            {
                let app_for_refresh = handle.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        let updated =
                            crate::update_check::fetch_and_cache(app_for_refresh.clone()).await;
                        let _ =
                            crate::events::LatestVersionsUpdated(updated).emit(&app_for_refresh);
                        tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
                    }
                });
            }

            // Setup system tray
            tray::setup_tray(handle)?;
            commands::window::ensure_overlay(handle);
            commands::window::ensure_copy_toast(handle);

            // Register global shortcuts (⌘⇧V main window focus)
            window_manage::register_global_shortcuts(handle);
            // Register the opt-in "send current clipboard" shortcut (no-op if unset)
            window_manage::register_send_shortcut(handle);

            // Make the window movable by external window managers (Rectangle, Moom, etc.).
            // decorations:false sets NSWindowStyleMaskBorderless whose default is isMovable=false,
            // so Rectangle's AX-based "Move to Next Display" silently fails.
            window_manage::configure_macos_window(handle);

            // Run as a background menu-bar agent: no Dock icon, hidden from the
            // Cmd+Tab switcher, no top-left app menu. The tray status icon stays.
            // When a window opens the app flips to Regular and the Dock shows the
            // bundle icon (icons/icon.icns) — the same icon macOS renders in
            // notifications, so the two stay in sync. We deliberately do NOT
            // override applicationIconImage with a theme-specific variant: that
            // affects only the Dock/Cmd+Tab tile, not Notification Center, so it
            // would make the Dock and notification icons diverge in Light mode.
            window_manage::configure_activation_policy(handle);

            // Seed AuthState from persisted config. Plan 03 Task 2.
            {
                let auth_handle: AuthStateHandle = app.state::<AuthStateHandle>().inner().clone();
                let initial_state = if config_for_auth_seed.is_configured()
                    && !config_for_auth_seed.active_device_id.is_empty()
                {
                    AuthState::Authenticated {
                        user_id: config_for_auth_seed.user_id.clone(),
                        device_id: config_for_auth_seed.active_device_id.clone(),
                        hostname: config_for_auth_seed.hostname.clone(),
                        relay_url: config_for_auth_seed.relay_url.clone(),
                        active_relay_id: active_relay_id_seed.clone(),
                        machine_id: client_core::machine::stable_machine_id(),
                    }
                } else {
                    AuthState::LocalOnly
                };
                auth::transition(handle, &auth_handle, initial_state);
            }

            // Deep-link handler: cinch://auth/callback?token=X&device_id=Y&user_id=Z&relay_url=R
            // Handles the "hot app" case where the browser redirects while app is running.
            // The "cold start" case (app launched via URL) is handled by React calling
            // handle_deeplink via getCurrent().
            deep_link::install_deep_link_handler(
                app,
                auth_state_handle.clone(),
                ws_status.clone(),
                relay_connected.clone(),
                multi_config_handle.clone(),
                ws_abort_handle.clone(),
                pending_relay_add.clone(),
                pending_auth_relay.clone(),
            );

            if is_configured {
                // Show dashboard on launch
                show_on_active_monitor(handle);

                // Note: delta-sync of the legacy per-app clips.db has been removed.
                // The client_core::sync::Writer (started above, before the Tauri builder)
                // handles all REST backfill and live WS writes into the shared client-core
                // store (~/.cinch/store.db).
                let _ = (ws_relay_url, ws_token); // consumed by Writer above

                // Reflect the boot-time writer result into ws_status + tray. startup.rs
                // sets the WriterHandle but never touches ws_status, which is initialized
                // to "connecting" — so without this the tray would read "Connecting…"
                // forever until the next restart_writer (sign-in/token refresh).
                let writer_present = app
                    .state::<crate::app_state::WriterHandle>()
                    .lock()
                    .map(|g| g.is_some())
                    .unwrap_or(false);
                let initial_ws = if writer_present {
                    "connected"
                } else {
                    "connecting"
                };
                ws_status.set(initial_ws);
                let h = handle.clone();
                let ws_value = initial_ws.to_string();
                tauri::async_runtime::spawn(async move {
                    crate::events::WsStatus(ws_value.clone()).emit(&h).ok();
                    let auth_handle: AuthStateHandle = h.state::<AuthStateHandle>().inner().clone();
                    // Recover the snapshot even if the auth mutex was poisoned
                    // by a prior panic — never cascade a second panic here.
                    let snapshot = auth_handle
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .clone();
                    crate::tray::set_status(&h, &snapshot, &ws_value);
                });
            } else {
                // No config — show window immediately with setup instructions
                show_on_active_monitor(handle);
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    crate::events::WsStatus("unconfigured".into()).emit(&h).ok();
                    let auth_handle: AuthStateHandle = h.state::<AuthStateHandle>().inner().clone();
                    // Recover the snapshot even if the auth mutex was poisoned
                    // by a prior panic — never cascade a second panic here.
                    let snapshot = auth_handle
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .clone();
                    crate::tray::set_status(&h, &snapshot, "unconfigured");
                });
            }

            // Spawn local clipboard monitor — captures to local history only.
            // It never contacts the relay; a clip leaves the device only via
            // the explicit `send_clip` command. Runs regardless of auth.
            clipboard::monitor::spawn_clipboard_monitor(
                handle,
                clipboard_service.clone(),
                app.state::<crate::SharedStore>().inner().clone(),
            );

            // Spawn local retention sweep — purges clips older than the
            // local_retention_days setting (default 30) every hour. D-06.
            retention::spawn_retention_sweep(app.state::<crate::SharedStore>().inner().clone());

            // Spawn the FS watcher for cross-process credential propagation (AUTH-03).
            // Best-effort — if the watcher fails to start, the app still runs but without
            // cross-process propagation (desktop would require restart to see CLI changes).
            if let Err(e) =
                auth::spawn_credential_watcher(handle.clone(), auth_state_handle.clone())
            {
                log::warn!("credential watcher failed to start: {}", e);
            }

            // TTL sweeper: drop pending device-code entries older than 5 minutes
            // every 30 seconds.
            {
                let pending: crate::auth::state::PendingCodesHandle = app
                    .state::<crate::auth::state::PendingCodesHandle>()
                    .inner()
                    .clone();
                tauri::async_runtime::spawn(async move {
                    let ttl = std::time::Duration::from_secs(5 * 60);
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
                    // First tick fires immediately; skip it so we don't sweep before the app is ready.
                    tick.tick().await;
                    loop {
                        tick.tick().await;
                        crate::auth::state::sweep_expired(&pending, ttl);
                    }
                });
            }

            info!("Cinch desktop app started (configured={})", is_configured);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, event| match event {
            // Keep the app alive when macOS fires an implicit ExitRequested
            // (e.g., the last window closed) — the "close to tray" path.
            tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } => {
                api.prevent_exit();
            }
            // Explicit quit (tray "Quit Cinch" / Cmd+Q, code = Some): best-effort
            // flush of any buffered telemetry before the process exits. Bounded so
            // quit is never delayed more than briefly; all errors are swallowed.
            tauri::RunEvent::ExitRequested { code: Some(_), .. } => {
                tauri::async_runtime::block_on(telemetry::shutdown_flush(
                    std::time::Duration::from_millis(800),
                ));
            }
            _ => {}
        });
}
