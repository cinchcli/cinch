use client_core::http::RestClient;
use client_core::version::ClientInfo;
use wiremock::{
    matchers::{body_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn complete_device_code_posts_user_code_with_bearer_auth() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth/device-code/complete"))
        .and(header("authorization", "Bearer local-token"))
        .and(body_json(serde_json::json!({"user_code": "ABCD-EFGH"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "complete"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = RestClient::new(
        server.uri(),
        "local-token".to_string(),
        ClientInfo::for_test(),
    )
    .unwrap();
    client
        .complete_device_code("ABCD-EFGH")
        .await
        .expect("complete succeeds");
}
