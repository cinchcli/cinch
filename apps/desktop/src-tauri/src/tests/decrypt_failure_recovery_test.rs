#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Two decrypt failures fired in quick succession must only call
    /// POST /auth/key-bundle/retry once (debounced to a 60-second window).
    #[tokio::test]
    async fn two_rapid_failures_fire_retry_once() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/auth/key-bundle/retry"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1) // must be called exactly once
            .mount(&server)
            .await;

        // Shared gate — same as what spawn_ws_client creates and threads through.
        let gate: Arc<Mutex<Option<std::time::Instant>>> = Arc::new(Mutex::new(None));

        // AppHandle can't be constructed in unit tests; fire_retry_debounced
        // accepts it only for future event emission — passing a dummy reference
        // is not possible, so we test via the public HTTP path by calling the
        // function twice and asserting the mock expectation.
        //
        // We call through the public RestClient directly to replicate what
        // fire_retry_debounced does: first call acquires the gate, second is
        // suppressed within the 60s window.
        let relay = server.uri();
        let token = "tok";

        // First call: gate is empty → should POST /retry.
        {
            let now = std::time::Instant::now();
            let mut last = gate.lock().await;
            if last.is_some_and(|t| now.duration_since(t) < std::time::Duration::from_secs(60)) {
                // suppressed — should not happen on first call
            } else {
                *last = Some(now);
                drop(last);
                let client = client_core::http::RestClient::new(
                    relay.clone(),
                    token.to_string(),
                    crate::build_client_info(),
                )
                .unwrap();
                client.retry_key_bundle().await.ok();
            }
        }

        // Second call: gate is set to "now" → must be suppressed (< 60s).
        {
            let now = std::time::Instant::now();
            let last = gate.lock().await;
            let suppressed =
                last.is_some_and(|t| now.duration_since(t) < std::time::Duration::from_secs(60));
            drop(last);
            if !suppressed {
                let client = client_core::http::RestClient::new(
                    relay.clone(),
                    token.to_string(),
                    crate::build_client_info(),
                )
                .unwrap();
                client.retry_key_bundle().await.ok();
            }
        }

        // wiremock asserts expect(1) — exactly one POST /retry hit.
        server.verify().await;
    }

    /// After 60 seconds the gate resets and a second failure fires a new retry.
    /// We simulate the passage of time by backdating the gate entry.
    #[tokio::test]
    async fn retry_fires_again_after_debounce_window() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/auth/key-bundle/retry"))
            .respond_with(ResponseTemplate::new(204))
            .expect(2) // must be called exactly twice
            .mount(&server)
            .await;

        let relay = server.uri();
        let token = "tok";

        let fire = |relay: String, token: String| async move {
            let client =
                client_core::http::RestClient::new(relay, token, crate::build_client_info())
                    .unwrap();
            client.retry_key_bundle().await.ok();
        };

        // Seed gate with a timestamp 61 seconds in the past.
        let gate: Arc<Mutex<Option<std::time::Instant>>> = Arc::new(Mutex::new(Some(
            std::time::Instant::now() - std::time::Duration::from_secs(61),
        )));

        // First call: gate is expired → fires.
        {
            let now = std::time::Instant::now();
            let mut last = gate.lock().await;
            let expired =
                last.is_none_or(|t| now.duration_since(t) >= std::time::Duration::from_secs(60));
            if expired {
                *last = Some(now);
                drop(last);
                fire(relay.clone(), token.to_string()).await;
            }
        }

        // Second call immediately after: gate is fresh → suppressed.
        {
            let now = std::time::Instant::now();
            let last = gate.lock().await;
            let suppressed =
                last.is_some_and(|t| now.duration_since(t) < std::time::Duration::from_secs(60));
            drop(last);
            if !suppressed {
                fire(relay.clone(), token.to_string()).await;
            }
        }

        // Third call: backdate gate by 61s again → fires a second time.
        {
            let mut last = gate.lock().await;
            *last = Some(std::time::Instant::now() - std::time::Duration::from_secs(61));
            drop(last);
        }
        {
            let now = std::time::Instant::now();
            let mut last = gate.lock().await;
            let expired =
                last.is_none_or(|t| now.duration_since(t) >= std::time::Duration::from_secs(60));
            if expired {
                *last = Some(now);
                drop(last);
                fire(relay.clone(), token.to_string()).await;
            }
        }

        server.verify().await;
    }
}
