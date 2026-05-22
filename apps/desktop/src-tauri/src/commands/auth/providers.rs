/// list_auth_providers — fetches `GET {relay}/auth/providers` from the relay.
///
/// Lives in Rust because the Tauri WebView's CSP (`connect-src 'self' ipc:`)
/// blocks the frontend from issuing a cross-origin `fetch` to relay URLs.
/// Returns `[]` on any failure so the dialog can fall back to a generic
/// sign-in button without surfacing a transient error during typing.
#[tauri::command]
#[specta::specta]
pub async fn list_auth_providers(relay_url: String) -> Vec<String> {
    let relay = relay_url.trim().trim_end_matches('/');
    if !relay.starts_with("http") {
        return Vec::new();
    }
    let url = format!("{}/auth/providers", relay);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let resp = match client.get(&url).send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return Vec::new(),
    };
    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    body["providers"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}
