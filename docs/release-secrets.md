# Release Secrets

GitHub Actions secrets required for `publish.yml` and `proto-sync-relay.yml`
to operate. Set via `gh secret set <NAME> -R cinchcli/cinch` or the GitHub UI
(Settings -> Secrets and variables -> Actions).

## Inventory

| Secret | Used by | Purpose | Rotation |
|---|---|---|---|
| `RELAY_SYNC_TOKEN` | `proto-sync-relay.yml` | PAT with `repo` scope (or fine-grained: contents+pull-requests on `cinchcli/relay`) so the proto-sync workflow can open PRs against the relay repo | Rotate annually or on personnel change |
| `HOMEBREW_TAP_TOKEN` | `publish.yml` | PAT with `contents:write` on `cinchcli/homebrew-tap`; lets the release pipeline push Cask + Formula bumps | Rotate annually |
| `TAURI_SIGNING_PRIVATE_KEY` | `publish.yml` (build-desktop-*) | Minisign private key for signing Tauri updater payloads; clients verify against the embedded pubkey in `apps/desktop/src-tauri/tauri.conf.json` | **Never rotate without forcing a manual reinstall** — clients verify with the pinned pubkey; a new key invalidates all existing installs' update path |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | `publish.yml` | Decrypts the minisign key | Rotate alongside the key (i.e. almost never) |
| `APPLE_CERTIFICATE` | `publish.yml` (build-desktop-macos) | Base64-encoded `.p12` export of the Developer ID Application certificate | Renew before certificate expiry (typically 5 years from issue); re-export and re-encode on renewal |
| `APPLE_CERTIFICATE_PASSWORD` | `publish.yml` | Password for the `.p12` | Rotate with the certificate |
| `APPLE_SIGNING_IDENTITY` | `publish.yml` | Identity name `codesign` uses (e.g., `Developer ID Application: Your Name (TEAMID)`); discoverable via `security find-identity -p codesigning` | Set once per developer account; update if team name changes |
| `APPLE_ID` | `publish.yml` (notarize step) | Apple ID email used for `xcrun notarytool submit` | Rotate when changing account ownership |
| `APPLE_PASSWORD` | `publish.yml` | **App-specific password** from `appleid.apple.com -> Sign-In and Security -> App-Specific Passwords` (NOT the Apple ID login password) | Rotate if revoked or compromised |
| `APPLE_TEAM_ID` | `publish.yml` | 10-character Team ID from `developer.apple.com -> Membership` | Set once per developer team |
| `CINCH_TELEMETRY_KEY` | `publish.yml` (build-desktop-macos) | PostHog project key compiled into the desktop binary via `option_env!` for anonymous usage stats; optional — desktop builds successfully without it (telemetry just stays off) | Rotate if compromised |
| `CINCH_TELEMETRY_URL` | `publish.yml` (build-desktop-macos) | PostHog ingest URL paired with `CINCH_TELEMETRY_KEY`; same `option_env!` build-time gate | Rotate with the key |

## Regenerating the minisign keypair

The Tauri updater pubkey is embedded in `apps/desktop/src-tauri/tauri.conf.json`.
If you ever need to rotate the keypair, every existing installation will need
to reinstall manually — the new pubkey won't verify old signatures or vice
versa.

To generate a new pair:

```bash
brew install minisign
minisign -G -p tauri-public.pub -s tauri-private.key
# Then base64-encode tauri-private.key, paste contents into TAURI_SIGNING_PRIVATE_KEY,
# and replace the `pubkey` field in tauri.conf.json with the contents of tauri-public.pub
# (base64-encoded — Tauri expects the public key's raw text base64'd).
```

There is also `scripts/generate-tauri-keypair.sh` which automates the
generation step.

## Verifying secrets are present

```bash
gh secret list -R cinchcli/cinch
```

Expected: all 10 secrets above should be listed (presence only — values are
not visible).

For local verification before upload, see `scripts/verify-secrets.sh`. The
canonical local template lives at `scripts/.secrets.env.template`.

## What is NOT a secret

The Tauri updater **public** key is embedded in `tauri.conf.json` and is
not sensitive — it's distributed with every binary. Same for the macOS
Developer ID certificate's _public_ side; only the `.p12` (which contains
the private key) is sensitive.
