# Redesign Brief v2: Cinch Desktop — A Clipboard Developers Trust

## 1. Mission

Redesign the cinch macOS menu-bar app so a developer's reaction in the first five seconds is *"I trust this, and it's already out of my way."* Cinch is an end-to-end-encrypted clipboard that syncs across a developer's machines. The app today is functional and keyboard-fast, but it **under-communicates trust, hides its filters, loses context on every navigation, only mentions encryption when something breaks, and leaves its fastest interaction (a global-shortcut quick-paste loop that already exists in the Rust layer) undesigned.** We want a UI that feels **trustworthy, calm, fast, and developer-grade** — closer to Raycast's instant calm and Linear's restraint than to a settings-heavy "sync utility." Keep the editorial dark aesthetic that already differentiates it; sharpen the hierarchy, surface the trust honestly, and make the grab-a-clip loop sub-second. North-star feeling: *quiet competence.*

A note on honesty, because this is a security product: this brief has been audited against the actual code. Where the product's trust story is weaker than marketing copy would imply (the relay brokers public keys during key exchange; revoke does not rotate the shared key today; device fingerprints exist in the data model but are never compared), we say so plainly and design **toward** the credible version rather than writing copy the crypto cannot back. See §11 (Threat model) — it is load-bearing for every trust string in this doc.

## 2. Product context

- **What it is:** a synced clipboard for developers. Copy on your laptop, paste on an SSH box, a Docker container, or CI. Clips are encrypted client-side (AES-256-GCM, keys exchanged via X25519); the relay stores ciphertext only. History is stored locally (SQLite + FTS5) and is fully searchable.
- **Daily job-to-be-done:** *"I copied something somewhere; I need it here, now, without thinking."* This is a Raycast/Spotlight *grab* job, not a window-browsing job — the design must treat it as the hero. Secondary jobs: re-grab a clip from earlier today, pin a frequently reused snippet, add/approve a machine, confirm sync is actually working.
- **Two personalities of one app** (see §6b): a fast **Quick-Paste** front door (summoned by global shortcut) and a fuller **Manager** window for browsing, devices, and settings. Both are the same React app; the Rust layer already supports repositioning on the active monitor and restoring previous-app focus.
- **Form factor and shell:** see §8 (stated once there). In short: frameless, fixed-size tray panel; today a three-pane layout (56px icon Rail / list column / detail pane) plus two helper webviews (snap-guide overlay and a full-screen copy-success toast).

## 3. Who we're designing for

A working developer on macOS who keeps terminals, SSH sessions, and an editor open all day. They live on the keyboard, distrust apps that touch their clipboard, and have taste calibrated by **Raycast** (instant, keyboard-native, zero chrome), **Linear** (typographic restraint, fast transitions, opinionated defaults), **Warp** (developer-dense but legible), **1Password** (trust made visible — you always know what's encrypted, what's verified, and what a destructive action will do), and **Arc** (playful-but-precise motion). That is the bar.

**Primary persona: the keyboard-first developer** (Raycast/Linear taste). **Tiebreak rule:** when power-user density and newcomer legibility conflict, default to **legible-first visuals with keyboard-fast accelerators layered on top** — every visible control also has a shortcut. Never ship two separate modes that fight each other. This rule resolves the comfortable-vs-compact question in §7: comfortable visuals are the default; speed comes from keyboard accelerators, not from a denser skin.

## 4. Design goals, made concrete

Five testable principles. Each says what it means *here* and the anti-pattern to avoid. Measurable targets are in §4b.

**1. Trustworthy — make encryption, provenance, and *verification* visible as calm fact.**
Here: a developer can tell, at a glance and without jargon, (a) that clips are encrypted on this device before they leave it, (b) which machine a clip came from, (c) which devices are *verified* vs. merely *joined*, and (d) exactly what a destructive action will do before they confirm it. Encryption is stated as fact in a Trust/Privacy surface and a quiet lock affordance — never headlined, never fearful, never overstated beyond what §11's threat model supports.
Anti-pattern: encryption surfacing **only** in error toasts (`ClipDecryptFailedToast`, `OfflineQueueDroppedToast`), which makes E2EE read like a bug; "🔒 BANK-GRADE SECURITY" marketing badges (no fearful security marketing anywhere — this is the one place that rule is stated); and the inverse danger — trust copy that claims a guarantee the code does not deliver (e.g. "the server only sees scrambled text" as an unqualified global claim, or a fingerprint shown but never comparable).

**2. Cleaner — one clear thing per surface, ruthless hierarchy.**
Here: the clip list reads as a scannable feed (type at a glance, source, time, preview) with real visual hierarchy in its time-bucket headers; the detail pane states its context; destructive controls live in a visually distinct zone, not inline next to routine ones.
Anti-pattern: 11px faint bucket labels that read as artifacts; a "Revoke" button styled like a normal control; scattered "Add via SSH" entry points; **code clips rendered in the body face in the list row** (the list preview is already proportional — the fix is to switch *code* previews to mono, not to overhaul prose; see §7).

**3. More intuitive — affordances you can see, not sigils you must learn.**
Here: filtering by type or device is a visible control you can click and discover with no hint; the current panel and active filters are always legible; onboarding shows the *outcome* (copy → paste) before the mechanism.
Anti-pattern: filters whose *resting* affordance only appears once you've already typed `#` or `@`; read-only-looking shortcut-capture fields; a first-run dialog titled "Connect to relay" with zero explanation.

**4. Simpler — preserve context, sensible defaults, no needless modality.**
Here: switching panels, clicking away to paste, or copying a clip never silently wipes the user's search/filter in Manager mode; settings and pin-notes don't have to be full blocking takeovers; defaults are chosen so most users never open Settings.
Anti-pattern: the four independent context-wipe sites documented in §6a; full-app-blocking guidance dialogs (`CleanupDialog`).

**5. Fast — the grab-a-clip loop is sub-second and keyboard-only.**
Here: from global shortcut to clip-on-clipboard is under one second for the common case (top clip, no typing), and every keystroke in the picker feels instant. Speed is a stated, measured property, not a vibe.
Anti-pattern: a success story that tops out at "two visible clicks then Enter" (mouse-anchored); a picker that requires an extra ArrowDown because nothing is pre-selected; a search box that round-trips to disk on every keystroke even for the hot recent set.

## 4b. Success criteria (measurable, one per goal)

Validation method: **5-person unmoderated usability sessions** (lightweight) for the qualitative goals, plus **binary regression gates** in code for the deterministic ones. Capture a "before" baseline on the current build for the two timed metrics so "it worked" is measured against a number.

- **Trustworthy** — after onboarding, ≥4/5 developers can, unprompted, correctly answer "what can the server see?" (*only ciphertext*) and "is removing a device reversible?"; **0/5** describe encryption as "a problem/error." ≥4/5 can locate where a device's verification state is shown and explain what "verified vs. joined" means.
- **Cleaner** — ≥4/5 can identify a clip's content-type (code vs. url vs. text vs. image) from the list row alone in <2s without opening detail; bucket headers rated "clearly a section header, not metadata" by ≥4/5.
- **Intuitive** — ≥4/5 discover and apply a Type or Device filter via visible controls with **zero** hint about `#`/`@`; first-run completion (sign-in → first synced clip visible) ≥90% with no external help.
- **Simpler** *(binary regression gate, not a study)* — search/filter context survives a Manager panel switch, a window blur, and a copy in **100%** of runs; median "find a known earlier clip and copy it" ≤3 actions and ≤10s.
- **Fast** *(measured)* — Quick-Paste: global-shortcut → top clip copied ≤1s with zero typing; per-keystroke filter render <120ms on the hot recent set (≈200 clips); no dropped frames rendering a 1MB log clip.

## 5. Current state — keep vs. fix

**Keep (do not regress these):**
- Keyboard-first navigation: `Cmd+1/2/3`, `Tab`, arrow + vim (`Ctrl+J/K`) clip nav, `Cmd+F` / `/` search, `Cmd+P` pin, `Ctrl+H/L` source cycle, `Cmd+C` / Enter copy, `Cmd+Enter` send. This is the product's real moat — extend it, never water it down.
- The editorial **dark** palette (cream-tinted `#16140F` bg / `#EDE6D6` text, Porcelain Teal `#4FB3A9` accent) **and its light counterpart** (`#FBFBFA` bg / `#2F3437` text / `#2F7F78` accent) — present both as token pairs, never hardcode the dark hexes (§7). Single-family Geist + Geist Mono, custom Phosphor-style icon set, tokenized spacing/radii (`--sp-*`, `--radius-sm/md/lg/xl` already exist). It looks designed, not generated.
- Deterministic per-device source coloring (6-slot pastel pills) with user override — strong "which machine" identity.
- Auto content-type classification and smart rendering in **detail** (code as `<pre>`, prose as measured text, images centered, CSS colors as swatches, JSON pretty-printed) — the detail pane already has the prose/code split logic; the list should borrow it.
- CSS-variable theming (no re-render on theme switch); optimistic UI on edits; solid a11y scaffolding (aria-current/selected/label, dialog roles). *(Theme is CSS-variable-driven but selected via JS — the SearchBar theme menu toggles `html.light` for light/dark/system; "zero-JS" referred only to the variable swap.)*
- The existing global-shortcut quick-paste plumbing — `register_global_shortcuts`, `show_on_active_monitor` (centers on the cursor's monitor, captures the frontmost app), `focus_previous_app` (hides and re-activates the previous app). The design must **adopt and name** this loop, not leave it implicit.

**Fix (highest-severity first, cited to the audit and verified in code):**
- **Context loss (IA, high) — four independent sites, not one.** See §6a for the exact spec; a fix that only addresses Rail switches ships a still-broken app.
- **No content-type glyph in the list row (high):** users open the detail pane just to learn if a clip is code vs. url. Add an inline glyph. *(Note: the list preview already uses the proportional body face; only `code` previews need to switch to mono.)*
- **Onboarding/auth communicates nothing about trust (high ×4):** `AddRelayDialog` first-run is titled "Connect to relay" with no explanation; no proactive privacy statement anywhere; key generation is opaque; device approval (`PendingLoginCard`) is a one-click grant with zero stated consequence. Worse, `LocalOnly` traps the user — the first-run `AddRelayDialog` renders with `hideClose` and no "use locally for now" escape.
- **No device verification surface (high):** `Device.public_key_fingerprint` exists in the wire type but is never rendered, never compared between devices, and is computed at inconsistent lengths across join paths. Today the app is TOFU-via-relay, not verified E2EE. This is the central trust gap; see §6c and §11.
- **Trust/connection state under-surfaced (medium):** the tray *already* shows a status row (`status_label` composes auth state × ws string), but it's non-interactive; in-panel, `getWsStatus()` is fetched into a variable literally named `_status` and never rendered.
- **Filter discovery via `#`/`@` only (medium):** the `[Type ▾]`/`[Device ▾]` chips already exist in `SearchBar` but render only *after* a filter is active — there is no resting affordance, and the chip ✕ is a 9px hit target.
- **Destructive actions lack consistent affordance (medium):** Revoke looks routine; the `ConfirmDialog` uses a destructive glow but the `DevicesPanel` revoke button uses none — inconsistent even within the app. Alert toggles differ by a 4-step gray shade only.
- **Code-in-list typography (medium):** code previews render in the body face; switch them to mono to match detail.
- **Full-screen takeovers / blocking modals (medium–low):** Settings replaces the whole window; `PinNoteDialog` and `CleanupDialog` block the app.

## 5b. Priority & phasing

Each phase is independently shippable; the timeline can be cut at a phase boundary without leaving a half-redesign. Anchor priorities to the two highest-leverage, brief-named problems (context loss; the trust/onboarding gap) plus first-pass scannability.

**P0 — trust, context, and scannability (the credibility floor).** Ship-one-thing-only? Ship this.
- Persist the filter/search context object across all four wipe sites (§6a). This is a binary regression gate.
- Inline content-type glyph on every row + switch code previews to mono (§6a, §7).
- Strengthen time-bucket headers (§6a).
- Honest trust copy everywhere it ships today — onboarding, approval, decrypt-failure, offline, revoke (§6c, §10-microcopy) — written against §11, removing every overstated claim.
- Reframe `AddRelayDialog` first-run (title, subtitle, FAQ) **and** give `LocalOnly` a real "use locally for now" escape.
- Make the tray status row interactive (click-to-explain + retry) and render the discarded `_status` in-panel.
- Resting `[Type ▾] [Device ▾]` affordance + larger chip ✕ hit target.

**P1 — the fast path and the danger zone.**
- The Quick-Paste HUD as a named hero interaction: top-row pre-selected on open, number-key quick-pick, two-tier search, recents-on-top (§6b). *Open question §10: how far to take auto-paste — see permission cost.*
- Device verification flow: standardize the fingerprint, render it in agreeing places, add a per-device Unverified→Verified state keyed to the fingerprint (§6c, §11). **Prerequisite:** unify the `digest[..4]` vs `digest[..8]` divergence and expose a fingerprint on the auth payload (it is not there today).
- Visible "Danger zone" with consistent destructive styling across `ConfirmDialog` *and* the revoke button; spelled-out consequences (§6d).
- Decrypt-failure as an inline, recoverable state on the affected row (§6a/§6c), not a 6-second auto-dismissing toast.
- Consolidate the SSH-add entry points to one CTA; resolve the dual source of truth for device names (§6d).

**P2 — polish and hygiene.**
- Token migration: unify the type scale, button-padding set, and `borderRadius` literals onto tokens; add `--radius-pill`; reconcile the pill palette and destructive glow across themes; light-theme contrast pass (§7).
- Reduce takeover modality (Settings as drawer/split; non-blocking `PinNoteDialog`/`CleanupDialog`).
- Image thumbnails in rows (lazy/virtualized, backend-generated preview — **not** a naive inline decode; see §8 perf note).
- Retention preview-before-commit; pinned-panel grouping/sort.

## 6. Surface-by-surface direction (the core of this brief)

### (a) Clip history · search · copy
**Goal:** a feed you can scan in one pass and act on without leaving the keyboard.

Key UX moves:
- **Add a content-type glyph to every list row** using the existing `typeGlyph()` set (imported into `ClipList`, which never calls it today). Type must be readable without opening detail. Four values only — `text`, `code`, `url`, `image`; never a fifth.
- **Fix code-in-list typography (narrow fix).** The list preview already renders in the proportional body face (`var(--font-body)`, 13.5px) — prose is *not* the problem. Switch **code/JSON** previews to mono in the row, mirroring `ClipDetail`'s existing `isProse` logic, and unify the prose size token between list (13.5) and detail (14.5) onto one step.
- **Strengthen time-bucket headers** (Today / Yesterday / This Week / Older): larger, slightly bolder, higher contrast, with a subtle rule. Today they use the faint text token at 11px and read as artifacts — promote them off the decorative-faint token (see §7).
- **Make filters discoverable.** The `[Type ▾]`/`[Device ▾]` chips and the `#`/`@` accelerators **already exist** in `SearchBar`; the gap is that the chips render only once a filter is active. Add a **resting** `[Type ▾] [Device ▾]` affordance so the controls are visible before any filter is set. Keep `#`/`@` as accelerators that route into the same controls. Enlarge the chip ✕ from 9px to a comfortable hit target. Active filters show as removable chips.
- **Persist the filter/search context across the four sites that wipe it today** (verified line refs):
  1. `Rail.onSelect` resets `selectedClip` + `selectedSource` + `activeFilter` (App.tsx ~593–598).
  2. `Cmd+1/2/3` and `Tab` reset `selectedClip` + `selectedSource` but **leave `activeFilter`** (App.tsx ~433–457) — so mouse-switch and keyboard-switch already diverge.
  3. `tauri://blur` clears `selectedClip` + `searchQuery` (App.tsx ~281–286) — this is why your query vanishes the instant you click into a terminal to paste. It is **not** a navigation event.
  4. `finishCopy` clears `searchQuery` + `selectedClip` (App.tsx ~306–318).
  Spec: define one App-level context object `{ searchQuery, selectedSource, activeFilter }`. (a) Manager panel switches (mouse **and** keyboard, made identical) touch only `selectedClip`. (b) **Remove the `searchQuery` reset from the blur handler.** (c) After copy, keep filter + source; clearing `searchQuery` is acceptable only if explicitly chosen, and the type/device chips must survive regardless. (Quick-Paste HUD is the deliberate exception — it resets to recents-on-top each summon; see §6b.)
- **Image clips get a real thumbnail** (~32–40px) in the row instead of `Image (NNN B)` — but **lazy/virtualized and backend-generated**, never a per-row inline decode of full media (§8 perf).
- **Rank row actions; demote Send.** Today the per-row `Send` button is the only always-visible action, mis-weighting the UI toward a rare broadcast over the daily copy. Action hierarchy: **primary = Copy** (Enter / click / number key), secondary = Pin (`Cmd+P`), tertiary = Send / Delete (hover or overflow menu only, never always-visible). Remove `Send` from the resting row; surface it in the detail footer and a row hover-overflow. *(Send is broadcast-only — it re-pushes to all the user's devices; the data model has no per-device targeting. Any label must say "Send to your devices," never "Send to…" a target.)*
- **Distinguish loading from empty.** Add a loading skeleton (a `.skeleton-shimmer` exists). Never reuse "No clips yet" during load. Specify the live transition: `GettingStartedCard` shows only while `clips.length === 0 && devices.length <= 1`; the instant the first clip lands it should be replaced by the list with a quiet transition (the emotional payoff moment) — decide whether the card animates out or simply yields.
- **Decrypt-failure is an inline row state, not a transient toast.** Mark the affected clip in the list with a calm "can't decrypt yet" state, show that auto-recovery (key re-exchange) is happening, and offer a manual "request key from your devices" / retry. Tie it to the lock affordance in §6c so failure and success live in one surface.
- **Pinned panel:** named-note groups sorted alphabetically or by recency, with the catch-all "Unnamed" bucket pinned **last** and internally time-sorted, so a 50-item Unnamed group isn't one wall. **The Type/Device filter chips must either work in Pinned or be hidden there** — today `listPinnedClips()` ignores the query, so the chips would render but do nothing (a §4.3 violation).

"Done well" looks like: a developer hits the shortcut, the most-recent clip is already selected, they press Enter and it's pasting in ~600ms with zero typing — and when they instead open the Manager and filter to "code from build-server," that filter is still there when they come back ten seconds later, even after clicking away to paste.

### (b) The two window personalities — Quick-Paste HUD & Manager — and the tray
**Goal:** make the everyday grab sub-second; never lose your place in the fuller view; always know whether sync is alive.

**Hero interaction — the Quick-Paste HUD.** This is the front door. The plumbing exists; the design does not.
- Loop: press the global shortcut anywhere → HUD appears centered on the cursor's monitor (`show_on_active_monitor`), search field focused, **most-recent clip pre-selected** → type to fuzzy-filter, **or** press a number key `1–9`, **or** arrow → Enter → clip is on the clipboard, window hides, focus returns to the previous app (`focus_previous_app`). Target <1s for the no-typing case.
- **Number-key quick-pick:** `1–9` copy the Nth visible clip immediately; show a small leading `1–9` affordance on the first nine rows, fading when results exceed nine.
- **Top row pre-selected on every summon** so Enter works with no navigation.
- **Recents-on-top is the HUD's default sort** (the existing `clipRecency` / `applyInboxRecency` logic already does this — surface it as a guarantee).
- **HUD geometry differs from Manager:** compact (~640×420), search bar + list only, no Rail, no detail pane (or a slim inline preview on the selected row). The HUD **resets to recents-on-top each invoke** and does not restore a stale filter — the existing blur handler already clears search on close, which supports this. (Persistence belongs to Manager mode; this is the one place the §6a "persist context" rule deliberately does not apply.)

**Manager mode** is the full three-pane window at the §8 presets, invoked from the tray or a second shortcut, for browsing, devices, and settings. Same React app, gated on a mode flag; the Rust side already supports repositioning and `setSize`. This is a new *mode*, not a new window class — flag it explicitly in your deliverable.

> **Open question (do not pre-decide):** is the HUD a true separate compact mode, or do we reframe the existing panel as "the HUD that can expand into Manager"? See §10.

**Manager navigation & shell:**
- **Keep the Rail** (Inbox / Pinned / Devices / Settings) and `Cmd+1/2/3`. But acknowledge the real IA seam: Inbox/Pinned are clip-collections that share the SearchBar and detail pane; Devices is an admin grid that ignores the SearchBar; Settings replaces the whole window. **Decide and state** one of: (a) split the Rail into a primary content zone (Inbox, Pinned — SearchBar live) and a footer admin zone (Devices, Settings — SearchBar hidden, page title instead), treating "enter Devices/Settings" as a mode change, not a peer switch; or (b) collapse Devices into Settings (it already renders inside Settings → Servers) and give the Rail's third slot a clip-centric destination. **Resolve the duplicate-DevicesPanel question:** Devices currently renders both as a top-level Rail panel and inside Settings → Servers — it cannot legibly live at two depths.
- **Give the detail pane context.** Context-aware placeholder copy: Inbox empty-selection "Select a clip to preview it."; Pinned "Pinned · {n} items. Select one to preview."; keep the keyboard-hint line below it.
- **Enrich the existing tray status row** (it already exists — `status_label` renders status + Open Dashboard + Settings + Check for Updates + Quit). The status is a function of **two orthogonal axes**: `AuthState` (`Authenticated` / `LocalOnly` / `Authenticating` / `ErrorRecoverable`) × ws string (`connecting` / `connected` / else→Disconnected). Design the **cross-product**, not a flat six-item list — e.g. "authenticated but disconnected" reads differently from "LocalOnly" (which never tries to connect). The genuinely *new* asks (scope only these): make the status row **interactive** (today it's a disabled item), add **click-to-explain** ("Your clips are saved on this Mac; we'll sync the moment your connection's back"), and a **"retry now."**
- **Reflect sync state in-panel** (a quiet header indicator) by rendering the value currently discarded as `_status`. *(Caveat: `WsStatus` is typed as a bare `string`; if you design against named states, note you're proposing a tighter contract than the wire guarantees today.)*
- **Reduce takeover modality:** consider Settings as a drawer/split rather than a full replacement; let `PinNoteDialog` be a non-blocking popover so users can cross-reference other clips while writing a note. *(Marked as open — see §10.)*
- **Persist Manager navigation state across restart** (active panel, last filter/source/query) — distinct from session persistence. List the keys explicitly in your spec; the HUD never restores them.

"Done well" looks like: the menu-bar icon tells the truth at a glance; the HUD grabs your top clip in under a second; opening the Manager returns you to your last context; and you can adjust a setting without losing sight of your clips.

### (c) Onboarding · auth · pairing — and making E2EE trust *visible and honest*
**Goal:** earn trust before asking for it; show the payoff (copy → paste) before the mechanism; and never claim a guarantee the crypto doesn't deliver (§11).

**First-run, sequenced against the real auth branches** (today the outcome-first payoff is shown *last*, after a blocking relay dialog — invert that). Spec the flow as an ordered state list mapped to `auth.variant`:
1. **`LocalOnly` + never-signed-in →** a dismissible welcome that shows the copy→paste outcome *and* offers **"Use locally for now"** as a real option. *(Today `AddRelayDialog` renders with `hideClose` and traps the user with no escape — fix this; it's a current bug.)* Let the user feel the loop locally first: "Copy anything, then hit the shortcut" as a live first card.
2. **Sign-in chooser** (reframed `AddRelayDialog`).
3. **`Authenticating` →** the loading screen.
4. **"Device ready" step** (new — `AuthLoadingScreen` goes straight to the dashboard today). **Gate "ready" on key receipt:** `auth_bootstrap` polls up to 30s for the canonical key from a peer, and clips can't decrypt during that window. So the confirmation must read **"Ready — waiting for your other devices to share the key"** until the key arrives, *then* "This device is ready to sync encrypted clips." Do not fire a false-ready signal before the key lands.
5. **`Authenticated` + empty history →** `GettingStartedCard`.
State which steps are blocking vs. dismissible.

**CLI-handoff entry (a real, implemented path — don't break it).** `cinch auth login` emits `cinch://login`; the app receives `cliHandoffRequested` and renders `AddRelayDialog` with `fromCli` (different title, no method radios, pre-filled relay). This is many developers' *first* contact. Spec the reframed dialog's `fromCli` variant alongside cold-start so the redesign covers both.

**Soften jargon for users; keep precise terms for docs/code.** Prefer "add a machine" over "pair," "sync server" over "relay" in the UI. But in the Privacy & Security surface, name the relay **precisely** and surface self-hosting as a trust affordance — "sync server (relay)" plumbing-speak must not erase that running your own relay is the escape hatch from relay trust. `AddRelayDialog` exposes a self-hostable Relay-URL field; keep it.

**Make device approval an honest trust gate.** `approveRemoteLogin` POSTs `device-code/complete` with only the `user_code` — it is cryptographically **blind** (no fingerprint check, no key binding); the hostname/region shown are relay-supplied strings. So approval confirms *"a sign-in is happening,"* not *"this verified device is authorized."* **Editorial decision (resolving the trust lens's two options):** keep approval as intent-confirmation for now and write copy that doesn't imply verification — then route the user to verify the device's fingerprint afterward (P1). Do **not** ship "Approve [machine] to receive encrypted clips" as written; see the rewrite in §10-microcopy.

**Device verification (P1, design-to-add — not present today).** This is the move that actually makes a security-conscious developer relax, and the data model half-supports it (`Device.public_key_fingerprint` exists). Spec:
1. **Standardize the fingerprint** to one length/format app-wide (recommend 8 bytes → 16 hex grouped `XXXX-XXXX-XXXX-XXXX`, or a word/emoji safety-number for legibility). **Prerequisite:** fix the `digest[..4]` vs `digest[..8]` divergence so the same key shows one fingerprint on every join path, and **expose a fingerprint on the auth payload** (`AuthenticatedPayload` has `user_id`/`device_id`/`hostname`/`relay_url`/`active_relay_id`/`machine_id` — **no fingerprint field today**; mark this as a backend/wire addition).
2. **Render it in three agreeing places:** the post-sign-in "device ready" step (this device), the clip detail lock popover (this device), and each Devices row (peer's `public_key_fingerprint`).
3. **Give it a job:** a per-device **Verify** control — "Compare this code with what [device] shows. Match? Mark verified." — backed by a local `verified` flag **keyed to the exact `public_key_fingerprint` that was verified.** If a device's fingerprint changes, the badge MUST drop to **"Unverified — key changed, re-verify"** in a warning tone. (This rule is what makes the fingerprint protective rather than decorative; binding "verified" to name or id instead would silently inherit a stale badge — the exact attack verification is meant to catch.) **Until this flow exists, do not render a fingerprint anywhere** — a code the user can't compare is security theater.

**Encryption as quiet, persistent fact — with two clip-lock states.** A small lock affordance on the clip detail with copy precise to scope: **"Encrypted on this device. The sync server stores only ciphertext."** The lock has **two honest states**, not one: *encrypted-and-readable* (key present) vs. *encrypted-but-key-pending* (the `auth_bootstrap` 30s window where decrypt will fail) — distinguish them, don't show a uniform lock.

**Privacy & Security surface** — facts in plain English up top, acronyms demoted to a labeled technical row:
- Headline: "Your clips are private." Body: "Encrypted on this Mac before they're sent." / "Only your devices hold the key." / "The sync server stores scrambled text only — it can't read your clips, and neither can we."
- An **honest trust-model line** (not an absolute claim): "Devices exchange the shared key using X25519. Today the sync server relays public keys between your devices — verify a new device's fingerprint (below) before trusting it."
- A "Technical details" group: cipher AES-256-GCM · key exchange X25519 · this device's fingerprint · sync-server (relay) URL with the self-host note · local storage path · retention.
- **Key-recovery answer** (a security developer will ask): state what happens if the last device holding the key is lost — even if the answer is "those clips are unrecoverable; that's the cost of E2EE."

"Done well" looks like: a security-conscious developer finishes onboarding **more** confident than when they started — they know what's encrypted, what the server can and can't see, that the server brokers keys (and how to verify), what approving a device means, and that everything administrative is reversible.

### (d) Devices · settings · destructive actions
**Goal:** administration that is calm, accountable, and never accidental — with copy that matches what the code actually does.

Key UX moves:
- **Create a visible "Danger zone."** Group Revoke / Disconnect / Clear history into a distinct red-bordered region with **consistent** destructive styling — and apply the destructive token set (`--destructive-bg`/`-fg`/`-glow`) to **both** the `ConfirmDialog` primary **and** the `DevicesPanel` revoke button, which today uses a transparent bg with no glow (an inconsistency even within the app). Give Revoke real affordance (warning icon, emphasized red) so it never reads routine.
- **Spell out consequences — and only consequences the code produces.** Revoke does **not** rotate or remove the AES master key today; the relay marks the device revoked server-side, but a device that already holds the key is not cryptographically locked out. **Do not write "the shared key is removed."** Honest copy: "Revoking [machine] stops the server from syncing new clips to it and removes it from your account. Clips it already received stay on that machine. To rotate the shared key, re-pair your remaining devices." *(If key rotation on revoke is implemented later, update the copy to match — then and only then.)* Full rewrite in §10-microcopy.
- **Approve-an-incoming-sign-in is an asynchronous interrupt, not a step in "add a device."** It can fire any time, is time-sensitive (codes age), surfaces only as an OS notification (if `notifyOnRemoteLogin` is on) plus a card buried in Settings → Sessions. Surface pending approvals as a **top-level badge/banner**, not only inside a Settings tab — it's a security decision with a clock on it. Spec where it appears when the panel is open vs. closed.
- **Split "add a device" from "approve a device."** Consolidate the SSH-add entry points (two CTAs in `DevicesPanel` + the "Pair another device" dashed row, all feeding one `AddSshMachineDialog`) into one prominent CTA. Approval is the separate interrupt flow above.
- **Strengthen the alert toggle** (icon + fill + color, not a 4-step gray difference). Show a real loading state for the device version dot instead of a static gray "unknown" during the async check.
- **Make retention safer:** preview impact *before* committing ("this removes ~N clips"), not after the slider releases; translate the readout into plain consequence ("auto-delete clips older than 30 days"). Fix the current asymmetry (lowering warns, raising doesn't, both only after the fact).
- **Settings scannable and jargon-free** — a calm grouped sheet (Account & Sync · Privacy & Security · Devices & Trust · Appearance & Shortcuts) rather than five tabs with inconsistent rhythm. Make shortcut-capture fields obviously interactive ("Press a shortcut," live capture feedback).
- **Make guidance non-blocking:** `CleanupDialog` (post-revoke "run `cinch auth logout`") becomes an inline panel/toast with a copy button, not a modal that freezes the app.
- **Resolve the dual source of truth for device names** (backend `nickname` vs. localStorage `displayName`; `saveDisplayName` writes both, the merge prefers `displayNames[sourceKey] ?? device.nickname`). Pick one as canonical so renames can't show stale. *(This is a data-model decision, not solvable in the design layer alone — flag it as such.)*

"Done well" looks like: a developer can rename, recolor, verify, and revoke a machine confidently; they always know what a destructive action will *actually* do before they do it; and they rarely need Settings because defaults are sensible.

## 7. Visual & interaction language

**Typography.** Stay single-family — Geist (body/UI) + Geist Mono; drive hierarchy with weight and tracking. The list already renders prose in the body face; the fix is to render **code/JSON previews in mono** in the row (mirroring `ClipDetail`'s `isProse` logic) and unify the prose size token between list and detail.
- **Document the type scale** (rem at 14px root) and migrate onto it — there are ~22 distinct fontSize values in use today: `--fs-2xs` 10 (mono meta only), `--fs-xs` 11, `--fs-sm` 12, `--fs-base` 13 (the single list/detail prose target — retire 13.5/14.5), `--fs-md` 14, `--fs-lg` 16, `--fs-xl` 20, `--fs-2xl` 22 (panel titles). Collapse 9/9.5/10.5/11.5/12.5/15/17/18/26/28 onto the nearest step.

**Color & themes.** Present every core color as a **token pair with both theme values side by side** — components reference `var(--*)` only, never raw hexes:
- bg `#16140F` / `#FBFBFA`; text-primary `#EDE6D6` / `#2F3437`; accent `#4FB3A9` / `#2F7F78`; accent-on `#16140F` / `#FFFFFF`.
- **Contrast fixes (corrected from v1).** Dark `--text-muted` is **already** `#A8A092` (7.11:1 on bg) — leave it; the v1 "bump C.t2" instruction was asking for work already done. The real weak spots: (1) **light `--text-muted` `#787774`** is only 4.48:1 — darken to ~`#6B6A67` (≈5.2:1) for headroom; (2) **`--text-faint`** (3.1:1 dark / 2.08:1 light) is the token used for the 11px bucket labels and meta the brief criticizes — **split it** into a *decorative-faint* token (icons/dividers, no ratio target) and a *legible-secondary* token at AA, and move all ≥11px text-bearing uses (bucket headers, clip meta) onto the legible one. Target: any text-bearing token ≥4.5:1 normal / ≥3:1 for ≥14px-bold in **both** themes.
- **Reserve accent for focus rings and the offline pulse — and enforce it (this is a fix, not a keep).** Today `SearchBar`'s `chip_url` maps its foreground to `var(--accent)` over a hardcoded **purple** background (`rgba(170,102,255,…)`) — purple-on-teal, internally contradictory. **Define four content-type token pairs** (`--type-text-*`, `--type-code-*`, `--type-url-*`, `--type-image-*`) for both themes and have the SearchBar chips, the new list glyph, and ClipDetail **all** consume them. Remove every hardcoded RGB from `SearchBar` `S.chip_*`. Choose one mapping used identically everywhere (e.g. text=info/blue, code=warning/amber, url=a distinct hue that is **not** the reserved teal accent, image=success/green) so "a code clip looks like code everywhere" is enforceable.
- **Inline label badge token.** `ClipDetail` hardcodes `#ff6b6b` / `rgba(255,107,107,0.1)` — the one place a raw hex leaks into a component. Absorb it into the badge formalization as `--label-bg`/`--label-fg`.
- **Reconcile the pill palette** — it's two palettes sharing slot indices, not one rendered two ways (dark uses 10%-alpha tint bgs with light pastel fgs; light uses opaque pastel bgs with dark-saturated fgs, and the *foreground hues differ per theme*). Define one 6-slot hue identity (mint/amber/sky/lilac/rose/sage) and derive each theme's bg/fg from it by a documented rule, ≥4.5:1 fg-on-bg in both. Fix `--pill-local-fg`, which still uses the stale pre-bump `#9C9486` while the six color pills sit at 9.5–11.6:1 — the neutral pill is the odd one out.
- **Equalize the destructive glow.** Dark is `rgba(255,99,99,0.15) 0 0 20px 5px` (effective intensity ≈3.75); light `rgba(185,28,28,0.12) 0 0 14px 3px` (≈2.04). Pin one `--destructive-glow` per theme at matched weight — e.g. dark `rgba(255,99,99,0.15) 0 0 16px 4px`, light `rgba(185,28,28,0.18) 0 0 16px 4px` — and apply it to both destructive surfaces (§6d).
- **Ship a contrast table** for the formalized chip/badge component covering both themes (light `--info` `#2563EB`, `--success` `#16A34A`, `--warning` `#CA8A04`, `--error` `#9F2F2D` as chip fgs over light pastel bgs need explicit ratio targets).

**Spacing, radius & buttons.** The `--sp-*` (2/4/8/12/16/24/32) and `--radius-sm/md/lg/xl` (4/6/8/14) tokens already exist — the work is **migration**, not invention. Add `--radius-pill` (9999) and `--radius-xs` (2). The debt: ~82 numeric `borderRadius` literals vs. 18 token uses, and 15+ distinct button paddings. Define **exactly three button sizes** (an *example* set, designer to finalize, aligned to what's already common to minimize diff): small ≈4×10, medium ≈6×12, large ≈8×16. State the *rule* — one documented padding scale; adjacent actions align — rather than treating the pixels as mandated.

**Density.** Per §3's tiebreak: **comfortable visuals are the default**; speed comes from keyboard accelerators, not a denser skin. A compact toggle is an open question (§10), not a requirement.

**Motion.** Snappy 80–120ms; motion preserves continuity (panel switches, list re-sorts, dialog enter/exit, the first-clip-arrives transition) — never decorative. Respect `prefers-reduced-motion`. The copy-success and snap overlays are quiet confirmations, not spectacle; give them an error boundary so a failed webview never leaves a stuck overlay. *(Note: `CopyToastOverlay` uses `fontSize: '30vh'` and lives in a separate Tauri webview, as does `SnapOverlay` — these render outside the main token scope; document them as the explicit exception to the type-scale rule.)*

**Iconography.** Extend the Phosphor-style set (1.75 stroke, `currentColor`). Use the type glyphs consistently across list, detail, and filters.

**Focus & a11y.** Reconcile one **focus token** (color, width, offset) across native `:focus-visible` (currently 2px solid accent, 2px offset), the `RetentionSlider`'s custom 4px-spread ring, and `ConfirmDialog`'s glow. Maintain full keyboard operability; every new control gets a shortcut or Tab/arrow access; modals trap focus and default to the safe action; color is never the only signal.

**Empty / loading / error.** Every data view needs all three — skeleton on load, a purposeful empty state (keep the CLI-snippet energy), calm error with retry. No view ambiguous about which state it's in.

**Avoid generic "AI slop":** no purple-to-blue gradients, no glassmorphism, no glowing rounded everything, no emoji-as-icons (drop the `✨` in `GettingStartedCard`), no generic SaaS card grids. Target: a distinctive, editorial, terminal-adjacent developer aesthetic — warm dark canvas, precise type, restrained accent, dense-but-legible. It should look like a tool a senior engineer chose, not a template.

**Token-migration acceptance criterion (definition of done for §7):** zero numeric `fontSize` / `borderRadius` / button-padding literals in component files — all reference `--fs-*` / `--radius-*` / `--sp-*` (or a button-size helper), with the two full-screen-webview overlays as the documented exception.

## 7a. Voice principles

Five testable rules a writer can apply line-by-line.
1. **State, don't sell.** Facts in plain declarative sentences. No marketing intensifiers ("bank-grade," "military-grade," "✨"), no fear ("WARNING," "DANGER").
2. **Outcome before mechanism.** Say what the user gets ("paste it on any of your machines") before how it works ("relay," "key exchange").
3. **Reassure on failure, never blame the user.** Every error names what's still safe and what we're doing about it, in that order — *but never claim safety the code can't verify* (§11).
4. **Plain words for users, precise terms for docs.** UI: "sync server," "add a machine," "scrambled before it leaves this Mac." Docs/code/technical-details disclosure: "relay," "pair," "AES-256-GCM," "X25519."
5. **Short, specific, no hedging.** One idea per line; name the actual machine/count/action; cut "just," "simply," "please note."

**Reusable error shape:** `[what's still safe] → [what we're doing / what you can do] → [optional one CTA]`.
**Conventions:** sentence case everywhere; `…` for in-progress; em-dash for asides.
**Ban-list:** "bank-grade," "military-grade," "WARNING," "DANGER," emoji-as-icon, "simply," "just," "oops."
**Honesty guardrails:** don't say "on all your devices" when you mean "machines running the CLI"; don't promise auto-sync from the not-signed-in `LocalOnly` state (there's no account or key yet); don't claim verification, key-removal, or whole-lifecycle secrecy the code doesn't deliver (§11).

## 7b. Microcopy — before → after (pin these to the components)

**`AddRelayDialog` (first run).**
Title "Connect to relay" → **"Sign in to Cinch."** Subtitle → "Your clipboard, on every machine you work from — encrypted before it leaves this Mac." Field "Relay URL" → **"Sync server"**, helper "Where your encrypted clips are relayed between machines. Using Cinch's hosted server? Leave as is." Token method "Paste pairing token" → "Paste a sign-in code," placeholder "Code from: cinch auth login." (Keep the prefilled hosted default as the happy path.)

**Onboarding FAQ (verbatim).**
Q "What's a sync server?" A "A small server that passes your clips between your machines. It only ever sees scrambled text — never what you copied. Cinch hosts one for you, or you can run your own."
Q "How is my clipboard kept private?" A "Every clip is encrypted on this Mac before it's sent. The key stays on your devices; the server can't read your clips, and neither can we."
Q "What can Cinch see?" A "Which devices are yours and when they sync — not the contents of a single clip. It does route the public keys your devices use to share the key, so verify a new device's fingerprint when you add it." *(Cipher names live behind a "Technical details" link in this answer, not in the headline.)*

**`PendingLoginCard` (approval — honest, no implied verification).**
Heading → **"Approve {hostname} to sync your clips?"** Body → "Approving lets {hostname} send and receive your encrypted clipboard. Only do this if you started a sign-in there {age}. After it joins, verify its fingerprint in Settings → Devices before relying on synced clips. You can remove it anytime." Keep "Verification code {userCode}." Buttons → primary **"Approve {hostname}"** (not bare "Approve"), secondary **"Not me — deny."** On success → "Now syncing with {hostname}."

**`ClipDecryptFailedToast` → inline row state (calm, no unprovable blanket reassurance).**
Primary → **"Couldn't decrypt this clip yet — fetching the missing key from your devices."** Escalation (after repeated failures, honest not reassuring) → "Still can't decrypt clips from {device}. This usually means a key mismatch — verify {device}'s fingerprint in Settings, or re-pair it." *(Reserve "your other clips are safe" only for a confirmed local key-skew, where it's true.)*

**Offline — split into two honest messages.**
(A) Transient disconnect (authenticated, network down) → **"You're offline. New clips are saved on this Mac and will sync the moment your connection's back."** No "relay." *(Note: this message is valid only in an authenticated-but-disconnected state — never in `LocalOnly`, which is the not-signed-in state and has no key to sync.)*
(B) `OfflineQueueDroppedToast` (the real data-loss case — clips dropped because the key is missing) → **"Couldn't sync {N} clip{s} — your encryption key isn't loaded on this Mac. Sign in again to restore it; the originals are still on the machine you copied them from."** Don't dress data loss as a feature; point to recovery.

**`ConfirmDialog` for revoke (consequences that match the code).**
Title → **"Stop syncing with {hostname}?"** Body → "{hostname} will no longer send or receive your clips, and it's removed from your account. Clips it already received stay on that machine. To use Cinch there again, you'll sign in from scratch." Primary → **"Stop syncing"** (destructive). Secondary → "Keep it." *(Note: do **not** write "the shared key is removed" — revoke does not rotate the key today; §6d.)*

**`CleanupDialog` (non-blocking, with the why).**
Title → **"Finish removing {hostname}."** Body → "{hostname} no longer syncs with you. If you still have access to it, run this there to remove its saved sign-in and local copy:" + command. Add → "No longer have that machine? It's already cut off — this just tidies up its local files." Deliver as a dismissible inline panel/toast with a copy button.

**Empty / loading states (`ClipList`).**
First-run empty → **"Nothing here yet. Copy something on any of your machines — it'll show up here."** + the existing `echo "hello" | cinch push` snippet. No-results → **"No clips match "{query}.""** + a quiet "Clear search and filters" link. Loading → no prose required; if a caption shows, "Loading your history…" — never "No clips yet" during load.

**`GettingStartedCard`.** Drop the `✨`. Heading → **"You're in. Send your first clip."**

**Connection-state labels (user-facing, never the engineer enum).** Map the cross-product to plain strings: Authenticated+connected → "Synced — {hostname}"; Authenticated+connecting → "Connecting…"; Authenticated+disconnected → "Offline — clips saved on this Mac"; `LocalOnly` → "Not signed in — clips stay on this Mac"; `Authenticating` → "Signing in…"; `ErrorRecoverable` → "Sign-in problem — open to fix." "LocalOnly" as a literal label must never reach the tray.

## 8. Hard constraints (must respect)

- **Form factor (stated once, here).** macOS menu-bar / tray panel app. Frameless, fixed-size window. Manager presets: 760×480 / 960×600 / 1120×720; the Quick-Paste HUD is a compact (~640×420) mode of the same app. Design for the small panel first; the tray menu is part of the product. Not a full-window or web-page canvas. §2/§6/§9 reference this; they do not re-specify it.
- **Stack.** Tauri v2 + React 19, hand-rolled design system (CSS custom properties + inline styles / small CSS-in-JS). No Tailwind, no shadcn, no heavyweight component library.
- **Content-type vocabulary is exactly four values:** `text`, `code`, `url`, `image`. No fifth type, sub-types, or MIME strings in the UI.
- **Keyboard-first is non-negotiable.** Anything mouse-only is a regression. Every new control and flow (approve device, retry decryption, open the resting filter dropdowns, verify a device) needs a keyboard path — map them in §9.4.
- **Performance.** Search runs against a large local FTS5 history and must feel instant; large payloads (logs, transcripts) render without jank. Don't load or render everything at once. **Image thumbnails** must be lazy/virtualized and backend-generated previews, **not** per-row inline decodes of full media — naive decode collides with this constraint.
- **Data model is generated, not hand-authored.** Design only against fields that exist. On `LocalClip`: `content`, `content_type`, `source`, `created_at`/`received_at`, `byte_size`, `is_pinned`, `pin_note`, `sync_state`. On `Device`: `hostname`, `nickname`, `paired_at`, **`last_push_at`** (the "last seen" label is derived from this — there is **no** `Device.last_seen`), `online`, `public_key_fingerprint`, `machine_id`. `SourceInfo` has its own `last_seen`. **Do not invent clip metadata or wire fields.** Two trust features in this brief require *new* exposed fields and are explicitly marked "design-to-add": a fingerprint on `AuthenticatedPayload` (not present today) and a tighter `WsStatus` enum (currently a bare `string`).
- **Cross-platform.** macOS is the design target; copy stays honest (desktop on macOS; CLI on macOS/Linux/Windows). Don't promise surfaces that don't exist.

## 8b. Non-goals (out of scope)

We are **not** redesigning: the CLI's terminal output or command surface; the relay/web UI or any browser surface; Windows/Linux desktop visual chrome (keep copy honest, don't mock them); the wire/data model or any new clip field (restated from §8); the content-type taxonomy (stays at four); the underlying crypto/key-exchange flow (we *surface* and *honestly describe* it — and recommend the fingerprint-standardization and verification *additions* in §6c/§11 — but we do not re-architect the protocol); the offline-queue/sync engine behavior (we restyle its states, we don't re-architect it); the existing keyboard bindings (extend only, never rebind a working chord — but see the one proposed remap in §10). The auth backend flow (device-code, SSH pairing) stays; only its UI/copy changes.

## 9. Deliverables & how to respond

1. **Annotated flows** showing how context is preserved, for: (a) **Quick-Paste grab** (shortcut → pre-selected → Enter/number → paste, with the latency budget); (b) Manager find → filter → copy; (c) first-run onboarding incl. the new "device ready / waiting for key" and trust moments, plus the `fromCli` handoff variant; (d) add-a-device **and** the separate approve-an-incoming-sign-in interrupt; (e) recover from offline vs. key-missing (two distinct flows) and from a clip that won't decrypt.
2. **Hi-fi mockups per surface** in **both dark and light**, at the small sizes: the **Quick-Paste HUD** (~640×420) and the **Manager** (start at 960×600, show 760×480 holding); clip list + detail with the new glyph and resting filter controls; tray status (the auth×ws cross-product, interactive); onboarding/auth incl. the trust steps; devices incl. the verification state and the "Danger zone"; settings; and the key empty/loading/error states.
3. **Component & token spec:** the unified type scale, color **token pairs** (with the corrected contrast fixes — light muted, the split faint token, the four content-type pairs, the label-badge token, the reconciled pills, the equalized destructive glow, one focus token), spacing/radius tokens (incl. `--radius-pill`), the three-size button set, and a formalized badge/chip component (source pill, type glyph, version, sync state, verification badge). Map to the existing CSS-custom-property approach. State the **token-migration acceptance criterion** as a done-gate.
4. **Interaction notes:** keyboard map (existing + new — number keys `1–9`, verify, retry-decrypt, the resting filter dropdowns; and the proposed global-summon remap in §10), motion/timing, focus order, reduced-motion behavior.
5. **A short rationale** tying each major decision to the goals in §4, and a one-line note on each §10 open question you resolved.

Attach **current app screenshots** for before/after, and call out explicitly anywhere your proposal departs from §8 so we can discuss the trade-off.

## 10. Open questions for the designer (decisions to make — distinct from the fixed §8 constraints)

These are genuine forks; the brief does **not** pre-decide them. Resolve each with a one-line rationale.

- **HUD vs. panel framing.** Is the Quick-Paste HUD a separate compact mode, or do we reframe the existing panel as "the HUD that expands into Manager"? (Hero job is the grab; the three-pane is the manage/browse mode.)
- **Auto-paste depth.** Should Enter = *paste directly into the focused app* (synthesize Cmd+V after `focus_previous_app`) with Cmd+Enter = copy-only, or stay copy-to-clipboard + manual paste? **Trade-off to weigh:** auto-paste needs macOS Accessibility / `CGEventPost` — a **new entitlement** the app may not request today; "copy only" must be the no-permission fallback. (If auto-paste wins, Enter should own the lowest-friction chord — propose remapping the current `Cmd+Enter` "send" to `Cmd+Shift+Enter`.)
- **Global-summon shortcut.** The registered default is `CmdOrCtrl+Shift+W`, which collides with "close window" muscle memory (and `CmdOrCtrl+Shift+V` is already referenced as a default elsewhere in the code — an inconsistency to resolve). Keep `W`, or change to `Cmd+Shift+V` (clipboard mnemonic)? Stays user-configurable.
- **Pins as a fast tier.** Treat pinned clips as the "frequently-pasted" fast tier (pinned-on-top section in the HUD, claiming the low number keys), or keep them a storage view? `Cmd+P` becomes "promote to fast-access" if the former.
- **Search model.** Confirm the two-tier model: instant **client-side** fuzzy ranking on the hot recent set (zero debounce), falling back to the debounced FTS5 query for deep-history search. Where's the boundary (query length? an explicit "search all" affordance)?
- **Rail IA seam.** §6b option (a) two-zone Rail vs. (b) collapse Devices into Settings — pick one and resolve the duplicate-DevicesPanel.
- **Modality.** Settings as drawer vs. split vs. keep-modal; `PinNoteDialog` popover vs. inline; in-panel sync indicator as header pill vs. Rail badge.
- **Density.** Ship a comfortable/compact toggle, or pick the comfortable default and defer the toggle?
- **Type indicator form.** Glyph vs. colored dot vs. both for the list-row content-type indicator — choose and justify.
- **Thumbnail size.** ~32–40px is a range; resolve it against the lazy/virtualized backend-preview constraint.

## 11. Threat model (the basis for every trust string)

State this plainly so copy can be written truthfully against it; surface a user-readable version of the "today" line in Privacy & Security.

- **Defended:** a passive relay reading clips. The relay stores ciphertext only (AES-256-GCM); it never holds the decryption key. "The sync server stores only ciphertext" is true for clips at rest.
- **Not defended today:** an active relay MITM during key exchange. The relay brokers X25519 public keys between devices (`key_exchange.rs`: "the relay vouches for its origin"; the peer pubkey is read from the relay's device list). A compromised/malicious relay can substitute its own pubkey and capture the AES master key. Therefore **"the server only sees scrambled text" must not ship as an unqualified, whole-lifecycle claim** — qualify it to clips-at-rest and pair it with the verify-the-fingerprint affordance.
- **Mitigation path (P1, design-to-add):** out-of-band fingerprint verification (§6c). Until it ships, fingerprints are not displayed (a non-comparable fingerprint is theater), approval copy claims intent-confirmation not verification, and the self-host relay option is surfaced as the trust escape hatch.
- **Revoke is not re-keying today.** Revoke marks a device revoked server-side; it does not rotate or remove the AES master key, so a device that retained the key is not cryptographically locked out. Copy must say so (§6d/§7b) until rotation-on-revoke is implemented.
- **Key recovery.** If the last device holding the key is lost, those clips are unrecoverable — that's the cost of E2EE. State it honestly in Privacy & Security rather than letting a security-conscious developer discover it the hard way.
