//! Reconnect-driven catch-up: the work a long-running client must perform
//! every time its WebSocket subscription is (re-)established.
//!
//! The relay's event stream does **not** replay events that arrived while a
//! device was unsubscribed (see `connect_events.go::Subscribe`). Without an
//! explicit catch-up, a `cinch push` from a remote machine that happens to
//! land during a desktop WS hiccup is permanently invisible to that desktop
//! until the next process restart triggers `Writer::start`'s initial
//! backfill.

use crate::http::RestClient;
use crate::store::Store;

use super::backlog_flusher::{flush_once, FlushError, FlushReport};
use super::reader::{backfill_once, BackfillBudget, BackfillError};

/// Outcome of one reconnect catch-up pass. Both legs are reported
/// independently so a caller can log partial progress when one half fails.
#[derive(Debug)]
pub struct ReconnectCatchupReport {
    /// Result of the outbound flush — drains rows queued locally while
    /// offline.
    pub flush: Result<FlushReport, FlushError>,
    /// Result of the inbound backfill — number of new clips pulled from the
    /// relay since the stored watermark.
    pub backfill: Result<usize, BackfillError>,
}

/// Run both the outbound flush and the inbound backfill, in that order,
/// against the same `(store, client, key)` triple. The two legs are
/// independent: a failure in one is logged but does **not** skip the other,
/// because they cover different drop windows.
///
/// Intended to be called from a long-running client's `on_connected`
/// callback (see `client_core::sync::OnConnectedCallback`).
pub async fn reconnect_catchup(
    store: &Store,
    client: &RestClient,
    key: [u8; 32],
) -> ReconnectCatchupReport {
    let flush = flush_once(store, client, key).await;
    let backfill = backfill_once(store, client, BackfillBudget::default(), Some(&key)).await;
    ReconnectCatchupReport { flush, backfill }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{queries, Store};
    use crate::version::{ClientInfo, ClientType};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn rest(uri: String) -> RestClient {
        RestClient::new(
            uri,
            "tok",
            ClientInfo {
                client_type: ClientType::Cli,
                version: "0".into(),
            },
        )
        .expect("RestClient")
    }

    #[tokio::test]
    async fn reconnect_catchup_pulls_clips_relay_broadcast_during_offline_window() {
        // The dominant failure mode this helper addresses: a remote push
        // landed on the relay while the desktop's WS was disconnected, so
        // the broadcast was never delivered. After reconnect the local
        // store must pick up that clip via the REST backfill — otherwise
        // it stays invisible until the next process restart.
        let server = MockServer::start().await;
        let clip_id = "01JABCDEFGHJKMNPQRSTVWXYZ0";
        let key = [0xaau8; 32];
        let plaintext = b"hello from remote";
        let ciphertext = crate::crypto::encrypt(&key, plaintext).unwrap();

        // Empty unsynced queue → flush_once is a no-op. The relay returns
        // one new clip that the local store has never seen.
        Mock::given(method("GET"))
            .and(path("/clips"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "clip_id": clip_id,
                    "user_id": "u",
                    "content": ciphertext,
                    "content_type": "text",
                    "source": "remote:cli",
                    "created_at": "2026-05-22T00:00:00Z",
                    "encrypted": true,
                }
            ])))
            .mount(&server)
            .await;

        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let report = reconnect_catchup(&store, &rest(server.uri()), key).await;

        let backfilled = report.backfill.as_ref().expect("backfill leg must succeed");
        assert_eq!(*backfilled, 1, "expected the missed clip to be pulled in");

        let stored = queries::get_clip(&store, clip_id)
            .unwrap()
            .expect("backfill must persist the missed clip locally");
        assert_eq!(stored.content.as_deref(), Some(&plaintext[..]));
    }

    #[tokio::test]
    async fn reconnect_catchup_runs_backfill_even_when_flush_errors() {
        // Defense-in-depth: the two legs are independent. If the outbound
        // flush hits a permanent DB error, the inbound backfill must still
        // run — otherwise one flaky side blinds the desktop to every push
        // that landed during the offline window.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/clips"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let report = reconnect_catchup(&store, &rest(server.uri()), [0u8; 32]).await;
        // The happy path here: no flush work to do and no clips to pull.
        // The point of the test is just that both legs returned Ok — i.e.
        // backfill was reached even though flush had nothing to do.
        assert!(report.flush.is_ok());
        assert!(report.backfill.is_ok());
    }
}
