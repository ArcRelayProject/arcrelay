use super::*;

pub(super) struct TransferSystemNotification {
    pub(super) sound: crate::sound::SoundEvent,
    pub(super) category: crate::desktop_notification::DesktopNotificationCategory,
    pub(super) title: String,
    pub(super) body: String,
    pub(super) transfer_request_id: Option<String>,
}

#[derive(Default)]
pub(super) struct TransferNotificationTracker {
    statuses: HashMap<String, TransferStatus>,
}

impl TransferNotificationTracker {
    pub(super) fn from_snapshot(snapshot: &TransferSnapshot) -> Self {
        Self {
            statuses: snapshot
                .transfers
                .iter()
                .map(|transfer| (transfer.id.clone(), transfer.status))
                .collect(),
        }
    }

    pub(super) fn update(
        &mut self,
        snapshot: &TransferSnapshot,
        language: crate::settings::LanguagePreference,
    ) -> Vec<TransferSystemNotification> {
        let previous = std::mem::take(&mut self.statuses);
        let mut notifications = Vec::new();

        for transfer in &snapshot.transfers {
            self.statuses.insert(transfer.id.clone(), transfer.status);
            if previous.get(&transfer.id).copied() == Some(transfer.status) {
                continue;
            }

            match transfer.status {
                TransferStatus::AwaitingApproval
                    if transfer.direction == TransferDirection::Receiving =>
                {
                    notifications.push(TransferSystemNotification {
                        sound: crate::sound::SoundEvent::TransferRequest,
                        category: crate::desktop_notification::DesktopNotificationCategory::TransferRequest,
                        title: crate::desktop_notification::localized(
                            language,
                            &format!("来自 {} 的文件传输请求", transfer.peer_name),
                            &format!("File transfer request from {}", transfer.peer_name),
                        ),
                        body: crate::desktop_notification::localized(
                            language,
                            &format!("{}，等待你的确认", transfer_summary(transfer, language)),
                            &format!("{} is waiting for your approval", transfer_summary(transfer, language)),
                        ),
                        transfer_request_id: Some(transfer.id.clone()),
                    });
                }
                TransferStatus::Completed => {
                    let (title, body) = if transfer.direction == TransferDirection::Receiving {
                        (
                            crate::desktop_notification::localized(
                                language,
                                &format!("已接收来自 {} 的文件", transfer.peer_name),
                                &format!("Files received from {}", transfer.peer_name),
                            ),
                            crate::desktop_notification::localized(
                                language,
                                &format!(
                                    "{}，已保存到 {}",
                                    transfer_summary(transfer, language),
                                    snapshot.receive_directory
                                ),
                                &format!(
                                    "{} was saved to {}",
                                    transfer_summary(transfer, language),
                                    snapshot.receive_directory
                                ),
                            ),
                        )
                    } else {
                        (
                            crate::desktop_notification::localized(
                                language,
                                &format!("文件已发送给 {}", transfer.peer_name),
                                &format!("Files sent to {}", transfer.peer_name),
                            ),
                            crate::desktop_notification::localized(
                                language,
                                &format!("{} 已发送完成", transfer_summary(transfer, language)),
                                &format!(
                                    "{} finished sending",
                                    transfer_summary(transfer, language)
                                ),
                            ),
                        )
                    };
                    notifications.push(TransferSystemNotification {
                        sound: if transfer.direction == TransferDirection::Receiving { crate::sound::SoundEvent::TransferReceived } else { crate::sound::SoundEvent::TransferSent },
                        category: crate::desktop_notification::DesktopNotificationCategory::TransferCompleted,
                        title,
                        body,
                        transfer_request_id: None,
                    });
                }
                TransferStatus::Failed => {
                    let receiving = transfer.direction == TransferDirection::Receiving;
                    notifications.push(TransferSystemNotification {
                        sound: crate::sound::SoundEvent::TransferFailed,
                        category: crate::desktop_notification::DesktopNotificationCategory::TransferFailed,
                        title: crate::desktop_notification::localized(
                            language,
                            &if receiving {
                                format!("接收来自 {} 的文件失败", transfer.peer_name)
                            } else {
                                format!("发送文件给 {} 失败", transfer.peer_name)
                            },
                            &format!(
                                "Failed to {} files {} {}",
                                if receiving { "receive" } else { "send" },
                                if receiving { "from" } else { "to" },
                                transfer.peer_name
                            ),
                        ),
                        body: format!(
                            "{}{}{}",
                            transfer_summary(transfer, language),
                            if language.uses_english_fallback() {
                                ": "
                            } else {
                                "："
                            },
                            transfer.error_message.as_deref().unwrap_or_else(|| {
                                if language.uses_english_fallback() {
                                    "Unknown error"
                                } else {
                                    "未知错误"
                                }
                            })
                        ),
                        transfer_request_id: None,
                    });
                }
                _ => {}
            }
        }

        notifications
    }
}

#[derive(Default)]
pub(super) struct AutomationTransferEventTracker {
    statuses: HashMap<String, TransferStatus>,
}

impl AutomationTransferEventTracker {
    pub(super) fn from_snapshot(snapshot: &TransferSnapshot) -> Self {
        Self {
            statuses: snapshot
                .transfers
                .iter()
                .map(|transfer| (transfer.id.clone(), transfer.status))
                .collect(),
        }
    }

    pub(super) fn update(
        &mut self,
        snapshot: &TransferSnapshot,
    ) -> Vec<crate::domain::host_event::HostEvent> {
        let previous = std::mem::take(&mut self.statuses);
        let mut events = Vec::new();
        for transfer in &snapshot.transfers {
            self.statuses.insert(transfer.id.clone(), transfer.status);
            if transfer.status != TransferStatus::Completed
                || previous.get(&transfer.id).copied() == Some(TransferStatus::Completed)
            {
                continue;
            }
            let file_kinds = transfer
                .files
                .iter()
                .filter_map(|file| serde_json::to_value(file.kind).ok())
                .collect::<Vec<_>>();
            events.push(crate::domain::host_event::HostEvent::new(
                "transfer.completed",
                serde_json::json!({
                    "transferId": transfer.id,
                    "deviceId": transfer.peer_id,
                    "deviceName": transfer.peer_name,
                    "direction": transfer.direction,
                    "fileKinds": file_kinds,
                    "files": transfer.files,
                    "receiveDirectory": snapshot.receive_directory,
                    "totalBytes": transfer.total_bytes,
                }),
            ));
        }
        events
    }
}

pub(super) fn transfer_summary(
    transfer: &TransferView,
    language: crate::settings::LanguagePreference,
) -> String {
    match transfer.files.as_slice() {
        [file] if language.uses_english_fallback() => {
            format!("{} ({})", file.name, format_transfer_bytes(file.size))
        }
        [file] => format!("{}（{}）", file.name, format_transfer_bytes(file.size)),
        files if language.uses_english_fallback() => format!(
            "{} files ({} total)",
            files.len(),
            format_transfer_bytes(transfer.total_bytes)
        ),
        files => format!(
            "{} 个文件（共 {}）",
            files.len(),
            format_transfer_bytes(transfer.total_bytes)
        ),
    }
}

pub(super) fn format_transfer_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(super) fn show_transfer_system_notification(
    app: &AppHandle,
    settings: &SettingsManager,
    value: TransferSystemNotification,
) {
    let mut notification = crate::desktop_notification::DesktopNotification::new(
        value.category,
        value.title,
        value.body,
    );
    if let Some(transfer_id) = value.transfer_request_id {
        notification = notification.open_transfer_request(transfer_id);
    }
    if let Err(error) =
        crate::desktop_notification::show_with_sound(app, settings, notification, value.sound, None)
    {
        tracing::warn!(%error, "Failed to show transfer notification");
    }
}
