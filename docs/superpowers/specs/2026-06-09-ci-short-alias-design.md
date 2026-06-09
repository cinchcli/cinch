# `ci` — short alias for the `cinch` command

**Date:** 2026-06-09
**Status:** Design approved, pending spec review
**Scope:** cinch monorepo (CLI + desktop), homebrew-tap (formula + cask), website (docs)

## Problem

`cinch` is the command users type all day, and it's long. We want a short
form that "just works" on install, without losing the `cinch` brand.

## Decision

Ship **`ci` as a thin symlink to the `cinch` binary**, created defensively
at install time. `cinch` remains the canonical name everywhere — the Cargo
`[[bin]]` name, the clap `#[command(name = "cinch")]`, all help/usage text,
all docs, and the brand. `ci` is purely an additional entry point to the
same binary.

Rejected alternatives:

- **Full rename `cinch` → `ci`.** Churns ~191 doc/website references, the
  homebrew formula, and desktop dispatch, and divorces the typed command
  from the product name during active brand work. Not worth a length win.
- **Opt-in only (`cinch alias install`).** The user wants the short command
  available immediately on install, not behind a manual step.
- **A different short name (`cn`, `cx`, …).** Considered to dodge the `ci`
  collision; rejected because the collision is low and self-healing (below),
  and `ci` is the most natural shorthand for `cinch`.

## Collision analysis (`ci`)

Verified on a current macOS dev machine: `ci` is unbound — no binary on
PATH, no alias, no function, no man page, nothing owns `/opt/homebrew/bin/ci`.

| Source | Real binary? | Conflicts with our symlink? | Prevalence today |
|---|---|---|---|
| RCS `ci` (check-in) | Yes (`/usr/bin/ci`, GNU RCS) | **Yes** — the one real case | Gone from modern macOS, not default on Linux; only via `brew install rcs` / `apt install rcs`. Rare. |
| "CI" = continuous integration | No (abbreviation) | No (no binary) | Ubiquitous as a *word*; nothing to clash with — only a momentary mental double-take |
| `git ci` = commit | No (git subcommand alias) | No — different namespace | Common but lives under `git`, not standalone `ci` |
| `alias ci='git commit'` etc. | No (user shell alias) | No — *their* alias wins, ours is shadowed | Some power users. Harmless: we never break them; we're just unreachable as `ci` for them |
| PowerShell `ci` | No (not a default alias) | No | Copy-Item is `cp`/`copy`/`cpi`; verify before any Windows `ci` work |

**Conclusion:** the only genuine binary collision is RCS `ci`, which is
effectively extinct on modern dev machines. Because we ship a **symlink, not
a rename**, any pre-existing `ci` (alias or binary) keeps winning and we
never break a user's setup. The install path is made defensive so `cinch`
itself always installs cleanly even when `ci`'s slot is taken.

## Implementation

### 1. Desktop argv[0] dispatch — `apps/desktop/src-tauri/src/main.rs`

Extend `invoked_as_cli()` to recognize `ci`:

```rust
matches!(exe, "cinch" | "cinch.exe" | "ci" | "ci.exe")
```

The desktop launcher invokes the app as `Cinch` (capital), so adding `ci`
stays unambiguous. Add a unit test asserting `ci` / `ci.exe` dispatch to the
CLI and `Cinch` does not. (`ci.exe` also future-proofs Windows.)

### 2. Shell completions for `ci` — `crates/cli/src/lib.rs`

The only non-trivial code change. Today completion is hard-keyed to `cinch`:
`bin_name = full.get_name()` is `"cinch"`, and the hand-written override
scripts (`ZSH_/BASH_/FISH_FROM_OVERRIDE`) define `_cinch_*` functions and
register against `cinch`. Two requirements: `ci <tab>` must work, and the
`cinch` and `ci` completion files must not collide when both are sourced
(the formula installs both).

- Derive the emit-name from `argv[0]`'s basename: `cinch completion zsh`
  emits `_cinch` (byte-identical to today); `ci completion zsh` emits `_ci`.
  Fall back to `"cinch"` if argv[0] is unavailable.
- Parameterize the override templates on that name via an explicit
  placeholder (not a blind `str::replace`), so `_cinch_devices_names` /
  `_cinch_generated` / `_cinch_with_from` and the `complete … cinch`
  registration become `_ci_*` / `complete … ci`. This is what prevents
  function-name collisions between the two sourced files.
- Extract a pure `render_completion(shell, name) -> String` so both outputs
  are unit-testable without spawning a process.

The dynamic data-fetch invocation inside the override (`cinch fleet list
--names`) substitutes to the emit-name too (`ci fleet list --names`); both
binaries always coexist, so either is valid.

**Decision (accepted):** `ci --help` will still print `Usage: cinch …`
because clap's `name` stays `"cinch"`. This is accurate (`ci` *is* cinch)
and reinforces the brand; making help print `ci` would require routing the
parse through `Command::bin_name(...)` and is intentionally out of scope.

### 3. Homebrew formula — `homebrew-tap/main/Formula/cinchcli.rb`

Defensive `ci` symlink plus its completions:

```ruby
def install
  bin.install "cinch"
  generate_completions_from_executable(bin/"cinch", "completion",
                                       shells: [:bash, :zsh, :fish])
  unless File.exist?("#{HOMEBREW_PREFIX}/bin/ci")
    bin.install_symlink "cinch" => "ci"
    generate_completions_from_executable(bin/"ci", "completion",
                                         shells: [:bash, :zsh, :fish],
                                         base_name: "ci")
  end
end
```

Add `system bin/"ci", "--version"` to `test do` (CI hosts have no RCS, so
`ci` is always linked there). The `File.exist?` guard is the defensiveness:
if `ci` is free (the common case) it is linked; if taken (RCS), `cinch`
still installs perfectly and the user uses the full name. This is necessary
because a `brew link` conflict on one file would otherwise leave the whole
keg unlinked — including `cinch`.

**Auto-bump safety:** `scripts/update-homebrew-formula.sh` rewrites only the
`url` + `sha256` lines (Python regex keyed on the target triple); it never
touches `def install`. The hand-edited `ci` block survives every release.

### 4. Homebrew cask — `homebrew-tap/main/Casks/cinchcli.rb`

Add a second binary stanza for desktop-app parity:

```ruby
binary "#{appdir}/Cinch.app/Contents/MacOS/Cinch", target: "ci"
```

The cask `binary` stanza can't take the `File.exist?` guard as cleanly as
the formula. Accepted as a documented caveat: the cask already
`conflicts_with` the formula, and a cask user *also* having RCS's `ci` is
vanishingly rare. If it happens, the cask install warns on the one `ci`
file; `cinch` is unaffected.

**Auto-bump safety:** `scripts/update-homebrew-cask.sh` rewrites only
`version` + `sha256`; it never touches `binary` stanzas. The `ci` stanza
survives every release.

### 5. curl installer — `website/main/public/install.sh`

The Linux `curl … | sh` installer (`brew` is recommended on macOS) drops
`cinch` into `${PREFIX}/bin` (default `/usr/local`). Add the same defensive
`ci` symlink right after, only when `ci` is free on PATH:

```sh
# Short alias: link `ci` → cinch, but only if `ci` is free (don't clobber
# RCS check-in or a user's own ci).
if command -v ci >/dev/null 2>&1 || [ -e "${PREFIX}/bin/ci" ]; then
  info "Skipping 'ci' alias — a 'ci' command already exists. Use 'cinch'."
else
  run_root ln -s cinch "${PREFIX}/bin/ci"
  info "Also linked 'ci' as a short alias for 'cinch'."
fi
```

Edit `public/install.sh` only; `dist/install.sh` is build output. The
symlink target is relative (`ln -s cinch`, not an absolute path) so it
survives a relocated prefix.

### 6. Docs — README + website

Low churn (the chosen low-blast-radius path). All examples stay `cinch …`.
Add one sentence to the README install section and the website quick-start:

> `ci` is installed as a short alias for `cinch` — every command works under
> both names.

## Out of scope (v1)

- **Windows auto-`ci`.** Windows ships a *separate* `cinch.exe` via Tauri
  `externalBin` (no symlinks), so auto-creating `ci.exe` is different
  mechanics and deferred. The step-1 dispatch already matches `ci.exe`, so
  it's a small follow-up; a user who copies `cinch.exe` → `ci.exe` gets it
  working today.
- **Hand-placed binaries / `cargo install`.** Users who place the binary
  manually (not via brew or `install.sh`) get `cinch` only. Document a
  one-line `ln -s cinch ci` for them.

## Cross-repo footprint

| Repo | Changes |
|---|---|
| cinch monorepo (`agent/claude/ci-alias`) | Steps 1, 2, README sentence (step 6) |
| homebrew-tap | Steps 3, 4 |
| website | Step 5 (`install.sh`), step 6 quick-start sentence |

## Testing

- **Desktop:** unit test for `invoked_as_cli()` covering `ci`, `ci.exe`,
  `cinch`, `cinch.exe` (true) and `Cinch` (false).
- **CLI:** unit tests on `render_completion(shell, name)` for both names —
  `ci` output contains `_ci` / `complete … ci` and no `_cinch` token; the
  `cinch` output is unchanged from today (guards against accidental drift in
  the existing script). Cover zsh, bash, fish.
- **Formula:** `brew test` runs `cinch --version` and `ci --version`.
- **install.sh:** `sh -n install.sh` (syntax) + a manual run into a temp
  `--prefix` confirming `ci` is symlinked when free and skipped when a
  pre-seeded `ci` exists.
- **Manual:** `brew install` from the local tap on a clean machine; confirm
  both `cinch` and `ci` resolve and `ci <tab>` completes a subcommand.
