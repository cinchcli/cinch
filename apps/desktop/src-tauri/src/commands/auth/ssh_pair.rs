use client_core::pair_script::{
    sh_single_quote, FIND_SUPPORTED_CINCH_BLOCK, INSTALL_BLOCK, SKIP_INSTALL_BLOCK,
};
use tauri::AppHandle;
use tauri_specta::Event;

/// pair_via_ssh — SSH into a remote machine, install/upgrade cinch, and
/// authenticate it against the same relay account as the local desktop.
///
/// Verification contract (fixes the 0.1.5 silent-success bug):
///   1. The local desktop must already be signed in to the target relay.
///      Otherwise we cannot tell whether the remote ended up linked to the
///      right user_id — abort up front rather than report false success.
///   2. The remote script always emits a `<<CINCH-PAIRED-OK>>{...}<<END>>`
///      marker on stdout when it considers the remote paired (either it
///      reused an existing matching pairing, or it ran a fresh device-code
///      login). Without that marker, SSH exit 0 means nothing.
///   3. After the SSH process exits, we require the marker to have been
///      observed AND its `user_id` to match the local user. A blank or
///      mismatching marker becomes a hard error so the UI shows "Setup
///      failed" instead of "paired successfully".
///
/// In parallel, when the remote emits the legacy `<<CINCH-DEVICE-CODE>>`
/// marker (fresh-pair path) we still fire `SshPairMarkerFound` so the
/// frontend opens the browser.
#[tauri::command]
#[specta::specta]
pub async fn pair_via_ssh(
    app: AppHandle,
    cache: tauri::State<'_, crate::commands::clips::DeviceCacheHandle>,
    target: String,
    relay_url: Option<String>,
    skip_install: bool,
) -> Result<(), String> {
    use std::process::Stdio;
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::process::Command as TokioCommand;

    let multi_cfg = client_core::auth::load_multi_config().map_err(|e| e.to_string())?;
    let remote_relay = match relay_url {
        Some(s) if !s.trim().is_empty() => s.trim().trim_end_matches('/').to_string(),
        _ => multi_cfg
            .active_profile()
            .map(|p| p.relay_url.trim_end_matches('/').to_string())
            .unwrap_or_default(),
    };
    if remote_relay.is_empty() {
        return Err(
            "No relay configured on this machine — sign in first, then add an SSH machine.".into(),
        );
    }
    let expected_user_id = multi_cfg
        .relays
        .iter()
        .find(|p| p.relay_url.trim_end_matches('/') == remote_relay)
        .map(|p| p.user_id.clone())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            format!(
                "Not signed in to relay {} on this machine. Sign in here first so the remote can be linked to your account.",
                remote_relay
            )
        })?;

    let script = build_pair_script(&remote_relay, skip_install, &expected_user_id);

    let mut child = TokioCommand::new("ssh")
        .arg(&target)
        .arg("sh")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("SSH spawn failed: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .await
            .map_err(|e| format!("Writing script: {}", e))?;
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "SSH stdout was not captured".to_string())?;
    let app_for_stdout = app.clone();
    let pairing_marker: Arc<StdMutex<Option<client_core::auth::PairingCompleteMarker>>> =
        Arc::new(StdMutex::new(None));
    let pairing_marker_writer = pairing_marker.clone();
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| format!("Reading output: {}", e))?
        {
            if let Some(marker) = client_core::auth::parse_device_code_marker(&line) {
                if let Err(e) = tauri_plugin_opener::open_url(&marker.url, None::<&str>) {
                    log::warn!("pair_via_ssh: failed to open browser: {}", e);
                }
                crate::events::SshPairMarkerFound { url: marker.url }
                    .emit(&app_for_stdout)
                    .ok();
            } else if let Some(complete) = client_core::auth::parse_pairing_complete_marker(&line) {
                if let Ok(mut slot) = pairing_marker_writer.lock() {
                    *slot = Some(complete);
                }
            } else {
                log::info!("pair_via_ssh stdout: {}", line);
            }
        }
        Ok::<(), String>(())
    });

    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut recent = std::collections::VecDeque::new();
            while let Some(line) = lines
                .next_line()
                .await
                .map_err(|e| format!("Reading stderr: {}", e))?
            {
                log::warn!("pair_via_ssh stderr: {}", line);
                if recent.len() == 12 {
                    recent.pop_front();
                }
                recent.push_back(line);
            }
            Ok::<Vec<String>, String>(recent.into_iter().collect())
        })
    });

    let status = child.wait().await.map_err(|e| format!("SSH wait: {}", e))?;
    stdout_task
        .await
        .map_err(|e| format!("SSH stdout task: {}", e))??;
    let stderr_tail = if let Some(task) = stderr_task {
        task.await
            .map_err(|e| format!("SSH stderr task: {}", e))??
    } else {
        Vec::new()
    };
    if !status.success() {
        let mut message = format!("Remote setup failed (exit {})", status.code().unwrap_or(-1));
        if !stderr_tail.is_empty() {
            message.push_str(": ");
            message.push_str(&stderr_tail.join("\n"));
        }
        return Err(message);
    }

    // Exit 0 alone isn't enough: a remote that was already signed in as a
    // different user (or whose `cinch auth login` short-circuited) could
    // also exit 0 without actually pairing to our account. The marker is
    // the only ground truth.
    let marker = pairing_marker
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .ok_or_else(|| {
            "Pairing did not complete: remote did not confirm the linked account. The remote may be running an old cinch that does not emit the pairing-complete marker — run the remote install once with this version of the desktop app.".to_string()
        })?;
    if marker.user_id != expected_user_id {
        return Err(format!(
            "Remote paired as a different user (remote user_id={}, expected={}). The browser sign-in must use the same account as this machine.",
            marker.user_id, expected_user_id
        ));
    }
    cache.invalidate();
    if let Err(e) = crate::events::DevicesChanged.emit(&app) {
        log::warn!("DevicesChanged emit failed: {}", e);
    }

    Ok(())
}

/// list_ssh_hosts — return concrete aliases from the user's ~/.ssh/config.
#[tauri::command]
#[specta::specta]
pub fn list_ssh_hosts() -> Result<Vec<String>, String> {
    let Some(home) = dirs::home_dir() else {
        return Ok(Vec::new());
    };
    let config_path = home.join(".ssh").join("config");
    match std::fs::read_to_string(&config_path) {
        Ok(config) => Ok(parse_ssh_config_hosts(&config)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("Reading {}: {}", config_path.display(), e)),
    }
}

fn parse_ssh_config_hosts(config: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for raw_line in config.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };
        if !keyword.eq_ignore_ascii_case("host") {
            continue;
        }

        for alias in parts {
            if alias.starts_with('!') || alias.contains('*') || alias.contains('?') {
                continue;
            }
            if !hosts.iter().any(|existing| existing == alias) {
                hosts.push(alias.to_string());
            }
        }
    }
    hosts
}

fn build_pair_script(relay_url: &str, skip_install: bool, expected_user_id: &str) -> String {
    let mut s = String::new();
    s.push_str("#!/bin/sh\nset -e\n\n");
    s.push_str(&format!("RELAY_URL={}\n", sh_single_quote(relay_url)));
    s.push_str(&format!(
        "EXPECTED_USER_ID={}\n\n",
        sh_single_quote(expected_user_id)
    ));

    // The remote must be told *which* local account we expect it to match.
    // Empty here would mean the desktop forgot to pass it — fail fast rather
    // than silently re-using whatever happens to be on the remote disk.
    s.push_str(
        r#"if [ -z "$EXPECTED_USER_ID" ]; then
  echo "Error: pair invoked without an expected user_id (desktop is not signed in to this relay)." >&2
  exit 1
fi

"#,
    );

    if skip_install {
        s.push_str(SKIP_INSTALL_BLOCK);
    } else {
        s.push_str(INSTALL_BLOCK);
    }

    s.push_str(FIND_SUPPORTED_CINCH_BLOCK);

    s.push_str(
        r#"CINCH_DIR="$HOME/.cinch"
CINCH_CONFIG="$CINCH_DIR/config.json"
mkdir -p "$CINCH_DIR"

# Extract a field from the on-disk config, handling both the MultiConfig
# shape (active_relay_id + relays[]) and the legacy single-relay Config
# (top-level user_id / active_device_id). Prints empty string when the
# file is missing, empty, or malformed.
cinch_active_field() {
  FIELD="$1"
  LEGACY_FIELD="$FIELD"
  if [ "$FIELD" = "device_id" ]; then
    LEGACY_FIELD="active_device_id"
  fi
  if [ ! -f "$CINCH_CONFIG" ] || [ ! -s "$CINCH_CONFIG" ]; then
    return 0
  fi
  if command -v jq >/dev/null 2>&1; then
    jq -r --arg f "$FIELD" --arg lf "$LEGACY_FIELD" '
      if ((.relays // []) | length) > 0 and ((.active_relay_id // "") | length) > 0 then
        (.active_relay_id as $aid
          | (.relays[] | select(.id == $aid) | .[$f]) // "")
      else
        (.[$lf] // "")
      end
    ' "$CINCH_CONFIG" 2>/dev/null || true
  elif command -v python3 >/dev/null 2>&1; then
    python3 - "$CINCH_CONFIG" "$FIELD" "$LEGACY_FIELD" <<'PYEOF' 2>/dev/null || true
import json, sys
path, field, legacy = sys.argv[1], sys.argv[2], sys.argv[3]
try:
    with open(path) as f:
        cfg = json.load(f)
except Exception:
    print("")
    sys.exit(0)
relays = cfg.get("relays")
active = cfg.get("active_relay_id") or ""
if isinstance(relays, list) and active:
    for r in relays:
        if isinstance(r, dict) and r.get("id") == active:
            print(r.get(field, "") or "")
            sys.exit(0)
    print("")
    sys.exit(0)
print(cfg.get(legacy, "") or "")
PYEOF
  fi
}

REMOTE_USER_ID="$(cinch_active_field user_id || true)"

if [ -n "$REMOTE_USER_ID" ]; then
  if [ "$REMOTE_USER_ID" = "$EXPECTED_USER_ID" ]; then
    # Same user on disk — verify the relay still trusts the token before
    # claiming reuse. `cinch auth status` writes to stderr only, so capture
    # both streams.
    STATUS_OUT="$("$CINCH_BIN" auth status 2>&1 || true)"
    case "$STATUS_OUT" in
      *"Credentials expired or revoked"*|*"Not authenticated"*)
        echo "Local credentials no longer valid — re-pairing..." >&2
        "$CINCH_BIN" auth logout >/dev/null 2>&1 || true
        ;;
      *Authenticated*)
        REMOTE_DEVICE_ID="$(cinch_active_field device_id || true)"
        printf '<<CINCH-PAIRED-OK>>{"user_id":"%s","device_id":"%s","reused":true}<<END>>\n' \
          "$REMOTE_USER_ID" "$REMOTE_DEVICE_ID"
        exit 0
        ;;
      *)
        echo "Unexpected auth status output; re-pairing to be safe:" >&2
        printf '%s\n' "$STATUS_OUT" >&2
        "$CINCH_BIN" auth logout >/dev/null 2>&1 || true
        ;;
    esac
  else
    echo "Remote is signed in as a different user ($REMOTE_USER_ID); logging out before re-pair..." >&2
    "$CINCH_BIN" auth logout >/dev/null 2>&1 || true
  fi
fi

echo "Authenticating with relay at $RELAY_URL..."
"$CINCH_BIN" auth login --headless --force --relay "$RELAY_URL"

NEW_USER_ID="$(cinch_active_field user_id || true)"
NEW_DEVICE_ID="$(cinch_active_field device_id || true)"
printf '<<CINCH-PAIRED-OK>>{"user_id":"%s","device_id":"%s","reused":false}<<END>>\n' \
  "$NEW_USER_ID" "$NEW_DEVICE_ID"
"#,
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ssh_config_hosts_returns_concrete_aliases_only() {
        let config = r#"
Host *
  AddKeysToAgent yes

Host oci_atlas_1 jgopi
  User opc

Host 192.168.* ?ast
  User ignored

Host HomeServer
  ProxyJump jgopi
"#;

        assert_eq!(
            parse_ssh_config_hosts(config),
            vec![
                "oci_atlas_1".to_string(),
                "jgopi".to_string(),
                "HomeServer".to_string(),
            ],
        );
    }

    #[test]
    fn build_pair_script_is_valid_posix_shell() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let script = build_pair_script("https://api.cinchcli.com", false, "01HXYZ_USER");
        let mut child = Command::new("sh")
            .arg("-n")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn sh -n");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(script.as_bytes())
            .expect("write script");
        let out = child.wait_with_output().expect("wait sh");
        assert!(
            out.status.success(),
            "generated script failed sh -n:\nstderr:\n{}\nscript:\n{}",
            String::from_utf8_lossy(&out.stderr),
            script
        );
    }

    #[test]
    fn build_pair_script_verifies_remote_cinch_supports_headless_login() {
        let script = build_pair_script("https://api.cinchcli.com", false, "01HXYZ_USER");

        assert!(script.contains("find_supported_cinch"));
        assert!(script.contains("does not support SSH pairing"));
        // Fresh-pair branch must use --force to bypass the CLI's already-signed-in
        // short-circuit; without it the remote could exit 0 with no marker.
        assert!(
            script.contains("\"$CINCH_BIN\" auth login --headless --force --relay \"$RELAY_URL\"")
        );
    }

    #[test]
    fn build_pair_script_embeds_expected_user_id() {
        let script = build_pair_script("https://api.cinchcli.com", true, "01HXYZ_USER");
        assert!(script.contains("EXPECTED_USER_ID='01HXYZ_USER'"));
        assert!(script.contains("RELAY_URL='https://api.cinchcli.com'"));
    }

    #[test]
    fn build_pair_script_emits_pairing_complete_marker_on_both_paths() {
        let script = build_pair_script("https://api.cinchcli.com", false, "u1");
        // Reused path (already paired): marker with reused=true.
        assert!(script.contains(
            "<<CINCH-PAIRED-OK>>{\"user_id\":\"%s\",\"device_id\":\"%s\",\"reused\":true}<<END>>"
        ));
        // Fresh-pair path: marker with reused=false after login completes.
        assert!(script.contains(
            "<<CINCH-PAIRED-OK>>{\"user_id\":\"%s\",\"device_id\":\"%s\",\"reused\":false}<<END>>"
        ));
    }

    #[test]
    fn build_pair_script_logs_out_other_user_before_repair() {
        let script = build_pair_script("https://api.cinchcli.com", false, "u1");
        assert!(script.contains("signed in as a different user"));
        assert!(script.contains("\"$CINCH_BIN\" auth logout"));
    }

    #[test]
    fn build_pair_script_aborts_when_expected_user_id_is_empty() {
        let script = build_pair_script("https://api.cinchcli.com", false, "");
        assert!(script.contains("EXPECTED_USER_ID=''"));
        assert!(script.contains("pair invoked without an expected user_id"));
    }

    #[test]
    fn build_pair_script_handles_multi_config_via_jq_and_python() {
        let script = build_pair_script("https://api.cinchcli.com", false, "u1");
        // Must look up the active relay profile, not just the legacy top-level field.
        assert!(script.contains(".active_relay_id"));
        assert!(script.contains("active_relay_id"));
        // Python fallback for hosts without jq.
        assert!(script.contains("python3 -"));
    }

    #[test]
    fn build_pair_script_upgrades_cinch_when_install_not_skipped() {
        let script = build_pair_script("https://api.cinchcli.com", false, "u1");
        assert!(script.contains("Installing/upgrading cinch"));
        assert!(script.contains("curl -fsSL https://cinchcli.com/install.sh"));
    }

    // sh_single_quote and its tests live in
    // `client_core::pair_script` now — see the inline #[cfg(test)] mod
    // there for the same `it's` / empty-string coverage.
}
