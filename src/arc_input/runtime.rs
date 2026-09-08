use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use arcrelay_input::{
    align_system_display_groups, derive_auto_portals, proto, CaptureOptions, CapturedInputEvent,
    ControlArbiter, ControlEpoch, ControlSession, DeskPointUm, DeskVectorUm, DisplayId,
    DisplayInventory, DisplayInventoryPort, HeldInputState, InputCapturePort, InputInjectionPort,
    InventoryRevision, KeyboardMappingEngine, KeyboardProfile, KeyboardProfileKind, LogicalPoint,
    MappedKeyboardEvent, OsFamily, PlatformCapabilities, PortalDirection, PortalId, PortalResolver,
    PortalStatus, RuntimeHeader, RuntimeRouter, RuntimeRoutingSnapshot, ScrollEvent,
    ScrollMomentumPhase, ScrollPhase, ScrollUnit, SemanticAction, SessionState, TopologyRevision,
    WorkspaceId, WorkspaceLayout, DEFAULT_MINIMUM_PORTAL_SPAN_UM,
    DEFAULT_PORTAL_ADJACENCY_TOLERANCE_UM,
};
use arcrelay_network::NetworkRuntime;
use arcrelay_peer::{should_dial, DeviceId, DialIntent, PeerRecord, ServiceInstanceId};
use arcrelay_protocol::server::WorkspaceInputSnapshot;
use tokio::sync::{broadcast, oneshot, Mutex as AsyncMutex};

use super::discovery::DiscoveredPeer;
use super::network::{
    wait_for_network, InputEnqueueOutcome, NetworkError, NetworkEvent, PeerConnectionHub,
};
use super::platform::NativePlatform;
use super::store::{HorizontalScrollBehavior, StoreError, WorkspaceConfiguration, WorkspaceStore};
use super::{ProductIdentity, ProductPaths};

type HandoffWaiter = oneshot::Sender<Result<(), String>>;
type HandoffWaiters = BTreeMap<(ServiceInstanceId, u64), HandoffWaiter>;
type ControlWaiter = oneshot::Sender<Result<(), String>>;
type ControlWaiters = BTreeMap<ServiceInstanceId, ControlWaiter>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RelayedPeerRoute {
    endpoints: Vec<SocketAddr>,
    certificate_sha256: [u8; 32],
}

#[derive(Debug, Clone)]
struct ObservedControlState {
    version: u64,
    active: Option<(ServiceInstanceId, ControlEpoch)>,
}

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    SnapshotChanged,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRecord {
    pub timestamp_ms: u64,
    pub category: String,
    pub message: String,
    pub latency_micros: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub revision: u64,
    pub product_id: &'static str,
    pub service_instance_id: String,
    pub local_operating_system: OsFamily,
    pub remote_operating_systems: BTreeMap<String, OsFamily>,
    pub capabilities: PlatformCapabilities,
    pub configuration: WorkspaceConfiguration,
    pub discovered_peers: Vec<String>,
    pub nearby_peers: Vec<NearbyPeerView>,
    pub connected_peers: Vec<String>,
    pub display_availability: BTreeMap<DisplayId, DisplayAvailability>,
    pub controller: Option<String>,
    pub control_epoch: Option<u64>,
    pub capture_active: bool,
    pub diagnostics: Vec<DiagnosticRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
pub enum DisplayAvailability {
    Ready,
    Offline,
    SharingDisabled,
    PermissionRequired,
    DisplayDisconnected,
    Connecting,
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct NearbyPeerView {
    pub service_instance_id: String,
    pub display_name: Option<String>,
    pub addresses: Vec<String>,
    pub port: u16,
    pub certificate_sha256: String,
    pub capability_digest: String,
    pub paired: bool,
    pub connected: bool,
    pub capabilities: Option<PlatformCapabilities>,
}

struct DwellGate {
    portal_id: PortalId,
    since: Instant,
}

const POINTER_DISPATCH_INTERVAL: Duration = Duration::from_millis(4);
const SCROLL_DISPATCH_INTERVAL: Duration = Duration::from_millis(8);
const CONTROL_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(750);
const HANDOFF_TIMEOUT: Duration = Duration::from_millis(750);
const HANDOFF_REENTRY_GUARD: Duration = Duration::from_millis(75);
const REMOTE_TAKEOVER_MOTION_THRESHOLD: f64 = 12.0;
const REMOTE_TAKEOVER_WINDOW: Duration = Duration::from_millis(80);
const REMOTE_TAKEOVER_ARMED_FOR: Duration = Duration::from_millis(120);
const HORIZONTAL_NAVIGATION_CLASSIFY_DELTA: f64 = 16.0;
const HORIZONTAL_NAVIGATION_TRIGGER_DELTA: f64 = 80.0;
const HORIZONTAL_NAVIGATION_DOMINANCE: f64 = 1.5;
const HORIZONTAL_NAVIGATION_MAX_BUFFERED_EVENTS: usize = 256;

fn proto_scroll_unit(unit: ScrollUnit) -> i32 {
    match unit {
        ScrollUnit::Unspecified => proto::ScrollUnit::Unspecified as i32,
        ScrollUnit::Pixel => proto::ScrollUnit::Pixel as i32,
        ScrollUnit::WheelDetent => proto::ScrollUnit::WheelDetent as i32,
    }
}

fn scroll_unit_from_proto(value: i32, precise: bool) -> ScrollUnit {
    match proto::ScrollUnit::try_from(value).unwrap_or(proto::ScrollUnit::Unspecified) {
        proto::ScrollUnit::Pixel => ScrollUnit::Pixel,
        proto::ScrollUnit::WheelDetent => ScrollUnit::WheelDetent,
        proto::ScrollUnit::Unspecified => {
            if precise {
                ScrollUnit::Pixel
            } else {
                ScrollUnit::WheelDetent
            }
        }
    }
}

fn proto_scroll_phase(phase: ScrollPhase) -> i32 {
    match phase {
        ScrollPhase::Unspecified => proto::ScrollGesturePhase::Unspecified as i32,
        ScrollPhase::MayBegin => proto::ScrollGesturePhase::MayBegin as i32,
        ScrollPhase::Began => proto::ScrollGesturePhase::Began as i32,
        ScrollPhase::Changed => proto::ScrollGesturePhase::Changed as i32,
        ScrollPhase::Ended => proto::ScrollGesturePhase::Ended as i32,
        ScrollPhase::Cancelled => proto::ScrollGesturePhase::Cancelled as i32,
    }
}

fn scroll_phase_from_proto(gesture_phase: i32, legacy_phase: u32) -> ScrollPhase {
    let value = if gesture_phase == proto::ScrollGesturePhase::Unspecified as i32 {
        i32::try_from(legacy_phase).unwrap_or_default()
    } else {
        gesture_phase
    };
    match proto::ScrollGesturePhase::try_from(value)
        .unwrap_or(proto::ScrollGesturePhase::Unspecified)
    {
        proto::ScrollGesturePhase::MayBegin => ScrollPhase::MayBegin,
        proto::ScrollGesturePhase::Began => ScrollPhase::Began,
        proto::ScrollGesturePhase::Changed => ScrollPhase::Changed,
        proto::ScrollGesturePhase::Ended => ScrollPhase::Ended,
        proto::ScrollGesturePhase::Cancelled => ScrollPhase::Cancelled,
        proto::ScrollGesturePhase::Unspecified => ScrollPhase::Unspecified,
    }
}

fn proto_momentum_phase(phase: ScrollMomentumPhase) -> i32 {
    match phase {
        ScrollMomentumPhase::Unspecified => proto::ScrollMomentumPhase::Unspecified as i32,
        ScrollMomentumPhase::Began => proto::ScrollMomentumPhase::Began as i32,
        ScrollMomentumPhase::Changed => proto::ScrollMomentumPhase::Changed as i32,
        ScrollMomentumPhase::Ended => proto::ScrollMomentumPhase::Ended as i32,
    }
}

fn momentum_phase_from_proto(value: i32) -> ScrollMomentumPhase {
    match proto::ScrollMomentumPhase::try_from(value)
        .unwrap_or(proto::ScrollMomentumPhase::Unspecified)
    {
        proto::ScrollMomentumPhase::Began => ScrollMomentumPhase::Began,
        proto::ScrollMomentumPhase::Changed => ScrollMomentumPhase::Changed,
        proto::ScrollMomentumPhase::Ended => ScrollMomentumPhase::Ended,
        proto::ScrollMomentumPhase::Unspecified => ScrollMomentumPhase::Unspecified,
    }
}

fn simultaneous_claim_wins(
    incoming_controller: &ServiceInstanceId,
    active_controller: &ServiceInstanceId,
    expected: ControlEpoch,
    incoming: ControlEpoch,
    active: ControlEpoch,
) -> bool {
    incoming == active
        // After reconnect/merging components, a fresh claim has no prior
        // owner (expected=0) but its clock need not restart at epoch 1.
        && (expected.0 == 0 || expected.0.saturating_add(1) == incoming.0)
        && incoming_controller > active_controller
}

#[derive(Debug, Default)]
struct PendingPointerDelta {
    x: f64,
    y: f64,
}

#[derive(Debug, Default)]
struct RemoteTakeoverIntent {
    x: f64,
    y: f64,
    window_started: Option<Instant>,
    last_sample: Option<Instant>,
    armed_until: Option<Instant>,
}

impl RemoteTakeoverIntent {
    fn observe_portal_crossing(
        &mut self,
        crosses_remote: bool,
        x: f64,
        y: f64,
        now: Instant,
    ) -> bool {
        let intended = self.observe(x, y, now);
        crosses_remote && intended
    }

    fn observe(&mut self, x: f64, y: f64, now: Instant) -> bool {
        if !x.is_finite() || !y.is_finite() {
            self.clear();
            return false;
        }
        if self.armed_until.is_some_and(|until| now < until) {
            return true;
        }
        self.armed_until = None;

        let window_expired = self
            .window_started
            .is_none_or(|started| now.saturating_duration_since(started) >= REMOTE_TAKEOVER_WINDOW);
        let input_went_idle = self
            .last_sample
            .is_some_and(|last| now.saturating_duration_since(last) >= REMOTE_TAKEOVER_WINDOW);
        if window_expired || input_went_idle {
            self.x = 0.0;
            self.y = 0.0;
            self.window_started = Some(now);
        }
        self.x += x;
        self.y += y;
        self.last_sample = Some(now);
        let intended = self.x.hypot(self.y) >= REMOTE_TAKEOVER_MOTION_THRESHOLD;
        if intended {
            self.x = 0.0;
            self.y = 0.0;
            self.window_started = None;
            self.armed_until = Some(now + REMOTE_TAKEOVER_ARMED_FOR);
        }
        intended
    }

    fn clear(&mut self) {
        self.x = 0.0;
        self.y = 0.0;
        self.window_started = None;
        self.last_sample = None;
        self.armed_until = None;
    }
}

#[derive(Debug, Default)]
struct PendingScrollDelta {
    event: Option<ScrollEvent>,
}

impl PendingScrollDelta {
    fn push(&mut self, event: ScrollEvent) -> bool {
        if !event.has_delta()
            || !matches!(event.phase, ScrollPhase::Changed | ScrollPhase::Unspecified)
        {
            return false;
        }
        match self.event.as_mut() {
            Some(pending)
                if pending.unit == event.unit
                    && pending.phase == event.phase
                    && pending.momentum_phase == event.momentum_phase =>
            {
                pending.delta_x += event.delta_x;
                pending.delta_y += event.delta_y;
            }
            Some(_) => return false,
            None => self.event = Some(event),
        }
        true
    }

    fn take(&mut self) -> Option<ScrollEvent> {
        self.event.take()
    }

    fn clear(&mut self) {
        self.event = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HorizontalNavigationAction {
    Back,
    Forward,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum HorizontalGestureState {
    #[default]
    Idle,
    Tracking,
    NativeScroll,
    Navigate,
}

#[derive(Debug, Default)]
struct HorizontalNavigationOutput {
    scroll_events: Vec<ScrollEvent>,
    action: Option<HorizontalNavigationAction>,
}

#[derive(Debug, Default)]
struct HorizontalNavigationRecognizer {
    state: HorizontalGestureState,
    buffered: Vec<ScrollEvent>,
    total_x: f64,
    total_y: f64,
    navigation_emitted: bool,
    suppress_momentum: bool,
}

impl HorizontalNavigationRecognizer {
    fn process(&mut self, event: ScrollEvent) -> HorizontalNavigationOutput {
        if event.momentum_phase != ScrollMomentumPhase::Unspecified {
            if self.suppress_momentum {
                if event.momentum_phase == ScrollMomentumPhase::Ended {
                    self.suppress_momentum = false;
                }
                return HorizontalNavigationOutput::default();
            }
            self.clear_gesture();
            return HorizontalNavigationOutput {
                scroll_events: vec![event],
                action: None,
            };
        }

        match event.phase {
            ScrollPhase::MayBegin | ScrollPhase::Began => {
                self.start_gesture();
                self.track(event)
            }
            ScrollPhase::Changed => match self.state {
                HorizontalGestureState::Idle => HorizontalNavigationOutput {
                    scroll_events: vec![event],
                    action: None,
                },
                HorizontalGestureState::NativeScroll => HorizontalNavigationOutput {
                    scroll_events: vec![event],
                    action: None,
                },
                HorizontalGestureState::Tracking | HorizontalGestureState::Navigate => {
                    self.track(event)
                }
            },
            ScrollPhase::Ended => self.finish(event, false),
            ScrollPhase::Cancelled => self.finish(event, true),
            ScrollPhase::Unspecified => {
                self.clear_gesture();
                HorizontalNavigationOutput {
                    scroll_events: vec![event],
                    action: None,
                }
            }
        }
    }

    fn reset(&mut self) {
        self.clear_gesture();
        self.suppress_momentum = false;
    }

    fn start_gesture(&mut self) {
        self.clear_gesture();
        self.suppress_momentum = false;
        self.state = HorizontalGestureState::Tracking;
    }

    fn track(&mut self, event: ScrollEvent) -> HorizontalNavigationOutput {
        self.total_x += event.delta_x;
        self.total_y += event.delta_y;
        self.buffered.push(event);

        let horizontal = self.horizontal_dominant();
        if self.state == HorizontalGestureState::Tracking {
            if horizontal && self.total_x.abs() >= HORIZONTAL_NAVIGATION_CLASSIFY_DELTA {
                self.state = HorizontalGestureState::Navigate;
            } else if self.total_y.abs() >= HORIZONTAL_NAVIGATION_CLASSIFY_DELTA
                || self.total_x.hypot(self.total_y) >= HORIZONTAL_NAVIGATION_CLASSIFY_DELTA * 2.0
                || self.buffered.len() >= HORIZONTAL_NAVIGATION_MAX_BUFFERED_EVENTS
            {
                self.state = HorizontalGestureState::NativeScroll;
                return HorizontalNavigationOutput {
                    scroll_events: std::mem::take(&mut self.buffered),
                    action: None,
                };
            }
        } else if self.state == HorizontalGestureState::Navigate && !horizontal {
            self.state = HorizontalGestureState::NativeScroll;
            return HorizontalNavigationOutput {
                scroll_events: std::mem::take(&mut self.buffered),
                action: None,
            };
        }

        if self.state == HorizontalGestureState::Navigate
            && !self.navigation_emitted
            && self.total_x.abs() >= HORIZONTAL_NAVIGATION_TRIGGER_DELTA
        {
            self.navigation_emitted = true;
            self.suppress_momentum = true;
            self.buffered.clear();
            return HorizontalNavigationOutput {
                scroll_events: Vec::new(),
                action: Some(if self.total_x < 0.0 {
                    HorizontalNavigationAction::Back
                } else {
                    HorizontalNavigationAction::Forward
                }),
            };
        }

        HorizontalNavigationOutput::default()
    }

    fn finish(&mut self, event: ScrollEvent, cancelled: bool) -> HorizontalNavigationOutput {
        match self.state {
            HorizontalGestureState::Idle => HorizontalNavigationOutput {
                scroll_events: vec![event],
                action: None,
            },
            HorizontalGestureState::NativeScroll => {
                self.clear_gesture();
                HorizontalNavigationOutput {
                    scroll_events: vec![event],
                    action: None,
                }
            }
            HorizontalGestureState::Tracking | HorizontalGestureState::Navigate => {
                let mut output = self.track(event);
                if !self.navigation_emitted && output.scroll_events.is_empty() {
                    output.scroll_events = std::mem::take(&mut self.buffered);
                }
                if cancelled {
                    output.action = None;
                    self.suppress_momentum = false;
                }
                self.clear_gesture();
                output
            }
        }
    }

    fn horizontal_dominant(&self) -> bool {
        self.total_x.abs() >= self.total_y.abs() * HORIZONTAL_NAVIGATION_DOMINANCE
    }

    fn clear_gesture(&mut self) {
        self.state = HorizontalGestureState::Idle;
        self.buffered.clear();
        self.total_x = 0.0;
        self.total_y = 0.0;
        self.navigation_emitted = false;
    }
}

impl PendingPointerDelta {
    fn push(&mut self, x: f64, y: f64) {
        self.x += x;
        self.y += y;
    }

    fn take(&mut self) -> Option<(f64, f64)> {
        let x = std::mem::take(&mut self.x);
        let y = std::mem::take(&mut self.y);
        (x != 0.0 || y != 0.0).then_some((x, y))
    }

    fn clear(&mut self) {
        self.x = 0.0;
        self.y = 0.0;
    }
}

#[derive(Debug)]
struct HandoffReentryGuard {
    portal_id: PortalId,
    source_display: DisplayId,
    target_display: DisplayId,
    activated_at: Instant,
}

impl HandoffReentryGuard {
    fn from_crossing(crossing: &arcrelay_input::PortalCrossing, activated_at: Instant) -> Self {
        Self {
            portal_id: crossing.portal_id.clone(),
            source_display: crossing.source_display.clone(),
            target_display: crossing.target_display.clone(),
            activated_at,
        }
    }

    fn blocks(&self, crossing: &arcrelay_input::PortalCrossing, now: Instant) -> bool {
        now.saturating_duration_since(self.activated_at) < HANDOFF_REENTRY_GUARD
            && crossing.portal_id == self.portal_id
            && crossing.source_display == self.target_display
            && crossing.target_display == self.source_display
    }

    fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.activated_at) >= HANDOFF_REENTRY_GUARD
    }
}

pub struct ArcInputRuntime {
    runtime_handle: tokio::runtime::Handle,
    paths: ProductPaths,
    identity: Arc<ProductIdentity>,
    store: WorkspaceStore,
    unified_network: Arc<tokio::sync::OnceCell<Arc<NetworkRuntime>>>,
    paired_peers: RwLock<BTreeMap<ServiceInstanceId, PeerRecord>>,
    network: Arc<PeerConnectionHub>,
    platform: Arc<NativePlatform>,
    #[cfg(test)]
    availability_capabilities: RwLock<Option<PlatformCapabilities>>,
    arbiter: ControlArbiter,
    router: RwLock<Option<Arc<RuntimeRouter>>>,
    session: Mutex<Option<ControlSession>>,
    control_participants: Mutex<BTreeSet<ServiceInstanceId>>,
    retired_control_epochs: Mutex<BTreeMap<ServiceInstanceId, ControlEpoch>>,
    control_epoch_clock: AtomicU64,
    system_gesture_generation: AtomicU64,
    consumer_generation: AtomicU64,
    consumer_route: Mutex<Option<(ServiceInstanceId, DisplayId, u32)>>,
    observed_control: Mutex<ObservedControlState>,
    takeover_pending_peers: Mutex<BTreeMap<ServiceInstanceId, ControlEpoch>>,
    control_waiters: Mutex<ControlWaiters>,
    handoff_waiters: Mutex<HandoffWaiters>,
    handoff_reentry_guard: Mutex<Option<HandoffReentryGuard>>,
    remote_takeover_intent: Mutex<RemoteTakeoverIntent>,
    pointer: Mutex<Option<DeskPointUm>>,
    dwell_gate: Mutex<Option<DwellGate>>,
    standby_held: Mutex<HeldInputState>,
    capture_active: AtomicBool,
    local_inventory: RwLock<Option<DisplayInventory>>,
    discovered: RwLock<BTreeMap<ServiceInstanceId, DiscoveredPeer>>,
    relayed_routes: RwLock<BTreeMap<ServiceInstanceId, RelayedPeerRoute>>,
    reconnect_attempts: Mutex<BTreeMap<ServiceInstanceId, reconnect::ReconnectAttempt>>,
    automatic_control_retry_at: Mutex<Option<Instant>>,
    remote_operating_systems: RwLock<BTreeMap<ServiceInstanceId, OsFamily>>,
    remote_capabilities: RwLock<BTreeMap<ServiceInstanceId, PlatformCapabilities>>,
    remote_sharing_enabled: RwLock<BTreeMap<ServiceInstanceId, bool>>,
    remote_inventories: RwLock<BTreeMap<ServiceInstanceId, DisplayInventory>>,
    keyboard_mapper: Mutex<Option<(ServiceInstanceId, KeyboardMappingEngine)>>,
    horizontal_navigation: Mutex<HorizontalNavigationRecognizer>,
    snapshot_revision: Mutex<u64>,
    diagnostics: Mutex<VecDeque<DiagnosticRecord>>,
    input_diagnostic_counter: AtomicU64,
    input_capture_count: AtomicU64,
    input_capture_errors: AtomicU64,
    input_capture_max_micros: AtomicU64,
    input_scroll_captured: AtomicU64,
    input_scroll_sent: AtomicU64,
    input_scroll_received: AtomicU64,
    input_scroll_native_applied: AtomicU64,
    input_scroll_portable_applied: AtomicU64,
    input_scroll_native_failed: AtomicU64,
    input_scroll_inject_max_micros: AtomicU64,
    events: broadcast::Sender<RuntimeEvent>,
    external_input_owner: Mutex<Option<String>>,
    external_input_gate: AsyncMutex<()>,
    workspace_input_events: broadcast::Sender<WorkspaceInputSnapshot>,
    last_physical_focus_emitted: Mutex<Option<Instant>>,
}

mod availability;
mod coalesced_wakeup;
mod external;
mod input;
mod lifecycle;
mod network_runtime;
mod reconnect;
mod wire;

use wire::*;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Identity(#[from] super::identity::IdentityError),
    #[error(transparent)]
    PeerId(#[from] arcrelay_peer::PeerIdError),
    #[error(transparent)]
    DeviceId(#[from] arcrelay_peer::DeviceIdError),
    #[error(transparent)]
    Identifier(#[from] arcrelay_input::IdentifierError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Network(#[from] NetworkError),
    #[error(transparent)]
    Topology(#[from] arcrelay_input::TopologyError),
    #[error(transparent)]
    Route(#[from] arcrelay_input::RuntimeRouteError),
    #[error(transparent)]
    Control(#[from] arcrelay_input::ControlError),
    #[error(transparent)]
    Platform(#[from] arcrelay_input::PlatformError),
    #[error(transparent)]
    Display(#[from] arcrelay_input::DisplayError),
    #[error("workspace layout is not configured")]
    NoLayout,
    #[error("workspace has no display")]
    NoDisplay,
    #[error("there is no active control session")]
    NoControlSession,
    #[error("cross-screen input is disabled on this device")]
    InputSharingDisabled,
    #[error("runtime message has no header")]
    MissingHeader,
    #[error("input arrived from the wrong peer")]
    WrongPeer,
    #[error("held input state checksum mismatched")]
    HeldStateMismatch,
    #[error("the target did not prepare the handoff in time")]
    HandoffTimeout,
    #[error("a peer did not confirm input ownership in time")]
    ControlAcquireTimeout,
    #[error("the cross-screen workspace is still connecting its device mesh")]
    WorkspaceMeshIncomplete,
    #[error("the target rejected the handoff: {0}")]
    HandoffRejected(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("discovery channel closed")]
    DiscoveryClosed,
    #[error("network event channel closed")]
    NetworkClosed,
    #[error("cannot start capture router thread: {0}")]
    CaptureThread(String),
}

#[cfg(test)]
mod tests;
