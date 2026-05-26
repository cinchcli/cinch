# Transform Copy Actions Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add local deterministic transform-copy actions shared by Cinch Desktop and the Cinch CLI.

**Architecture:** Put transform logic in `client-core` so Desktop and CLI call one tested engine. Desktop adds a Tauri command that reads a clip from the shared store, applies a transform, and writes the transformed text with `ClipboardService`; CLI adds `cinch clip transform` with stdout and `--copy` modes.

**Tech Stack:** Rust workspace, `client-core`, Clap, Tauri v2, React/TypeScript, tauri-specta bindings, SQLite-backed local store.

---

### Task 1: Add The Shared Transform Engine

**Files:**
- Create: `crates/client-core/src/transform.rs`
- Modify: `crates/client-core/src/lib.rs`
- Test: `crates/client-core/src/transform.rs`

**Step 1: Write the failing tests**

Create `crates/client-core/src/transform.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_json_formats_valid_json() {
        let out = apply_transform(TransformAction::PrettyJson, r#"{"b":2,"a":1}"#, "json").unwrap();
        assert_eq!(out, "{\n  \"b\": 2,\n  \"a\": 1\n}");
    }

    #[test]
    fn pretty_json_rejects_invalid_json() {
        let err = apply_transform(TransformAction::PrettyJson, "{nope", "text").unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput(_)));
    }

    #[test]
    fn minify_json_compacts_valid_json() {
        let out = apply_transform(TransformAction::MinifyJson, "{\n  \"a\": 1\n}", "json").unwrap();
        assert_eq!(out, r#"{"a":1}"#);
    }

    #[test]
    fn trim_whitespace_trims_edges_and_line_ends() {
        let out = apply_transform(TransformAction::TrimWhitespace, "  a  \n b\t \n", "text").unwrap();
        assert_eq!(out, "a\n b");
    }

    #[test]
    fn collapse_whitespace_uses_single_spaces() {
        let out = apply_transform(TransformAction::CollapseWhitespace, "a \n\t b   c", "text").unwrap();
        assert_eq!(out, "a b c");
    }

    #[test]
    fn shell_single_quote_escapes_embedded_quotes() {
        let out = apply_transform(TransformAction::ShellSingleQuote, "can't", "text").unwrap();
        assert_eq!(out, "'can'\"'\"'t'");
    }

    #[test]
    fn markdown_code_block_uses_content_type_hint() {
        let out = apply_transform(TransformAction::MarkdownCodeBlock, "let x = 1;", "code").unwrap();
        assert_eq!(out, "```text\nlet x = 1;\n```");
    }

    #[test]
    fn url_encode_and_decode_roundtrip_utf8() {
        let enc = apply_transform(TransformAction::UrlEncode, "hello world/한글", "text").unwrap();
        assert_eq!(enc, "hello%20world%2F%ED%95%9C%EA%B8%80");
        let dec = apply_transform(TransformAction::UrlDecode, &enc, "text").unwrap();
        assert_eq!(dec, "hello world/한글");
    }

    #[test]
    fn redact_secrets_masks_common_assignments() {
        let out = apply_transform(
            TransformAction::RedactSecrets,
            "api_key = sk-1234567890abcdef\npassword: hunter2",
            "text",
        )
        .unwrap();
        assert!(out.contains("api_key = [REDACTED]"));
        assert!(out.contains("password: [REDACTED]"));
    }

    #[test]
    fn image_content_type_is_unsupported() {
        let err = apply_transform(TransformAction::TrimWhitespace, "ignored", "image").unwrap_err();
        assert_eq!(err, TransformError::UnsupportedContentType("image".to_string()));
    }

    #[test]
    fn action_ids_roundtrip() {
        for action in TransformAction::ALL {
            assert_eq!(TransformAction::from_id(action.id()), Some(action));
        }
        assert_eq!(TransformAction::from_id("pretty-json"), Some(TransformAction::PrettyJson));
        assert_eq!(TransformAction::from_id("pretty_json"), Some(TransformAction::PrettyJson));
    }
}
```

**Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p cinchcli-core transform::
```

Expected: compile failure because `TransformAction`, `TransformError`, and `apply_transform` do not exist.

**Step 3: Implement the transform module**

Implement:

```rust
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransformAction {
    PrettyJson,
    MinifyJson,
    TrimWhitespace,
    CollapseWhitespace,
    RedactSecrets,
    ShellSingleQuote,
    MarkdownCodeBlock,
    UrlEncode,
    UrlDecode,
}

impl TransformAction {
    pub const ALL: [TransformAction; 9] = [
        TransformAction::PrettyJson,
        TransformAction::MinifyJson,
        TransformAction::TrimWhitespace,
        TransformAction::CollapseWhitespace,
        TransformAction::RedactSecrets,
        TransformAction::ShellSingleQuote,
        TransformAction::MarkdownCodeBlock,
        TransformAction::UrlEncode,
        TransformAction::UrlDecode,
    ];

    pub fn id(self) -> &'static str {
        match self {
            TransformAction::PrettyJson => "pretty-json",
            TransformAction::MinifyJson => "minify-json",
            TransformAction::TrimWhitespace => "trim-whitespace",
            TransformAction::CollapseWhitespace => "collapse-whitespace",
            TransformAction::RedactSecrets => "redact-secrets",
            TransformAction::ShellSingleQuote => "shell-single-quote",
            TransformAction::MarkdownCodeBlock => "markdown-code-block",
            TransformAction::UrlEncode => "url-encode",
            TransformAction::UrlDecode => "url-decode",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TransformAction::PrettyJson => "Pretty JSON",
            TransformAction::MinifyJson => "Minify JSON",
            TransformAction::TrimWhitespace => "Trim Whitespace",
            TransformAction::CollapseWhitespace => "Collapse Whitespace",
            TransformAction::RedactSecrets => "Redact Secrets",
            TransformAction::ShellSingleQuote => "Shell Quote",
            TransformAction::MarkdownCodeBlock => "Markdown Code Block",
            TransformAction::UrlEncode => "URL Encode",
            TransformAction::UrlDecode => "URL Decode",
        }
    }

    pub fn from_id(raw: &str) -> Option<Self> {
        let normalized = raw.trim().replace('_', "-");
        Self::ALL.into_iter().find(|a| a.id() == normalized)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformActionInfo {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransformError {
    UnsupportedContentType(String),
    InvalidInput(String),
}

impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransformError::UnsupportedContentType(ct) => write!(f, "unsupported content type: {ct}"),
            TransformError::InvalidInput(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TransformError {}
```

Add helpers:

```rust
pub fn list_transform_actions(content_type: &str) -> Vec<TransformActionInfo> {
    if !is_text_like(content_type) {
        return Vec::new();
    }
    TransformAction::ALL
        .into_iter()
        .map(|a| TransformActionInfo {
            id: a.id().to_string(),
            label: a.label().to_string(),
        })
        .collect()
}

pub fn apply_transform(
    action: TransformAction,
    input: &str,
    content_type: &str,
) -> Result<String, TransformError> {
    if !is_text_like(content_type) {
        return Err(TransformError::UnsupportedContentType(content_type.to_string()));
    }
    match action {
        TransformAction::PrettyJson => pretty_json(input),
        TransformAction::MinifyJson => minify_json(input),
        TransformAction::TrimWhitespace => Ok(trim_whitespace(input)),
        TransformAction::CollapseWhitespace => Ok(input.split_whitespace().collect::<Vec<_>>().join(" ")),
        TransformAction::RedactSecrets => Ok(redact_secrets(input)),
        TransformAction::ShellSingleQuote => Ok(shell_single_quote(input)),
        TransformAction::MarkdownCodeBlock => Ok(markdown_code_block(input, content_type)),
        TransformAction::UrlEncode => Ok(percent_encode(input.as_bytes())),
        TransformAction::UrlDecode => percent_decode(input),
    }
}

fn is_text_like(content_type: &str) -> bool {
    matches!(content_type, "text" | "code" | "url" | "json") || content_type.starts_with("text/")
}
```

Implement percent encode/decode without adding dependencies:

```rust
fn percent_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(input: &str) -> Result<String, TransformError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(TransformError::InvalidInput("invalid percent escape".to_string()));
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|_| TransformError::InvalidInput("invalid percent escape".to_string()))?;
            let value = u8::from_str_radix(hex, 16)
                .map_err(|_| TransformError::InvalidInput("invalid percent escape".to_string()))?;
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| TransformError::InvalidInput("decoded text is not valid UTF-8".to_string()))
}
```

Implement remaining helpers:

```rust
fn pretty_json(input: &str) -> Result<String, TransformError> {
    let value: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| TransformError::InvalidInput(format!("invalid JSON: {e}")))?;
    serde_json::to_string_pretty(&value)
        .map_err(|e| TransformError::InvalidInput(format!("JSON format failed: {e}")))
}

fn minify_json(input: &str) -> Result<String, TransformError> {
    let value: serde_json::Value = serde_json::from_str(input)
        .map_err(|e| TransformError::InvalidInput(format!("invalid JSON: {e}")))?;
    serde_json::to_string(&value)
        .map_err(|e| TransformError::InvalidInput(format!("JSON minify failed: {e}")))
}

fn trim_whitespace(input: &str) -> String {
    input
        .trim()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

fn shell_single_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\"'\"'"))
}

fn markdown_code_block(input: &str, content_type: &str) -> String {
    let lang = if content_type == "json" { "json" } else { "text" };
    format!("```{lang}\n{}\n```", input.trim_end())
}

fn redact_secrets(input: &str) -> String {
    input
        .lines()
        .map(redact_secret_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_secret_line(line: &str) -> String {
    const KEYS: [&str; 8] = ["api_key", "apikey", "token", "access_token", "secret", "password", "passwd", "authorization"];
    let lower = line.to_lowercase();
    if !KEYS.iter().any(|k| lower.contains(k)) {
        return line.to_string();
    }
    for sep in [" = ", "=", ": "] {
        if let Some(idx) = line.find(sep) {
            let prefix = &line[..idx + sep.len()];
            return format!("{prefix}[REDACTED]");
        }
    }
    "[REDACTED]".to_string()
}
```

Modify `crates/client-core/src/lib.rs`:

```rust
pub mod transform;
```

**Step 4: Run tests**

Run:

```bash
cargo test -p cinchcli-core transform::
```

Expected: all transform tests pass.

**Step 5: Commit**

Run:

```bash
git add crates/client-core/src/lib.rs crates/client-core/src/transform.rs
git commit -m "feat(core): add deterministic clip transforms"
```

---

### Task 2: Add CLI Transform Command

**Files:**
- Modify: `crates/cli/src/commands/clip.rs`
- Create: `crates/cli/src/commands/transform.rs`
- Modify: `crates/cli/src/commands/mod.rs`
- Test: `crates/cli/src/commands/transform.rs`

**Step 1: Write the failing command tests**

Create tests around pure helper functions, not Clap integration:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use client_core::store::{
        models::{StoredClip, SyncState},
        queries, Store,
    };
    use std::path::Path;

    fn store_with_clip(id: &str, content: &[u8], content_type: &str) -> Store {
        let store = Store::open(Path::new(":memory:")).unwrap();
        queries::insert_clip(
            &store,
            &StoredClip {
                id: id.to_string(),
                source: "local".to_string(),
                source_key: None,
                content_type: content_type.to_string(),
                content: Some(content.to_vec()),
                media_path: None,
                byte_size: content.len() as i64,
                created_at: 1,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Local,
            },
        )
        .unwrap();
        store
    }

    #[test]
    fn transform_clip_by_prefix_returns_text() {
        let store = store_with_clip("01HXABCDEFGHABCDEFGHABCD", br#"{"a":1}"#, "json");
        let out = transform_clip_from_store(&store, "01HX", "pretty-json").unwrap();
        assert_eq!(out, "{\n  \"a\": 1\n}");
    }

    #[test]
    fn transform_clip_rejects_image() {
        let store = store_with_clip("01HXABCDEFGHABCDEFGHABCD", b"png", "image");
        let err = transform_clip_from_store(&store, "01HX", "trim-whitespace").unwrap_err();
        assert!(err.to_string().contains("unsupported content type"));
    }
}
```

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p cinch-cli commands::transform
```

Expected: compile failure because the module does not exist.

**Step 3: Implement `commands/transform.rs`**

Define Clap args:

```rust
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Clip id or unique id prefix.
    pub clip: Option<String>,
    /// Transform action id, e.g. pretty-json or redact-secrets.
    #[arg(long, short = 'a')]
    pub action: Option<String>,
    /// List available transform actions.
    #[arg(long)]
    pub list_actions: bool,
    /// Copy transformed output to the system clipboard instead of stdout.
    #[arg(long)]
    pub copy: bool,
}
```

Implement pure helper:

```rust
pub(crate) fn transform_clip_from_store(
    store: &client_core::store::Store,
    clip_prefix: &str,
    action_id: &str,
) -> Result<String, crate::exit::ExitError> {
    let id = client_core::store::prefix::resolve_clip_prefix(store, clip_prefix)
        .map_err(|e| crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, e.to_string(), ""))?;
    let clip = client_core::store::queries::get_clip(store, &id)
        .map_err(|e| crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, e.to_string(), ""))?
        .ok_or_else(|| crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, "clip not found", ""))?;
    let content = clip
        .content
        .as_deref()
        .ok_or_else(|| crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, "clip has no text content", ""))?;
    let text = std::str::from_utf8(content)
        .map_err(|_| crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, "clip content is not valid UTF-8", ""))?;
    let action = client_core::transform::TransformAction::from_id(action_id)
        .ok_or_else(|| crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, format!("unknown transform action: {action_id}"), ""))?;
    client_core::transform::apply_transform(action, text, &clip.content_type)
        .map_err(|e| crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, e.to_string(), ""))
}
```

Implement `run`:

```rust
pub async fn run(args: Args) -> Result<(), crate::exit::ExitError> {
    if args.list_actions {
        for a in client_core::transform::list_transform_actions("text") {
            println!("{}\t{}", a.id, a.label);
        }
        return Ok(());
    }

    let clip = args.clip.as_deref().ok_or_else(|| {
        crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, "missing clip id", "")
    })?;
    let action = args.action.as_deref().ok_or_else(|| {
        crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, "missing --action", "")
    })?;

    let ctx = crate::runtime::open_ctx().map_err(|_| {
        crate::exit::ExitError::new(
            crate::exit::AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login",
        )
    })?;
    crate::runtime::opportunistic_backfill(&ctx).await;

    let out = transform_clip_from_store(&ctx.store, clip, action)?;
    if args.copy {
        copy_text_to_clipboard(&out)?;
    } else {
        print!("{out}");
    }
    Ok(())
}
```

Add local clipboard helper using `arboard`:

```rust
fn copy_text_to_clipboard(text: &str) -> Result<(), crate::exit::ExitError> {
    let mut cb = arboard::Clipboard::new().map_err(|e| {
        crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, format!("could not open clipboard: {e}"), "")
    })?;
    cb.set_text(text).map_err(|e| {
        crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, format!("clipboard write failed: {e}"), "")
    })
}
```

Wire module:

```rust
// crates/cli/src/commands/mod.rs
pub mod transform;
```

Wire subcommand under `cinch clip`:

```rust
Transform(crate::commands::transform::Args),
```

and dispatch:

```rust
Cmd::Transform(a) => crate::commands::transform::run(a).await,
```

**Step 4: Run tests**

Run:

```bash
cargo test -p cinch-cli commands::transform
cargo run -p cinch-cli -- clip transform --list-actions
```

Expected: tests pass; list command prints action ids and labels.

**Step 5: Commit**

Run:

```bash
git add crates/cli/src/commands/clip.rs crates/cli/src/commands/mod.rs crates/cli/src/commands/transform.rs
git commit -m "feat(cli): add clip transform command"
```

---

### Task 3: Add Desktop Tauri Transform Commands

**Files:**
- Create: `apps/desktop/src-tauri/src/commands/clips/transform.rs`
- Modify: `apps/desktop/src-tauri/src/commands/clips/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Generated: `apps/desktop/src/bindings.ts`
- Test: `apps/desktop/src-tauri/src/commands/clips/transform.rs`

**Step 1: Write failing Rust command tests**

Create tests around an inner function:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use client_core::store::{
        models::{StoredClip, SyncState},
        queries, Store,
    };
    use std::path::Path;

    fn store_with_clip(content: &[u8], content_type: &str) -> Store {
        let store = Store::open(Path::new(":memory:")).unwrap();
        queries::insert_clip(
            &store,
            &StoredClip {
                id: "01HXABCDEFGHABCDEFGHABCD".to_string(),
                source: "local".to_string(),
                source_key: None,
                content_type: content_type.to_string(),
                content: Some(content.to_vec()),
                media_path: None,
                byte_size: content.len() as i64,
                created_at: 1,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Local,
            },
        )
        .unwrap();
        store
    }

    #[test]
    fn transform_clip_inner_returns_label_and_text() {
        let store = store_with_clip(br#"{"a":1}"#, "json");
        let result = transform_clip_inner(&store, "01HXABCDEFGHABCDEFGHABCD", "pretty-json").unwrap();
        assert_eq!(result.label, "Pretty JSON");
        assert_eq!(result.content, "{\n  \"a\": 1\n}");
    }
}
```

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p cinch-desktop commands::clips::transform
```

Expected: compile failure because the module does not exist.

**Step 3: Implement Tauri command module**

Define Specta-returned types:

```rust
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::State;

use crate::clipboard::ClipboardService;
use crate::SharedStore;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TransformActionDto {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TransformCopyResult {
    pub action_id: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TransformPreview {
    pub action_id: String,
    pub label: String,
    pub content: String,
}
```

Implement commands:

```rust
#[tauri::command]
#[specta::specta]
pub fn list_transform_actions(content_type: String) -> Result<Vec<TransformActionDto>, String> {
    Ok(client_core::transform::list_transform_actions(&content_type)
        .into_iter()
        .map(|a| TransformActionDto { id: a.id, label: a.label })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub fn copy_transformed_clip_to_clipboard(
    store: State<'_, SharedStore>,
    clipboard: State<'_, Arc<ClipboardService>>,
    clip_id: String,
    action_id: String,
) -> Result<TransformCopyResult, String> {
    let preview = transform_clip_inner(&store, &clip_id, &action_id)?;
    clipboard.write_text(&preview.content).map_err(|e| e.to_string())?;
    Ok(TransformCopyResult {
        action_id: preview.action_id,
        label: preview.label,
    })
}
```

Implement inner helper:

```rust
pub(crate) fn transform_clip_inner(
    store: &client_core::store::Store,
    clip_id: &str,
    action_id: &str,
) -> Result<TransformPreview, String> {
    let clip = client_core::store::queries::get_clip(store, clip_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("clip {clip_id} not found"))?;
    let bytes = clip.content.as_deref().ok_or("clip has no text content")?;
    let text = std::str::from_utf8(bytes).map_err(|_| "clip content is not valid UTF-8")?;
    let action = client_core::transform::TransformAction::from_id(action_id)
        .ok_or_else(|| format!("unknown transform action: {action_id}"))?;
    let content = client_core::transform::apply_transform(action, text, &clip.content_type)
        .map_err(|e| e.to_string())?;
    Ok(TransformPreview {
        action_id: action.id().to_string(),
        label: action.label().to_string(),
        content,
    })
}
```

Wire exports:

```rust
// apps/desktop/src-tauri/src/commands/clips/mod.rs
mod transform;
pub use transform::*;
```

Wire Specta commands in `apps/desktop/src-tauri/src/lib.rs`:

```rust
commands::clips::list_transform_actions,
commands::clips::copy_transformed_clip_to_clipboard,
```

Regenerate bindings:

```bash
make generate
```

**Step 4: Run tests**

Run:

```bash
cargo test -p cinch-desktop commands::clips::transform
make generate
```

Expected: Rust tests pass and `apps/desktop/src/bindings.ts` includes both new commands and DTOs.

**Step 5: Commit**

Run:

```bash
git add apps/desktop/src-tauri/src/commands/clips/transform.rs apps/desktop/src-tauri/src/commands/clips/mod.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src/bindings.ts
git commit -m "feat(desktop): expose transform copy commands"
```

---

### Task 4: Add Desktop Transform Menu UI

**Files:**
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/components/ClipDetail.tsx`
- Test: `apps/desktop/src/components/ClipDetail.test.tsx`
- Test: `apps/desktop/src/App.test.tsx`

**Step 1: Write failing UI tests**

In `ClipDetail.test.tsx`, add:

```tsx
it('shows transform menu for text clips', () => {
  render(
    <ClipDetail
      clip={textClip}
      onCopy={() => {}}
      onCopyTransform={() => {}}
      transformActions={[{ id: 'pretty-json', label: 'Pretty JSON' }]}
      onPin={() => {}}
      onDelete={() => {}}
      onSaveImage={() => {}}
    />
  );
  expect(screen.getByRole('button', { name: /transform copy/i })).toBeInTheDocument();
});

it('does not show transform menu for image clips', () => {
  render(
    <ClipDetail
      clip={imageClip}
      onCopy={() => {}}
      onCopyTransform={() => {}}
      transformActions={[{ id: 'pretty-json', label: 'Pretty JSON' }]}
      onPin={() => {}}
      onDelete={() => {}}
      onSaveImage={() => {}}
    />
  );
  expect(screen.queryByRole('button', { name: /transform copy/i })).not.toBeInTheDocument();
});
```

**Step 2: Run tests to verify failure**

Run:

```bash
cd apps/desktop
pnpm test -- ClipDetail.test.tsx
```

Expected: TypeScript/React test failure because props and UI do not exist.

**Step 3: Implement UI props and menu**

Add a local type in `ClipDetail.tsx`:

```tsx
interface TransformAction {
  id: string;
  label: string;
}
```

Extend props:

```tsx
onCopyTransform: (clip: LocalClip, actionId: string) => void;
transformActions: TransformAction[];
```

Render next to Copy for non-image clips:

```tsx
{!isImage && transformActions.length > 0 && (
  <select
    aria-label="Transform copy"
    onChange={(e) => {
      const actionId = e.currentTarget.value;
      e.currentTarget.value = "";
      if (actionId) onCopyTransform(clip, actionId);
    }}
    className="btn-ghost"
    style={S.transformSelect}
    defaultValue=""
  >
    <option value="" disabled>Transform</option>
    {transformActions.map((a) => (
      <option key={a.id} value={a.id}>{a.label}</option>
    ))}
  </select>
)}
```

Add compact style:

```tsx
transformSelect: {
  ...S.btnGhost,
  maxWidth: 180,
}
```

**Step 4: Wire App handlers**

In `App.tsx`, add state:

```tsx
const [transformActions, setTransformActions] = useState<{ id: string; label: string }[]>([]);
```

Load once:

```tsx
useEffect(() => {
  unwrap(commands.listTransformActions('text'))
    .then(setTransformActions)
    .catch((e) => console.error('failed to load transform actions:', e));
}, []);
```

Add handler:

```tsx
const copyTransformedClip = useCallback(async (clip: LocalClip, actionId: string) => {
  try {
    const result = await unwrap(commands.copyTransformedClipToClipboard(clip.id, actionId));
    showToast(`Copied as ${result.label}`, 'copy');
    setSearchQuery('');
    setDebouncedQuery('');
    setSelectedClip(null);
    void unwrap(commands.markClipCopied(clip.id))
      .then(refreshClips)
      .catch((e) => console.error('failed to mark clip copied:', e));
    void commands.focusPreviousApp();
  } catch (e) {
    console.error('transform copy failed:', e);
    showToast(e instanceof Error ? e.message : 'Transform failed', 'error');
  }
}, [showToast, refreshClips]);
```

Pass props to `ClipDetail`:

```tsx
onCopyTransform={copyTransformedClip}
transformActions={transformActions}
```

**Step 5: Run UI tests**

Run:

```bash
cd apps/desktop
pnpm test -- ClipDetail.test.tsx App.test.tsx
pnpm build
```

Expected: tests and TypeScript build pass.

**Step 6: Commit**

Run:

```bash
git add apps/desktop/src/App.tsx apps/desktop/src/components/ClipDetail.tsx apps/desktop/src/components/ClipDetail.test.tsx apps/desktop/src/App.test.tsx
git commit -m "feat(desktop): add transform copy menu"
```

---

### Task 5: End-To-End Verification And Polish

**Files:**
- Modify if needed: `README.md`
- Modify if needed: `apps/desktop/README.md`

**Step 1: Run focused Rust tests**

Run:

```bash
cargo test -p cinchcli-core transform::
cargo test -p cinch-cli commands::transform
cargo test -p cinch-desktop commands::clips::transform
```

Expected: all pass.

**Step 2: Run frontend tests and build**

Run:

```bash
cd apps/desktop
pnpm test -- ClipDetail.test.tsx App.test.tsx
pnpm build
```

Expected: all pass.

**Step 3: Run CLI smoke checks**

Run:

```bash
cargo run -p cinch-cli -- clip transform --list-actions
```

Expected: action ids include `pretty-json`, `redact-secrets`, and `shell-single-quote`.

If a local store has a known clip:

```bash
cargo run -p cinch-cli -- clip transform <clip-prefix> --action trim-whitespace
```

Expected: transformed text prints to stdout.

**Step 4: Run formatting and linting**

Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both pass.

**Step 5: Update docs only if CLI help is insufficient**

If README needs a short mention, add:

```markdown
### Transform a saved clip

```bash
cinch clip transform <clip-prefix> --action pretty-json
cinch clip transform <clip-prefix> --action redact-secrets --copy
```
```

**Step 6: Commit**

Run:

```bash
git add README.md apps/desktop/README.md
git commit -m "docs: document transform copy actions"
```

Skip this commit if no docs changed.

---

### Task 6: Final Review

**Files:**
- Review all changed files.

**Step 1: Check status**

Run:

```bash
git status --short
git log --oneline --max-count=6
```

Expected: working tree clean and recent commits match the task commits.

**Step 2: Inspect diff against main**

Run:

```bash
git diff main...HEAD --stat
git diff main...HEAD -- crates/client-core/src/transform.rs crates/cli/src/commands/transform.rs apps/desktop/src-tauri/src/commands/clips/transform.rs
```

Expected: transform code is shared, Desktop and CLI call into `client-core`, and no relay/proto/generated Go files changed.

**Step 3: Commit any final fixes**

If verification revealed fixes:

```bash
git add <fixed-files>
git commit -m "fix: polish transform copy actions"
```

Expected: clean branch ready for review.
