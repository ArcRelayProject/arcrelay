use rfd::{AsyncMessageDialog, MessageButtons, MessageDialogResult, MessageLevel};

use crate::settings::LanguagePreference;

pub async fn request_pairing_approval(
    device_name: &str,
    pairing_code: &str,
    permissions: &[String],
    language: LanguagePreference,
) -> bool {
    let tr = |source, english| crate::localization::translate(language, source, english);
    let permission_text = if permissions.is_empty() {
        tr("基础连接权限", "Basic connection access")
    } else {
        permissions.join(", ")
    };
    let description = format!(
        "{}: {device_name}\n\n{}: {pairing_code}\n{}: {permission_text}\n\n{}",
        tr("请求连接的设备", "Device requesting connection"),
        tr("配对码", "Pairing code"),
        tr("请求权限", "Requested access"),
        tr("请先确认请求设备上显示的配对码一致。仅允许你认识并信任的设备接入。", "Confirm that the same pairing code appears on the requesting device. Allow only devices you know and trust."),
    );
    let allow = tr("允许", "Allow");
    let deny = tr("拒绝", "Deny");

    match AsyncMessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title(tr("ArcRelay 新设备连接请求", "ArcRelay New Device Request"))
        .set_description(description)
        .set_buttons(MessageButtons::OkCancelCustom(
            allow.to_string(),
            deny.to_string(),
        ))
        .show()
        .await
    {
        MessageDialogResult::Ok | MessageDialogResult::Yes => true,
        MessageDialogResult::Custom(value) => value == allow,
        MessageDialogResult::No | MessageDialogResult::Cancel => false,
    }
}
