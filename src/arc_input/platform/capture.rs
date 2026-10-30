//! Desktop-only capture instrumentation. The shared InputCapturePort still
//! returns the original events; both entry points use the same native hooks.
use arcrelay_input::CapturedInputEvent;
#[cfg(target_os = "linux")]
use std::sync::mpsc::Sender;
use std::sync::mpsc::SyncSender;
use std::time::Instant;

#[derive(Debug)]
pub(crate) struct CapturedInputSample {
    pub event: CapturedInputEvent,
    pub captured_at: Instant,
}

pub(super) trait CaptureOutput<T>: Send + Sync + 'static {
    fn enqueue(&self, value: T) -> Result<(), ()>;
}

impl<T: Send + 'static> CaptureOutput<T> for SyncSender<T> {
    fn enqueue(&self, value: T) -> Result<(), ()> {
        self.try_send(value).map_err(|_| ())
    }
}

#[cfg(target_os = "linux")]
impl<T: Send + 'static> CaptureOutput<T> for Sender<T> {
    fn enqueue(&self, value: T) -> Result<(), ()> {
        self.send(value).map_err(|_| ())
    }
}

// The sink is allocated once per native capture session, not once per event.
// It preserves each backend's existing bounded/unbounded channel semantics.
pub(super) struct CaptureSender(Box<dyn Fn(CapturedInputEvent) -> Result<(), ()> + Send + Sync>);

impl CaptureSender {
    pub fn plain(sender: impl CaptureOutput<CapturedInputEvent>) -> Self {
        Self(Box::new(move |event| sender.enqueue(event)))
    }

    pub fn timed(sender: impl CaptureOutput<CapturedInputSample>) -> Self {
        Self(Box::new(move |event| {
            sender.enqueue(CapturedInputSample {
                event,
                captured_at: Instant::now(),
            })
        }))
    }

    pub fn try_send(&self, event: CapturedInputEvent) -> Result<(), ()> {
        (self.0)(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_stamped_at_enqueue_without_an_async_runtime() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let sender = CaptureSender::timed(sender);
        let before = Instant::now();
        sender
            .try_send(CapturedInputEvent::EmergencyRelease)
            .unwrap();
        let after = Instant::now();
        let received = receiver.recv().unwrap();
        assert!(received.captured_at >= before && received.captured_at <= after);
        assert_eq!(received.event, CapturedInputEvent::EmergencyRelease);
    }

    #[test]
    fn instrumentation_preserves_full_and_closed_channel_failures() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let sender = CaptureSender::timed(sender);
        sender
            .try_send(CapturedInputEvent::EmergencyRelease)
            .unwrap();
        assert!(sender
            .try_send(CapturedInputEvent::EmergencyRelease)
            .is_err());
        drop(receiver);
        assert!(sender
            .try_send(CapturedInputEvent::EmergencyRelease)
            .is_err());
    }
}
