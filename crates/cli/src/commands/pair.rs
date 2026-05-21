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
use client_core::pair_script::{
    sh_single_quote, FIND_SUPPORTED_CINCH_BLOCK, INSTALL_BLOCK, SKIP_INSTALL_BLOCK,
};
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
    // POSIX-safe quoting — a relay URL containing a `'` (rare but legal
    // for custom relays) used to break the script before this was
    // hoisted into `client_core::pair_script::sh_single_quote`.
    s.push_str(&format!("RELAY_URL={}\n\n", sh_single_quote(relay_url)));

    if skip_install {
        s.push_str(SKIP_INSTALL_BLOCK);
    } else {
        s.push_str(INSTALL_BLOCK);
    }

    // Validate that the cinch binary on the remote actually supports the
    // `--headless` device-code flow before driving it. Without this the
    // CLI used to invoke a stale (pre-monorepo, Go-era) cinch and fail
    // with a confusing "unknown flag" error.
    s.push_str(FIND_SUPPORTED_CINCH_BLOCK);

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
"$CINCH_BIN" auth login --headless --relay "$RELAY_URL"
"#,
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // The remote bootstrap script is the on-the-wire contract between
    // `cinch pair` and a freshly-installed remote — the local desktop
    // SSHes the script in and reads the device-code marker off stdout, so
    // any drift in the script shape (missing chmod, install command, or
    // login invocation) silently breaks the pair-via-SSH flow. These
    // tests pin the load-bearing lines without locking in every whitespace.

    #[test]
    fn build_remote_script_embeds_relay_url() {
        let s = build_remote_script("https://relay.example", false);
        assert!(
            s.contains("RELAY_URL='https://relay.example'"),
            "relay URL must be exported for downstream commands; got:\n{s}"
        );
    }

    #[test]
    fn build_remote_script_default_includes_curl_installer() {
        let s = build_remote_script("https://relay.example", false);
        assert!(
            s.contains("curl -fsSL https://cinchcli.com/install.sh"),
            "default path must pipe install.sh through curl; got:\n{s}"
        );
        assert!(
            s.contains("Error: cinch installation failed."),
            "default path must guard against install.sh exit-0 with no binary; got:\n{s}"
        );
    }

    #[test]
    fn build_remote_script_skip_install_omits_curl_and_guards_missing_binary() {
        let s = build_remote_script("https://relay.example", true);
        assert!(
            !s.contains("install.sh"),
            "--skip-install must NOT pipe install.sh; got:\n{s}"
        );
        assert!(
            s.contains("Error: cinch not found. Remove --skip-install"),
            "--skip-install must fail loudly when cinch is missing; got:\n{s}"
        );
    }

    #[test]
    fn build_remote_script_always_writes_config_and_logs_via_validated_cinch_bin() {
        for skip in [false, true] {
            let s = build_remote_script("https://relay.example", skip);
            assert!(
                s.contains(r#"chmod 600 "$CINCH_CONFIG""#),
                "config must be chmod 600 (token storage); skip_install={skip}\n{s}"
            );
            // The login MUST go through $CINCH_BIN (set by
            // FIND_SUPPORTED_CINCH_BLOCK), not bare `cinch` — otherwise
            // a stale Go-era cinch on PATH would handle the call and
            // fail the device-code flow with a confusing error.
            assert!(
                s.contains(r#""$CINCH_BIN" auth login --headless --relay "$RELAY_URL""#),
                "must invoke headless login via $CINCH_BIN; skip_install={skip}\n{s}"
            );
        }
    }

    #[test]
    fn build_remote_script_starts_with_strict_sh_shebang() {
        // `set -e` is load-bearing: any failure in the install or login
        // pipeline must abort the script so the local side sees a
        // non-zero exit on SSH disconnect.
        let s = build_remote_script("https://relay.example", false);
        assert!(s.starts_with("#!/bin/sh\nset -e\n"), "got:\n{s}");
    }

    #[test]
    fn build_remote_script_safely_quotes_relay_url_with_single_quote() {
        // Pre-refactor the CLI emitted `RELAY_URL='https://x/'foo''` for a
        // URL like `https://x/'foo'`, which made the shell parse `foo` as
        // a bare word and break the script. The shared `sh_single_quote`
        // produces the POSIX-canonical `'…'\''…'` form instead.
        let s = build_remote_script("https://x/'foo'", false);
        assert!(
            s.contains(r"RELAY_URL='https://x/'\''foo'\'''"),
            "expected POSIX-safe quoted relay URL; got:\n{s}"
        );
    }

    #[test]
    fn build_remote_script_includes_find_supported_cinch() {
        // The block guards against a stale cinch on the remote's PATH
        // that doesn't support `--headless`. Pin its presence so a
        // future refactor doesn't silently drop it.
        for skip in [false, true] {
            let s = build_remote_script("https://relay.example", skip);
            assert!(
                s.contains("find_supported_cinch()"),
                "must define find_supported_cinch shell helper; skip_install={skip}\n{s}"
            );
            assert!(
                s.contains(r#"CINCH_BIN="$(find_supported_cinch)""#),
                "must capture validated binary into $CINCH_BIN; skip_install={skip}\n{s}"
            );
        }
    }
}
