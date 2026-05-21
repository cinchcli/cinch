//! Shared building blocks for the SSH pair bootstrap script.
//!
//! Both `cinch pair <ssh-target>` (CLI) and the desktop's `pair_via_ssh`
//! Tauri command compose a POSIX shell script that runs on the remote
//! machine to install/upgrade cinch and then drive a device-code login.
//! Before this module existed each caller hand-rolled its own copy of
//! the installer fragment, the `find_supported_cinch` shell function,
//! and the shell-quoting helper — and they had already started to drift
//! (the CLI used naive `'{}'` quoting that broke on URLs containing a
//! single quote, and skipped the `--headless`-support check on the
//! installed cinch entirely).
//!
//! The pieces here are the common substrate. Each caller still composes
//! its own surrounding logic (the desktop adds an `EXPECTED_USER_ID`
//! gate and emits a `<<CINCH-PAIRED-OK>>` marker; the CLI writes the
//! relay URL into `~/.cinch/config.json` before logging in), so this
//! module deliberately exposes building blocks rather than a single
//! `build_script(opts)` entry point.

/// Escape a value for safe use inside POSIX single-quoted shell literals.
/// `foo'bar` → `'foo'\''bar'`.
///
/// Always returns a string surrounded by single quotes — callers should
/// embed the result directly:
///
/// ```
/// use client_core::pair_script::sh_single_quote;
/// let s = format!("URL={}\n", sh_single_quote("https://relay.example/"));
/// assert_eq!(s, "URL='https://relay.example/'\n");
/// ```
pub fn sh_single_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Shell block (default path): pipe `install.sh` through curl + sudo, then
/// fail loudly if the install ran exit-0 but no `cinch` ended up on PATH.
///
/// install.sh is idempotent and always installs the latest published
/// build — re-running it upgrades any older cinch already on disk to
/// the version that supports `--headless` and the
/// `<<CINCH-PAIRED-OK>>` marker.
pub const INSTALL_BLOCK: &str = r#"echo "Installing/upgrading cinch..."
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
"#;

/// Shell block (`--skip-install` / `skip_install=true` path): gate that
/// fails loudly if `cinch` isn't already on PATH, telling the user how
/// to recover.
pub const SKIP_INSTALL_BLOCK: &str = r#"if ! command -v cinch >/dev/null 2>&1; then
  echo "Error: cinch not found. Remove --skip-install or install manually." >&2
  exit 1
fi
"#;

/// Shell function definition + invocation that locates a `cinch` binary
/// supporting the `--headless` device-code flow.
///
/// Pre-2026-05 binaries (e.g. legacy Go cinch) are on PATH on many
/// developer machines but don't understand `--headless`, so the script
/// must validate the candidate before driving it. The function probes
/// `command -v cinch` first, then a list of common install locations
/// (`~/.local/bin`, Homebrew, /usr/local), printing the first absolute
/// path whose `auth login --help` mentions `--headless`.
///
/// On success the absolute path is stored in `$CINCH_BIN`. Callers
/// invoke `$CINCH_BIN` thereafter instead of bare `cinch`.
pub const FIND_SUPPORTED_CINCH_BLOCK: &str = r#"find_supported_cinch() {
  if command -v cinch >/dev/null 2>&1; then
    CANDIDATE="$(command -v cinch)"
    if "$CANDIDATE" auth login --help 2>&1 | grep -q -- "--headless"; then
      printf '%s\n' "$CANDIDATE"
      return 0
    fi
  fi

  for CANDIDATE in "$HOME/.local/bin/cinch" /usr/local/bin/cinch /opt/homebrew/bin/cinch /home/linuxbrew/.linuxbrew/bin/cinch /usr/bin/cinch; do
    if [ -x "$CANDIDATE" ] && "$CANDIDATE" auth login --help 2>&1 | grep -q -- "--headless"; then
      printf '%s\n' "$CANDIDATE"
      return 0
    fi
  done

  return 1
}

CINCH_BIN="$(find_supported_cinch)" || {
  echo "Error: installed cinch does not support SSH pairing." >&2
  echo "Install or upgrade to a cinch build with 'cinch auth login --headless'." >&2
  exit 1
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    // --- sh_single_quote ---------------------------------------------------

    #[test]
    fn sh_single_quote_wraps_simple_value() {
        assert_eq!(sh_single_quote("hello"), "'hello'");
    }

    #[test]
    fn sh_single_quote_handles_embedded_single_quote() {
        // POSIX single-quoted strings cannot contain a single quote; the
        // canonical workaround is close-quote, escape, reopen-quote:
        //   foo'bar  →  'foo'\''bar'
        assert_eq!(sh_single_quote("foo'bar"), r"'foo'\''bar'");
    }

    #[test]
    fn sh_single_quote_handles_multiple_single_quotes() {
        assert_eq!(sh_single_quote("a'b'c"), r"'a'\''b'\''c'");
    }

    #[test]
    fn sh_single_quote_empty_string_is_two_quotes() {
        assert_eq!(sh_single_quote(""), "''");
    }

    #[test]
    fn sh_single_quote_passes_through_double_quotes_and_dollar() {
        // Inside single quotes everything except `'` is literal — `$`,
        // `"`, backticks, etc. all pass through unescaped. This is the
        // whole reason we use single quotes for relay URLs.
        assert_eq!(sh_single_quote(r#"$RELAY"`x"#), r#"'$RELAY"`x'"#);
    }

    // --- INSTALL_BLOCK -----------------------------------------------------

    #[test]
    fn install_block_pipes_install_sh_through_sudo() {
        assert!(INSTALL_BLOCK.contains("curl -fsSL https://cinchcli.com/install.sh | $SUDO sh"));
        // SUDO must be empty when running as root.
        assert!(INSTALL_BLOCK.contains(r#"if [ "$(id -u)" -ne 0 ]"#));
    }

    #[test]
    fn install_block_aborts_on_missing_binary_after_install() {
        // install.sh exiting 0 without producing a `cinch` binary is the
        // exact failure that motivated this check — see the pre-fix
        // GLIBC_2.39 incident on Oracle Linux 9.
        assert!(INSTALL_BLOCK.contains("Error: cinch installation failed."));
        assert!(INSTALL_BLOCK.contains(r#"if ! command -v cinch >/dev/null 2>&1"#));
        assert!(INSTALL_BLOCK.contains("exit 1"));
    }

    // --- SKIP_INSTALL_BLOCK ------------------------------------------------

    #[test]
    fn skip_install_block_fails_loudly_when_cinch_missing() {
        assert!(SKIP_INSTALL_BLOCK.contains(r#"if ! command -v cinch >/dev/null 2>&1"#));
        assert!(SKIP_INSTALL_BLOCK.contains("Error: cinch not found."));
        assert!(SKIP_INSTALL_BLOCK.contains("exit 1"));
        // skip_install must NOT silently pipe install.sh.
        assert!(!SKIP_INSTALL_BLOCK.contains("install.sh"));
    }

    // --- FIND_SUPPORTED_CINCH_BLOCK ---------------------------------------

    #[test]
    fn find_supported_cinch_block_checks_headless_flag_support() {
        // The whole point of this block is to reject a `cinch` binary
        // that doesn't understand `--headless` — a stale install would
        // otherwise pass `command -v` and then fail the device-code
        // flow with a confusing "unknown flag" error.
        assert!(FIND_SUPPORTED_CINCH_BLOCK.contains(r#"grep -q -- "--headless""#));
    }

    #[test]
    fn find_supported_cinch_block_probes_common_install_dirs() {
        for dir in &[
            "$HOME/.local/bin/cinch",
            "/usr/local/bin/cinch",
            "/opt/homebrew/bin/cinch",
            "/home/linuxbrew/.linuxbrew/bin/cinch",
            "/usr/bin/cinch",
        ] {
            assert!(
                FIND_SUPPORTED_CINCH_BLOCK.contains(dir),
                "expected fallback path {dir}; got:\n{FIND_SUPPORTED_CINCH_BLOCK}"
            );
        }
    }

    #[test]
    fn find_supported_cinch_block_exposes_result_as_cinch_bin() {
        // Callers downstream invoke `"$CINCH_BIN" auth login …`, not
        // bare `cinch`. Pin the variable name so a future rename can't
        // silently break the contract.
        assert!(FIND_SUPPORTED_CINCH_BLOCK.contains(r#"CINCH_BIN="$(find_supported_cinch)""#));
    }

    #[test]
    fn find_supported_cinch_block_aborts_when_no_candidate_supports_headless() {
        assert!(FIND_SUPPORTED_CINCH_BLOCK
            .contains("Error: installed cinch does not support SSH pairing."));
    }
}
