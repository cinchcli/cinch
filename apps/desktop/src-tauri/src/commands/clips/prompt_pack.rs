use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::State;

use crate::clipboard::ClipboardService;
use crate::commands::clips::normalize_content_type;
use crate::SharedStore;
use client_core::store::{models::StoredClip, queries};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PromptRecipeDto {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PromptPackResult {
    pub recipe_id: String,
    pub label: String,
    pub clip_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PromptRecipe {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    task: &'static str,
    output_requirements: &'static str,
}

const RECIPES: [PromptRecipe; 6] = [
    PromptRecipe {
        id: "summarize-actions",
        label: "Summarize + Actions",
        description: "Summarize the context and extract concrete next steps.",
        task: "Summarize the context clearly and extract the concrete action items.",
        output_requirements:
            "Return concise bullets. Separate Summary, Decisions, Action Items, and Open Questions.",
    },
    PromptRecipe {
        id: "better-final-answer",
        label: "Better Final Answer",
        description: "Combine the selected context into one stronger answer.",
        task: "Synthesize the selected context into the strongest final answer.",
        output_requirements:
            "Preserve the best ideas, remove duplication, resolve contradictions, and write the final answer directly.",
    },
    PromptRecipe {
        id: "html-mockup",
        label: "HTML Mockup",
        description: "Turn the context into a self-contained visual HTML mockup.",
        task: "Turn the context into a polished, self-contained HTML mockup.",
        output_requirements:
            "Output one complete HTML file. Use inline CSS, realistic copy, responsive layout, and no external dependencies.",
    },
    PromptRecipe {
        id: "comparison-table",
        label: "Comparison Table",
        description: "Compare options, tradeoffs, risks, and recommendations.",
        task: "Compare the selected context and identify the meaningful differences.",
        output_requirements:
            "Return a table with criteria, option-by-option assessment, risks, and a clear recommendation.",
    },
    PromptRecipe {
        id: "polite-email-reply",
        label: "Polite Email Reply",
        description: "Draft a clear, respectful reply using the selected context.",
        task: "Draft a clear and respectful email reply using the selected context.",
        output_requirements:
            "Keep it concise, specific, and natural. Include a subject line if useful.",
    },
    PromptRecipe {
        id: "clearer-shorter",
        label: "Clearer + Shorter",
        description: "Rewrite the context so it is simpler and easier to scan.",
        task: "Rewrite the context to be clearer, shorter, and easier to scan.",
        output_requirements:
            "Keep the original meaning. Remove filler. Prefer plain language and useful structure.",
    },
];

#[tauri::command]
#[specta::specta]
pub fn list_prompt_recipes() -> Result<Vec<PromptRecipeDto>, String> {
    Ok(RECIPES
        .iter()
        .map(|recipe| PromptRecipeDto {
            id: recipe.id.to_string(),
            label: recipe.label.to_string(),
            description: recipe.description.to_string(),
        })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub fn copy_prompt_pack_to_clipboard(
    store: State<'_, SharedStore>,
    clipboard: State<'_, Arc<ClipboardService>>,
    primary_clip_id: String,
    context_clip_ids: Vec<String>,
    recipe_id: String,
) -> Result<PromptPackResult, String> {
    let built = build_prompt_pack_inner(
        store.inner(),
        &primary_clip_id,
        &context_clip_ids,
        &recipe_id,
    )?;
    clipboard
        .write_text(&built.content)
        .map_err(|e| e.to_string())?;
    Ok(PromptPackResult {
        recipe_id: built.recipe_id,
        label: built.label,
        clip_count: built.clip_count,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuiltPromptPack {
    pub recipe_id: String,
    pub label: String,
    pub clip_count: usize,
    pub content: String,
}

pub(crate) fn build_prompt_pack_inner(
    store: &client_core::store::Store,
    primary_clip_id: &str,
    context_clip_ids: &[String],
    recipe_id: &str,
) -> Result<BuiltPromptPack, String> {
    let recipe =
        recipe_by_id(recipe_id).ok_or_else(|| format!("unknown prompt recipe: {recipe_id}"))?;

    let mut clips = Vec::with_capacity(context_clip_ids.len() + 1);
    let primary = load_text_clip(store, primary_clip_id)?;
    clips.push(("Primary".to_string(), primary));

    for clip_id in context_clip_ids {
        if clip_id == primary_clip_id || clips.iter().any(|(_, c)| c.id == *clip_id) {
            continue;
        }
        let label = format!("Additional {}", clips.len());
        clips.push((label, load_text_clip(store, clip_id)?));
    }

    let mut content = String::new();
    content.push_str("Task:\n");
    content.push_str(recipe.task);
    content.push_str("\n\nContext:\n");

    for (index, (label, clip)) in clips.iter().enumerate() {
        let text = clip_text(clip)?;
        content.push_str(&format!(
            "\n[{} Clip {} | Source: {} | Type: {}]\n",
            label,
            index + 1,
            clip.source,
            normalize_content_type(clip.content_type.clone())
        ));
        let fence = markdown_fence_for(text);
        content.push_str(&fence);
        content.push_str("text\n");
        content.push_str(text.trim());
        content.push('\n');
        content.push_str(&fence);
        content.push('\n');
    }

    content.push_str("\nOutput requirements:\n");
    content.push_str(recipe.output_requirements);
    content.push('\n');

    Ok(BuiltPromptPack {
        recipe_id: recipe.id.to_string(),
        label: recipe.label.to_string(),
        clip_count: clips.len(),
        content,
    })
}

fn recipe_by_id(id: &str) -> Option<PromptRecipe> {
    RECIPES.iter().copied().find(|recipe| recipe.id == id)
}

fn load_text_clip(store: &client_core::store::Store, clip_id: &str) -> Result<StoredClip, String> {
    let clip = queries::get_clip(store, clip_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("clip {clip_id} not found"))?;
    let content_type = normalize_content_type(clip.content_type.clone());
    if content_type == "image" {
        return Err(format!(
            "clip {clip_id} is an image; prompt packs use text clips"
        ));
    }
    let text = clip_text(&clip)?;
    if text.trim().is_empty() {
        return Err(format!("clip {clip_id} has no text content"));
    }
    Ok(clip)
}

fn clip_text(clip: &StoredClip) -> Result<&str, String> {
    let bytes = clip
        .content
        .as_deref()
        .ok_or_else(|| format!("clip {} has no text content", clip.id))?;
    std::str::from_utf8(bytes).map_err(|_| format!("clip {} content is not valid UTF-8", clip.id))
}

fn markdown_fence_for(text: &str) -> String {
    let mut fence = "```".to_string();
    while text.contains(&fence) {
        fence.push('`');
    }
    fence
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_core::store::{
        models::{StoredClip, SyncState},
        queries, Store,
    };
    use std::path::Path;

    fn store_with_clips() -> Store {
        let store = Store::open(Path::new(":memory:")).unwrap();
        insert(&store, "primary", "Latest copied text", "text", 2);
        insert(&store, "ctx1", "Prior answer", "text", 1);
        store
    }

    fn insert(store: &Store, id: &str, content: &str, content_type: &str, created_at: i64) {
        queries::insert_clip(
            store,
            &StoredClip {
                id: id.to_string(),
                source: "local".to_string(),
                source_key: None,
                content_type: content_type.to_string(),
                content: Some(content.as_bytes().to_vec()),
                media_path: None,
                byte_size: content.len() as i64,
                created_at,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Local,
            },
        )
        .unwrap();
    }

    #[test]
    fn list_prompt_recipes_has_user_facing_defaults() {
        let recipes = list_prompt_recipes().unwrap();
        assert!(recipes.iter().any(|r| r.id == "better-final-answer"));
        assert!(recipes.iter().any(|r| r.id == "html-mockup"));
    }

    #[test]
    fn build_prompt_pack_uses_primary_then_context() {
        let store = store_with_clips();
        let result = build_prompt_pack_inner(
            &store,
            "primary",
            &[String::from("ctx1")],
            "better-final-answer",
        )
        .unwrap();

        assert_eq!(result.label, "Better Final Answer");
        assert_eq!(result.clip_count, 2);
        assert!(result.content.contains("Task:\nSynthesize"));
        assert!(result.content.contains("[Primary Clip 1"));
        assert!(result.content.contains("Latest copied text"));
        assert!(result.content.contains("[Additional 1 Clip 2"));
        assert!(result.content.contains("Prior answer"));
    }

    #[test]
    fn build_prompt_pack_deduplicates_primary_context() {
        let store = store_with_clips();
        let result = build_prompt_pack_inner(
            &store,
            "primary",
            &[String::from("primary"), String::from("ctx1")],
            "summarize-actions",
        )
        .unwrap();

        assert_eq!(result.clip_count, 2);
    }

    #[test]
    fn build_prompt_pack_uses_longer_fence_when_context_contains_backticks() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        insert(&store, "primary", "```text\nnested\n```", "text", 1);

        let result = build_prompt_pack_inner(&store, "primary", &[], "clearer-shorter").unwrap();

        assert!(result
            .content
            .contains("````text\n```text\nnested\n```\n````"));
    }
}
