pub mod auth;
mod auth_bootstrap;
mod clipboard;
mod commands;
pub mod crypto;
pub mod events;
pub mod media;
pub mod protocol;
mod store;
mod sync_status;
pub mod telemetry;
mod tray;
pub mod update_check;
mod window_snap;

#[cfg(test)]
mod tests;

use log::info;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri_specta::{collect_commands, collect_events, Builder, Event};

use auth::state::PendingCodesHandle;
use auth::{AuthState, AuthStateHandle};
use protocol::MultiConfigHandle;

/// Handle to the shared `client_core` store (new Phase 4 store).
/// Commands that need to read/write the new store access this via Tauri state.
pub type SharedStore = Arc<client_core::store::Store>;

/// The long-lived sync writer started at startup.  Wrapped in `Mutex<Option<…>>`
/// so the shutdown path can `take()` it and call `Writer::shutdown`.
pub type WriterHandle = Mutex<Option<client_core::sync::Writer>>;

/// Local-clip ingest pipeline (encrypt + push to relay + write-through to
/// shared store). Lives independently of `Writer` so reader-mode desktops
/// (lock held by another process) can still publish locally-detected clips.
/// Wrapped so `restart_writer` can swap it on credential change.
pub type LocalPusherHandle = Arc<Mutex<Option<client_core::sync::LocalPusher>>>;

pub type PreviousAppPid = Arc<Mutex<Option<i32>>>;

/// Builds the `ClientInfo` block that identifies this desktop binary to
/// `cinch-core`'s REST + WS clients. Cinch-core attaches it as HTTP
/// headers and as the WS `client_hello` payload, so the relay can
/// persist the per-device version row used by `cinch devices` and the
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
fn build_offline_pusher_handle(
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

/// Sender side of the channel that forwards remote `NewClip` notifications
/// from `client_core::sync::Writer`'s `on_new_clip` callback into Tauri's
/// event bus. Stored in Tauri state so `restart_writer` can rebuild the
/// callback with the same delivery target after a credential swap.
pub(crate) struct ClipNotifierTx(
    pub(crate) tokio::sync::mpsc::UnboundedSender<client_core::protocol::Clip>,
);

pub fn make_specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::clips::list_clips,
            commands::clips::list_pinned_clips,
            commands::clips::pin_clip,
            commands::clips::unpin_clip,
            commands::clips::search_clips,
            commands::clips::get_sources,
            commands::clips::delete_clip,
            commands::clips::get_clip_count,
            commands::clips::get_config_info,
            commands::clips::get_source_auto_copy,
            commands::clips::set_source_auto_copy,
            commands::clips::get_all_source_settings,
            commands::clips::get_source_alert_enabled,
            commands::clips::set_source_alert_enabled,
            commands::clips::get_all_source_alert_settings,
            commands::clips::mark_clip_copied,
            commands::clips::copy_clip_to_clipboard,
            commands::clips::copy_image_to_clipboard,
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
            commands::window::snap_drag_start,
        ])
        .events(collect_events![
            events::AuthStateChanged,
            events::WsStatus,
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

    // Open local database
    let db_path = dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/share"))
        .join("com.cinch.app")
        .join("clips.db");

    let db = match store::db::Database::open(&db_path) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!("ERROR: failed to open database: {}", e);
            std::process::exit(1);
        }
    };

    let ws_relay_url = config.relay_url.clone();
    let ws_token = config.token.clone();
    let config_for_auth_seed = config.clone();

    // ── Phase 4: open shared client-core Store at ~/.cinch/store.db ──────────
    // This is the unified store shared between the desktop and the CLI writer.
    // The legacy com.cinch.app/clips.db remains in use by existing commands
    // (Task 4.2 will migrate those commands to use this store directly).
    let shared_store: SharedStore = match client_core::store::default_db_path() {
        Ok(path) => match client_core::store::Store::open(&path) {
            Ok(s) => {
                info!("client-core store opened at {}", path.display());
                Arc::new(s)
            }
            Err(e) => {
                log::warn!("client-core store open failed (non-fatal): {}", e);
                // Construct an in-memory fallback so the app still starts.
                Arc::new(
                    client_core::store::Store::open(std::path::Path::new(":memory:"))
                        .expect("in-memory store"),
                )
            }
        },
        Err(e) => {
            log::warn!("cannot resolve store path (non-fatal): {}", e);
            Arc::new(
                client_core::store::Store::open(std::path::Path::new(":memory:"))
                    .expect("in-memory store"),
            )
        }
    };

    // ── Phase 4: build WsConfig, start sync::Writer, and build LocalPusher ───
    // We do this in the outer run() scope (before Tauri's setup hook) so the
    // writer is started exactly once at launch with the credentials that were
    // live at startup.  The writer handle is moved into managed state so Tauri
    // keeps it alive for the full process lifetime.
    //
    // The LocalPusher is built independently — it does not require the lock,
    // so a reader-mode desktop (lock held by CLI/another desktop) can still
    // push locally-detected clips. Both handles are swapped together by
    // `restart_writer` on credential changes.
    // Build the NewClip notifier channel before Writer::start so the initial
    // writer — spawned synchronously below, before Tauri's AppHandle exists —
    // can deliver remote clip arrivals to a consumer task that we'll spawn
    // inside `.setup()` once `app.handle()` is available.
    let (clip_notif_tx, clip_notif_rx) =
        tokio::sync::mpsc::unbounded_channel::<client_core::protocol::Clip>();

    let (writer_handle, local_pusher_handle): (WriterHandle, LocalPusherHandle) = {
        if is_configured && !config.token.is_empty() && !config.relay_url.is_empty() {
            let enc_key = client_core::credstore::read_encryption_key(&config.user_id);
            let ws_cfg = client_core::ws::WsConfig {
                relay_url: config.relay_url.clone(),
                token: config.token.clone(),
                encryption_key: enc_key,
                client_info: Some(build_client_info()),
            };
            match client_core::http::RestClient::new(
                config.relay_url.clone(),
                config.token.clone(),
                build_client_info(),
            ) {
                Ok(rest_client) => {
                    let rest_arc = Arc::new(rest_client);
                    let pusher = client_core::sync::LocalPusher::new(
                        shared_store.clone(),
                        rest_arc.clone(),
                        enc_key,
                    );
                    let store_for_writer = shared_store.clone();
                    let lock_p = client_core::store::lock_path()
                        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/cinch.lock"));
                    let initial_cb_tx = clip_notif_tx.clone();
                    let on_new_clip: client_core::sync::OnNewClipCallback = Arc::new(move |clip| {
                        let _ = initial_cb_tx.send(clip.clone());
                    });
                    // T2: backlog flush on every WS (re)connect. The callback
                    // fires on initial connect AND after every reconnect, so
                    // any clips queued while offline get flushed.
                    let on_connected: Option<client_core::sync::OnConnectedCallback> = if let Some(
                        key,
                    ) =
                        enc_key
                    {
                        let store_cb = shared_store.clone();
                        let rest_cb = rest_arc.clone();
                        Some(Arc::new(move || {
                            let store = store_cb.clone();
                            let rest = rest_cb.clone();
                            tauri::async_runtime::spawn(async move {
                                match client_core::sync::flush_once(&store, &rest, key).await {
                                    Ok(report) => {
                                        if report.flushed > 0 || report.dropped > 0 {
                                            log::info!(
                                                    "desktop reconnect flush: flushed={} dropped={} remaining={}",
                                                    report.flushed,
                                                    report.dropped,
                                                    report.remaining,
                                                );
                                        }
                                    }
                                    Err(e) => log::debug!("desktop reconnect flush failed: {}", e),
                                }
                            });
                        }))
                    } else {
                        None
                    };
                    let writer =
                        match tauri::async_runtime::block_on(client_core::sync::Writer::start(
                            store_for_writer,
                            rest_arc.clone(),
                            ws_cfg,
                            lock_p,
                            client_core::sync::LockKind::Desktop,
                            Some(on_new_clip),
                            on_connected,
                        )) {
                            Ok(Some(w)) => {
                                info!("client-core sync::Writer started");
                                Mutex::new(Some(w))
                            }
                            Ok(None) => {
                                log::warn!("sync::Writer: lock held by another process, skipping");
                                Mutex::new(None)
                            }
                            Err(e) => {
                                log::warn!("sync::Writer::start failed (non-fatal): {}", e);
                                Mutex::new(None)
                            }
                        };

                    // T1: backlog flush at boot. Idempotent and cheap when the
                    // queue is empty, so it's safe to run unconditionally as
                    // long as we have an encryption key. Drains anything left
                    // behind by a previous offline session before the WS
                    // (re)connect-driven flush gets a chance to fire.
                    if let Some(key) = enc_key {
                        let store_for_flush = shared_store.clone();
                        let rest_for_flush = rest_arc.clone();
                        tauri::async_runtime::spawn(async move {
                            match client_core::sync::flush_once(
                                &store_for_flush,
                                &rest_for_flush,
                                key,
                            )
                            .await
                            {
                                Ok(report) => {
                                    if report.flushed > 0 || report.dropped > 0 {
                                        log::info!(
                                            "desktop boot flush: flushed={} dropped={} remaining={}",
                                            report.flushed,
                                            report.dropped,
                                            report.remaining,
                                        );
                                    }
                                }
                                Err(e) => log::debug!("desktop boot flush failed: {}", e),
                            }
                        });
                    }

                    (writer, Arc::new(Mutex::new(Some(pusher))))
                }
                Err(e) => {
                    log::warn!("cannot build RestClient for Writer (non-fatal): {}", e);
                    // Even without a working RestClient we still want a
                    // LocalPusher so captures queue locally (the pusher
                    // short-circuits to enqueue_local when enc_key is None
                    // and on any transient relay error otherwise).
                    let pusher_handle = build_offline_pusher_handle(&shared_store, &config);
                    (Mutex::new(None), pusher_handle)
                }
            }
        } else {
            // Pre-login: construct a LocalPusher with no key so captures queue
            // locally. The RestClient is never invoked because LocalPusher
            // short-circuits to enqueue_local when enc_key is None.
            let pusher_handle = build_offline_pusher_handle(&shared_store, &config);
            (Mutex::new(None), pusher_handle)
        }
    };

    let multi_config_handle: MultiConfigHandle = Arc::new(Mutex::new(multi_config));
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

    tauri::Builder::default()
        .register_uri_scheme_protocol("cinch", {
            move |app, request| {
                let uri = request.uri().to_string();
                let clip_id = uri
                    .strip_prefix("cinch://media/")
                    .or_else(|| uri.strip_prefix("cinch://media\\"))
                    .unwrap_or("");
                let store = app.app_handle().state::<crate::SharedStore>();
                let r = crate::media::serve_clip_image(store.inner(), clip_id);
                tauri::http::Response::builder()
                    .status(r.status)
                    .header("Content-Type", r.content_type)
                    .body(r.body)
                    .unwrap()
            }
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(db.clone())
        .manage(multi_config_handle.clone())
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
        // Phase 4: shared client-core Store, sync Writer, and LocalPusher
        .manage(shared_store)
        .manage(writer_handle)
        .manage(ClipNotifierTx(clip_notif_tx.clone()))
        .manage(local_pusher_handle.clone())
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            tauri::WindowEvent::Moved(_) if window.label() == "main" => {
                commands::window::on_window_moved(window.app_handle());
            }
            _ => {}
        })
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
                tauri::async_runtime::spawn(async move {
                    while let Some(clip) = rx.recv().await {
                        let payload = clipboard::monitor::clip_received_stub(
                            &clip.clip_id,
                            &clip.source,
                            clip.byte_size,
                            &clip.content_type,
                        );
                        let _ = crate::events::RemoteClipReceived(payload).emit(&app_for_consumer);
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

            // Register global shortcuts (⌘⇧V main window focus)
            register_global_shortcuts(handle);

            // Make the window movable by external window managers (Rectangle, Moom, etc.).
            // decorations:false sets NSWindowStyleMaskBorderless whose default is isMovable=false,
            // so Rectangle's AX-based "Move to Next Display" silently fails.
            configure_macos_window(handle);

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
            {
                use tauri_plugin_deep_link::DeepLinkExt;

                let dl_auth_handle = auth_state_handle.clone();
                let dl_app_handle = app.handle().clone();
                let dl_ws_status = ws_status.clone();
                let dl_relay_connected = relay_connected.clone();
                let dl_mc = multi_config_handle.clone();
                let dl_ws_abort = ws_abort_handle.clone();
                let dl_pending = pending_relay_add.clone();
                let dl_pending_auth = pending_auth_relay.clone();
                app.deep_link().on_open_url(move |event| {
                    let urls = event.urls();
                    for url in &urls {
                        // CLI handoff route: `cinch://login?relay=…&from=cli`.
                        // Focus the main window and emit an event so the React
                        // layer opens the AddRelayDialog with the relay
                        // pre-filled. No credential write here — the user
                        // still has to complete OAuth in the dialog.
                        let is_login = url.host_str() == Some("login")
                            || url.path() == "/login"
                            || (url.scheme() == "cinch" && url.path() == "/login");
                        if is_login {
                            if let Some(window) = dl_app_handle.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                            let relay = url
                                .query_pairs()
                                .find(|(k, _)| k == "relay")
                                .map(|(_, v)| v.to_string())
                                .unwrap_or_default();
                            if let Err(e) =
                                (crate::events::CliHandoffRequested { relay_url: relay })
                                    .emit(&dl_app_handle)
                            {
                                log::warn!("emit CliHandoffRequested failed: {}", e);
                            }
                            continue;
                        }

                        let is_auth =
                            url.host_str() == Some("auth") || url.path() == "/auth/callback";
                        if !is_auth {
                            continue;
                        }

                        let token = url
                            .query_pairs()
                            .find(|(k, _)| k == "token")
                            .map(|(_, v)| v.to_string());
                        let device_id = url
                            .query_pairs()
                            .find(|(k, _)| k == "device_id")
                            .map(|(_, v)| v.to_string());
                        let user_id = url
                            .query_pairs()
                            .find(|(k, _)| k == "user_id")
                            .map(|(_, v)| v.to_string());
                        let relay_url = url
                            .query_pairs()
                            .find(|(k, _)| k == "relay_url")
                            .map(|(_, v)| v.to_string());

                        if let (Some(token), Some(device_id), Some(user_id)) =
                            (token, device_id, user_id)
                        {
                            if token.len() != 64 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
                                log::warn!("deep-link: rejected malformed token");
                                return;
                            }

                            let relay =
                                relay_url.unwrap_or_else(|| "https://api.cinchcli.com".to_string());

                            if let Err(e) = crate::validate_relay_url(&relay) {
                                log::warn!("deep-link: rejected invalid relay_url: {}", e);
                                return;
                            }

                            let hostname = client_core::machine::hostname_or_unknown();

                            let pending_info = dl_pending.take();
                            let active_relay_id = if let Some(info) = pending_info {
                                match crate::auth::add_relay_profile(
                                    &user_id,
                                    &device_id,
                                    &token,
                                    &relay,
                                    &hostname,
                                    info.label.as_deref(),
                                    "",
                                ) {
                                    Ok(relay_id) => {
                                        if let Ok(new_mc) = crate::auth::load_multi_config() {
                                            let mut g = dl_mc.lock().unwrap();
                                            *g = new_mc;
                                        }
                                        relay_id
                                    }
                                    Err(e) => {
                                        log::error!("deep-link add_relay_profile failed: {}", e);
                                        return;
                                    }
                                }
                            } else {
                                // Security: require a pending standard-auth relay URL that
                                // matches the callback.  Rejects crafted deep-links that
                                // arrive with no prior login being initiated (Finding 1).
                                // I3: peek first so a junk deep-link cannot consume the
                                // pending state before the legitimate callback arrives.
                                let pending_auth_url = dl_pending_auth.peek();
                                if let Err(reason) = crate::validate_auth_callback(
                                    pending_auth_url.as_deref(),
                                    &relay,
                                ) {
                                    log::warn!("deep-link: {}", reason);
                                    return;
                                }
                                // Validation passed — now consume the pending state.
                                dl_pending_auth.clear();

                                if let Err(e) = client_core::auth_session::install_credentials(
                                    client_core::auth_session::InstallParams {
                                        user_id: &user_id,
                                        device_id: &device_id,
                                        token: &token,
                                        relay_url: &relay,
                                        hostname: &hostname,
                                        device_private_key: None,
                                        email: "",
                                        identity_provider: "",
                                        display_name: "",
                                    },
                                ) {
                                    log::error!("deep-link install_credentials failed: {}", e);
                                    return;
                                }
                                let relay_id = crate::auth::load_multi_config()
                                    .ok()
                                    .and_then(|mc| {
                                        let id = mc.active_relay_id.clone();
                                        let mut g = dl_mc.lock().unwrap();
                                        *g = mc;
                                        id
                                    })
                                    .unwrap_or_default();
                                relay_id
                            };

                            crate::auth::transition(
                                &dl_app_handle,
                                &dl_auth_handle,
                                crate::auth::AuthState::Authenticated {
                                    user_id: user_id.clone(),
                                    device_id: device_id.clone(),
                                    hostname: hostname.clone(),
                                    relay_url: relay.clone(),
                                    active_relay_id: active_relay_id.clone(),
                                    machine_id: client_core::machine::stable_machine_id(),
                                },
                            );

                            // Restart the client-core Writer with the new credentials.
                            let app_for_writer = dl_app_handle.clone();
                            let writer_relay = relay.clone();
                            let writer_token = token.clone();
                            let dl_ws_status2 = dl_ws_status.clone();
                            let dl_relay_connected2 = dl_relay_connected.clone();
                            let jh = tauri::async_runtime::spawn(async move {
                                if let Err(e) = restart_writer(
                                    &app_for_writer,
                                    &writer_relay,
                                    &writer_token,
                                    &dl_ws_status2,
                                    &dl_relay_connected2,
                                )
                                .await
                                {
                                    log::error!("deep-link: restart_writer failed: {}", e);
                                }
                            });
                            dl_ws_abort.replace(jh);

                            log::info!(
                                "deep-link auth complete: user={}, device={}, relay_id={}",
                                user_id,
                                device_id,
                                active_relay_id,
                            );
                        }
                    }
                });
            }

            if is_configured {
                // Show dashboard on launch
                show_on_active_monitor(handle);

                // Note: delta-sync of the legacy com.cinch.app/clips.db has been removed.
                // The client_core::sync::Writer (started above, before the Tauri builder)
                // handles all REST backfill and live WS writes into the shared client-core
                // store (~/.cinch/store.db). Task 4.3 will delete ws.rs once that path
                // is confirmed stable in production.
                let _ = (ws_relay_url, ws_token); // consumed by Writer above
            } else {
                // No config — show window immediately with setup instructions
                show_on_active_monitor(handle);
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    crate::events::WsStatus("unconfigured".into()).emit(&h).ok();
                });
            }

            // Spawn local clipboard monitor — always runs (relay-independent).
            // The LocalPusher handle drives encrypt + push + store-write; if it
            // is `None` (unconfigured) the monitor short-circuits and drops the
            // capture rather than writing plaintext locally.
            clipboard::monitor::spawn_clipboard_monitor(
                handle,
                db.clone(),
                clipboard_service.clone(),
                local_pusher_handle.clone(),
            );

            // Spawn local retention sweep — purges clips older than the
            // local_retention_days setting (default 30) every hour. D-06.
            spawn_retention_sweep(db.clone());

            // Spawn the FS watcher for cross-process credential propagation (AUTH-03).
            // Best-effort — if the watcher fails to start, the app still runs but without
            // cross-process propagation (desktop would require restart to see CLI changes).
            if let Err(e) =
                auth::spawn_credential_watcher(handle.clone(), auth_state_handle.clone())
            {
                log::warn!("credential watcher failed to start: {}", e);
            }

            // TTL sweeper: drop pending device-code entries older than 5 minutes
            // every 30 seconds; refresh tray badge when the count changes.
            {
                let pending: crate::auth::state::PendingCodesHandle = app
                    .state::<crate::auth::state::PendingCodesHandle>()
                    .inner()
                    .clone();
                let sweeper_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let ttl = std::time::Duration::from_secs(5 * 60);
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
                    // First tick fires immediately; skip it so we don't sweep before the app is ready.
                    tick.tick().await;
                    loop {
                        tick.tick().await;
                        let before = crate::auth::state::pending_count(&pending);
                        crate::auth::state::sweep_expired(&pending, ttl);
                        let after = crate::auth::state::pending_count(&pending);
                        if before != after {
                            crate::tray::set_pending_count(&sweeper_handle, after);
                        }
                    }
                });
            }

            info!("Cinch desktop app started (configured={})", is_configured);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}

/// Spawn the local retention sweep — purges clips older than the
/// configured `local_retention_days` setting (default 30 per D-05).
///
/// Cadence: hourly (D-06). Uses `MissedTickBehavior::Skip` so a laptop
/// that slept for 45 days does not trigger 45 back-to-back sweeps —
/// the next aligned tick suffices.
///
/// First tick fires immediately (tokio's documented behavior) — this
/// catches stale clips that accumulated while the app was quit longer
/// than the retention window. Intentional per RESEARCH.md Open Question 1.
fn spawn_retention_sweep(db: Arc<store::db::Database>) {
    tauri::async_runtime::spawn(async move {
        const DEFAULT_RETENTION_DAYS: i64 = 30;
        const SWEEP_INTERVAL_SECS: u64 = 60 * 60;

        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(SWEEP_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        log::info!(
            "retention sweep started (interval = {}s, default = {}d)",
            SWEEP_INTERVAL_SECS,
            DEFAULT_RETENTION_DAYS,
        );

        loop {
            interval.tick().await; // first tick fires immediately — intentional
            let days = match db.get_setting("local_retention_days") {
                Ok(Some(v)) => v.parse::<i64>().unwrap_or(DEFAULT_RETENTION_DAYS),
                _ => DEFAULT_RETENTION_DAYS,
            };
            let cutoff = chrono::Utc::now().timestamp() - days * 86_400;
            match db.purge_before(cutoff) {
                Ok(n) if n > 0 => {
                    log::info!("retention sweep deleted {} clips older than {}d", n, days,)
                }
                Ok(_) => {}
                Err(e) => log::error!("retention sweep failed: {}", e),
            }
        }
    });
}

/// Show the main window centered on the monitor that currently has the mouse cursor.
/// Falls back to simple show+focus if cursor or monitor data is unavailable.
pub(crate) fn show_on_active_monitor(app: &tauri::AppHandle) {
    // Capture the frontmost app before Cinch steals focus, so we can restore it on copy.
    #[cfg(target_os = "macos")]
    capture_frontmost_app_pid(app);

    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    let result = (|| -> tauri::Result<()> {
        let cursor = app.cursor_position()?;
        let monitors = app.available_monitors()?;
        let boxes: Vec<crate::window_snap::MonitorBox> =
            monitors.iter().map(commands::window::to_box).collect();

        let s = window
            .outer_size()
            .unwrap_or(tauri::PhysicalSize::new(960, 600));
        let win = crate::window_snap::WinSize {
            w: s.width as i32,
            h: s.height as i32,
        };

        let saved = app
            .try_state::<Arc<store::db::Database>>()
            .and_then(|db| commands::window::load_placement(&db));

        // Always reposition: choose_placement restores the saved per-monitor
        // placement, else anchors on the cursor/first monitor, bottoming out
        // at (0,0) only when no monitors are reported (degenerate/headless).
        let (x, y) =
            crate::window_snap::choose_placement(saved.as_ref(), &boxes, (cursor.x, cursor.y), win);
        window.set_position(tauri::PhysicalPosition::new(x, y))?;
        Ok(())
    })();

    if let Err(e) = result {
        log::warn!("show_on_active_monitor: could not reposition window: {}", e);
    }

    let _ = window.show();
    // Promote the whole app above other apps before focusing the window —
    // `set_focus` alone only reorders within the active app on macOS.
    #[cfg(target_os = "macos")]
    activate_self();
    let _ = window.set_focus();
}

/// Configure the NSWindow so that external window managers (Rectangle, Moom, etc.) can
/// move it via the Accessibility API.
///
/// `decorations: false` produces NSWindowStyleMaskBorderless, whose macOS default is
/// `isMovable = false`. Rectangle calls `AXUIElementSetAttributeValue(kAXPositionAttribute)`
/// which silently no-ops when `isMovable` is false. Setting it to true fixes "Move to
/// Next/Previous Display" while leaving mouse drag behavior unchanged.
///
/// NSWindowCollectionBehaviorManaged (bit 2) makes the window appear in Mission Control
/// and participate in Spaces, which some window managers require before they will manage it.
/// Captures the pid of the macOS frontmost application and stores it in PreviousAppPid state.
#[cfg(target_os = "macos")]
fn capture_frontmost_app_pid(app: &tauri::AppHandle) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    let pid: i32 = unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let frontmost: *mut Object = msg_send![workspace, frontmostApplication];
        if frontmost.is_null() {
            return;
        }
        msg_send![frontmost, processIdentifier]
    };

    if let Some(state) = app.try_state::<PreviousAppPid>() {
        if let Ok(mut guard) = state.lock() {
            *guard = Some(pid);
        }
    }
}

/// Activates a macOS application by its process identifier.
#[cfg(target_os = "macos")]
pub(crate) fn activate_app_by_pid(pid: i32) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let app: *mut Object =
            msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid];
        if app.is_null() {
            return;
        }
        // NSApplicationActivateIgnoringOtherApps = 2
        let _: bool = msg_send![app, activateWithOptions: 2u64];
    }
}

/// Brings the current process to the front on macOS.
///
/// `NSWindow.makeKeyAndOrderFront:` (what Tauri's `set_focus` calls) only reorders
/// windows *within* the active application. If another app is frontmost when the
/// global shortcut fires, the Cinch window appears layered between that app's
/// windows instead of on top of everything. Activating the running application
/// itself promotes Cinch above all other apps in the global window order.
#[cfg(target_os = "macos")]
fn activate_self() {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let app: *mut Object = msg_send![class!(NSRunningApplication), currentApplication];
        if app.is_null() {
            return;
        }
        // NSApplicationActivateIgnoringOtherApps = 2
        let _: bool = msg_send![app, activateWithOptions: 2u64];
    }
}

fn configure_macos_window(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        use objc::runtime::{Object, YES};
        use objc::{msg_send, sel, sel_impl};

        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        let Ok(ns_window_ptr) = window.ns_window() else {
            return;
        };
        unsafe {
            let ns_window = ns_window_ptr as *mut Object;
            // Allow AX-based moves (fixes Rectangle "Move to Next/Prev Display")
            let _: () = msg_send![ns_window, setMovable: YES];
            // NSWindowCollectionBehaviorManaged=4, NSWindowCollectionBehaviorParticipatesInCycle=32
            let behavior: u64 = (1 << 2) | (1 << 5);
            let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
        }
    }
}

fn register_global_shortcuts(app: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    // Read persisted shortcut preference, fall back to default (D-08)
    let shortcut_str = app
        .state::<Arc<store::db::Database>>()
        .get_setting("global_shortcut")
        .ok()
        .flatten()
        .unwrap_or_else(|| "CmdOrCtrl+Shift+W".to_string());

    let handle = app.clone();
    if let Err(e) =
        app.global_shortcut()
            .on_shortcut(shortcut_str.as_str(), move |_app, shortcut, event| {
                if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                    info!("global shortcut pressed: {}", shortcut);
                    show_on_active_monitor(&handle);
                }
            })
    {
        log::warn!(
            "failed to register {} shortcut: {} (may conflict with another app)",
            shortcut_str,
            e
        );
    }
}

/// Validate that a relay URL uses http(s) and has a non-empty host.
/// Prevents deep-link injection where relay_url points to attacker infrastructure.
pub(crate) fn validate_relay_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| format!("invalid relay URL: {}", url))?;
    match parsed.scheme() {
        "https" | "http" => {}
        s => return Err(format!("relay URL scheme must be http(s), got: {}", s)),
    }
    if parsed.host().is_none() {
        return Err("relay URL must have a host".into());
    }
    Ok(())
}

/// Validate an incoming `cinch://auth/callback` deep-link against the relay URL
/// that was recorded when the user actually initiated a login.
///
/// Returns `Ok(())` only when:
/// - `pending_relay_url` is `Some` (a login was actively in progress), AND
/// - it matches `callback_relay_url` exactly (prevents relay-substitution attacks).
///
/// This is a pure function so it can be unit-tested without a running Tauri app.
pub(crate) fn validate_auth_callback(
    pending_relay_url: Option<&str>,
    callback_relay_url: &str,
) -> Result<(), &'static str> {
    match pending_relay_url {
        None => Err("no pending auth — deep-link rejected (no login was initiated)"),
        Some(pending) => {
            let pending_norm = pending.trim_end_matches('/');
            let callback_norm = callback_relay_url.trim_end_matches('/');
            if pending_norm != callback_norm {
                Err("relay_url mismatch — deep-link rejected (possible relay-substitution attack)")
            } else {
                Ok(())
            }
        }
    }
}

// delta_sync and tombstone_sync were removed in Task 4.2.
// Backfill into the shared client-core store is now handled exclusively by
// client_core::sync::Writer (started at app launch in run()). The legacy
// com.cinch.app/clips.db no longer receives startup syncs.

/// Replace the active `client_core::sync::Writer` with a fresh one built from
/// new credentials.  Called from the deep-link auth-callback handler (and from
/// `commands/auth.rs` / `commands/relays.rs`) after a fresh sign-in so the
/// writer reconnects to the relay with the updated token.
///
/// Shuts down the previous writer (releasing the lock) before starting the
/// new one.  There is a brief window between the two where no writer holds the
/// lock; that is acceptable — a second desktop instance that swoops in will
/// simply become writer and the first will fall back to reader on next start.
pub(crate) async fn restart_writer(
    app: &tauri::AppHandle,
    relay_url: &str,
    token: &str,
    ws_status: &std::sync::Arc<sync_status::WsStatus>,
    relay_connected: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    use tauri::Manager;

    // Resolve the user_id for the encryption key lookup.
    let user_id = {
        let mc = app.state::<crate::protocol::MultiConfigHandle>();
        let guard = mc.lock().unwrap();
        let id = guard
            .active_profile()
            .map(|p| p.user_id.clone())
            .unwrap_or_default();
        id
    };

    let enc_key = client_core::credstore::read_encryption_key(&user_id);
    let ws_cfg = client_core::ws::WsConfig {
        relay_url: relay_url.to_string(),
        token: token.to_string(),
        encryption_key: enc_key,
        client_info: Some(build_client_info()),
    };

    let rest = client_core::http::RestClient::new(
        relay_url.to_string(),
        token.to_string(),
        build_client_info(),
    )
    .map_err(|e| e.to_string())?;
    let rest_arc = std::sync::Arc::new(rest);

    let store: crate::SharedStore = app.state::<crate::SharedStore>().inner().clone();
    let lock_path = client_core::store::lock_path()
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/cinch.lock"));

    // Rebuild the LocalPusher with the new credentials so the next clipboard
    // capture pushes through the live token. Done before swapping the Writer
    // so a capture racing the swap still has a working pusher.
    {
        let pusher = client_core::sync::LocalPusher::new(store.clone(), rest_arc.clone(), enc_key);
        let handle = app.state::<crate::LocalPusherHandle>();
        let mut guard = handle.lock().map_err(|e| e.to_string())?;
        *guard = Some(pusher);
    }

    // T3: backlog flush after credentials propagate. Drains anything the
    // pre-login (no-key) pusher queued locally, plus any leftover unsynced
    // rows from a previous offline session, as soon as a usable enc_key
    // becomes available.
    if let Some(key) = enc_key {
        let store_for_flush = store.clone();
        let rest_for_flush = rest_arc.clone();
        tauri::async_runtime::spawn(async move {
            match client_core::sync::flush_once(&store_for_flush, &rest_for_flush, key).await {
                Ok(report) => {
                    if report.flushed > 0 || report.dropped > 0 {
                        log::info!(
                            "desktop credential-propagate flush: flushed={} dropped={} remaining={}",
                            report.flushed,
                            report.dropped,
                            report.remaining,
                        );
                    }
                }
                Err(e) => log::debug!("desktop credential-propagate flush failed: {}", e),
            }
        });
    }

    // Shut down the old writer first so it releases the lockfile.
    // Take the Writer out while holding the lock, then drop the lock before
    // calling shutdown().await — std::sync::MutexGuard is not Send and must
    // not be held across an await point.
    let old_writer = {
        let writer_handle = app.state::<crate::WriterHandle>();
        let mut guard = writer_handle.lock().map_err(|e| e.to_string())?;
        guard.take()
    };
    if let Some(w) = old_writer {
        w.shutdown().await;
    }

    ws_status.set("connecting");
    relay_connected.store(false, Ordering::Relaxed);

    // Forward NewClip notifications from the rebuilt Writer through the same
    // mpsc channel that the consumer task spawned in `.setup` is draining,
    // so per-source desktop alerts keep firing after a credential swap.
    let cb_tx = app.state::<crate::ClipNotifierTx>().inner().0.clone();
    let on_new_clip: client_core::sync::OnNewClipCallback = std::sync::Arc::new(move |clip| {
        let _ = cb_tx.send(clip.clone());
    });

    // T2: backlog flush on every WS (re)connect for the rebuilt Writer.
    let on_connected: Option<client_core::sync::OnConnectedCallback> = if let Some(key) = enc_key {
        let store_cb = store.clone();
        let rest_cb = rest_arc.clone();
        Some(std::sync::Arc::new(move || {
            let store = store_cb.clone();
            let rest = rest_cb.clone();
            tauri::async_runtime::spawn(async move {
                match client_core::sync::flush_once(&store, &rest, key).await {
                    Ok(report) => {
                        if report.flushed > 0 || report.dropped > 0 {
                            log::info!(
                                "desktop reconnect flush: flushed={} dropped={} remaining={}",
                                report.flushed,
                                report.dropped,
                                report.remaining,
                            );
                        }
                    }
                    Err(e) => log::debug!("desktop reconnect flush failed: {}", e),
                }
            });
        }))
    } else {
        None
    };

    match client_core::sync::Writer::start(
        store,
        rest_arc,
        ws_cfg,
        lock_path,
        client_core::sync::LockKind::Desktop,
        Some(on_new_clip),
        on_connected,
    )
    .await
    .map_err(|e| e.to_string())?
    {
        Some(new_writer) => {
            let writer_handle = app.state::<crate::WriterHandle>();
            let mut guard = writer_handle.lock().map_err(|e| e.to_string())?;
            *guard = Some(new_writer);
            ws_status.set("connected");
            relay_connected.store(true, Ordering::Relaxed);
            log::info!("restart_writer: new Writer started for relay={}", relay_url);
        }
        None => {
            log::warn!("restart_writer: lock held by another process — running as reader");
        }
    }

    Ok(())
}
