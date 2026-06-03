# Desktop Settings Re-plan — Design

> **Status:** Approved design (brainstorming complete). Drives the implementation plan.
> **Date:** 2026-06-04
> **Scope:** `apps/desktop/src/SettingsPane.tsx` + the components it renders.
> **Driver:** the `docs/research/` AI-era positioning, reconciled with `specs/desktop-redesign-brief.md`.
> **Companions:** `docs/research/ai-era-clipboard-strategy.md`, `docs/research/cinch-competitive-landscape.md`,
> `specs/2026-05-24-ai-clipboard-mcp-design.md` (the shipped `cinch mcp` surface this exposes).

---

## 1. Motivation

cinch's positioning is **locked** as *"The clipboard for the AI-era."* The research docs are unanimous
that cinch's only structurally-defensible identity is the intersection of **(1) a true headless Unix-pipe
CLI** and **(2) an agent-native AI/MCP surface** — sync, encryption, relay, and self-host are all
parity-or-behind vs. ClipCascade.

`OnboardingScreen` already sells this (`copy → relay → agent·mcp` diagram, *"let your agents reach the
same clips over MCP"*). But **Settings has zero trace of AI/MCP or the CLI** — its five tabs
(General / Account / Shortcuts / Servers / Sessions) predate the positioning. Even the existing
`apps/desktop/preview/redesign-mockups/settings.html` mockup is the *same* old IA with only a visual
refresh. That gap is the "옛날 기준" the re-plan fixes.

Separately, **Account holds a Display name field the product does not use.** Removing it empties the
Account tab down to three read-only rows, so the tab is dissolved.

## 2. Goals

1. **Surface the AI-era identity in Settings** via one new first-class section, *Agents & CLI*, that
   tells a developer exactly how to connect Claude Code / Cursor and use the `cinch` pipe — using only
   **truthful, verified** commands.
2. **Remove the unused Display name** field and dissolve the now-near-empty Account tab.
3. **Re-group the surviving settings** into a calmer, intent-named IA (Privacy is the trust home;
   Devices merges relay + sessions) without dropping any genuinely-used control.
4. **Keep** the existing sidebar + scrollable-content shell and the nendo visual language; this is an
   IA + content change, not a shell rewrite.

## 3. Non-goals (deliberately out of scope)

- **Display name backend removal.** This design removes the *UI* and the desktop's *use* of
  `setDisplayName`. Fully ripping out the `set_display_name` Tauri command, the
  `UserProfile.display_name` field, and the relay's `POST /auth/display-name` is a coordinated wire +
  generated-bindings + relay change — tracked as a **follow-up**, not bundled here.
- **Functional MCP control** in the desktop (server on/off, `scope:"fleet"` toggle). The chosen depth is
  **informational only**. `scope:"fleet"` is not built yet and must not be implied.
- **Deeper redesign-brief items**: device fingerprint/verification surface (§6c/§11), danger-zone
  restyle, top-level pending-approval badge, Settings-as-drawer/split. Real, but separate efforts.
- **Token migration / type-scale unification** from the brief's P2. Out of scope; match existing tokens.

## 4. The new IA — five sections

Nav order and the content each section owns. Everything except *Agents & CLI* is relocated, not new.

| Nav | Title (header) | Owns |
|---|---|---|
| **General** | "General" | read-only identity (email · provider · user ID) · appearance (theme) · window size · notifications |
| **Privacy** | "Storage & privacy" | "What the relay can see" trust block · local retention · remote retention · clear local history |
| **Devices** | "Relay & devices" | relay connection (re-authenticate / disconnect) · remote devices · pending sign-ins |
| **Agents & CLI** | "Agents & CLI" | **NEW** — MCP connect (Claude Code + Cursor) · `cinch` CLI status + `cinch pull` quickstart |
| **Shortcuts** | "Keyboard" | launch shortcut · send shortcut · built-in key reference (unchanged) |

**`Tab` type changes** from `"general" | "account" | "shortcuts" | "servers" | "sessions"` to
`"general" | "privacy" | "devices" | "agents" | "shortcuts"`.

### 4.1 Content migration map (from today → new)

- **old General ("Storage & privacy")** splits:
  - trust block + local retention + remote retention + clear history → **Privacy**
  - window size + notifications → **General**
- **old Account ("Profile")**: display name **removed**; email/provider/user ID rows → **General** (read-only identity block). The `AccountSection` component is dissolved (see §6).
- **old Servers** (relay card + `DevicesPanel`) → **Devices**.
- **old Sessions** (`PendingLoginCard` list + `ManualApproveForm`) → **Devices** (a second block below remote devices). Sessions is an approval interrupt, not a peer tab.
- **old Shortcuts** → **Shortcuts** unchanged.

### 4.2 General — new "Appearance / theme" control

Add a segmented control **System · Light · Dark** that **reuses the existing `useTheme()` hook**
(`apps/desktop/src/lib/state/theme.ts`, returns `{ mode, theme, setMode }`, persists
`localStorage["cinch-theme"]`, toggles `html.light`). It is a second *view* onto that mechanism, **not
new state**. No desync risk: Settings replaces the whole window, so the SearchBar theme menu is
unmounted while Settings is open and re-reads `localStorage` on remount when Settings closes. The
SearchBar theme menu stays as-is (both entry points are fine).

Window-size presets and the notifications checkbox move here verbatim from old General.

## 5. The new section — "Agents & CLI" (informational, verified)

The hero of the re-plan. Two blocks. **Every command below is verified against the repo
(`README.md`, `crates/cli/`).** Copy must stay truthful per the brief's voice rules (§7a) and threat
model (§11).

### 5.1 MCP — connect your agent

Intro line (honest — `cinch mcp` is read-only over the **local** store; `CONTRIBUTING.md` guarantees no
auto-send to AI providers):

> *"Let your coding agent search and read this Mac's clipboard history — including clips synced here from
> your other devices — read-only. Cinch never sends a clip to an AI provider on its own."*

Two copyable command rows (mono box + copy button each):

- **Claude Code** — `claude mcp add cinch -- cinch mcp`
- **Cursor** (`~/.cursor/mcp.json` or project `.cursor/mcp.json`):
  ```json
  { "mcpServers": { "cinch": { "command": "cinch", "args": ["mcp"] } } }
  ```

A quiet "What can it read?" affordance lists the three tools honestly: `search_clipboard`,
`list_recent_clipboard`, `get_clipboard_item`. A "Read the MCP guide →" link points at the docs site.

> **Honesty guardrails (load-bearing):**
> - Do **not** describe MCP as reading *other machines'* clipboards directly. Today `cinch mcp` reads the
>   **local** `~/.cinch/store.db`; the desktop populates that store with synced clips, so "clips synced
>   here" is the truthful framing. `scope:"fleet"` remote-read is **not built**.
> - Do **not** show `cmd | cinch push` as a cross-machine send — **`cinch push` is local-only**
>   (`crates/cli/src/commands/push.rs`: *"local-only; the relay is never contacted"*) and there is **no
>   `cinch send`** verb.

### 5.2 Command line

- **`cinch` status row.** Floor: static one-liner that the `cinch` CLI ships with the app and how to
  confirm it's on `PATH`. Optional enhancement: a live read-only check (see §7) that shows
  *installed · `/opt/homebrew/bin/cinch` · vX.Y.Z* or a "not on PATH" hint.
- **Truthful pipe example** (mono, copyable): `cinch pull | pbcopy`
  - caption: *"Print the most recent clip from any of your devices to stdout."*
  - This is accurate: `cinch pull` hits the relay (`GET /clips/latest`) across all paired devices
    (`crates/cli/src/commands/pull.rs`). It is the real headless cross-machine read.
- "CLI docs →" link.

### 5.3 New components

- **`CopyField`** — reusable: a mono code box + a copy button (writes to the clipboard via the Tauri
  clipboard plugin or `navigator.clipboard`), with a brief "Copied" flash. Used for both MCP command
  rows and the CLI example. Keep it small and token-aligned to the existing button styles.
- **`AgentsSection`** (new file `apps/desktop/src/components/AgentsSection.tsx`) — composes the two
  blocks above. Rendered when `activeTab === "agents"`.

## 6. Display name removal & Account dissolution

- **`AccountSection.tsx`**: remove the Display name `<input>`, the Save button, `draft`/`saving`/`error`/
  `savedFlash` state, the `onSave` handler, and the `commands.setDisplayName` call. Keep the read-only
  `email · provider · user ID` rows (fed by `commands.getUserProfile()`).
- **Repurpose, don't delete the file wholesale**: rename/trim `AccountSection` into a lean read-only
  **`AccountIdentity`** component (email/provider/user ID `<dl>`), rendered inside **General**. (Keeping
  one component avoids scattering the `getUserProfile` call.)
- **`getUserProfile`** stays; it still returns `display_name` (now ignored by the frontend) — harmless,
  removed in the backend follow-up.
- **Tests**: update `AccountSection.test.tsx` → drop the display-name save/validation cases; keep/adjust
  the identity-rows rendering test under the new component name.

## 7. Backend touchpoints

This design is **frontend-only by default**. One optional, clearly-bounded addition:

- **(Optional) `get_cli_status` Tauri command** for §5.2's live status row. Read-only: resolves whether
  `cinch` is on `PATH` (e.g. `which cinch`) and its `--version`. Returns a Specta-exported
  `CliStatus { on_path: bool, path: Option<String>, version: Option<String> }`. If we ship it,
  regenerate bindings (`cargo test export_bindings -- --ignored`) per `apps/desktop/CLAUDE.md`. If we
  skip it, the section ships with static guidance and the copyable commands — the chosen
  *informational* depth holds either way.

No proto/wire changes. No relay changes.

## 8. Deep-link wiring (onboarding → Privacy)

`OnboardingScreen.onShowSettings` is documented as "Opens Settings → 'What the relay can see'." That
trust block now lives in **Privacy**, so the deep-link must land there.

- Add an optional `initialTab?: Tab` prop to `SettingsPane`; seed `activeTab` from it (default
  `"general"`).
- In `App.tsx`, hold a `settingsInitialTab` alongside `showSettings`; `OnboardingScreen.onShowSettings`
  sets it to `"privacy"`. Other open paths (tray, `Cmd+,`, dashboard button) default to `"general"`.

## 9. `SettingsPane.tsx` changes (summary)

- `Tab` union + `CATEGORY_META` rebuilt for the five new sections (titles/subtitles per §4); keep the
  honest retention/clear-history copy already flagged load-bearing in the file header.
- **Nav label casing fix**: the nav currently relies on CSS `textTransform: capitalize`, which would
  render "Agents & CLI" as "Agents & Cli". Store labels already-cased and drop the `capitalize`
  transform (or special-case) so "Agents & CLI" renders correctly.
- Section routing: `general` → `AccountIdentity` + theme + window + notifications; `privacy` → trust +
  retention×2 + clear; `devices` → relay card + `DevicesPanel` + sessions; `agents` → `AgentsSection`;
  `shortcuts` → unchanged.
- The retention `LoadState` (loading/error/ready) now gates the **Privacy** tab (it owns the retention
  sliders), not General.

## 10. Honesty & threat-model alignment

- Trust block ("Clip contents — never" / "Names, timing & size — yes" / "Want zero metadata? self-host")
  moves verbatim into Privacy — it is already honest and brief-aligned.
- MCP/CLI copy follows §5's guardrails: local-store read, no fleet claim, no fictional `send`/`push`
  cross-machine framing.
- No new security claims (fingerprint/verification) are introduced; those remain a separate effort.

## 11. Testing

- **`SettingsPane`**: nav renders exactly the five sections with correct labels (incl. literal
  "Agents & CLI"); switching tabs renders the right block; `initialTab="privacy"` opens on Privacy.
- **`AccountIdentity`**: renders email/provider/user ID; no display-name input exists.
- **`AgentsSection`**: renders both verified commands; `CopyField` copy button writes the exact command
  string and shows the flash.
- **(If shipped) `get_cli_status`**: Rust unit test for the on-path / not-on-path branches.
- Keep desktop test conventions (`commands`/`events` via `bindings`, no raw `invoke`).

## 12. Open decisions — resolved

- **Identity placement** → General (read-only block at top). *(Resolved with user.)*
- **"Agents & CLI" label/scope** → keep both MCP setup and CLI quickstart in one section; MCP is the
  hero, CLI is lean. *(Resolved with user.)*
- **MCP/CLI depth** → informational; live `get_cli_status` is optional. *(User chose informational.)*
- **Shell** → keep existing sidebar + content; no drawer/split. *(In scope note.)*
- **Section count** → five (Shortcuts stays standalone; keyboard-first is the moat).
