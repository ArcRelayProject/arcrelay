use roxmltree::Document;

use super::quick_action::QuickAction;

pub const MAX_ICON_SVG_BYTES: usize = 16 * 1024;

const BUILTIN_ICONS: &[(&str, &str)] = &[
    ("zap", include_str!("../../assets/icons/zap.svg")),
    ("folder", include_str!("../../assets/icons/folder.svg")),
    ("globe", include_str!("../../assets/icons/globe.svg")),
    ("terminal", include_str!("../../assets/icons/terminal.svg")),
    ("keyboard", include_str!("../../assets/icons/keyboard.svg")),
    ("scroll", include_str!("../../assets/icons/scroll.svg")),
    ("settings", include_str!("../../assets/icons/settings.svg")),
    ("play", include_str!("../../assets/icons/play.svg")),
    ("radio", include_str!("../../assets/icons/radio.svg")),
    ("link", include_str!("../../assets/icons/link.svg")),
    ("toggle", include_str!("../../assets/icons/toggle-left.svg")),
    (
        "crosshair",
        include_str!("../../assets/icons/crosshair.svg"),
    ),
    (
        "smartphone",
        include_str!("../../assets/icons/smartphone.svg"),
    ),
    ("masks", include_str!("../../assets/icons/masks.svg")),
    ("edit", include_str!("../../assets/icons/edit.svg")),
    ("circle", include_str!("../../assets/icons/circle.svg")),
    ("grip", include_str!("../../assets/icons/grip.svg")),
    ("close", include_str!("../../assets/icons/x.svg")),
];

pub fn builtin_svg(id: &str) -> Option<&'static str> {
    BUILTIN_ICONS
        .iter()
        .find_map(|(candidate, svg)| (*candidate == id).then_some(*svg))
}

/// Ensures an action has a safe, portable icon. Built-in SVG is canonicalized
/// from the icon ID, while custom SVG must pass the security checks below.
pub fn normalize_action_icon(action: &mut QuickAction) -> Result<bool, String> {
    let previous_icon_id = action.icon_id.clone();
    let previous_svg = action.icon_svg.clone();
    action.icon_id = action.icon_id.trim().to_string();
    action.icon_svg = action.icon_svg.trim().to_string();

    if let Some(svg) = builtin_svg(&action.icon_id) {
        action.icon_svg = svg.to_string();
    } else if action.icon_id == "custom" {
        validate_custom_svg(&action.icon_svg)?;
    } else {
        return Err("select a valid SVG icon".to_string());
    }

    Ok(action.icon_id != previous_icon_id || action.icon_svg != previous_svg)
}

pub fn validate_custom_svg(svg: &str) -> Result<(), String> {
    let trimmed = svg.trim();
    if trimmed.is_empty() {
        return Err("SVG icon cannot be empty".to_string());
    }
    if trimmed.len() > MAX_ICON_SVG_BYTES {
        return Err("SVG icon cannot exceed 16 KiB".to_string());
    }
    if !trimmed.starts_with("<svg") || trimmed.contains("<!DOCTYPE") {
        return Err("icon file must be a standalone SVG document".to_string());
    }

    let document = Document::parse(trimmed).map_err(|error| format!("invalid SVG: {error}"))?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return Err("icon root element must be <svg>".to_string());
    }

    const FORBIDDEN_ELEMENTS: &[&str] = &[
        "script",
        "style",
        "link",
        "foreignobject",
        "iframe",
        "object",
        "embed",
        "image",
        "use",
        "audio",
        "video",
        "animate",
        "animatemotion",
        "animatetransform",
        "set",
    ];
    for node in document.descendants().filter(roxmltree::Node::is_element) {
        let element_name = node.tag_name().name();
        if FORBIDDEN_ELEMENTS.contains(&element_name.to_ascii_lowercase().as_str()) {
            return Err(format!(
                "SVG icon cannot contain a <{element_name}> element"
            ));
        }
        for attribute in node.attributes() {
            let name = attribute.name().to_ascii_lowercase();
            let value = attribute.value().trim().to_ascii_lowercase();
            if name.starts_with("on")
                || matches!(name.as_str(), "href" | "src" | "style")
                || attribute
                    .namespace()
                    .is_some_and(|namespace| namespace.contains("xlink"))
                || value.contains("javascript:")
                || value.contains("data:")
                || value.contains("http://")
                || value.contains("https://")
                || value.contains("file:")
                || (value.contains("url(") && !value.contains("url(#"))
            {
                return Err(format!(
                    "SVG icon contains an unsafe attribute: {}",
                    attribute.name()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple_custom_svg() {
        validate_custom_svg(
            r#"<svg viewBox="0 0 24 24"><path fill="currentColor" d="M2 2h20v20H2z"/></svg>"#,
        )
        .unwrap();
    }

    #[test]
    fn rejects_active_or_external_svg_content() {
        assert!(validate_custom_svg(r#"<svg><script>alert(1)</script></svg>"#).is_err());
        assert!(validate_custom_svg(r#"<svg><path onclick="alert(1)" d="M0 0"/></svg>"#).is_err());
        assert!(
            validate_custom_svg(r#"<svg><image href="https://example.com/icon.png"/></svg>"#)
                .is_err()
        );
    }
}
