# Onboarding flow — design spec

Status: draft (pending user review)
Date: 2026-05-22
Surfaces: desktop app, CLI

## Problem

A first-time Cinch user finishes sign-in and lands on an empty dashboard
or a bare `cinch --help` screen with no concrete next step. Three
specific moments cause confusion today:

1. **Desktop post-login emptiness.** After OAuth completes, the dashboard
   renders with an empty inbox and only the user's own device in the
   devices list. There is no copy that explains "send your first clip"
   or "add another device."
2. **Desktop ↔ CLI relationship.** New users do not realize that
   `brew install --cask cinchcli/tap/cinch` already drops the `cinch`
   CLI on PATH, that the GUI and CLI share one account, and that the
   easiest "first clip" is `echo … | cinch push` from the same machine.
3. **CLI first-command discovery.** Running bare `cinch` prints clap's
   help with no opinion on which command to try first. New users who
   land in the terminal first do not know whether to start with `auth
   login`, `push`, or `pull`.

The pairing flow itself (`cinch pair`, SSH key bundle exchange, device
approval) is in scope to *mention*, not to *redesign*. It is already
discoverable from the Devices tab and `cinch pair --help` once users
know to look.

## Goals

- A new user who lands on the desktop after OAuth sees one concrete
  call-to-action that gets a clip into the dashboard within ~30 seconds
  and graduates the user out of the empty state.
- A new user who runs `cinch` for the first time in the terminal sees
  the three commands that matter (`auth login`, `push`, `pull`) before
  the clap help.
- A user who runs `cinch push` or `cinch pull` before signing in gets a
  one-line corrected prompt (`cinch auth login`) rather than a network
  or 401 error.
- No new modal at sign-in. No new top-level CLI subcommand. No marker
  files. No localization.

## Non-goals

- `cinch init` subcommand or any interactive wizard.
- Welcome modal, coachmarks, spotlight, sample-clip injection, or any
  Rail/sidebar additions on the desktop.
- Telemetry on the onboarding state. The trigger conditions are
  observable from existing data (auth state, clip count, device count).
- Localized strings. All onboarding copy is English, matching repo
  policy.

## Design

### Desktop — `GettingStartedCard`

#### Placement

`apps/desktop/src/App.tsx`, inside the `Authenticated` branch, renders
the card in place of `<ClipList />` / `<ClipDetail />` when the inbox is
empty and only the local device is registered. The existing
`EmptyState.tsx` (which handles "no search results") is unchanged — the
new card is a distinct component with a different purpose and copy.

```tsx
// inside <main data-testid="dashboard-root"> when the render condition
// (see below) holds. Shown in the body slot where <ClipList /> /
// <ClipDetail /> normally render for the 'inbox' panel.
<GettingStartedCard
  onCopySnippet={(text) => {
    void unwrap(commands.copyClipToClipboard(text));
    showToast('Copied to clipboard', 'copy');
  }}
  onOpenDevices={() => setActivePanel('devices')}
/>
```

The gate uses unfiltered `clips.length`, not `filteredClips.length` —
a fresh user has no source filter or search query active, so the two
are equivalent, but the unfiltered count is the conceptually correct
signal ("the user has zero clips total," not "their current filter
returns zero rows").

#### Render condition

```ts
auth.variant === 'Authenticated'
&& activePanel === 'inbox'
&& clips.length === 0
&& devices.length <= 1
&& !dismissed
```

`dismissed` is read from `localStorage.getItem('cinchGettingStartedDismissed') === '1'`.
The card graduates the user naturally as soon as either a clip arrives
(`clips.length > 0`) or a second device is paired (`devices.length > 1`).
Dismissal is the only path that writes localStorage.

The `devices.length <= 1` rather than `=== 0` accommodates the 1-second
delay between sign-in and the first `refreshDevices()` call in `App.tsx`
— during that window `devices.length === 0`, after it the user's own
device is present, so `<= 1` covers both states without flicker.

#### Content

Three blocks, single column, ~360px wide, fits on default 960×600 window.

1. **Heading**: "✨ You're signed in. Now send your first clip."
2. **Try it now** — code block + Copy button:
   ```
   echo "hello cinch" | cinch push
   ```
   Copying the snippet to the clipboard *also* lets the user paste it
   into their own terminal. Once they hit enter, the desktop receives
   the clip, `clips.length > 0`, and the card unmounts on the next
   render. This is the primary "magic moment."
3. **Add another device** — two short lines:
   - SSH: "Press ⌘3, then **Add machine** to pair a remote host."
     Clicking the text calls `onOpenDevices()` which sets `activePanel`
     to `'devices'`.
   - Other Mac: "Install the CLI with `brew install cinchcli/tap/cinch`."
4. **Dismiss** (top-right) — small ghost button labeled `Dismiss`,
   writes `cinchGettingStartedDismissed=1` to localStorage and unmounts.

Visual style reuses the existing `dialogPrimitives.dialog` /
`design.ts` color tokens (`C.card2`, `C.border`, monospace for the
snippet). No new design system primitives.

#### Component contract

`apps/desktop/src/components/GettingStartedCard.tsx` (new file):

```tsx
interface GettingStartedCardProps {
  onCopySnippet: (text: string) => void;
  onOpenDevices: () => void;
}
export function GettingStartedCard(props: GettingStartedCardProps): JSX.Element;
```

The component owns the dismissed state via a small internal
`useDismissed()` hook. `App.tsx` does not need to know about
localStorage — it only branches on `clips.length` and `devices.length`,
and the card unmounts itself when dismissed by setting an internal
boolean.

Trade-off: the dismissed state is checked twice — once internally to
decide whether to render the card's contents, and once externally
implicitly via the natural-graduation condition. Keeping both keeps
`App.tsx`'s JSX free of localStorage reads. Acceptable.

### CLI — first-run welcome + push/pull guard

#### Bare `cinch` (no args)

`crates/cli/src/lib.rs::run()` entry, before `Cli::parse()`:

```rust
pub fn run() -> i32 {
    if std::env::args().len() == 1 && !is_authenticated() {
        print_first_run_welcome();
        // fall through — clap will print help and exit on missing args
    }
    let cli = Cli::parse();
    // … existing logic
}
```

`is_authenticated()` calls `client_core::auth::load_multi_config()` and
checks whether the active profile has a non-empty token. Failure to
read the file (corrupt, missing, permissions) is treated as
unauthenticated — safer to greet a corrupt-config user than to swallow
the message.

`print_first_run_welcome()` writes to **stderr** so any caller piping
stdout is unaffected:

```
Welcome to Cinch — pipe clipboard between machines.

Get started:
  cinch auth login           Sign in via browser
  echo "hello" | cinch push  Send your clipboard
  cinch pull                 Receive the latest clip

Docs: https://cinchcli.com/docs/

```

Plain ASCII. No color, no Unicode glyphs other than already-acceptable
characters. The trailing blank line separates it from clap's help.

#### `cinch push` / `cinch pull` while unauthenticated

`crates/cli/src/commands/push.rs` and `pull.rs` each gain a guard at
the very top of their `run()` function:

```rust
ensure_authenticated()?;
```

`ensure_authenticated()` lives in `crates/cli/src/commands/mod.rs`:

```rust
pub(crate) fn ensure_authenticated() -> Result<(), ExitError> {
    if is_authenticated() { return Ok(()); }
    Err(ExitError::new(
        AUTH_REQUIRED_EXIT,
        "Not signed in on this machine.".to_string(),
        "Run `cinch auth login` to get started.".to_string(),
    ))
}
```

The two `is_authenticated()` helpers (lib.rs and mod.rs) are the same
function — define it once in `crates/cli/src/commands/mod.rs` (or a new
`crates/cli/src/auth_state.rs`) and reuse from both sites.

`AUTH_REQUIRED_EXIT` is a new constant alongside the existing exit
codes in `exit.rs` (currently includes `GENERIC_ERROR`, `NETWORK_ERROR`,
etc.). Reuses the existing `ExitError` rendering — which already
handles TTY-aware color/Unicode gating — so no new formatting code.

v1 only guards `push` and `pull` because they are the two commands a
new user is most likely to try first. `list`, `pin`, `search`, `pull
--from`, etc. fall through to the existing 401-handling path; adding
guards to all of them is mechanical follow-up work but not part of this
spec.

### Why no marker file?

The natural "first time" gate is **authentication state**, not "have we
shown this message yet?" Logout → login again is a valid second visit
where re-seeing the welcome is harmless. A marker file would introduce
edge cases around uninstall/reinstall, multiple users on the same
machine, and stale state, with no obvious payoff. The auth-state gate
is also free — no new files written by the CLI.

## Edge cases

### Desktop

- **`AdoptedAuthToast` co-occurrence**: When CLI hands OAuth off to the
  desktop and the desktop adopts the token, a toast appears top-right
  while the dashboard renders. The `GettingStartedCard` sits in the
  body and does not overlap the toast region. Both can render
  simultaneously without visual conflict.
- **`AddRelayDialog` (CLI handoff) over the card**: If the user is
  signing in via a CLI handoff and the dialog opens *after* the card is
  rendered, the modal overlay covers the card, then the card resumes
  when the modal closes. No special handling required — z-index of
  `dialogPrimitives.overlay` already wins.
- **First clip-deleted edge**: A user who receives a clip and deletes
  it returns to `clips.length === 0`. If `devices.length > 1` (they
  paired something while the card was hidden), the card does *not*
  re-appear — they have already used the product. If they happen to be
  back to a single-device state (deleted device), the card does
  re-appear unless dismissed. Both behaviors are intentional.
- **Theme**: Card honors current light/dark theme via the existing `C`
  color tokens. No additional theme work.

### CLI

- **`cinch -h` / `cinch --help`**: `args().len() > 1`, so the welcome
  message does not appear — the user explicitly asked for help.
- **`cinch completion zsh > file`**: `args().len() > 1`, welcome not
  shown. stdout untouched. Welcome is stderr anyway, so even a
  hypothetical bare-`cinch` plus stdout redirection would not pollute
  the completion output.
- **Headless / pipe environments (CI, Docker)**: stderr is acceptable
  collateral; if a CI step runs `cinch` to discover commands they will
  see the welcome but no script depends on stderr being clean.
- **`load_multi_config()` failure**: Treated as unauthenticated; the
  welcome shows. Better than silently swallowing it.
- **Concurrent CLI runs during sign-in**: One terminal runs `cinch auth
  login`; another runs bare `cinch`. If the second runs after the
  config write, no welcome. If before, welcome shows. Either is
  correct.

## Testing

### Desktop

- `apps/desktop/src/components/GettingStartedCard.test.tsx` (new,
  vitest):
  - Renders heading and snippet text.
  - Copy button invokes `onCopySnippet` with the exact snippet string.
  - `onOpenDevices` is called when the "Press ⌘3" line is clicked.
  - Dismiss button writes `cinchGettingStartedDismissed=1` to
    localStorage and the component returns `null` on the next render.
  - When localStorage already has the dismissed marker on mount, the
    component returns `null` immediately.
- `apps/desktop/src/App.test.tsx`: one new test case that mounts `App`
  with mocked auth (`Authenticated`), zero clips, one device (self),
  and asserts the card is present. A second case with one clip asserts
  the card is absent.

### CLI

- `is_authenticated()` (in whichever module it lands) — unit tests for
  three cases: profile with non-empty token, profile with empty token,
  no config file. Use a tempdir-scoped `XDG_CONFIG_HOME` (or whatever
  `load_multi_config()` reads from) so the test doesn't touch the real
  user's config.
- `ensure_authenticated()` — unit test that asserts the returned
  `ExitError` carries `AUTH_REQUIRED_EXIT` and a hint containing
  `cinch auth login`.
- `print_first_run_welcome()` — no unit test (stderr side effect on
  process-wide stream). Snapshot-equivalent assertion is the unit test
  above on `is_authenticated()` plus a manual smoke check.
- Integration: `crates/cli/tests/onboarding.rs` (new, optional) — spawn
  the binary with `CINCH_HOME` pointing at an empty tempdir and bare
  `cinch` args; assert stderr contains "Get started:". A second case
  with `cinch push` against an empty tempdir asserts exit code matches
  `AUTH_REQUIRED_EXIT` and stderr contains "cinch auth login".

## Files changed

```
apps/desktop/src/components/GettingStartedCard.tsx          (new)
apps/desktop/src/components/GettingStartedCard.test.tsx     (new)
apps/desktop/src/App.tsx                                    (modified)
apps/desktop/src/App.test.tsx                               (modified, +1-2 cases)
crates/cli/src/lib.rs                                       (modified, ~10 lines)
crates/cli/src/commands/mod.rs                              (modified, helper)
crates/cli/src/commands/push.rs                             (modified, +1 line)
crates/cli/src/commands/pull.rs                             (modified, +1 line)
crates/cli/src/exit.rs                                      (modified, +1 const)
crates/cli/tests/onboarding.rs                              (new, optional)
```

No proto changes. No relay changes. No version bumps. No homebrew tap
changes.

## Open questions for review

None at draft time — all decisions are recorded above. Reviewer may
push back on:

- Snippet text (`echo "hello cinch" | cinch push`) vs. something
  shorter or more memorable.
- Whether `ensure_authenticated()` should also guard `list`, `search`,
  `pin`, etc. in this same PR or in a follow-up.
- Whether the welcome message should include a fourth line pointing at
  `cinch pair` for SSH setup. Current draft keeps it to three commands
  for minimum noise.
