//! AUTH-03 integration tests — FS watcher fires on credential_version bump.
//! Wave 1 implementation: version-compare and AuthState classification are tested
//! via the pure helpers in propagate.rs (decide_emit + next_state_from_config).
//! Full Tauri harness tests are marked #[ignore] — they require a running Tauri app handle.

#[test]
#[ignore = "requires full Tauri AppHandle — version-compare logic covered by propagate::tests::decide_emit_gates_on_version_bump"]
fn propagate_fires_on_version_bump() {
    // Given: a config.json with credential_version: 1
    // When: write config.json with credential_version: 2 via atomic rename
    // Then: auth-state-changed fires within 2 seconds
    // NOTE: The version-compare logic is tested directly in propagate::tests::decide_emit_gates_on_version_bump
}

#[test]
#[ignore = "requires full Tauri AppHandle — same-version no-fire covered by propagate::tests::decide_emit_gates_on_version_bump"]
fn propagate_no_fire_on_unchanged_version() {
    // Writing config.json with the same credential_version must NOT fire the signal.
    // NOTE: Covered by propagate::tests::decide_emit_gates_on_version_bump (same-version case)
}

#[test]
#[ignore = "wave-1"]
fn propagate_debounces_rename() {
    // The 400ms debouncer coalesces rapid rename events into a single handler invocation.
}
