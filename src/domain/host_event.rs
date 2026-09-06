/// Events emitted by current host capabilities. The automation adapter owns
/// their normalization; retired workflow persistence is not a dependency.
#[derive(Clone)]
pub struct HostEvent {
    pub kind: String,
    pub payload: serde_json::Value,
}
impl HostEvent {
    pub fn new(kind: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            kind: kind.into(),
            payload,
        }
    }
}
