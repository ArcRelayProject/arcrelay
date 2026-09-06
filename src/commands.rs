use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use image::{ImageEncoder, RgbaImage};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use arcrelay_core::domain::clipboard::{
    ClipboardContentKind, ClipboardCursor, ClipboardImageOcr, ClipboardLabel, ClipboardQuery,
    ClipboardSortBy,
};
use arcrelay_core::domain::input_control::InputPermissionState;

use crate::application::action_service::ActionImportResult;
use crate::application::preset_catalog::ActionPresetView;
use crate::backend::{BackendCommand, BootstrapState, DesktopState, NotificationView};
use crate::clipboard_sync::NearbyDesktopView;
use crate::domain::quick_action::QuickAction;
use crate::notification::{NewNotification, NotificationKind};
use crate::privacy::{PrivacySettings, PrivacySnapshot, PRIVACY_ACTION_ID};
use crate::remote_files::LocalSharedDirectory;
use crate::settings::{AppSettings, ClipboardSortPreference};
use arcrelay_protocol::remote_files::{
    RemoteFileDirectoryPage, RemoteFileEntry, RemoteFileKind, RemoteFileShare,
    RemoteFileSortDirection, RemoteFileSortKey,
};

mod actions;
mod automations;
mod clipboard;
mod devices;
mod diagnostics;
mod mcp;
mod printing;
mod remote_files;
pub(crate) mod settings;
mod system_share;
mod transfer;

pub use actions::*;
pub use automations::*;
pub use clipboard::*;
pub use devices::*;
pub use diagnostics::*;
pub use mcp::*;
pub use printing::*;
pub use remote_files::*;
pub use settings::*;
pub use system_share::*;
pub use transfer::*;

#[cfg(test)]
pub(crate) fn register_ipc_contract(registry: &mut crate::ipc_contract::Registry) {
    actions::register_ipc_contract(registry);
    automations::register_ipc_contract(registry);
    clipboard::register_ipc_contract(registry);
    devices::register_ipc_contract(registry);
    diagnostics::register_ipc_contract(registry);
    mcp::register_ipc_contract(registry);
    printing::register_ipc_contract(registry);
    remote_files::register_ipc_contract(registry);
    settings::register_ipc_contract(registry);
    system_share::register_ipc_contract(registry);
    transfer::register_ipc_contract(registry);
}
