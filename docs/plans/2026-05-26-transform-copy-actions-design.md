# Transform Copy Actions Design

- **Date:** 2026-05-26
- **Status:** Design
- **Scope:** Local deterministic transform actions shared by Cinch Desktop and the Cinch CLI

## Problem

Cinch already makes clipboard history searchable and copyable, but copying a saved clip often needs a small deterministic cleanup step before it is useful: format JSON, redact secrets, quote text for a shell command, collapse whitespace, or wrap text in a Markdown code block. Users can do this manually today, but that breaks the fast recall flow.

The v1 goal is to let users explicitly copy a transformed version of a text clip without adding any AI provider, network call, or automatic paste behavior.

## Decision

Build a shared transform engine in `client-core` and expose it from both Desktop and CLI.

Desktop will add transform copy actions to the existing copy flow. The default Copy action remains unchanged and copies the original clip. Transform actions are explicit and available only for text-like clips.

CLI will add a command that reads a stored clip, applies the same transform engine, and writes either to stdout or to the system clipboard. This keeps the feature useful in headless and terminal workflows and avoids Desktop-only behavior.

## Transform Set

v1 ships deterministic transforms only:

- `pretty_json`: parse JSON and emit pretty JSON.
- `minify_json`: parse JSON and emit compact JSON.
- `trim_whitespace`: trim leading/trailing whitespace and trailing whitespace on each line.
- `collapse_whitespace`: collapse all whitespace runs to a single ASCII space.
- `redact_secrets`: mask common token/password/API-key forms.
- `shell_single_quote`: wrap as POSIX shell single-quoted text, escaping embedded single quotes.
- `markdown_code_block`: wrap content in a fenced Markdown code block.
- `url_encode`: percent-encode text.
- `url_decode`: percent-decode text as UTF-8.

Images and binary clips are out of scope for v1 transforms.

## Architecture

`client-core` owns the transform API:

- `TransformAction`: stable enum with string IDs for CLI and Desktop.
- `TransformError`: invalid input and unsupported content errors.
- `apply_transform(action, input, content_type) -> Result<String, TransformError>`.
- `list_transform_actions(content_type) -> Vec<TransformActionInfo>`.

Desktop calls a new Tauri command with `clip_id` and action ID. The command reads the clip from the shared store, validates that it is text-like, calls `client-core`, writes the result with `ClipboardService::write_text`, and returns action metadata for toast text.

CLI adds `cinch clip transform <CLIP> --action <ACTION> [--copy]`. Without `--copy`, it prints the transformed text to stdout. With `--copy`, it writes the transformed text to the OS clipboard.

## UX

Desktop:

- Keep the primary Copy button and Enter behavior as original-copy.
- Add a `Copy As...` trigger next to Copy in the detail view.
- `Cmd+K` opens `Copy As...` for the selected text-like clip by default.
- `Copy As...` opens a small command sheet with a search field and keyboard navigation.
- The command sheet lists transform actions by label and filters by typed text.
- Show success toast using the action label, e.g. `Copied as Pretty JSON`.
- On transform failure, show an error toast and do not copy original content as a fallback.

CLI:

- `cinch clip transform <id-prefix> --action pretty-json`
- `cinch clip transform <id-prefix> --action redact-secrets --copy`
- `cinch clip transform --list-actions`

The CLI should reuse the existing clip-prefix resolver pattern rather than requiring full ULIDs.

## Privacy And Safety

No network access is introduced. Transforms run locally over plaintext already present in the local store.

`redact_secrets` is best-effort and must not be documented as a guarantee. It should mask common high-signal patterns while avoiding broad destructive edits.

Invalid transforms fail closed: no clipboard write, clear error message, and no automatic fallback to original content.

## Non-Goals

- No LLM transforms.
- No embeddings or natural-language search changes.
- No automatic paste.
- No transform-on-capture.
- No relay-side changes.
- No image transforms.

## Success Criteria

1. Desktop users can copy a selected clip through at least the eight v1 transforms.
2. CLI users can apply the same transforms to a stored clip and either print or copy the result.
3. Invalid JSON transform attempts fail without changing the clipboard.
4. Transform logic has unit coverage in `client-core`, with Desktop and CLI command tests covering integration.
