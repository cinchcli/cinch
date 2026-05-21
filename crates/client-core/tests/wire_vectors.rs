//! Cross-language wire-format gate.
//!
//! Loads `testdata/wire-vectors.json` from the cinch-core repo root and
//! round-trips every named vector through the prost-generated Rust types.
//! The Go relay runs an equivalent test against its own copy of the same
//! fixture; if both pass byte-for-byte, the wire format is shape-equivalent
//! across languages.
//!
//! Round-trip: input JSON → typed deserialize → re-serialize → parse both
//! sides into `serde_json::Value` and compare structurally so JSON object
//! key ordering doesn't trip the assertion.

use std::path::PathBuf;

use client_core::proto::cinch::v1::{
    Clip, Device, DeviceCodePollResponse, DeviceCodeStartResponse, ErrorResponse, ListClipsRequest,
    ListClipsResponse, LoginRequest, LoginResponse, PushClipRequest, PushClipResponse,
};
use client_core::protocol::WSMessage;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

fn load_vectors() -> Value {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR is .../cinch-core/crates/client-core; the fixture
    // sits two levels up at the repo root.
    path.push("../../testdata/wire-vectors.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    serde_json::from_slice::<Value>(&bytes).expect("wire-vectors.json must be valid JSON")
}

fn assert_round_trip<T: DeserializeOwned + Serialize>(label: &str, input: &Value) {
    let typed: T = serde_json::from_value(input.clone())
        .unwrap_or_else(|e| panic!("{}: decode failed: {} (input: {})", label, e, input));
    let reencoded =
        serde_json::to_value(&typed).unwrap_or_else(|e| panic!("{}: encode failed: {}", label, e));
    assert_eq!(
        &reencoded, input,
        "{}: round-trip mismatch\n  input:    {}\n  output:   {}",
        label, input, reencoded
    );
}

fn vectors_for<'a>(root: &'a Value, message: &str) -> &'a serde_json::Map<String, Value> {
    root.get(message)
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("missing or non-object vector group: {}", message))
}

fn run_group<T: DeserializeOwned + Serialize>(root: &Value, message: &str) {
    for (name, vec) in vectors_for(root, message) {
        assert_round_trip::<T>(&format!("{}::{}", message, name), vec);
    }
}

#[test]
fn clip_vectors_round_trip() {
    let root = load_vectors();
    run_group::<Clip>(&root, "Clip");
}

#[test]
fn push_clip_request_vectors_round_trip() {
    let root = load_vectors();
    run_group::<PushClipRequest>(&root, "PushClipRequest");
}

#[test]
fn push_clip_response_vectors_round_trip() {
    let root = load_vectors();
    run_group::<PushClipResponse>(&root, "PushClipResponse");
}

#[test]
fn device_code_start_response_vectors_round_trip() {
    let root = load_vectors();
    run_group::<DeviceCodeStartResponse>(&root, "DeviceCodeStartResponse");
}

#[test]
fn device_code_poll_response_vectors_round_trip() {
    let root = load_vectors();
    run_group::<DeviceCodePollResponse>(&root, "DeviceCodePollResponse");
}

#[test]
fn login_response_vectors_round_trip() {
    let root = load_vectors();
    run_group::<LoginResponse>(&root, "LoginResponse");
}

#[test]
fn error_response_vectors_round_trip() {
    let root = load_vectors();
    run_group::<ErrorResponse>(&root, "ErrorResponse");
}

#[test]
fn device_vectors_round_trip() {
    let root = load_vectors();
    run_group::<Device>(&root, "Device");
}

/// Targeted assertion that the new optional version fields populate the
/// expected accessors and survive a JSON round-trip byte-equal to the input.
/// This complements the bulk `device_vectors_round_trip` group test by
/// asserting the *field-level* contract for client_version / client_type /
/// client_version_at — a regression here would silently break the
/// remote-version-check feature even if generic round-trip still passed.
#[test]
fn device_with_version_fields_roundtrips() {
    let root = load_vectors();
    let v = root
        .get("Device")
        .and_then(|d| d.get("with_version_reported"))
        .expect("Device::with_version_reported vector must exist");

    let device: Device =
        serde_json::from_value(v.clone()).expect("decode Device::with_version_reported");
    assert_eq!(device.client_version.as_deref(), Some("0.1.8"));
    assert_eq!(device.client_type.as_deref(), Some("cli"));
    assert_eq!(
        device.client_version_at.as_deref(),
        Some("2026-05-18T09:15:30Z")
    );

    // Re-encode and confirm it matches the input json (key-order-insensitive
    // via serde_json::Value equality).
    let reencoded = serde_json::to_value(&device).expect("encode Device");
    assert_eq!(reencoded, *v);
}

#[test]
fn list_clips_request_vectors_round_trip() {
    let root = load_vectors();
    run_group::<ListClipsRequest>(&root, "ListClipsRequest");
}

#[test]
fn list_clips_response_vectors_round_trip() {
    let root = load_vectors();
    run_group::<ListClipsResponse>(&root, "ListClipsResponse");
}

#[test]
fn ws_message_vectors_round_trip() {
    let root = load_vectors();
    run_group::<WSMessage>(&root, "WSMessage");
}

#[test]
fn login_request_vectors_round_trip() {
    let root = load_vectors();
    run_group::<LoginRequest>(&root, "LoginRequest");
}

#[test]
fn set_display_name_request_vectors_round_trip() {
    use client_core::proto::cinch::v1::SetDisplayNameRequest;
    let root = load_vectors();
    run_group::<SetDisplayNameRequest>(&root, "SetDisplayNameRequest");
}

#[test]
fn set_display_name_response_vectors_round_trip() {
    use client_core::proto::cinch::v1::SetDisplayNameResponse;
    let root = load_vectors();
    run_group::<SetDisplayNameResponse>(&root, "SetDisplayNameResponse");
}

/// Verify the pinned real-ciphertext vector decrypts to "hello" with the
/// all-zeros key. This is the one test that exercises the crypto path, not
/// just JSON shape: if the wire format or AES-GCM library ever diverges,
/// this test catches it before cross-language compat breaks in production.
#[test]
fn decrypt_encrypted_real_vector_yields_known_plaintext() {
    let root = load_vectors();
    let v = root
        .get("Clip")
        .and_then(|c| c.get("encrypted_real"))
        .expect("Clip::encrypted_real vector must exist");
    let content = v["content"].as_str().expect("content field");
    let key = [0u8; 32];
    let plaintext = client_core::crypto::decrypt(&key, content).expect("decrypt");
    assert_eq!(plaintext, b"hello", "decrypted bytes must equal b\"hello\"");
}
