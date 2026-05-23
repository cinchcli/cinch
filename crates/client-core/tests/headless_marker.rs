//! Integration test for the headless device-code marker format.
//!
//! Simulates the `cinch device pair` stdout-reading flow: mixed log lines interspersed
//! with the <<CINCH-DEVICE-CODE>> marker, verifying that only the marker line
//! triggers `parse_device_code_marker` and the payload round-trips cleanly.

use client_core::auth::{format_device_code_marker, parse_device_code_marker};

#[test]
fn marker_round_trips_through_simulated_ssh_stdout() {
    let url = "https://api.cinchcli.com/auth/browser?device_code=ABCD-1234";
    let user_code = "ABCD-1234";

    let marker_line = format_device_code_marker(url, user_code);

    // Simulate lines that arrive on SSH stdout before and after the marker.
    let stdout_lines = vec![
        "Installing cinch...".to_string(),
        "cinch 0.5.0 installed".to_string(),
        marker_line.clone(),
        "\u{2713} Paired".to_string(),
    ];

    let mut found: Vec<client_core::auth::DeviceCodeMarker> = Vec::new();
    for line in &stdout_lines {
        if let Some(m) = parse_device_code_marker(line) {
            found.push(m);
        }
    }

    assert_eq!(found.len(), 1, "exactly one marker should be detected");
    let m = &found[0];
    assert_eq!(m.url, url);
    assert_eq!(m.user_code, user_code);
    assert_eq!(
        m.approve_command.as_deref(),
        Some("cinch auth approve ABCD-1234")
    );
}

#[test]
fn noise_lines_do_not_trigger_marker() {
    let noise = [
        "",
        "Installing cinch...",
        "<<CINCH-DEVICE-CODE>>", // start sentinel alone
        "<<END>>",               // end sentinel alone
        "<<CINCH-DEVICE-CODE>>{invalid json}<<END>>",
    ];
    for line in &noise {
        assert!(
            parse_device_code_marker(line).is_none(),
            "should not parse: {:?}",
            line
        );
    }
}
