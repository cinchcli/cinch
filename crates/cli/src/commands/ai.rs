//! `cinch ai` — explicit AI workflows over terminal or clipboard context.
//!
//! Privacy boundary: this module is the only CLI surface that calls an AI
//! provider. `--no-send` only assembles the prompt and returns before provider
//! resolution.

use std::io::{IsTerminal, Read};
use std::path::Path;

use clap::ValueEnum;
use client_core::store::models::StoredClip;
use serde_json::{json, Value};

use crate::exit::{ExitError, GENERIC_ERROR, NETWORK_ERROR, RELAY_ERROR};
use crate::io::{copy_text_to_clipboard, write_to_stdout};

const MAX_CONTEXT_CHARS: usize = 120_000;
const OPENAI_DEFAULT_MODEL: &str = "gpt-4.1-mini";
const HOSTED_DEFAULT_MODEL: &str = "cinch-fix-v1";

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// Turn terminal/log/error context into a fix-oriented AI prompt or answer.
    Fix(FixArgs),
}

#[derive(Debug, clap::Args)]
pub struct FixArgs {
    /// `latest` or a clip ID prefix. Omit when piping stdin.
    pub input: Option<String>,

    /// AI provider to call. Without provider config, v1 prints the prompt only.
    #[arg(long, value_enum)]
    pub provider: Option<ProviderKind>,

    /// Provider model override.
    #[arg(long)]
    pub model: Option<String>,

    /// Copy the prompt or provider answer to the system clipboard.
    #[arg(long)]
    pub copy: bool,

    /// Print the assembled prompt and never call an AI provider.
    #[arg(long)]
    pub no_send: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// Cinch-operated managed provider. Requires operator-hosted endpoint config.
    HostedBedrock,
    /// User AWS Bedrock credentials. Boundary documented; not wired in this binary.
    BedrockByok,
    /// OpenAI-compatible /v1/chat/completions endpoint.
    OpenaiCompatible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FixContext {
    source: String,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProviderResolution {
    Configured(ProviderKind),
    NoConfiguredProvider,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::Fix(args) => run_fix(args).await,
    }
}

async fn run_fix(args: FixArgs) -> Result<(), ExitError> {
    let context = load_fix_context(args.input.as_deref())?;
    let prompt = build_fix_prompt(&context);

    if args.no_send {
        write_output(&prompt, args.copy)?;
        return Ok(());
    }

    match resolve_provider(args.provider)? {
        ProviderResolution::NoConfiguredProvider => {
            eprintln!(
                "No AI provider configured; printing the prompt only. To send explicitly, set CINCH_AI_PROVIDER=openai-compatible with CINCH_AI_BASE_URL, or pass --no-send."
            );
            write_output(&prompt, args.copy)?;
            Ok(())
        }
        ProviderResolution::Configured(provider) => {
            let answer = call_provider(provider, args.model.as_deref(), &prompt)
                .await
                .map_err(|err| provider_error_with_prompt(err, &prompt))?;
            write_output(&answer, args.copy)
        }
    }
}

fn load_fix_context(arg: Option<&str>) -> Result<FixContext, ExitError> {
    if let Some(text) = read_stdin_if_available()? {
        return Ok(FixContext {
            source: "stdin".to_string(),
            text,
        });
    }

    let selector = arg.ok_or_else(|| {
        ExitError::new(
            GENERIC_ERROR,
            "No input for ai fix.",
            "Pipe terminal output (`cat error.log | cinch ai fix`) or pass `latest` / a clip ID prefix.",
        )
    })?;

    let store_path = client_core::store::default_db_path()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store path: {e}"), ""))?;
    let store = client_core::store::Store::open(&store_path)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
    context_from_store(&store, selector)
}

fn read_stdin_if_available() -> Result<Option<String>, ExitError> {
    if std::io::stdin().is_terminal() {
        return Ok(None);
    }

    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Cannot read stdin: {e}"), ""))?;

    if input.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(input))
    }
}

pub(crate) fn context_from_store(
    store: &client_core::store::Store,
    selector: &str,
) -> Result<FixContext, ExitError> {
    let clip = if selector == "latest" {
        let mut rows = client_core::store::queries::list_clips(
            store,
            None,
            None,
            Some(1),
            Some(0),
            None,
            false,
            1,
        )
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
        rows.pop().ok_or_else(|| {
            ExitError::new(
                GENERIC_ERROR,
                "No clips found in local history.",
                "Pipe input to `cinch ai fix` or push/copy a clip first.",
            )
        })?
    } else {
        let id = client_core::store::prefix::resolve_clip_id(store, selector)
            .map_err(crate::commands::get::render_resolve_error)?;
        client_core::store::queries::get_clip(store, &id)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?
            .ok_or_else(|| ExitError::new(GENERIC_ERROR, "clip vanished after resolution", ""))?
    };

    let text = clip_text(&clip)?;
    Ok(FixContext {
        source: format!("clip:{}", clip.id),
        text,
    })
}

fn clip_text(clip: &StoredClip) -> Result<String, ExitError> {
    let bytes = if let Some(content) = &clip.content {
        content.clone()
    } else if let Some(media_path) = &clip.media_path {
        let abs = client_core::store::default_media_root()
            .map_err(|e| ExitError::new(GENERIC_ERROR, e.to_string(), ""))?
            .join(Path::new(media_path));
        std::fs::read(&abs)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("read media: {e}"), ""))?
    } else {
        Vec::new()
    };

    String::from_utf8(bytes).map_err(|_| {
        ExitError::new(
            GENERIC_ERROR,
            "Selected clip is not valid UTF-8 text.",
            "Use `cinch ai fix` with terminal/log/error text.",
        )
    })
}

pub(crate) fn build_fix_prompt(context: &FixContext) -> String {
    let (body, truncated) = truncate_context(&context.text);
    let truncated_note = if truncated {
        "\nNote: the context was truncated to the most recent 120000 characters."
    } else {
        ""
    };

    format!(
        "You are helping debug terminal, log, or error output captured by Cinch.\n\n\
Task:\n\
- Identify the most likely root cause.\n\
- Give the shortest practical fix first.\n\
- Include exact commands or file edits when they are inferable.\n\
- Call out missing information instead of inventing facts.\n\
- Keep the answer concise and action-oriented.\n\n\
Source: {source}{truncated_note}\n\n\
Context:\n```text\n{body}\n```\n",
        source = context.source,
        body = body,
        truncated_note = truncated_note,
    )
}

fn truncate_context(input: &str) -> (String, bool) {
    if input.chars().count() <= MAX_CONTEXT_CHARS {
        return (input.to_string(), false);
    }

    let truncated: String = input
        .chars()
        .rev()
        .take(MAX_CONTEXT_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    (truncated, true)
}

fn resolve_provider(explicit: Option<ProviderKind>) -> Result<ProviderResolution, ExitError> {
    if let Some(provider) = explicit {
        return Ok(ProviderResolution::Configured(provider));
    }

    if let Ok(raw) = std::env::var("CINCH_AI_PROVIDER") {
        if !raw.trim().is_empty() {
            return parse_provider(&raw).map(ProviderResolution::Configured);
        }
    }

    if has_env("CINCH_AI_BASE_URL") || has_env("OPENAI_BASE_URL") {
        return Ok(ProviderResolution::Configured(
            ProviderKind::OpenaiCompatible,
        ));
    }

    if has_env("CINCH_AI_HOSTED_URL") {
        return Ok(ProviderResolution::Configured(ProviderKind::HostedBedrock));
    }

    Ok(ProviderResolution::NoConfiguredProvider)
}

fn has_env(name: &str) -> bool {
    std::env::var(name)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn parse_provider(raw: &str) -> Result<ProviderKind, ExitError> {
    match raw.trim() {
        "hosted-bedrock" => Ok(ProviderKind::HostedBedrock),
        "bedrock-byok" => Ok(ProviderKind::BedrockByok),
        "openai-compatible" => Ok(ProviderKind::OpenaiCompatible),
        other => Err(ExitError::new(
            GENERIC_ERROR,
            format!("Unknown AI provider: {other}"),
            "Use hosted-bedrock, bedrock-byok, or openai-compatible.",
        )),
    }
}

async fn call_provider(
    provider: ProviderKind,
    model: Option<&str>,
    prompt: &str,
) -> Result<String, ProviderCallError> {
    match provider {
        ProviderKind::HostedBedrock => call_hosted_bedrock(model, prompt).await,
        ProviderKind::BedrockByok => Err(ProviderCallError::Configuration(
            "bedrock-byok is a documented provider boundary in v1, but this binary does not sign AWS Bedrock requests yet. Use hosted-bedrock or openai-compatible, or implement a provider adapter.".to_string(),
        )),
        ProviderKind::OpenaiCompatible => call_openai_compatible(model, prompt).await,
    }
}

#[derive(Debug)]
enum ProviderCallError {
    Configuration(String),
    Network(String),
    Response(String),
}

fn provider_error_with_prompt(err: ProviderCallError, prompt: &str) -> ExitError {
    eprintln!("AI provider failed before producing an answer. Assembled prompt follows:");
    eprintln!("```text\n{prompt}\n```");

    match err {
        ProviderCallError::Configuration(message) => ExitError::new(
            GENERIC_ERROR,
            message,
            "Run again with --no-send to print only the prompt.",
        ),
        ProviderCallError::Network(message) => ExitError::new(
            NETWORK_ERROR,
            format!("AI provider network error: {message}"),
            "Check the provider endpoint or run with --no-send.",
        ),
        ProviderCallError::Response(message) => ExitError::new(
            RELAY_ERROR,
            format!("AI provider response error: {message}"),
            "Check provider logs or run with --no-send.",
        ),
    }
}

async fn call_hosted_bedrock(
    model: Option<&str>,
    prompt: &str,
) -> Result<String, ProviderCallError> {
    let endpoint = env_required(
        "CINCH_AI_HOSTED_URL",
        "set CINCH_AI_HOSTED_URL to the hosted AI endpoint",
    )?;
    let model = model
        .map(str::to_string)
        .or_else(|| std::env::var("CINCH_AI_MODEL").ok())
        .unwrap_or_else(|| HOSTED_DEFAULT_MODEL.to_string());
    let token = std::env::var("CINCH_AI_HOSTED_TOKEN").ok();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| ProviderCallError::Configuration(format!("HTTP client: {e}")))?;

    let mut req = client.post(endpoint.trim()).json(&json!({
        "workflow": "fix",
        "model": model,
        "prompt": prompt,
    }));
    if let Some(token) = token.filter(|v| !v.trim().is_empty()) {
        req = req.bearer_auth(token);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| ProviderCallError::Network(e.to_string()))?;
    parse_provider_response(resp).await
}

async fn call_openai_compatible(
    model: Option<&str>,
    prompt: &str,
) -> Result<String, ProviderCallError> {
    let base = std::env::var("CINCH_AI_BASE_URL")
        .or_else(|_| std::env::var("OPENAI_BASE_URL"))
        .map_err(|_| {
            ProviderCallError::Configuration(
                "openai-compatible requires CINCH_AI_BASE_URL or OPENAI_BASE_URL, for example http://localhost:11434/v1".to_string(),
            )
        })?;
    let endpoint = chat_completions_endpoint(&base);
    let model = model
        .map(str::to_string)
        .or_else(|| std::env::var("CINCH_AI_MODEL").ok())
        .or_else(|| std::env::var("OPENAI_MODEL").ok())
        .unwrap_or_else(|| OPENAI_DEFAULT_MODEL.to_string());
    let api_key = std::env::var("CINCH_AI_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .ok();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| ProviderCallError::Configuration(format!("HTTP client: {e}")))?;

    let mut req = client.post(endpoint).json(&json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": "You are a concise senior engineer helping fix terminal and log errors."
            },
            { "role": "user", "content": prompt }
        ],
        "temperature": 0.2
    }));
    if let Some(api_key) = api_key.filter(|v| !v.trim().is_empty()) {
        req = req.bearer_auth(api_key);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| ProviderCallError::Network(e.to_string()))?;
    let status = resp.status();
    let value: Value = resp
        .json()
        .await
        .map_err(|e| ProviderCallError::Response(format!("invalid JSON: {e}")))?;

    if !status.is_success() {
        return Err(ProviderCallError::Response(format!(
            "HTTP {status}: {}",
            compact_json(&value)
        )));
    }

    value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            ProviderCallError::Response("missing choices[0].message.content".to_string())
        })
}

fn env_required(name: &str, fix: &str) -> Result<String, ProviderCallError> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| ProviderCallError::Configuration(fix.to_string()))
}

fn chat_completions_endpoint(base: &str) -> String {
    let trimmed = base.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

async fn parse_provider_response(resp: reqwest::Response) -> Result<String, ProviderCallError> {
    let status = resp.status();
    let value: Value = resp
        .json()
        .await
        .map_err(|e| ProviderCallError::Response(format!("invalid JSON: {e}")))?;

    if !status.is_success() {
        return Err(ProviderCallError::Response(format!(
            "HTTP {status}: {}",
            compact_json(&value)
        )));
    }

    for key in ["output", "text", "message"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Ok(text.to_string());
        }
    }
    Err(ProviderCallError::Response(
        "missing output/text/message field".to_string(),
    ))
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unprintable JSON>".to_string())
}

fn write_output(text: &str, copy: bool) -> Result<(), ExitError> {
    if copy {
        // Best-effort copy — never abort, so the answer always reaches stdout.
        copy_text_to_clipboard(text);
    }
    write_to_stdout(text.as_bytes())?;
    if !text.ends_with('\n') {
        write_to_stdout(b"\n")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use client_core::store::{
        models::{StoredClip, SyncState},
        queries, Store,
    };

    #[derive(Debug, Parser)]
    #[command(no_binary_name = true)]
    struct AiHarness {
        #[command(flatten)]
        args: Args,
    }

    fn store_with_clip(id: &str, content: &[u8], created_at: i64) -> Store {
        let store = Store::open(Path::new(":memory:")).unwrap();
        queries::insert_clip(
            &store,
            &StoredClip {
                id: id.to_string(),
                source: "local".to_string(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: "text".to_string(),
                content: Some(content.to_vec()),
                media_path: None,
                byte_size: content.len() as i64,
                created_at,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Local,
            },
        )
        .unwrap();
        store
    }

    #[test]
    fn parses_fix_no_send_with_provider_flags() {
        let harness = AiHarness::try_parse_from([
            "fix",
            "latest",
            "--provider",
            "openai-compatible",
            "--model",
            "llama3.1",
            "--no-send",
        ])
        .expect("parse ok");

        let Cmd::Fix(args) = harness.args.cmd;
        assert_eq!(args.input.as_deref(), Some("latest"));
        assert_eq!(args.provider, Some(ProviderKind::OpenaiCompatible));
        assert_eq!(args.model.as_deref(), Some("llama3.1"));
        assert!(args.no_send);
    }

    #[test]
    fn prompt_is_fix_oriented_and_keeps_context() {
        let ctx = FixContext {
            source: "stdin".into(),
            text: "error[E0425]: cannot find value `foo` in this scope".into(),
        };
        let prompt = build_fix_prompt(&ctx);
        assert!(prompt.contains("most likely root cause"));
        assert!(prompt.contains("error[E0425]"));
        assert!(prompt.contains("Source: stdin"));
    }

    #[test]
    fn latest_reads_newest_clip_from_store() {
        let store = store_with_clip("01HXAAAAAAAAAAAAAAAAAAAAAA", b"old", 1);
        queries::insert_clip(
            &store,
            &StoredClip {
                id: "01HXBBBBBBBBBBBBBBBBBBBBBB".to_string(),
                source: "local".to_string(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: "text".to_string(),
                content: Some(b"new".to_vec()),
                media_path: None,
                byte_size: 3,
                created_at: 2,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Local,
            },
        )
        .unwrap();

        let ctx = context_from_store(&store, "latest").unwrap();
        assert_eq!(ctx.text, "new");
        assert!(ctx.source.starts_with("clip:01HXBBBB"));
    }

    #[test]
    fn prefix_reads_matching_clip_from_store() {
        let store = store_with_clip("01HXCCCCCCCCCCCCCCCCCCCCCC", b"panic here", 1);
        let ctx = context_from_store(&store, "01HXCCCC").unwrap();
        assert_eq!(ctx.text, "panic here");
    }

    #[test]
    fn chat_endpoint_accepts_base_or_full_path() {
        assert_eq!(
            chat_completions_endpoint("http://localhost:11434/v1"),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_endpoint("http://x/v1/chat/completions"),
            "http://x/v1/chat/completions"
        );
    }
}
