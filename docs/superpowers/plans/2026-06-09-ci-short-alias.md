# `ci` short alias — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `ci` work as a short, drop-in alias for the `cinch` command, created automatically (and defensively) at install time, without renaming `cinch` or touching the brand.

**Architecture:** `cinch` stays the one real binary and canonical name everywhere. `ci` is a thin symlink to it, created by each installer only when the `ci` slot is free. The embedded-CLI dispatch and shell completions are taught to answer to both names. Spans three repos: cinch monorepo (CLI + desktop), homebrew-tap (formula + cask), website (`install.sh` + docs).

**Tech Stack:** Rust (clap, clap_complete), Tauri, Ruby (Homebrew formula/cask), POSIX `sh`, Markdown/MDX.

**Spec:** `docs/superpowers/specs/2026-06-09-ci-short-alias-design.md`

---

## Worktrees

The cinch work uses the already-created worktree. The other two repos get plain `git worktree`s (no helper script exists there).

```bash
# cinch (already created):  cinch/claude-ci-alias  on agent/claude/ci-alias
git -C homebrew-tap/main worktree add ../claude-ci-alias -b agent/claude/ci-alias
git -C website/main       worktree add ../claude-ci-alias -b agent/claude/ci-alias
```

All paths below are relative to the repo's worktree root. **Do not push** — integration/PR happens after the plan via the finishing-a-development-branch skill.

---

## Task 1: Desktop argv[0] dispatch recognizes `ci`

**Repo/worktree:** `cinch/claude-ci-alias`

**Files:**
- Modify: `apps/desktop/src-tauri/src/main.rs:7-20`
- Test: same file (`#[cfg(all(test, feature = "builtin-cli"))] mod tests`)

- [ ] **Step 1: Refactor the name match into a pure, testable fn + add the test**

Replace the existing `invoked_as_cli` (lines 7-20) with a pure `is_cli_name` helper plus a thin `invoked_as_cli`, and append a test module. Final state of that region:

```rust
#[cfg(feature = "builtin-cli")]
fn is_cli_name(exe: &str) -> bool {
    // The desktop launcher invokes the app as "Cinch" (capital), so matching
    // the lowercase CLI names is unambiguous. `ci` is the short symlink alias.
    matches!(exe, "cinch" | "cinch.exe" | "ci" | "ci.exe")
}

#[cfg(feature = "builtin-cli")]
fn invoked_as_cli() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(arg0) = args.first() else {
        return false;
    };
    let exe = Path::new(arg0)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    is_cli_name(exe)
}

#[cfg(all(test, feature = "builtin-cli"))]
mod tests {
    use super::is_cli_name;

    #[test]
    fn cli_invocation_names_dispatch_to_cli() {
        assert!(is_cli_name("cinch"));
        assert!(is_cli_name("cinch.exe"));
        assert!(is_cli_name("ci"));
        assert!(is_cli_name("ci.exe"));
    }

    #[test]
    fn desktop_and_unknown_names_do_not_dispatch() {
        assert!(!is_cli_name("Cinch"));
        assert!(!is_cli_name("Cinch.exe"));
        assert!(!is_cli_name("cinchd"));
        assert!(!is_cli_name("cid"));
        assert!(!is_cli_name(""));
    }
}
```

- [ ] **Step 2: Run the tests, expect them to pass**

Run: `cargo test -p cinch-desktop is_cli_name`
Expected: `cli_invocation_names_dispatch_to_cli` and `desktop_and_unknown_names_do_not_dispatch` both PASS (2 passed).
(Note: this builds the Tauri crate via its `build.rs`; run from the worktree root.)

- [ ] **Step 3: Format and commit**

```bash
cargo fmt -p cinch-desktop
git add apps/desktop/src-tauri/src/main.rs
git commit -m "feat(desktop): dispatch to CLI when invoked as 'ci'"
```

---

## Task 2: Shell completions keyed to the invoked name (`cinch` or `ci`)

**Repo/worktree:** `cinch/claude-ci-alias`

**Files:**
- Modify: `crates/cli/src/lib.rs` — the three `*_FROM_OVERRIDE` consts (lines 146-203), `print_completion_override` (lines 95-102), and the `Cmd::Completion` arm in `run()` (lines 271-280)
- Test: same file (new `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests**

Append to the bottom of `crates/cli/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::{completion_override, render_completion};
    use clap_complete::Shell;

    #[test]
    fn cinch_completion_keeps_existing_tokens() {
        let z = render_completion(Shell::Zsh, "cinch");
        assert!(z.contains("_cinch_generated"));
        assert!(z.contains("_cinch_devices_names"));
        let b = render_completion(Shell::Bash, "cinch");
        assert!(b.contains("complete -F _cinch_with_from cinch"));
        let f = render_completion(Shell::Fish, "cinch");
        assert!(f.contains("complete -c cinch"));
    }

    #[test]
    fn ci_completion_is_namespaced_to_ci() {
        let z = render_completion(Shell::Zsh, "ci");
        assert!(z.contains("_ci_generated"));
        assert!(z.contains("_ci_devices_names"));
        // The cinch-keyed override fns must NOT leak, or sourcing both the
        // _cinch and _ci files would collide.
        assert!(!z.contains("_cinch_generated"));
        let b = render_completion(Shell::Bash, "ci");
        assert!(b.contains("complete -F _ci_with_from ci"));
        assert!(!b.contains("_cinch_with_from"));
        let f = render_completion(Shell::Fish, "ci");
        assert!(f.contains("complete -c ci"));
        assert!(!f.contains("complete -c cinch"));
    }

    #[test]
    fn override_is_empty_for_unsupported_shell() {
        assert_eq!(completion_override(Shell::Elvish, "ci"), "");
    }
}
```

- [ ] **Step 2: Run the tests, expect a compile failure**

Run: `cargo test -p cinch-cli completion`
Expected: FAIL — `cannot find function render_completion` / `completion_override` in this scope (they don't exist yet).

- [ ] **Step 3: Convert the override consts to `{bin}` templates**

Replace the three consts (currently lines 146-203) so every `cinch` / `_cinch` token becomes `{bin}` / `_{bin}`. Shell `${...}` / `$functions[...]` syntax is left untouched (it contains no literal `{bin}`):

```rust
const ZSH_FROM_OVERRIDE: &str = r#"
# {bin} device-name dynamic completion
_{bin}_devices_names() {
  local -a devs
  devs=( ${(f)"$({bin} fleet list --names 2>/dev/null)"} )
  _describe 'device' devs
}
# clap_complete inlines subcommands inside _{bin}, so we rename it
# and wrap with a version that intercepts device-name completion.
# `compset -P '--flag='` strips the `--flag=` prefix from $PREFIX so
# candidates match against the value portion; it's a no-op when the
# current word has no `=`, so it works for both `--from <tab>` and
# `--from=<tab>`.
functions[_{bin}_generated]=$functions[_{bin}]
_{bin}() {
  if [[ ${words[2]} == pull ]] && \
     [[ ${words[CURRENT-1]} == --from || ${words[CURRENT]} == --from=* ]]; then
    compset -P '--from='
    _{bin}_devices_names
    return
  fi
  _{bin}_generated "$@"
}
"#;

const BASH_FROM_OVERRIDE: &str = r#"
# {bin} device-name dynamic completion.
# Bash's default COMP_WORDBREAKS contains `=`, so `--from=foo` is split
# into three tokens (`--from`, `=`, `foo`). Detect both forms by also
# inspecting the token two slots back when the previous token is `=`.
_{bin}_devices_names() {
  local word="${COMP_WORDS[COMP_CWORD]}"
  mapfile -t COMPREPLY < <({bin} fleet list --names 2>/dev/null | grep -- "^${word}")
}
_{bin}_with_from() {
  local cur prev prev2
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"
  prev2="${COMP_WORDS[COMP_CWORD-2]:-}"
  if [[ "$prev" == "--from" ]]; then
    _{bin}_devices_names
    return
  fi
  if [[ "$prev" == "=" && "$prev2" == "--from" ]]; then
    _{bin}_devices_names
    return
  fi
  _{bin}
}
complete -F _{bin}_with_from {bin}
"#;

const FISH_FROM_OVERRIDE: &str = r#"
# {bin} device-name dynamic completion
complete -c {bin} -n '__fish_seen_subcommand_from pull' -l from -f \
  -d 'Device nickname or hostname' \
  -a '({bin} fleet list --names 2>/dev/null)'
"#;
```

- [ ] **Step 4: Replace `print_completion_override` with name-aware helpers**

Delete `print_completion_override` (lines 95-102) and add in its place:

```rust
/// The basename the binary was invoked under, normalized to one of our two
/// supported command names. Invoked via the `ci` symlink → `"ci"`; anything
/// else (including `cinch` and odd argv[0]s) → `"cinch"`.
fn invoked_bin_name() -> &'static str {
    let arg0 = std::env::args_os().next();
    let base = arg0
        .as_deref()
        .map(std::path::Path::new)
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .map(|s| s.strip_suffix(".exe").unwrap_or(s));
    match base {
        Some("ci") => "ci",
        _ => "cinch",
    }
}

/// The hand-written device-name completion override, keyed to `name`
/// (`cinch` or `ci`) so the two completion files never collide when sourced.
fn completion_override(shell: Shell, name: &str) -> String {
    let template = match shell {
        Shell::Zsh => ZSH_FROM_OVERRIDE,
        Shell::Bash => BASH_FROM_OVERRIDE,
        Shell::Fish => FISH_FROM_OVERRIDE,
        _ => return String::new(),
    };
    template.replace("{bin}", name)
}

/// Render the full completion script for `name`: clap's AOT output (hidden
/// subcommands stripped) plus the dynamic override, both keyed to `name`.
fn render_completion(shell: Shell, name: &str) -> String {
    let full = Cli::command();
    // AOT generators don't skip `hide = true` subcommands; rebuild the tree
    // without them so deprecated aliases never appear as candidates (§4d).
    let mut cmd = strip_hidden_subcommands(&full).version(env!("CARGO_PKG_VERSION"));
    let mut buf: Vec<u8> = Vec::new();
    clap_complete::generate(shell, &mut cmd, name, &mut buf);
    let mut out = String::from_utf8(buf).expect("clap completion output is valid UTF-8");
    out.push_str(&completion_override(shell, name));
    out
}
```

- [ ] **Step 5: Rewrite the `Cmd::Completion` arm in `run()` to use them**

Replace the block at lines 271-280:

```rust
    if let Cmd::Completion { shell } = cli.cmd {
        let full = Cli::command();
        let bin_name = full.get_name().to_string();
        // AOT generators don't skip `hide = true` subcommands; rebuild the tree
        // without them so deprecated aliases never appear as candidates (§4d).
        let mut cmd = strip_hidden_subcommands(&full).version(env!("CARGO_PKG_VERSION"));
        clap_complete::generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
        print_completion_override(shell);
        return 0;
    }
```

with:

```rust
    if let Cmd::Completion { shell } = cli.cmd {
        // Key completions to the name we were invoked as, so the `ci` symlink
        // gets a working `_ci` script while `cinch` is byte-for-byte unchanged.
        print!("{}", render_completion(shell, invoked_bin_name()));
        return 0;
    }
```

- [ ] **Step 6: Run the tests, expect them to pass**

Run: `cargo test -p cinch-cli completion`
Expected: `cinch_completion_keeps_existing_tokens`, `ci_completion_is_namespaced_to_ci`, `override_is_empty_for_unsupported_shell` all PASS.

- [ ] **Step 7: Sanity-check the live output for both names**

Run: `cargo run -p cinch-cli --bin cinch -- completion zsh | head -5`
Expected: contains `#compdef cinch`.
Run: `cargo build -p cinch-cli && ln -sf cinch target/debug/ci && ./target/debug/ci completion zsh | head -5`
Expected: contains `#compdef ci` (proves argv[0] keying). Then `rm target/debug/ci`.

- [ ] **Step 8: Lint, format, commit**

```bash
cargo fmt -p cinch-cli
cargo clippy -p cinch-cli --all-targets -- -D warnings
git add crates/cli/src/lib.rs
git commit -m "feat(cli): generate shell completions for the invoked name (cinch or ci)"
```

---

## Task 3: README mentions the `ci` alias

**Repo/worktree:** `cinch/claude-ci-alias`

**Files:**
- Modify: `README.md:30-34` (after the Linux curl-installer block, before `### AI workflow v1`)

- [ ] **Step 1: Add the one-line note**

Insert a blockquote between the curl installer code fence and the `### AI workflow v1` heading. Target text to anchor on:

```markdown
**Linux — curl installer**:
```bash
curl -fsSL https://cinchcli.com/install.sh | sh
```

### AI workflow v1
```

becomes:

```markdown
**Linux — curl installer**:
```bash
curl -fsSL https://cinchcli.com/install.sh | sh
```

> **Short alias:** `ci` is installed as a shorthand for `cinch` — every command works under both names (`ci pull`, `ci send`, …).

### AI workflow v1
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs(readme): note that ci is a short alias for cinch"
```

---

## Task 4: Homebrew formula — defensive `ci` symlink + completions

**Repo/worktree:** `homebrew-tap/claude-ci-alias`

**Files:**
- Modify: `Formula/cinchcli.rb` (`def install` and `test do` blocks)

- [ ] **Step 1: Update `def install` and `test do`**

Replace:

```ruby
  def install
    bin.install "cinch"
    generate_completions_from_executable(bin/"cinch", "completion",
                                         shells: [:bash, :zsh, :fish])
  end

  test do
    system bin/"cinch", "--version"
  end
```

with:

```ruby
  def install
    bin.install "cinch"
    generate_completions_from_executable(bin/"cinch", "completion",
                                         shells: [:bash, :zsh, :fish])

    # `ci` is a short alias for `cinch`. Create it only when the slot is free,
    # so a pre-existing `ci` (e.g. RCS check-in) never blocks `brew link` and
    # leaves cinch itself unlinked.
    unless File.exist?("#{HOMEBREW_PREFIX}/bin/ci")
      bin.install_symlink "cinch" => "ci"
      generate_completions_from_executable(bin/"ci", "completion",
                                           shells: [:bash, :zsh, :fish],
                                           base_name: "ci")
    end
  end

  test do
    system bin/"cinch", "--version"
    system bin/"ci", "--version" if (bin/"ci").exist?
  end
```

- [ ] **Step 2: Verify the formula parses and lints**

Run: `ruby -c Formula/cinchcli.rb`
Expected: `Syntax OK`.
Run (if Homebrew available): `brew style Formula/cinchcli.rb`
Expected: no offenses (or only pre-existing, unrelated ones).

- [ ] **Step 3: Verify a real install creates `ci` (requires macOS + Homebrew + network)**

Run:
```bash
brew install --formula ./Formula/cinchcli.rb
ls -l "$(brew --prefix)/bin/ci" "$(brew --prefix)/bin/cinch"   # both present; ci -> cinch
brew test cinchcli
brew uninstall --formula cinchcli
```
Expected: `ci` is a symlink to `cinch`; `brew test` runs both `--version` calls and passes.
If a mac/Homebrew host is unavailable, mark this step verified-by-review and note it in the task report.

- [ ] **Step 4: Commit**

```bash
git add Formula/cinchcli.rb
git commit -m "feat: install ci as a defensive short alias for cinch"
```

---

## Task 5: Homebrew cask — `ci` binary stanza

**Repo/worktree:** `homebrew-tap/claude-ci-alias`

**Files:**
- Modify: `Casks/cinchcli.rb` (after the existing `binary … target: "cinch"` line)

- [ ] **Step 1: Add the `ci` binary stanza**

Replace:

```ruby
  binary "#{appdir}/Cinch.app/Contents/MacOS/Cinch", target: "cinch"
```

with:

```ruby
  binary "#{appdir}/Cinch.app/Contents/MacOS/Cinch", target: "cinch"
  # `ci` short alias — same embedded CLI binary, dispatched on argv[0].
  binary "#{appdir}/Cinch.app/Contents/MacOS/Cinch", target: "ci"
```

- [ ] **Step 2: Verify it parses and lints**

Run: `ruby -c Casks/cinchcli.rb`
Expected: `Syntax OK`.
Run (if Homebrew available): `brew style --cask Casks/cinchcli.rb`
Expected: no new offenses.

- [ ] **Step 3: Commit**

```bash
git add Casks/cinchcli.rb
git commit -m "feat: expose ci alias from the desktop cask"
```

---

## Task 6: curl installer — defensive `ci` symlink

**Repo/worktree:** `website/claude-ci-alias`

**Files:**
- Modify: `public/install.sh` (after the `mv` that installs `cinch`, around line 118). Do **not** edit `dist/install.sh` — it is build output.

- [ ] **Step 1: Insert the defensive symlink block**

Find:

```sh
info "Installing to ${PREFIX}/bin/cinch..."
run_root mkdir -p "${PREFIX}/bin"
run_root mv "$WORK/cinch" "${PREFIX}/bin/cinch"

info "Installed $("${PREFIX}/bin/cinch" --version 2>/dev/null || echo cinch)."
```

and insert the `ci` block between the `mv` and the `info "Installed …"` lines:

```sh
info "Installing to ${PREFIX}/bin/cinch..."
run_root mkdir -p "${PREFIX}/bin"
run_root mv "$WORK/cinch" "${PREFIX}/bin/cinch"

# Short alias: link `ci` → cinch, but only when `ci` is free, so a
# pre-existing `ci` (RCS check-in or the user's own) is never clobbered.
if command -v ci >/dev/null 2>&1 || [ -e "${PREFIX}/bin/ci" ]; then
  info "Skipping 'ci' alias — a 'ci' command already exists. Use 'cinch'."
else
  run_root ln -s cinch "${PREFIX}/bin/ci"
  info "Also linked 'ci' as a short alias for 'cinch'."
fi

info "Installed $("${PREFIX}/bin/cinch" --version 2>/dev/null || echo cinch)."
```

- [ ] **Step 2: Verify shell syntax**

Run: `sh -n public/install.sh`
Expected: no output, exit 0.
Run (if `shellcheck` available): `shellcheck public/install.sh`
Expected: no new errors from the added block.

- [ ] **Step 3: Behavioral check of the symlink logic (no network)**

Verify the branch in isolation with a stub, proving "free → link" and "taken → skip":

```bash
# free slot → links ci
D=$(mktemp -d); mkdir -p "$D/bin"; : > "$D/bin/cinch"; chmod +x "$D/bin/cinch"
PREFIX="$D" sh -c 'command -v ci >/dev/null 2>&1 || [ -e "$PREFIX/bin/ci" ] && echo SKIP || ln -s cinch "$PREFIX/bin/ci"'
test -L "$D/bin/ci" && echo "OK: ci linked"
# taken slot → skips
D2=$(mktemp -d); mkdir -p "$D2/bin"; : > "$D2/bin/cinch"; : > "$D2/bin/ci"
PREFIX="$D2" sh -c 'if command -v ci >/dev/null 2>&1 || [ -e "$PREFIX/bin/ci" ]; then echo "OK: skipped (ci exists)"; else ln -s cinch "$PREFIX/bin/ci"; fi'
rm -rf "$D" "$D2"
```
Expected: `OK: ci linked` then `OK: skipped (ci exists)`.

- [ ] **Step 4: Commit**

```bash
git add public/install.sh
git commit -m "feat(install.sh): link ci as a defensive short alias for cinch"
```

---

## Task 7: Website quick-start mentions the `ci` alias

**Repo/worktree:** `website/claude-ci-alias`

**Files:**
- Modify: `src/content/docs/docs/quick-start.mdx` (after the install `</Tabs>` at line 47, before step 2 at line 49)

- [ ] **Step 1: Add a tip Aside**

`Aside` is already imported (line 6). Insert after the `</Tabs>` that closes the install step (keep the 3-space indentation of the list-item body):

```mdx
   </Tabs>

   <Aside type="tip">
     `ci` is installed as a short alias for `cinch` — every command on this page works under both names (`ci pull`, `ci send`, …).
   </Aside>

2. **Open Cinch and sign in**
```

- [ ] **Step 2: Verify the site builds (if toolchain available)**

Run: `npm run build` (or `pnpm build`) from the website worktree root.
Expected: build succeeds; quick-start page renders the new Aside.
If the toolchain isn't available, verify the MDX is well-formed by review and note it.

- [ ] **Step 3: Commit**

```bash
git add src/content/docs/docs/quick-start.mdx
git commit -m "docs(quick-start): note that ci is a short alias for cinch"
```

---

## Task 8: Cross-repo verification

**Repos:** all three worktrees

- [ ] **Step 1: cinch workspace is green**

Run from `cinch/claude-ci-alias`:
```bash
cargo test -p cinch-cli completion
cargo test -p cinch-desktop is_cli_name
cargo fmt --check -p cinch-cli -p cinch-desktop
```
Expected: all PASS, fmt clean.

- [ ] **Step 2: Confirm the four install surfaces each create/recognize `ci`**

Checklist (review + the per-task verifications above):
- Formula `def install` creates `ci` when free (Task 4 step 3).
- Cask has a `ci` binary stanza (Task 5).
- `install.sh` links `ci` when free, skips when taken (Task 6 step 3).
- Desktop dispatch matches `ci`/`ci.exe` (Task 1).
- `ci`-keyed completions render (Task 2 step 7).

- [ ] **Step 3: Report status per repo**

Summarize: branches `agent/claude/ci-alias` in cinch / homebrew-tap / website, commits per task, and any verification steps that were review-only (no mac/Homebrew/node host). No pushes yet.

---

## Notes & invariants

- `cinch` remains the canonical name: Cargo `[[bin]] name = "cinch"`, clap `#[command(name = "cinch")]`, and all help/usage text are unchanged. `ci --help` intentionally prints `Usage: cinch …` (accepted decision).
- No second binary is built — `ci` is always a symlink to the one `cinch` binary.
- Auto-bump safety: `update-homebrew-formula.sh` rewrites only `url`/`sha256`; `update-homebrew-cask.sh` only `version`/`sha256`. The hand-edited `ci` blocks survive releases.
- Out of scope (per spec): Windows auto-`ci` (separate `cinch.exe`, no symlinks; dispatch already matches `ci.exe`), and hand-placed/`cargo install` binaries (documented `ln -s cinch ci`).
- Lefthook pre-commit enforces `rust-fmt` + version-parity on the cinch repo; run `cargo fmt` before each cinch commit.
