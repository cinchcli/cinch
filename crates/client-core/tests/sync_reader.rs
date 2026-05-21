//! Integration test for `client_core::sync::reader::backfill_once`.
//!
//! Skipped unless both `RELAY_INTEGRATION_URL` and `RELAY_INTEGRATION_TOKEN`
//! are set.  Run against a real relay instance via:
//!
//!   RELAY_INTEGRATION_URL=http://localhost:8080 \
//!   RELAY_INTEGRATION_TOKEN=<token> \
//!   cargo test -p cinchcli-core --test sync_reader -- --ignored

use client_core::http::RestClient;
use client_core::store::queries;
use client_core::store::Store;
use client_core::sync::reader::{backfill_once, BackfillBudget};
use client_core::version::ClientInfo;

#[tokio::test]
#[ignore = "requires RELAY_INTEGRATION_URL and RELAY_INTEGRATION_TOKEN"]
async fn backfill_advances_watermark() {
    let url = match std::env::var("RELAY_INTEGRATION_URL").ok() {
        Some(u) => u,
        None => return,
    };
    let token = match std::env::var("RELAY_INTEGRATION_TOKEN").ok() {
        Some(t) => t,
        None => return,
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(dir.path().join("test.db").as_path()).expect("Store::open");
    let client = RestClient::new(url, token, ClientInfo::for_test()).expect("RestClient::new");

    // Watermark must be absent before the first pass.
    let wm_before = queries::watermark(&store).expect("watermark before");
    assert!(wm_before.is_none(), "fresh store should have no watermark");

    let n = backfill_once(&store, &client, BackfillBudget::default(), None)
        .await
        .expect("backfill_once");

    let wm_after = queries::watermark(&store).expect("watermark after");

    // If the relay returned any clips the watermark must be set.
    if n > 0 {
        assert!(
            wm_after.is_some(),
            "watermark must be set after inserting clips"
        );
    } else {
        // No clips on the relay — watermark stays absent; that is correct.
        assert!(
            wm_after.is_none(),
            "watermark must stay absent when no clips were fetched"
        );
    }
}
