use std::sync::mpsc::Receiver;

use arcrelay_input::{
    CaptureOptions, CapturedInputEvent, DisplayId, DisplayInventory, DisplayInventoryPort,
    InputCapturePort, InputInjectionPort, LogicalPoint, MappedKeyboardEvent, PlatformCapabilities,
    PlatformError, ScrollEvent,
};

pub struct NativePlatform;

impl NativePlatform {
    pub fn new(_: arcrelay_peer::ServiceInstanceId) -> Self {
        Self
    }

    fn unsupported() -> PlatformError {
        PlatformError::Unsupported(
            "this build has no native Arc Input adapter for the current platform".into(),
        )
    }
}

impl Default for NativePlatform {
    fn default() -> Self {
        Self
    }
}

impl InputCapturePort for NativePlatform {
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            limitation: Some(Self::unsupported().to_string()),
            ..PlatformCapabilities::default()
        }
    }
    fn start(&self, _: CaptureOptions) -> Result<Receiver<CapturedInputEvent>, PlatformError> {
        Err(Self::unsupported())
    }
    fn set_suppress_local(&self, suppress: bool) -> Result<(), PlatformError> {
        if suppress {
            Err(Self::unsupported())
        } else {
            Ok(())
        }
    }
    fn current_pointer_position(&self) -> Result<LogicalPoint, PlatformError> {
        Err(Self::unsupported())
    }
    fn stop(&self) -> Result<(), PlatformError> {
        Ok(())
    }
}

impl InputInjectionPort for NativePlatform {
    fn capabilities(&self) -> PlatformCapabilities {
        InputCapturePort::capabilities(self)
    }
    fn place_pointer(&self, _: &DisplayId, _: LogicalPoint) -> Result<(), PlatformError> {
        Err(Self::unsupported())
    }
    fn apply_keyboard(&self, _: &MappedKeyboardEvent) -> Result<(), PlatformError> {
        Err(Self::unsupported())
    }
    fn pointer_button(&self, _: u16, _: bool, _: u8) -> Result<(), PlatformError> {
        Err(Self::unsupported())
    }
    fn scroll(&self, _: ScrollEvent) -> Result<(), PlatformError> {
        Err(Self::unsupported())
    }
    fn release_all(&self) -> Result<(), PlatformError> {
        Ok(())
    }
}

impl DisplayInventoryPort for NativePlatform {
    fn inventory(&self) -> Result<DisplayInventory, PlatformError> {
        Err(Self::unsupported())
    }
}
