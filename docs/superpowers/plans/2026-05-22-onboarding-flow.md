# Onboarding flow implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a desktop `GettingStartedCard` empty state and a CLI first-run welcome plus a `pull` auth guard so first-time Cinch users get a clear next action.

**Architecture:** Two surfaces, no shared code path. Desktop: new React component rendered conditionally from `App.tsx` when authenticated, zero clips, ≤1 device, not dismissed. CLI: new `auth_state.rs` module shared between `lib.rs::run()` (bare-invocation welcome) and the command guard in `pull.rs`. `push.rs` is intentionally NOT guarded because it supports `--token` and `CINCH_TOKEN` as stateless auth sources — its existing empty-token branch already emits the identical `AUTH_FAILURE` + `Run: cinch auth login` error. Auth state is the natural trigger for both surfaces — no marker file.

**Tech Stack:** React + TypeScript (vitest), Rust (clap, tokio), `client_core::auth::load_multi_config`.

---

## Spec reference

`docs/superpowers/specs/2026-05-22-onboarding-flow-design.md`

## Repo conventions you must follow

- This worktree is `cinch/claude-onboarding-flow/` on branch `agent/claude/onboarding-flow`. Every command runs here. Start each new shell with:
  ```bash
  cd /Users/jinmu/Programming/cinchcli/cinch/claude-onboarding-flow && pwd && git rev-parse --abbrev-ref HEAD
  ```
- All code, comments, commit messages, and docs in **English**.
- No `any` in TypeScript — use typed interfaces.
- Tauri command/event bindings (`apps/desktop/src/bindings.ts`) are auto-generated. Do not hand-edit. This plan does not change any commands so regeneration is not required.
- Pre-commit hook `lefthook.yml` runs `version-parity` automatically. Do not edit `Cargo.toml` versions or `package.json` versions — this plan does not require version bumps.

## File structure

```
apps/desktop/src/components/GettingStartedCard.tsx          (new, ~120 lines)
apps/desktop/src/components/GettingStartedCard.test.tsx     (new, ~110 lines)
apps/desktop/src/App.tsx                                    (modified, ~20 line diff)
apps/desktop/src/App.test.tsx                               (modified, +1 test)

crates/cli/src/auth_state.rs                                (new, ~40 lines)
crates/cli/src/lib.rs                                       (modified, ~15 line diff)
crates/cli/src/commands/pull.rs                             (modified, +1 line)
crates/cli/src/commands/push.rs                             (not modified — see Task 3, Step 3 note on the CINCH_TOKEN regression)

docs/superpowers/specs/2026-05-22-onboarding-flow-design.md  (already committed)
docs/superpowers/plans/2026-05-22-onboarding-flow.md         (this file)
```

**Spec correction**: the spec proposed a new `AUTH_REQUIRED_EXIT` constant. After reading `crates/cli/src/exit.rs:10`, the existing `AUTH_FAILURE = 2` constant already covers this — `HttpError::Unauthorized` maps to it with message "Authentication required." We reuse `AUTH_FAILURE` instead of introducing a new constant. No constant changes to `exit.rs`.

---

## Task 1: Add `auth_state` helper module to the CLI

**Files:**
- Create: `crates/cli/src/auth_state.rs`
- Modify: `crates/cli/src/lib.rs` (add `mod auth_state;` declaration only)
- Test: inline `#[cfg(test)] mod tests` inside `auth_state.rs`

The helper is a single boolean: does the active relay profile have a non-empty token? Used by both the welcome path in `lib.rs::run()` and the command guards in `push.rs`/`pull.rs`. Single source of truth.

- [ ] **Step 1: Write the failing test**

Append to `crates/cli/src/auth_state.rs` (we'll create the file in step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use client_core::config::{MultiConfig, RelayProfile};

    #[test]
    fn empty_multi_config_is_unauthenticated() {
        let mc = MultiConfig::default();
        assert!(!has_active_token(&mc));
    }

    #[test]
    fn profile_with_blank_token_is_unauthenticated() {
        let mc = MultiConfig {
            active_relay_id: Some("r1".into()),
            relays: vec![RelayProfile {
                id: "r1".into(),
                token: String::new(),
                ..RelayProfile::default()
            }],
            ..MultiConfig::default()
        };
        assert!(!has_active_token(&mc));
    }

    #[test]
    fn profile_with_token_is_authenticated() {
        let mc = MultiConfig {
            active_relay_id: Some("r1".into()),
            relays: vec![RelayProfile {
                id: "r1".into(),
                token: "abc".into(),
                ..RelayProfile::default()
            }],
            ..MultiConfig::default()
        };
        assert!(has_active_token(&mc));
    }
}
```

Note: `RelayProfile::default()` and `MultiConfig::default()` already exist (see `crates/client-core/src/config.rs`). If the spread `..RelayProfile::default()` fails to compile because some field is non-`Default`, fall back to explicit construction matching the struct definition in that file. Run `cargo doc --no-deps -p cinchcli-core --open` or `grep -n "struct RelayProfile" crates/client-core/src/config.rs` to see the field list.

- [ ] **Step 2: Create the module file with stub, run test to see it fail**

Create `crates/cli/src/auth_state.rs`:

```rust
//! Shared "is this machine signed in?" gate used by the bare-`cinch`
//! welcome message and by the `push`/`pull` pre-flight checks.
//!
//! Returning `false` on any load error (corrupt config, missing file,
//! permission denied) is intentional: greeting a corrupt-config user
//! is harmless, while silently swallowing the message would be worse.

use client_core::config::MultiConfig;

/// True when the active relay profile has a non-empty token.
pub fn has_active_token(mc: &MultiConfig) -> bool {
    todo!("implemented in step 3")
}

/// Convenience: load the disk config and check `has_active_token`.
/// Disk errors fold to `false` (unauthenticated).
pub fn is_authenticated() -> bool {
    todo!("implemented in step 3")
}
```

Add the module declaration to `crates/cli/src/lib.rs` (after the existing `mod` lines near line 11-23):

```rust
mod auth_state;
```

Run:
```bash
cargo test -p cinch-cli auth_state -- --nocapture
```
Expected: FAIL — three tests panic with `not yet implemented`.

- [ ] **Step 3: Implement the helpers**

Replace the body of both functions in `crates/cli/src/auth_state.rs`:

```rust
pub fn has_active_token(mc: &MultiConfig) -> bool {
    mc.active_profile().map(|p| !p.token.is_empty()).unwrap_or(false)
}

pub fn is_authenticated() -> bool {
    client_core::auth::load_multi_config()
        .map(|mc| has_active_token(&mc))
        .unwrap_or(false)
}
```

Run:
```bash
cargo test -p cinch-cli auth_state -- --nocapture
```
Expected: PASS — all three tests.

- [ ] **Step 4: Commit**

```bash
git add crates/cli/src/auth_state.rs crates/cli/src/lib.rs
git commit -m "feat(cli): add auth_state::is_authenticated helper"
```

---

## Task 2: Print first-run welcome from `lib.rs::run()`

**Files:**
- Modify: `crates/cli/src/lib.rs` (add helper function + 4-line branch before `Cli::parse()`)

The branch fires only when `std::env::args().len() == 1` (bare `cinch`) AND `auth_state::is_authenticated()` returns false. clap then takes over and prints the usual help because of `arg_required_else_help = true`.

- [ ] **Step 1: Read current `run()` to find the insertion point**

Run:
```bash
sed -n '195,210p' crates/cli/src/lib.rs
```

You should see `pub fn run() -> i32 {` at line 201 and `let cli = Cli::parse();` at line 202. The welcome branch goes immediately above `Cli::parse()`.

- [ ] **Step 2: Add the helper function and the branch**

Append this helper function to `crates/cli/src/lib.rs` (after the existing constants/functions, before the final `pub fn run()`):

```rust
fn print_first_run_welcome() {
    eprintln!("Welcome to Cinch — pipe clipboard between machines.");
    eprintln!();
    eprintln!("Get started:");
    eprintln!("  cinch auth login           Sign in via browser");
    eprintln!("  echo \"hello\" | cinch push  Send your clipboard");
    eprintln!("  cinch pull                 Receive the latest clip");
    eprintln!();
    eprintln!("Docs: https://cinchcli.com/docs/");
    eprintln!();
}
```

Modify `pub fn run()` to insert the branch right before `Cli::parse()`. The diff:

```rust
pub fn run() -> i32 {
    if std::env::args().len() == 1 && !auth_state::is_authenticated() {
        print_first_run_welcome();
        // Fall through — clap's `arg_required_else_help = true` will
        // print the usage block and exit with code 2.
    }
    let cli = Cli::parse();
    // … existing logic unchanged
```

- [ ] **Step 3: Smoke-test manually**

Run:
```bash
cargo build -p cinch-cli
```

Then:
```bash
# Point at an empty config dir so the helper sees unauthenticated state.
# Easiest portable way: temporarily move ~/.cinch out of the way if it
# exists, OR run with `HOME=/tmp/cinch-test` and ensure the dir is empty.
HOME=$(mktemp -d) ./target/debug/cinch 2>&1 | head -20
```

Expected stderr (before clap's usage block):
```
Welcome to Cinch — pipe clipboard between machines.

Get started:
  cinch auth login           Sign in via browser
  echo "hello" | cinch push  Send your clipboard
  cinch pull                 Receive the latest clip

Docs: https://cinchcli.com/docs/

```

Followed by clap's standard `Usage: cinch <COMMAND>` block.

Then verify `cinch -h` does NOT print the welcome:
```bash
HOME=$(mktemp -d) ./target/debug/cinch -h 2>&1 | head -5
```
Expected: clap help only, no "Welcome to Cinch" line.

And `cinch --help`:
```bash
HOME=$(mktemp -d) ./target/debug/cinch --help 2>&1 | head -5
```
Expected: clap help only, no welcome.

- [ ] **Step 4: Add an integration test**

Create `crates/cli/tests/onboarding.rs`:

```rust
//! Smoke tests for the bare-`cinch` welcome message.

use std::process::Command;

fn cinch_binary() -> std::path::PathBuf {
    // CARGO_BIN_EXE_<name> is set by Cargo for integration tests.
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_cinch"))
}

#[test]
fn bare_cinch_unauthenticated_prints_welcome() {
    let tmp_home = tempfile::tempdir().expect("tempdir");
    let output = Command::new(cinch_binary())
        .env("HOME", tmp_home.path())
        .output()
        .expect("run cinch");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Welcome to Cinch"),
        "expected welcome in stderr, got:\n{}",
        stderr
    );
    assert!(stderr.contains("cinch auth login"), "missing first-command hint:\n{}", stderr);
}

#[test]
fn cinch_help_does_not_print_welcome() {
    let tmp_home = tempfile::tempdir().expect("tempdir");
    let output = Command::new(cinch_binary())
        .arg("-h")
        .env("HOME", tmp_home.path())
        .output()
        .expect("run cinch -h");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        !combined.contains("Welcome to Cinch"),
        "welcome should not appear for -h, got:\n{}",
        combined
    );
}
```

Add `tempfile` to dev-deps if it isn't already present. Check first:

```bash
grep -A1 '\[dev-dependencies\]' crates/cli/Cargo.toml | grep tempfile
```

If absent, add to `crates/cli/Cargo.toml` under `[dev-dependencies]`:
```toml
tempfile = "3"
```

- [ ] **Step 5: Run the integration tests**

```bash
cargo test -p cinch-cli --test onboarding -- --nocapture
```
Expected: both tests PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/cli/src/lib.rs crates/cli/tests/onboarding.rs crates/cli/Cargo.toml
git commit -m "feat(cli): print first-run welcome on bare \`cinch\` invocation"
```

---

## Task 3: Add `ensure_authenticated` guard to `push` and `pull`

**Files:**
- Modify: `crates/cli/src/auth_state.rs` (add `ensure_authenticated` returning `Result<(), ExitError>`)
- Modify: `crates/cli/src/commands/push.rs` (add 1-line guard at top of `run()`)
- Modify: `crates/cli/src/commands/pull.rs` (add 1-line guard at top of `run()`)

The guard short-circuits before any network call when no token is present. Reuses `AUTH_FAILURE` (exit code 2) from `exit.rs`.

- [ ] **Step 1: Write the failing test**

Append to the existing `#[cfg(test)] mod tests` block in `crates/cli/src/auth_state.rs`:

```rust
#[test]
fn ensure_authenticated_errors_when_no_token() {
    // Force is_authenticated() to return false by pointing HOME at a
    // tempdir with no .cinch/ subdirectory.
    let tmp = tempfile::tempdir().expect("tempdir");
    let _guard = EnvGuard::set("HOME", tmp.path());

    let err = ensure_authenticated().expect_err("should fail");
    assert_eq!(err.code, crate::exit::AUTH_FAILURE);
    assert!(err.fix.contains("cinch auth login"), "fix line missing hint: {}", err.fix);
}

/// Scoped env var override that restores the previous value on drop.
/// Lives in this test module only; not exported.
struct EnvGuard {
    key: &'static str,
    prev: Option<std::ffi::OsString>,
}
impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let prev = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, prev }
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}
```

Run:
```bash
cargo test -p cinch-cli auth_state::tests::ensure_authenticated_errors_when_no_token
```
Expected: FAIL — `ensure_authenticated` not defined.

- [ ] **Step 2: Add `ensure_authenticated`**

Insert into `crates/cli/src/auth_state.rs` (above the `#[cfg(test)]` block):

```rust
use crate::exit::{ExitError, AUTH_FAILURE};

/// Pre-flight gate for commands that require an active token. Use at the
/// top of each command's `run()` to short-circuit before any network call
/// when the user hasn't signed in on this machine yet.
pub fn ensure_authenticated() -> Result<(), ExitError> {
    if is_authenticated() {
        return Ok(());
    }
    Err(ExitError::new(
        AUTH_FAILURE,
        "Not signed in on this machine.",
        "Run: cinch auth login",
    ))
}
```

Also add `tempfile` to `[dev-dependencies]` if Task 2 step 4 didn't already.

Run:
```bash
cargo test -p cinch-cli auth_state
```
Expected: all four tests PASS (`empty_multi_config_is_unauthenticated`, `profile_with_blank_token_is_unauthenticated`, `profile_with_token_is_authenticated`, `ensure_authenticated_errors_when_no_token`).

- [ ] **Step 3: Wire the guard into `pull.rs` only**

Wire the guard into `pull.rs` only. `push.rs`'s `resolve_config` already returns the same `AUTH_FAILURE` + `Run: cinch auth login` after considering `--token` and `CINCH_TOKEN`, so adding the guard there would override the documented stateless-push path (CI / containers that have no `~/.cinch/config.json` and pass auth via env / flag).

(Originally this step also patched `push.rs`. That edit was reverted after review caught the `CINCH_TOKEN` regression; the existing check at `push.rs:376` already produces the same error format for the truly-no-token case.)

- [ ] **Step 4: Wire the guard into `pull.rs`**

Open `crates/cli/src/commands/pull.rs`. Find `pub async fn run(args: Args) -> Result<(), ExitError> {` at line 80. Insert as the very first statement:

```rust
pub async fn run(args: Args) -> Result<(), ExitError> {
    crate::auth_state::ensure_authenticated()?;
    // … existing body unchanged
```

- [ ] **Step 5: Smoke-test**

```bash
cargo build -p cinch-cli
HOME=$(mktemp -d) ./target/debug/cinch push <<<"hello" 2>&1
echo "exit=$?"
```
Expected stderr (from push's existing empty-token check at `push.rs:376`, not from the removed guard):
```
✗ No auth token configured.
  Run: cinch auth login
```
Expected exit code: `2`.

And critically — the `CINCH_TOKEN` path stays alive:
```bash
HOME=$(mktemp -d) CINCH_TOKEN=fake ./target/debug/cinch push <<<"hello" 2>&1
echo "exit=$?"
```
Expected: NOT exit 2 from a missing-auth gate. Either a network error against the relay, or an `AUTH_FAILURE` further downstream (token rejected by the relay), but not the bare "Not signed in on this machine." short-circuit.

For pull (which has no `--token` / `CINCH_TOKEN` path, so it keeps the guard):
```bash
HOME=$(mktemp -d) ./target/debug/cinch pull 2>&1
echo "exit=$?"
```
Expected:
```
✗ Not signed in on this machine.
  Run: cinch auth login
```
Exit code `2`.

- [ ] **Step 6: Add integration test for the push guard**

Append to `crates/cli/tests/onboarding.rs`:

```rust
#[test]
fn push_without_auth_returns_auth_failure_exit_code() {
    let tmp_home = tempfile::tempdir().expect("tempdir");
    let output = Command::new(cinch_binary())
        .arg("push")
        .env("HOME", tmp_home.path())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run cinch push");

    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit 2 (AUTH_FAILURE), got {:?}",
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cinch auth login"), "missing hint in stderr: {}", stderr);
}
```

Run:
```bash
cargo test -p cinch-cli --test onboarding push_without_auth_returns_auth_failure_exit_code
```
Expected: PASS.

- [ ] **Step 7: Run the full CLI test suite to verify no regressions**

```bash
cargo test -p cinch-cli
```
Expected: all PASS. If pre-existing tests in `commands/push.rs` or `commands/pull.rs` rely on calling `run()` without setting up auth, they will now fail and need either (a) a mock auth state, or (b) `HOME` pointed at a fixture. If you see such failures, **stop and surface them** — don't paper over by removing the guard. The pre-existing test list at the time of writing this plan was checked by spec review; if you see new failures, report them.

- [ ] **Step 8: Commit**

```bash
git add crates/cli/src/auth_state.rs crates/cli/src/commands/pull.rs crates/cli/tests/onboarding.rs
git commit -m "feat(cli): guard pull with ensure_authenticated"
```

(The integration test `push_without_auth_returns_auth_failure_exit_code` still passes because `push.rs`'s pre-existing empty-token branch returns the same exit code and hint.)

---

## Task 4: Create `GettingStartedCard` component (desktop)

**Files:**
- Create: `apps/desktop/src/components/GettingStartedCard.tsx`
- Create: `apps/desktop/src/components/GettingStartedCard.test.tsx`

Self-contained presentational component. Owns its `dismissed` state internally via localStorage. Caller passes two callbacks: `onCopySnippet` (sends the snippet text to clipboard + toast) and `onOpenDevices` (switches the dashboard's active panel to Devices).

- [ ] **Step 1: Write the failing test file**

Create `apps/desktop/src/components/GettingStartedCard.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { GettingStartedCard } from './GettingStartedCard';

const STORAGE_KEY = 'cinchGettingStartedDismissed';

describe('GettingStartedCard', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('renders heading and snippet', () => {
    render(<GettingStartedCard onCopySnippet={() => {}} onOpenDevices={() => {}} />);
    expect(screen.getByText(/You're signed in/i)).toBeInTheDocument();
    expect(screen.getByText('echo "hello cinch" | cinch push')).toBeInTheDocument();
  });

  it('invokes onCopySnippet with the exact snippet text when Copy is clicked', () => {
    const onCopySnippet = vi.fn();
    render(<GettingStartedCard onCopySnippet={onCopySnippet} onOpenDevices={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: /copy/i }));
    expect(onCopySnippet).toHaveBeenCalledWith('echo "hello cinch" | cinch push');
  });

  it('invokes onOpenDevices when the Devices link is clicked', () => {
    const onOpenDevices = vi.fn();
    render(<GettingStartedCard onCopySnippet={() => {}} onOpenDevices={onOpenDevices} />);
    fireEvent.click(screen.getByText(/Add machine/i));
    expect(onOpenDevices).toHaveBeenCalled();
  });

  it('persists dismissal to localStorage and unmounts when Dismiss is clicked', () => {
    const { container } = render(
      <GettingStartedCard onCopySnippet={() => {}} onOpenDevices={() => {}} />,
    );
    fireEvent.click(screen.getByRole('button', { name: /dismiss/i }));
    expect(localStorage.getItem(STORAGE_KEY)).toBe('1');
    expect(container.firstChild).toBeNull();
  });

  it('renders nothing if localStorage already has the dismissed marker on mount', () => {
    localStorage.setItem(STORAGE_KEY, '1');
    const { container } = render(
      <GettingStartedCard onCopySnippet={() => {}} onOpenDevices={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });
});
```

Run:
```bash
cd apps/desktop && pnpm test -- GettingStartedCard
```
Expected: FAIL — file does not exist yet.

- [ ] **Step 2: Create the component**

Create `apps/desktop/src/components/GettingStartedCard.tsx`:

```tsx
import { useState } from 'react';
import { C } from '../design';

const STORAGE_KEY = 'cinchGettingStartedDismissed';
const SNIPPET = 'echo "hello cinch" | cinch push';

interface GettingStartedCardProps {
  onCopySnippet: (text: string) => void;
  onOpenDevices: () => void;
}

export function GettingStartedCard({ onCopySnippet, onOpenDevices }: GettingStartedCardProps) {
  const [dismissed, setDismissed] = useState<boolean>(
    () => localStorage.getItem(STORAGE_KEY) === '1',
  );

  if (dismissed) return null;

  const handleDismiss = () => {
    localStorage.setItem(STORAGE_KEY, '1');
    setDismissed(true);
  };

  return (
    <div
      data-testid="getting-started-card"
      style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '32px 16px',
      }}
    >
      <div
        style={{
          maxWidth: 360,
          background: C.card2,
          border: `1px solid ${C.border}`,
          borderRadius: 8,
          padding: 20,
          position: 'relative',
        }}
      >
        <button
          onClick={handleDismiss}
          aria-label="Dismiss"
          style={{
            position: 'absolute',
            top: 8,
            right: 10,
            background: 'transparent',
            border: 'none',
            color: C.t3,
            cursor: 'pointer',
            fontSize: 11,
            padding: '2px 6px',
          }}
        >
          Dismiss
        </button>

        <div
          style={{
            fontSize: 16,
            fontWeight: 600,
            color: C.t1,
            letterSpacing: '-0.012em',
            marginBottom: 12,
          }}
        >
          ✨ You're signed in. Now send your first clip.
        </div>

        <div style={{ marginBottom: 14 }}>
          <div style={{ fontSize: 11, color: C.t3, marginBottom: 6 }}>Try it now:</div>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              background: C.bg,
              border: `1px solid ${C.border}`,
              borderRadius: 4,
              padding: '8px 10px',
              fontFamily: 'var(--font-mono)',
              fontSize: 12,
              color: C.t1,
            }}
          >
            <code style={{ flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
              {SNIPPET}
            </code>
            <button
              onClick={() => onCopySnippet(SNIPPET)}
              aria-label="Copy"
              style={{
                background: 'transparent',
                border: `1px solid ${C.border}`,
                borderRadius: 3,
                color: C.t2,
                cursor: 'pointer',
                fontSize: 11,
                padding: '2px 8px',
              }}
            >
              Copy
            </button>
          </div>
        </div>

        <div style={{ fontSize: 12, color: C.t2, lineHeight: 1.5 }}>
          <div style={{ marginBottom: 4 }}>
            Add another device:{' '}
            <button
              onClick={onOpenDevices}
              style={{
                background: 'transparent',
                border: 'none',
                color: C.accent,
                cursor: 'pointer',
                padding: 0,
                fontSize: 12,
                textDecoration: 'underline',
              }}
            >
              Add machine
            </button>{' '}
            via SSH (⌘3 → Devices panel).
          </div>
          <div>
            Or install the CLI on another machine:{' '}
            <code style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: C.t1 }}>
              brew install cinchcli/tap/cinch
            </code>
          </div>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 3: Run the tests**

```bash
cd apps/desktop && pnpm test -- GettingStartedCard
```
Expected: all 5 tests PASS.

If any test fails because of jsdom localStorage behavior, the most likely cause is `beforeEach` not clearing between tests — that block is already in the test. If the SNIPPET text mismatches, ensure you didn't introduce smart quotes (the snippet uses straight `"`).

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src/components/GettingStartedCard.tsx apps/desktop/src/components/GettingStartedCard.test.tsx
git commit -m "feat(desktop): add GettingStartedCard empty state"
```

---

## Task 5: Wire `GettingStartedCard` into `App.tsx`

**Files:**
- Modify: `apps/desktop/src/App.tsx` (import + conditional render block in the `Authenticated` branch)
- Modify: `apps/desktop/src/App.test.tsx` (add one test for the render condition)

The component renders in the inbox panel's body slot when `clips.length === 0 && devices.length <= 1`. It replaces the `<ClipList />` + `<ClipDetail />` pair in that case so the user sees the card instead of a blank list.

- [ ] **Step 1: Read the current Authenticated render block**

Run:
```bash
sed -n '655,685p' apps/desktop/src/App.tsx
```

You'll see the `activePanel === 'devices' ? <DevicesPanel /> : activePanel === 'pinned' ? <PinnedPanel /> : (<><ClipList /><ClipDetail /></>)` ternary inside `<div style={S.body}>`. The card goes inside the `inbox` branch of that ternary.

- [ ] **Step 2: Add the import**

In `apps/desktop/src/App.tsx`, with the other component imports near line 38:

```tsx
import { GettingStartedCard } from './components/GettingStartedCard';
```

- [ ] **Step 3: Modify the inbox branch of the panel ternary**

Locate this block (around line 660-682):

```tsx
        ) : activePanel === 'pinned' ? (
          <PinnedPanel /* ... */ />
        ) : (
          <>
            <ClipList /* ... */ />
            <ClipDetail /* ... */ />
          </>
        )}
```

Change the final branch to:

```tsx
        ) : activePanel === 'pinned' ? (
          <PinnedPanel /* ... existing props ... */ />
        ) : clips.length === 0 && devices.length <= 1 ? (
          <GettingStartedCard
            onCopySnippet={(text) => {
              void unwrap(commands.copyClipToClipboard(text));
              showToast('Copied to clipboard', 'copy');
            }}
            onOpenDevices={() => {
              setActivePanel('devices');
              setSelectedClip(null);
              setSelectedSource(null);
              setActiveFilter('all');
            }}
          />
        ) : (
          <>
            <ClipList /* ... existing props ... */ />
            <ClipDetail /* ... existing props ... */ />
          </>
        )}
```

Do NOT replace the existing PinnedPanel/ClipList/ClipDetail prop lists — copy them through unchanged. The diff is purely the new branch wedged between `pinned` and the default `<>...</>` block.

- [ ] **Step 4: Verify typecheck**

```bash
cd apps/desktop && pnpm exec tsc --noEmit
```
Expected: clean.

- [ ] **Step 5: Add a test in `App.test.tsx`**

Open `apps/desktop/src/App.test.tsx`. The file already mocks `useAuthState`, `invoke`, and notification APIs. Add a new test inside the existing `describe` block. The exact insertion point: find an existing test that mocks an `Authenticated` auth state — use it as a template. If no such helper exists, add this self-contained test at the bottom of the outer `describe`:

```tsx
it('renders GettingStartedCard when authenticated, inbox empty, and only self device', async () => {
  const mockedUseAuth = vi.mocked(useAuthState);
  mockedUseAuth.mockReturnValue({
    variant: 'Authenticated',
    payload: {
      user_id: 'u_test',
      device_id: 'd_test',
      machine_id: 'm_test',
      relay_url: 'https://relay.test',
      email: 'a@b.com',
    },
  } as AuthState);

  render(<App />);

  // The card uses a data-testid for stable lookup.
  await waitFor(() => {
    expect(screen.getByTestId('getting-started-card')).toBeInTheDocument();
  });
});
```

If `AuthState`'s exact field shape differs from above, run:
```bash
grep -A20 "export type AuthState" apps/desktop/src/lib/state/auth.ts | head -30
```
and adjust the payload to match. The test's purpose is just to verify the card mounts under the documented condition.

- [ ] **Step 6: Run the test**

```bash
cd apps/desktop && pnpm test -- App.test
```
Expected: the new test PASSES alongside the existing ones.

- [ ] **Step 7: Manual smoke test**

```bash
cd apps/desktop && pnpm tauri dev
```

In the running app: sign in with a fresh account (or wipe `~/Library/Application Support/me.jinmu.cinch/` first). After sign-in the dashboard should show the card. Click Copy — toast appears. Click `Add machine` link — switches to Devices panel. Reload (Cmd+R) — card reappears. Click Dismiss — card disappears, reload — still gone. `localStorage.removeItem('cinchGettingStartedDismissed')` in DevTools console restores the card on next render.

- [ ] **Step 8: Commit**

```bash
git add apps/desktop/src/App.tsx apps/desktop/src/App.test.tsx
git commit -m "feat(desktop): show GettingStartedCard on empty inbox + single device"
```

---

## Task 6: Final cross-check + push

- [ ] **Step 1: Run the full test suite**

```bash
make test
```

Expected: all green. If any pre-existing test failed because of the `push`/`pull` auth guard (most likely cause: a fixture-driven test that runs `run()` against an empty `HOME`), surface the failure rather than papering over it — that test legitimately exercises the new guard and needs either updating to provide a valid `HOME` or annotating with a fixture.

- [ ] **Step 2: Lint**

```bash
make lint
```
Expected: clean. `cargo fmt --check` may complain on the new files — fix with `cargo fmt`.

- [ ] **Step 3: Push the branch**

```bash
git push -u origin agent/claude/onboarding-flow
```

- [ ] **Step 4: Open a draft PR**

```bash
gh pr create --draft --title "Onboarding flow: GettingStartedCard + CLI first-run welcome" --body "$(cat <<'EOF'
## Summary

- Desktop: new `GettingStartedCard` empty state shown when authenticated, inbox empty, ≤1 device, and not dismissed. Includes a copyable `echo ... | cinch push` snippet and a deep link to the Devices panel.
- CLI: bare `cinch` (unauthenticated) prints a 3-command welcome to stderr; `cinch push` and `cinch pull` short-circuit with `AUTH_FAILURE` and a `cinch auth login` hint when no token is on disk.

Spec: `docs/superpowers/specs/2026-05-22-onboarding-flow-design.md`

## Test plan

- [ ] `make test` passes locally
- [ ] Manual: fresh sign-in on the desktop shows the card; first clip dismisses it; explicit Dismiss persists
- [ ] Manual: `HOME=$(mktemp -d) cinch` shows the welcome
- [ ] Manual: `HOME=$(mktemp -d) cinch push <<<x` exits 2 with the auth hint
EOF
)"
```

---

## Self-review (run before handoff)

**Spec coverage check:**
- Desktop placement + render condition + content + dismiss → Task 4 + Task 5 ✓
- `useDismissed` hook is implemented inline via `useState(() => localStorage…)` rather than as a separate hook — spec language permitted either; the inline form is shorter ✓
- CLI welcome trigger + content + stderr → Task 2 ✓
- CLI push/pull guard + reuse of existing exit code → Task 3 (with note that `AUTH_REQUIRED_EXIT` from spec collapses to `AUTH_FAILURE`) ✓
- Tests for both surfaces → Tasks 1, 2, 3, 4, 5 ✓
- Edge cases (`-h`, `--help`, `completion`, `AdoptedAuthToast` coexistence) → covered by Task 2 integration test (`-h` does not show welcome) and by the unchanged dialog z-index (existing) ✓

**Placeholder scan:** searched plan for "TBD", "TODO", "implement later", "add appropriate error handling" — none found.

**Type consistency:**
- `has_active_token(&MultiConfig) -> bool` — defined Task 1 step 3, used implicitly by `is_authenticated()`.
- `is_authenticated() -> bool` — defined Task 1 step 3, called from `lib.rs` (Task 2) and `ensure_authenticated()` (Task 3).
- `ensure_authenticated() -> Result<(), ExitError>` — defined Task 3 step 2, called from `push.rs` and `pull.rs` (Task 3 steps 3-4).
- `GettingStartedCard` props `{ onCopySnippet: (text: string) => void; onOpenDevices: () => void }` — defined Task 4 step 2, consumed Task 5 step 3. ✓
- localStorage key `'cinchGettingStartedDismissed'` — same string in component (Task 4) and test (Task 4). ✓
- Snippet string `'echo "hello cinch" | cinch push'` — same in component (Task 4 step 2) and test assertions (Task 4 step 1). ✓
