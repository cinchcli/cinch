//! `cinch pair <ssh-target>` — set up cinch on a remote machine via SSH.
//!
//! Steps:
//! 1. Require active local auth.
//! 2. SSH into the remote and run a bootstrap script that installs cinch
//!    (unless `--skip-install`) and runs `cinch auth login --headless`.
//! 3. Read the `<<CINCH-DEVICE-CODE>>` marker from the remote's stdout and
//!    open the verification URL in a local browser.
//! 4. The remote process polls the device-code flow, stores credentials, and
//!    registers its public key with the relay. The local machine waits for it
//!    to exit, then the key-exchange daemon delivers the user key automatically.

use std::process::Stdio;

use client_core::auth::{load_config, parse_device_code_marker};
use client_core::http::{HttpError, RestClient};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// SSH target — anything `ssh <target>` accepts (host, user@host, alias).
    pub target: String,

    /// Skip cinch binary installation on the remote (use if already installed).
    #[arg(long = "skip-install")]
    pub skip_install: bool,

    /// Override relay URL configured on the remote machine.
    #[arg(long = "relay-url")]
    pub relay_url: Option<String>,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    let cfg = load_config()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not load config: {}", e), ""))?;
    if cfg.token.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "Not authenticated.",
            "Run: cinch auth login",
        ));
    }

    let remote_relay = args
        .relay_url
        .clone()
        .unwrap_or_else(|| cfg.relay_url.clone());

    eprintln!("  Connecting to {}...\n", args.target);
    let script = build_remote_script(&remote_relay, args.skip_install);

    let mut child = TokioCommand::new("ssh")
        .arg(&args.target)
        .arg("sh")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            ExitError::new(
                GENERIC_ERROR,
                format!("SSH spawn failed: {}", e),
                "Is `ssh` on your PATH?",
            )
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes()).await.map_err(|e| {
            ExitError::new(GENERIC_ERROR, format!("Writing remote script: {}", e), "")
        })?;
    }

    // Stream stdout: print each line; open browser when marker is detected.
    if let Some(stdout) = child.stdout.take() {
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await.map_err(|e| {
            ExitError::new(GENERIC_ERROR, format!("Reading remote output: {}", e), "")
        })? {
            if let Some(marker) = parse_device_code_marker(&line) {
                eprintln!("\u{2713} Approving remote login...");
                if let Err(e) =
                    approve_remote_login(&remote_relay, &cfg.token, &marker.user_code).await
                {
                    eprintln!("  Auto-approval failed: {}", e.message);
                    eprintln!(
                        "  Run manually: cinch auth approve {} --relay {}",
                        marker.user_code, remote_relay
                    );
                    eprintln!("  Browser fallback: {}", marker.url);
                } else {
                    eprintln!("\u{2713} Remote login approved.");
                }
            } else {
                eprintln!("{}", line);
            }
        }
    }

    let status = child.wait().await.map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Waiting for SSH: {}", e),
            format!("Connect manually to debug: ssh {}", args.target),
        )
    })?;
    if !status.success() {
        return Err(ExitError::new(
            GENERIC_ERROR,
            format!(
                "Remote setup failed (exit {}).",
                status.code().unwrap_or(-1)
            ),
            format!("Connect manually to debug: ssh {}", args.target),
        ));
    }

    eprintln!("\n\u{2713} {} is ready.", args.target);
    eprintln!("  Try it: ssh {} 'echo hello | cinch push'", args.target);
    Ok(())
}

async fn approve_remote_login(
    relay_url: &str,
    token: &str,
    user_code: &str,
) -> Result<(), ExitError> {
    let client = RestClient::new(
        relay_url.to_string(),
        token.to_string(),
        crate::client_info::for_cli(),
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not init client: {}", e), ""))?;
    client
        .complete_device_code(user_code)
        .await
        .map_err(|e| match e {
            HttpError::Unauthorized => ExitError::new(
                AUTH_FAILURE,
                "Local credentials were rejected by the relay.",
                "Run: cinch auth login --force",
            ),
            other => ExitError::new(
                GENERIC_ERROR,
                "Could not approve remote login.",
                format!("Open manually or retry: {}", other),
            ),
        })
}

fn build_remote_script(relay_url: &str, skip_install: bool) -> String {
    let mut s = String::new();
    s.push_str("#!/bin/sh\nset -e\n\n");
    s.push_str(&format!("RELAY_URL='{}'\n\n", relay_url));

    if !skip_install {
        s.push_str(
            r#"echo "Installing cinch..."
SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  fi
fi
curl -fsSL https://cinchcli.com/install.sh | $SUDO sh -s cinch
if ! command -v cinch >/dev/null 2>&1; then
  echo "Error: cinch installation failed." >&2
  exit 1
fi
echo ""
"#,
        );
    } else {
        s.push_str(
            r#"if ! command -v cinch >/dev/null 2>&1; then
  echo "Error: cinch not found. Remove --skip-install or install manually." >&2
  exit 1
fi
"#,
        );
    }

    s.push_str(
        r#"# Write relay URL to config (preserves other fields when possible)
CINCH_DIR="$HOME/.cinch"
CINCH_CONFIG="$CINCH_DIR/config.json"
mkdir -p "$CINCH_DIR"
if [ -f "$CINCH_CONFIG" ] && [ -s "$CINCH_CONFIG" ] && command -v jq >/dev/null 2>&1; then
  jq --arg url "$RELAY_URL" '. + {relay_url: $url}' "$CINCH_CONFIG" > "$CINCH_CONFIG.tmp" && mv "$CINCH_CONFIG.tmp" "$CINCH_CONFIG" || printf '{"relay_url":"%s"}\n' "$RELAY_URL" > "$CINCH_CONFIG"
elif [ -f "$CINCH_CONFIG" ] && [ -s "$CINCH_CONFIG" ] && command -v python3 >/dev/null 2>&1; then
  python3 - "$RELAY_URL" "$CINCH_CONFIG" << 'PYEOF' || printf '{"relay_url":"%s"}\n' "$RELAY_URL" > "$CINCH_CONFIG"
import json, sys
url = sys.argv[1]
path = sys.argv[2]
with open(path) as f: cfg = json.load(f)
cfg['relay_url'] = url
with open(path, 'w') as f: json.dump(cfg, f, indent=2)
PYEOF
else
  printf '{"relay_url":"%s"}\n' "$RELAY_URL" > "$CINCH_CONFIG"
fi
chmod 600 "$CINCH_CONFIG"

echo "Authenticating with relay at $RELAY_URL..."
cinch auth login --headless --relay "$RELAY_URL"
"#,
    );

    s
}
