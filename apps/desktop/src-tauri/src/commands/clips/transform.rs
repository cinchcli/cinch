use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::State;

use crate::clipboard::ClipboardService;
use crate::SharedStore;
use client_core::store::queries;

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

#[tauri::command]
#[specta::specta]
pub fn list_transform_actions(content_type: String) -> Result<Vec<TransformActionDto>, String> {
    Ok(
        client_core::transform::list_transform_actions(&content_type)
            .into_iter()
            .map(|action| TransformActionDto {
                id: action.id.to_string(),
                label: action.label.to_string(),
            })
            .collect(),
    )
}

#[tauri::command]
#[specta::specta]
pub fn copy_transformed_clip_to_clipboard(
    store: State<'_, SharedStore>,
    clipboard: State<'_, Arc<ClipboardService>>,
    clip_id: String,
    action_id: String,
) -> Result<TransformCopyResult, String> {
    let preview = transform_clip_inner(store.inner(), &clip_id, &action_id)?;
    clipboard
        .write_text(&preview.content)
        .map_err(|e| e.to_string())?;
    Ok(TransformCopyResult {
        action_id: preview.action_id,
        label: preview.label,
    })
}

pub(crate) fn transform_clip_inner(
    store: &client_core::store::Store,
    clip_id: &str,
    action_id: &str,
) -> Result<TransformPreview, String> {
    let clip = queries::get_clip(store, clip_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("clip {clip_id} not found"))?;
    let bytes = clip
        .content
        .as_deref()
        .ok_or_else(|| "clip has no text content".to_string())?;
    let text =
        std::str::from_utf8(bytes).map_err(|_| "clip content is not valid UTF-8".to_string())?;
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
        let result =
            transform_clip_inner(&store, "01HXABCDEFGHABCDEFGHABCD", "pretty-json").unwrap();
        assert_eq!(result.label, "Pretty JSON");
        assert_eq!(result.content, "{\n  \"a\": 1\n}");
    }
}
