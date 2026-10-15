use super::*;
use arcrelay_core::domain::collections::*;
use serde::Deserialize;

#[derive(Deserialize, ts_rs::TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CollectionRequest {
    List {
        after: String,
    },
    Save {
        draft: CollectionDraft,
    },
    Remove {
        id: String,
        base: CollectionVersion,
    },
    Favorite {
        id: String,
        favorite: bool,
    },
    Value {
        id: String,
        item: String,
    },
    Use {
        id: String,
        item: String,
        paste: bool,
    },
    FromHistory {
        ids: Vec<u64>,
    },
    OpenSettings,
    Retry,
    Sync {
        id: String,
    },
}
#[derive(Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionResponse {
    pub peers: Vec<crate::clipboard_sync::replication::CollectionPeerState>,
    pub records: Vec<ClipboardCollection>,
    pub saved: Option<ClipboardCollection>,
    pub value: Option<TotpCode>,
    pub items: Vec<CollectionItem>,
    pub sync_enabled: bool,
    pub sync_edits: bool,
    pub sync_favorites: bool,
}

#[arcrelay_desktop_ipc::command]
pub async fn collection_request(
    window: tauri::WebviewWindow,
    state: State<'_, DesktopState>,
    app: AppHandle,
    request: CollectionRequest,
) -> Result<CollectionResponse, String> {
    if !matches!(window.label(), "main" | "clipboard") {
        return Err("Collections are available only in the local clipboard interface".into());
    }
    let url = window.url().map_err(|_| "Cannot verify local interface")?;
    if !(matches!(url.scheme(), "tauri" | "asset")
        || url.host_str() == Some("tauri.localhost")
        || (cfg!(debug_assertions) && matches!(url.host_str(), Some("localhost" | "127.0.0.1"))))
    {
        return Err("Collections require a local interface".into());
    }
    crate::presence_access::require_clipboard_access(&state)?;
    let p = state.clipboard.sync_preferences();
    let mut response = CollectionResponse {
        sync_enabled: p.enabled,
        sync_edits: p.sync_edits_and_deletes,
        sync_favorites: p.sync_favorites,
        ..Default::default()
    };
    match request {
        CollectionRequest::Sync { id } => {
            response.peers = state.clipboard_sync.collection_states(&id).await?
        }
        CollectionRequest::List { after } => {
            response.records = state
                .clipboard
                .collections(after)
                .await
                .map_err(|e| e.to_string())?
        }
        CollectionRequest::Save { draft } => {
            response.saved = Some(
                state
                    .clipboard
                    .save_collection(draft)
                    .await
                    .map_err(|e| e.to_string())?,
            )
        }
        CollectionRequest::Remove { id, base } => {
            state
                .clipboard
                .change_collection(id, Some(base), None)
                .await
                .map_err(|e| e.to_string())?;
        }
        CollectionRequest::Favorite { id, favorite } => {
            state
                .clipboard
                .change_collection(id, None, Some(favorite))
                .await
                .map_err(|e| e.to_string())?;
        }
        CollectionRequest::Value { id, item } => {
            response.value = Some(
                state
                    .clipboard
                    .collection_value(id, item)
                    .await
                    .map_err(|e| e.to_string())?,
            )
        }
        CollectionRequest::FromHistory { ids } => {
            if ids.is_empty() || ids.len() > 64 {
                return Err("Select 1 to 64 text records".into());
            }
            for (index, id) in ids.into_iter().enumerate() {
                let value = state
                    .clipboard
                    .text_content(id)
                    .await
                    .map_err(|e| e.to_string())?;
                if value.len() > 64 * 1024 {
                    return Err("A value exceeds 64 KiB".into());
                }
                response.items.push(CollectionItem {
                    id: uuid::Uuid::new_v4().to_string(),
                    label: format!("内容 {}", index + 1),
                    value,
                    masked: false,
                    totp: None,
                });
            }
        }
        CollectionRequest::OpenSettings => {
            crate::windowing::hide_clipboard_window(&app).map_err(|e| e.to_string())?;
            crate::windowing::open_clipboard_settings(&app)?;
        }
        CollectionRequest::Retry => {
            state.clipboard_sync.merge_all().await?;
        }
        CollectionRequest::Use { id, item, paste } => {
            let _action = clipboard::begin_clipboard_action()?;
            let target = if paste {
                clipboard::require_input_permission(&state, &app).await?;
                Some(clipboard::capture_paste_target(&app)?)
            } else {
                None
            };
            let restore = if let Some(target) = target.as_ref() {
                clipboard::prepare_window_and_wait_for_paste(&app, target.clone()).await?
            } else {
                false
            };
            let result = async {
                let value = state
                    .clipboard
                    .collection_value(id, item)
                    .await
                    .map_err(|e| e.to_string())?;
                if value.expires_at_ms != 0
                    && value.expires_at_ms - chrono::Utc::now().timestamp_millis() < 5000
                {
                    return Err("TOTP 即将更新，请稍后再试".to_owned());
                }
                crate::presence_access::require_clipboard_access(&state)?;
                if let Some(target) = &target {
                    clipboard::ensure_paste_target_unchanged(&app, target)?;
                }
                state
                    .clipboard
                    .set_text(&value.code)
                    .await
                    .map_err(|e| e.to_string())?;
                if let Some(target) = &target {
                    if value.expires_at_ms != 0
                        && value.expires_at_ms <= chrono::Utc::now().timestamp_millis()
                    {
                        return Err("TOTP 已更新，请重新复制".to_owned());
                    }
                    clipboard::ensure_paste_target_unchanged(&app, target)?;
                    state
                        .clipboard
                        .paste_prepared(ClipboardContentKind::Text)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            }
            .await;
            if paste {
                let _ = crate::windowing::restore_clipboard_window_after_paste(&app, restore);
            }
            result?;
        }
    }
    Ok(response)
}
#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_commands_collections_ipc.rs"));
