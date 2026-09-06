//! Arc Input runtime integrated into the ArcRelay desktop application.

pub mod commands;
pub mod discovery;
pub mod identity;
pub mod network;
pub mod platform;
pub mod runtime;
pub mod store;

pub use identity::{ProductIdentity, ProductPaths};
pub use runtime::ArcInputRuntime;

pub(crate) fn log_peer_id(value: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(value.as_bytes());
    let mut encoded = String::with_capacity(17);
    encoded.push_str("peer-");
    for byte in digest.iter().take(6) {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

pub(crate) fn sanitize_log_text(value: &str, max_chars: usize) -> String {
    let mut sanitized = value.replace('\0', "");
    if let Some(home) = dirs::home_dir().and_then(|path| path.to_str().map(str::to_owned)) {
        if !home.is_empty() {
            sanitized = sanitized.replace(&home, "<home>");
        }
    }
    if sanitized.chars().count() > max_chars {
        sanitized = sanitized.chars().take(max_chars).collect::<String>();
        sanitized.push_str("…[truncated]");
    }
    sanitized
}
