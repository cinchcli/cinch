/// Generates desktop/src/bindings.ts from the registered Tauri commands.
/// Run with: cargo test export_bindings -- --ignored
/// This test is marked ignored so it only runs explicitly (not in CI by default),
/// because it writes a file to disk.
#[test]
#[ignore = "writes files to disk; run explicitly to regenerate bindings.ts"]
fn export_bindings() {
    use cinch_desktop_lib::make_specta_builder;

    make_specta_builder()
        .export(
            specta_typescript::Typescript::default(),
            "../src/bindings.ts",
        )
        .expect("Failed to export TypeScript bindings");

    assert!(
        std::path::Path::new("../src/bindings.ts").exists(),
        "bindings.ts was not created"
    );
}
