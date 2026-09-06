use super::*;
use crate::domain::quick_action::ActionType;
use std::cell::RefCell;

fn action(shortcut: &str) -> QuickAction {
    let mut action = QuickAction::new(
        "测试动作".into(),
        "常用".into(),
        ActionType::OpenUrl {
            url: "https://example.com".into(),
        },
    );
    action.global_shortcut = Some(shortcut.into());
    action
}

#[test]
fn normalizes_aliases_and_empty_bindings() {
    assert_eq!(normalize(None).unwrap(), None);
    assert_eq!(normalize(Some(" \n")).unwrap(), None);
    assert_eq!(
        normalize(Some("Ctrl+Shift+L")).unwrap(),
        normalize(Some("shift+control+KeyL")).unwrap()
    );
    assert!(normalize(Some("L")).is_err());
    assert!(normalize(Some("Shift+L")).is_err());
    assert!(normalize(Some("Ctrl+UnknownKey")).is_err());
    assert!(normalize(Some("Ctrl+Alt+F12")).is_ok());
}

#[test]
fn detects_alias_conflicts_without_confusing_an_action_with_itself() {
    let other = action("Ctrl+Shift+L");
    let candidate = action("Shift+Control+KeyL");
    let settings = AppSettings::default();
    assert!(validate_binding(&candidate, std::slice::from_ref(&other), &settings).is_err());
    assert!(validate_binding(&other, std::slice::from_ref(&other), &settings).is_ok());
}

#[test]
fn protects_builtin_and_enabled_feature_shortcuts() {
    let settings = AppSettings::default();
    for key in [
        crate::PRIVACY_UNLOCK_SHORTCUT,
        crate::INPUT_EMERGENCY_SHORTCUT,
        crate::INPUT_RECENTER_SHORTCUT,
        &settings.clipboard_shortcut,
    ] {
        assert!(validate_binding(&action(key), &[], &settings).is_err());
    }
    assert!(validate_binding(&action(&settings.screenshot_shortcut), &[], &settings).is_ok());
    let enabled = AppSettings {
        enhanced_screenshot_enabled: true,
        ..settings
    };
    assert!(validate_binding(&action(&enabled.screenshot_shortcut), &[], &enabled).is_err());
}

#[test]
fn failed_registration_keeps_previous_key_and_does_not_unregister_conflicting_owner() {
    let old = "Ctrl+Shift+L".parse().unwrap();
    let new = "Ctrl+Shift+K".parse().unwrap();
    let removed = RefCell::new(Vec::new());
    assert!(replace_registration(
        Some(old),
        Some(new),
        |_| Err("occupied".into()),
        |key| {
            removed.borrow_mut().push(key);
            Ok(())
        }
    )
    .is_err());
    assert!(removed.borrow().is_empty());
}

#[test]
fn failed_old_key_removal_rolls_back_only_the_new_registration() {
    let old = "Ctrl+Shift+L".parse().unwrap();
    let new = "Ctrl+Shift+K".parse().unwrap();
    let removed = RefCell::new(Vec::new());
    assert!(replace_registration(
        Some(old),
        Some(new),
        |_| Ok(()),
        |key| {
            removed.borrow_mut().push(key);
            if key == old {
                Err("failed".into())
            } else {
                Ok(())
            }
        }
    )
    .is_err());
    assert_eq!(*removed.borrow(), vec![old, new]);
}

#[test]
fn same_key_edit_does_not_reregister_and_clear_releases_key() {
    let key = "Ctrl+Shift+L".parse().unwrap();
    replace_registration(
        Some(key),
        Some(key),
        |_| panic!("must not register"),
        |_| panic!("must not unregister"),
    )
    .unwrap();
    let removed = RefCell::new(Vec::new());
    replace_registration(
        Some(key),
        None,
        |_| panic!("must not register"),
        |key| {
            removed.borrow_mut().push(key);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*removed.borrow(), vec![key]);
}

#[test]
fn held_keys_and_in_flight_actions_cannot_repeat() {
    let gate = Arc::new(TriggerGate::default());
    assert!(!gate.begin(ShortcutState::Pressed));
    gate.enabled.store(true, Ordering::Release);
    assert!(gate.begin(ShortcutState::Pressed));
    let running = Running(gate.clone());
    assert!(!gate.begin(ShortcutState::Pressed));
    assert!(!gate.begin(ShortcutState::Released));
    assert!(!gate.begin(ShortcutState::Pressed));
    drop(running);
    assert!(!gate.begin(ShortcutState::Pressed));
    assert!(!gate.begin(ShortcutState::Released));
    assert!(gate.begin(ShortcutState::Pressed));
}

#[test]
fn old_actions_still_load_and_bindings_survive_serialization() {
    let old: QuickAction = serde_json::from_str(r#"{"name":"Old","icon_id":"zap","action_type":{"type":"OpenUrl","url":"https://example.com"}}"#).unwrap();
    assert_eq!(old.global_shortcut, None);
    let configured = action("Ctrl+Alt+L");
    let restored: QuickAction =
        serde_json::from_slice(&serde_json::to_vec(&configured).unwrap()).unwrap();
    assert_eq!(configured, restored);
}
