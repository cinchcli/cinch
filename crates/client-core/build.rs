//! Build script: prost-build invocation against the canonical `.proto` sources
//! at the repo root (`proto/cinch/v1/*.proto`).
//!
//! Generated types live in `client_core::proto::cinch::v1::*` and carry serde
//! derives plus per-field `skip_serializing_if` predicates that mirror Go's
//! `encoding/json` `omitempty` behavior — without these, Rust would emit
//! `byte_size:0` / `encrypted:false` / etc. while the Go relay omits them,
//! breaking wire-vector parity.
//!
//! `Device` additionally derives `specta::Type` under the optional `specta`
//! feature so the desktop's TypeScript bindings stay in sync.

use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let manifest_dir = PathBuf::from(&manifest_dir);
    let proto_root = manifest_dir.join("proto");
    if !proto_root.exists() {
        return Err(format!(
            "proto sources not found at {}; the crate must ship its own proto/ tree",
            proto_root.display()
        )
        .into());
    }

    // event_stream.proto is intentionally excluded: the WSMessage envelope
    // stays hand-written in client-core for v1 (see plan, "Out of scope:
    // WSMessage"). Including it drags in the oneof Event, which breaks
    // package-wide serde(default) injection (struct-only attribute).
    let proto_files = [
        "cinch/v1/auth.proto",
        "cinch/v1/clips.proto",
        "cinch/v1/devices.proto",
    ];

    for f in &proto_files {
        println!("cargo:rerun-if-changed={}", proto_root.join(f).display());
    }
    println!("cargo:rerun-if-changed=build.rs");

    let mut config = prost_build::Config::new();
    config.protoc_arg("--experimental_allow_proto3_optional");

    // Apply serde derives + struct-level default to every cinch.v1 message
    // so missing JSON keys (Go's omitempty stripping) don't fail decode.
    config.type_attribute(
        ".cinch.v1",
        "#[derive(serde::Serialize, serde::Deserialize)]",
    );
    config.type_attribute(".cinch.v1", "#[serde(default)]");

    // Per-field skip predicates: drop zero-value scalars and empty
    // strings/vecs on serialize so wire output matches Go's omitempty.
    // Optional fields (proto3 `optional`) are already Option<T> and prost
    // emits them as `Option<...>`; we add skip_serializing_if for those too.
    let skip_str = "#[serde(skip_serializing_if = \"crate::ser::is_empty_str\")]";
    let skip_i32 = "#[serde(skip_serializing_if = \"crate::ser::is_zero_i32\")]";
    let skip_i64 = "#[serde(skip_serializing_if = \"crate::ser::is_zero_i64\")]";
    let skip_bool = "#[serde(skip_serializing_if = \"crate::ser::is_false\")]";
    let skip_vec = "#[serde(skip_serializing_if = \"crate::ser::is_empty_vec\")]";
    let skip_opt = "#[serde(skip_serializing_if = \"Option::is_none\")]";

    // ── clips.proto ──────────────────────────────────────────────────
    let string_fields = [
        // Clip
        ".cinch.v1.Clip.clip_id",
        ".cinch.v1.Clip.user_id",
        ".cinch.v1.Clip.content",
        ".cinch.v1.Clip.content_type",
        ".cinch.v1.Clip.source",
        ".cinch.v1.Clip.label",
        ".cinch.v1.Clip.created_at",
        // PushClipRequest
        ".cinch.v1.PushClipRequest.content",
        ".cinch.v1.PushClipRequest.content_type",
        ".cinch.v1.PushClipRequest.label",
        ".cinch.v1.PushClipRequest.source",
        // PushClipResponse
        ".cinch.v1.PushClipResponse.clip_id",
        // ListClipsRequest
        ".cinch.v1.ListClipsRequest.since",
        ".cinch.v1.ListClipsRequest.source_filter",
        ".cinch.v1.ListClipsRequest.exclude_source",
        // GetLatestClipRequest
        ".cinch.v1.GetLatestClipRequest.source",
        ".cinch.v1.GetLatestClipRequest.exclude_source",
        // DeleteClipRequest
        ".cinch.v1.DeleteClipRequest.clip_id",
        // auth.proto
        ".cinch.v1.LoginResponse.token",
        ".cinch.v1.LoginResponse.user_id",
        ".cinch.v1.LoginResponse.device_id",
        ".cinch.v1.DeviceCodeStartResponse.device_code",
        ".cinch.v1.DeviceCodeStartResponse.user_code",
        ".cinch.v1.DeviceCodeStartResponse.verification_uri",
        ".cinch.v1.DeviceCodePollRequest.code",
        ".cinch.v1.DeviceCodePollResponse.status",
        ".cinch.v1.DeviceCodeCompleteRequest.user_code",
        ".cinch.v1.DeviceCodeCompleteRequest.user_id",
        ".cinch.v1.DeviceCodeCompleteRequest.device_id",
        ".cinch.v1.DeviceCodeCompleteRequest.token",
        ".cinch.v1.DeviceCodeCompleteResponse.status",
        ".cinch.v1.RevokeDeviceRequest.device_id",
        ".cinch.v1.RevokeDeviceResponse.device_id",
        ".cinch.v1.RevokeDeviceResponse.revoked_at",
        ".cinch.v1.KeyBundlePutRequest.device_id",
        ".cinch.v1.KeyBundlePutRequest.ephemeral_public_key",
        ".cinch.v1.KeyBundlePutRequest.encrypted_bundle",
        ".cinch.v1.KeyBundleGetResponse.ephemeral_public_key",
        ".cinch.v1.KeyBundleGetResponse.encrypted_bundle",
        ".cinch.v1.ErrorResponse.error",
        ".cinch.v1.ErrorResponse.message",
        ".cinch.v1.ErrorResponse.fix",
        // devices.proto — Device skip predicates intentionally omitted: this
        // type crosses the Tauri command boundary, where tauri-specta
        // hardcodes specta_serde validation to Unified mode, which rejects
        // skip_serializing_if. Device is server-emitted and only deserialized
        // on the client, so wire-byte parity is not needed for outbound
        // traffic. Round-trip vectors for Device must use non-default
        // values for every field.
        ".cinch.v1.SetNicknameRequest.device_id",
        ".cinch.v1.SetNicknameRequest.nickname",
    ];
    for path in &string_fields {
        config.field_attribute(path, skip_str);
    }

    let i32_fields = [
        ".cinch.v1.ListClipsRequest.limit",
        ".cinch.v1.SetRetentionRequest.remote_retention_days",
        // Device.clip_count omitted — see Device note above.
    ];
    for path in &i32_fields {
        config.field_attribute(path, skip_i32);
    }

    let i64_fields = [
        ".cinch.v1.Clip.byte_size",
        ".cinch.v1.PushClipRequest.byte_size",
        ".cinch.v1.PushClipResponse.byte_size",
        ".cinch.v1.DeviceCodeStartResponse.expires_in",
        ".cinch.v1.DeviceCodeStartResponse.interval",
    ];
    for path in &i64_fields {
        config.field_attribute(path, skip_i64);
    }

    let bool_fields = [
        ".cinch.v1.Clip.encrypted",
        ".cinch.v1.Clip.is_pinned",
        ".cinch.v1.PushClipRequest.encrypted",
        ".cinch.v1.ListClipsRequest.exclude_image",
        ".cinch.v1.ListClipsRequest.exclude_text",
        ".cinch.v1.RevokeDeviceResponse.ok",
        ".cinch.v1.KeyBundlePutResponse.ok",
        ".cinch.v1.DeleteClipResponse.ok",
        ".cinch.v1.SetNicknameResponse.ok",
        ".cinch.v1.SetRetentionResponse.ok",
        // Device.online omitted — see Device note above.
    ];
    for path in &bool_fields {
        config.field_attribute(path, skip_bool);
    }

    let vec_fields = [
        ".cinch.v1.ListClipsRequest.clip_ids",
        ".cinch.v1.ListClipsResponse.clips",
        ".cinch.v1.ListDevicesResponse.devices",
    ];
    for path in &vec_fields {
        config.field_attribute(path, skip_vec);
    }

    // proto3 optional fields → Option<T> on the Rust side. Skip when None
    // so the empty Option doesn't serialize as `null` and break Go interop.
    let optional_fields = [
        // hostname-style nullable strings
        ".cinch.v1.LoginRequest.hostname",
        ".cinch.v1.LoginRequest.invite_code",
        ".cinch.v1.LoginRequest.display_name",
        ".cinch.v1.PairRequest.hostname",
        ".cinch.v1.PairRequest.device_public_key",
        ".cinch.v1.PairRequest.device_key_fingerprint",
        ".cinch.v1.DeviceCodeStartRequest.hostname",
        ".cinch.v1.DeviceCodeStartRequest.machine_id",
        ".cinch.v1.DeviceCodeStartRequest.user_hint",
        ".cinch.v1.DeviceCodeStartResponse.interval_ms",
        // device-code poll body — only populated when status == "complete"
        ".cinch.v1.DeviceCodePollResponse.token",
        ".cinch.v1.DeviceCodePollResponse.user_id",
        ".cinch.v1.DeviceCodePollResponse.device_id",
        ".cinch.v1.DeviceCodePollResponse.email",
        ".cinch.v1.DeviceCodePollResponse.identity_provider",
        ".cinch.v1.DeviceCodePollResponse.display_name",
        // clip optionals
        ".cinch.v1.Clip.media_path",
        ".cinch.v1.Clip.pin_note",
        ".cinch.v1.PushClipRequest.target_device_id",
        ".cinch.v1.PushClipRequest.media_path",
        ".cinch.v1.PushClipRequest.client_created_at",
        ".cinch.v1.PushClipRequest.idempotency_key",
        // Device.last_push_at omitted — see Device note in string_fields.
        // Device.machine_id intentionally omitted — Device crosses the Tauri
        // command boundary; specta unified mode rejects skip_serializing_if.
        // Device is server-emitted and only deserialized on the Rust side.
        // GetLatestClipResponse.clip is a message, prost emits Option<Clip>
        ".cinch.v1.GetLatestClipResponse.clip",
    ];
    for path in &optional_fields {
        config.field_attribute(path, skip_opt);
    }

    // Specta — only `Device` crosses the Rust↔TypeScript boundary today via
    // tauri-specta in the desktop app. The TypeScript type is emitted as
    // `Device` (not the legacy `DeviceInfo`); frontend imports were renamed
    // to match.
    config.type_attribute(
        ".cinch.v1.Device",
        "#[cfg_attr(feature = \"specta\", derive(specta::Type))]",
    );

    let proto_paths: Vec<PathBuf> = proto_files.iter().map(|f| proto_root.join(f)).collect();
    config.compile_protos(&proto_paths, &[&proto_root])?;
    Ok(())
}
