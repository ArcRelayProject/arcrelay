use arcrelay_input::ConsumerKey;
pub(super) fn windows_vk(key: ConsumerKey) -> Option<u16> {
    Some(match key {
        ConsumerKey::Mute => 0xad,
        ConsumerKey::VolumeDown => 0xae,
        ConsumerKey::VolumeUp => 0xaf,
        ConsumerKey::NextTrack => 0xb0,
        ConsumerKey::PreviousTrack => 0xb1,
        ConsumerKey::PlayPause => 0xb3,
        _ => return None,
    })
}
pub(super) fn from_windows_vk(vk: u16) -> Option<ConsumerKey> {
    ConsumerKey::ALL
        .into_iter()
        .find(|key| windows_vk(*key) == Some(vk))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_media_mapping_does_not_confuse_function_or_brightness_keys() {
        for key in ConsumerKey::ALL {
            if let Some(vk) = windows_vk(key) {
                assert_eq!(from_windows_vk(vk), Some(key));
            }
        }
        assert!(from_windows_vk(0x77).is_none()); // F8 remains F8.
        assert!(windows_vk(ConsumerKey::BrightnessUp).is_none());
    }
}
