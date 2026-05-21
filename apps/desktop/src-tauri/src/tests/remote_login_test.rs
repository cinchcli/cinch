#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::auth::state::{
        add_pending_code, pending_count, PendingCodesHandle, PendingDeviceCode,
    };
    use crate::commands::auth::{approve_remote_login_impl, deny_remote_login_impl};

    fn make_pending(user_code: &str) -> PendingCodesHandle {
        let h: PendingCodesHandle = Arc::new(Mutex::new(Vec::new()));
        add_pending_code(
            &h,
            PendingDeviceCode {
                user_code: user_code.into(),
                hostname: "dev-box-3".into(),
                source_region: "us-west".into(),
                requested_at: 0,
            },
        );
        h
    }

    /// approve_remote_login_impl posts to /auth/device-code/complete and clears
    /// the matching entry from the pending list.
    #[tokio::test]
    async fn approve_calls_complete_and_clears_pending() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/auth/device-code/complete"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"status":"complete"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let pending = make_pending("ABCD-1234");

        approve_remote_login_impl("ABCD-1234", &server.uri(), "test-token", &pending)
            .await
            .expect("approve should succeed");

        server.verify().await;
        assert_eq!(
            pending_count(&pending),
            0,
            "pending list must be empty after approve"
        );
    }

    /// deny_remote_login_impl posts to /cinch.v1.AuthService/DeviceCodeDeny and
    /// clears the matching entry from the pending list.
    #[tokio::test]
    async fn deny_calls_connect_rpc_and_clears_pending() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/cinch.v1.AuthService/DeviceCodeDeny"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok":true})))
            .expect(1)
            .mount(&server)
            .await;

        let pending = make_pending("ABCD-1234");

        deny_remote_login_impl("ABCD-1234", &server.uri(), "test-token", &pending)
            .await
            .expect("deny should succeed");

        server.verify().await;
        assert_eq!(
            pending_count(&pending),
            0,
            "pending list must be empty after deny"
        );
    }

    /// approve_remote_login_impl returns an error when the relay returns 401,
    /// but must NOT clear the pending entry (relay rejected the request).
    #[tokio::test]
    async fn approve_relay_error_does_not_clear_pending() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/auth/device-code/complete"))
            .respond_with(
                ResponseTemplate::new(401).set_body_json(
                    serde_json::json!({"error":"unauthorized","message":"","fix":""}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;

        let pending = make_pending("ABCD-1234");

        let result =
            approve_remote_login_impl("ABCD-1234", &server.uri(), "test-token", &pending).await;

        assert!(result.is_err(), "must propagate relay error");
        assert_eq!(
            pending_count(&pending),
            1,
            "pending list must be unchanged on error"
        );
    }

    /// deny_remote_login_impl returns an error on relay failure and does NOT
    /// clear the pending entry.
    #[tokio::test]
    async fn deny_relay_error_does_not_clear_pending() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/cinch.v1.AuthService/DeviceCodeDeny"))
            .respond_with(
                ResponseTemplate::new(401).set_body_json(
                    serde_json::json!({"error":"unauthorized","message":"","fix":""}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;

        let pending = make_pending("ABCD-1234");

        let result =
            deny_remote_login_impl("ABCD-1234", &server.uri(), "test-token", &pending).await;

        assert!(result.is_err(), "must propagate relay error");
        assert_eq!(
            pending_count(&pending),
            1,
            "pending list must be unchanged on error"
        );
    }
}
