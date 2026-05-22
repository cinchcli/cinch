//! Integration test for `client_core::sync::Writer`.
//!
//! Skipped unless both `RELAY_INTEGRATION_URL` and `RELAY_INTEGRATION_TOKEN`
//! are set.  Run against a real relay instance via:
//!
//!   RELAY_INTEGRATION_URL=http://localhost:8080 \
//!   RELAY_INTEGRATION_TOKEN=<token> \
//!   cargo test -p cinchcli-core --test sync_writer -- --ignored

use std::sync::Arc;

use client_core::http::RestClient;
use client_core::store::Store;
use client_core::sync::{LockKind, Writer};
use client_core::version::ClientInfo;
use client_core::ws::WsConfig;

#[tokio::test]
#[ignore = "requires RELAY_INTEGRATION_URL and RELAY_INTEGRATION_TOKEN"]
async fn writer_starts_and_stops() {
    let url = match std::env::var("RELAY_INTEGRATION_URL").ok() {
        Some(u) => u,
        None => return,
    };
    let token = match std::env::var("RELAY_INTEGRATION_TOKEN").ok() {
        Some(t) => t,
        None => return,
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(Store::open(dir.path().join("test.db").as_path()).expect("Store::open"));
    let client = Arc::new(
        RestClient::new(url.clone(), token.clone(), ClientInfo::for_test())
            .expect("RestClient::new"),
    );
    let ws_cfg = WsConfig {
        relay_url: url,
        token,
        encryption_key: None,
        client_info: None,
        media_fetcher: None,
    };

    let lock_path =
        std::env::temp_dir().join(format!("cinch-writer-test-{}.lock", std::process::id()));

    let writer = Writer::start(
        store,
        client,
        ws_cfg,
        lock_path.clone(),
        LockKind::Desktop,
        None,
        None,
    )
    .await
    .expect("Writer::start should not return an IO error");

    assert!(
        writer.is_some(),
        "writer should acquire the lock in a clean environment"
    );

    // Allow the initial backfill and WS connect attempt to settle briefly.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    writer.unwrap().shutdown().await;

    // Lock file should be release-able by a second writer after shutdown.
    let _ = std::fs::remove_file(&lock_path);
}

#[tokio::test]
#[ignore = "requires RELAY_INTEGRATION_URL and RELAY_INTEGRATION_TOKEN"]
async fn second_writer_cannot_acquire_held_lock() {
    let url = match std::env::var("RELAY_INTEGRATION_URL").ok() {
        Some(u) => u,
        None => return,
    };
    let token = match std::env::var("RELAY_INTEGRATION_TOKEN").ok() {
        Some(t) => t,
        None => return,
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let store1 =
        Arc::new(Store::open(dir.path().join("test1.db").as_path()).expect("Store::open 1"));
    let store2 =
        Arc::new(Store::open(dir.path().join("test2.db").as_path()).expect("Store::open 2"));

    let client1 = Arc::new(
        RestClient::new(url.clone(), token.clone(), ClientInfo::for_test())
            .expect("RestClient::new 1"),
    );
    let client2 = Arc::new(
        RestClient::new(url.clone(), token.clone(), ClientInfo::for_test())
            .expect("RestClient::new 2"),
    );

    let ws_cfg1 = WsConfig {
        relay_url: url.clone(),
        token: token.clone(),
        encryption_key: None,
        client_info: None,
        media_fetcher: None,
    };
    let ws_cfg2 = WsConfig {
        relay_url: url,
        token,
        encryption_key: None,
        client_info: None,
        media_fetcher: None,
    };

    let lock_path =
        std::env::temp_dir().join(format!("cinch-writer-test2-{}.lock", std::process::id()));

    let writer1 = Writer::start(
        store1,
        client1,
        ws_cfg1,
        lock_path.clone(),
        LockKind::Desktop,
        None,
        None,
    )
    .await
    .expect("first Writer::start should succeed");
    assert!(writer1.is_some(), "first writer should acquire the lock");

    let writer2 = Writer::start(
        store2,
        client2,
        ws_cfg2,
        lock_path.clone(),
        LockKind::Cli,
        None,
        None,
    )
    .await
    .expect("second Writer::start should not return an IO error");
    assert!(
        writer2.is_none(),
        "second writer must not acquire a lock already held"
    );

    writer1.unwrap().shutdown().await;
    let _ = std::fs::remove_file(&lock_path);
}
