use client_core::protocol::ACTION_CLIENT_HELLO;
use client_core::version::{ClientInfo, ClientType};

#[test]
fn client_type_str_roundtrip() {
    assert_eq!(ClientType::Cli.as_str(), "cli");
    assert_eq!(ClientType::Desktop.as_str(), "desktop");
}

#[test]
fn http_headers_pair_is_typed() {
    let info = ClientInfo {
        client_type: ClientType::Cli,
        version: "0.1.8".to_string(),
    };
    let pairs = info.http_headers();
    assert_eq!(pairs[0].0.as_str(), "x-cinch-client-version");
    assert_eq!(pairs[0].1.to_str().unwrap(), "0.1.8");
    assert_eq!(pairs[1].0.as_str(), "x-cinch-client-type");
    assert_eq!(pairs[1].1.to_str().unwrap(), "cli");
}

#[test]
fn client_hello_message_carries_fields() {
    let info = ClientInfo {
        client_type: ClientType::Desktop,
        version: "0.1.7".to_string(),
    };
    let msg = info.client_hello_message();
    assert_eq!(msg.action, ACTION_CLIENT_HELLO);
    let payload = msg.client_hello.expect("payload set");
    assert_eq!(payload.version, "0.1.7");
    assert_eq!(payload.type_, "desktop");
}
