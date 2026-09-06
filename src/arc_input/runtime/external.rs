use super::*;

use arcrelay_core::domain::input_control::InputEvent as DomainInputEvent;
use arcrelay_protocol::server::{
    HostCapabilityError, HostCapabilityErrorCode, HostCapabilityResult, WorkspaceInputFocusCause,
    WorkspaceInputRouteState, WorkspaceInputRouter, WorkspaceInputSnapshot,
};

pub struct WorkspaceInputRouterAdapter {
    runtime: Arc<ArcInputRuntime>,
}

impl WorkspaceInputRouterAdapter {
    pub fn new(runtime: Arc<ArcInputRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait]
impl WorkspaceInputRouter for WorkspaceInputRouterAdapter {
    fn available(&self) -> bool {
        self.runtime.workspace_input_available()
    }

    fn subscribe(&self) -> broadcast::Receiver<WorkspaceInputSnapshot> {
        self.runtime.subscribe_workspace_input()
    }

    async fn begin(&self, owner_device_id: &str) -> HostCapabilityResult<WorkspaceInputSnapshot> {
        self.runtime
            .begin_external_input(owner_device_id)
            .await
            .map_err(workspace_input_error)
    }

    async fn apply(
        &self,
        owner_device_id: &str,
        events: &[DomainInputEvent],
    ) -> HostCapabilityResult<WorkspaceInputSnapshot> {
        self.runtime
            .apply_external_input(owner_device_id, events)
            .await
            .map_err(workspace_input_error)
    }

    async fn end(&self, owner_device_id: &str, cause: WorkspaceInputFocusCause) {
        if let Err(error) = self
            .runtime
            .end_external_input(owner_device_id, cause)
            .await
        {
            tracing::debug!(%error, "Arc Input external session was already inactive");
        }
    }
}

fn workspace_input_error(error: RuntimeError) -> HostCapabilityError {
    let code = match error {
        RuntimeError::InputSharingDisabled
        | RuntimeError::NoLayout
        | RuntimeError::NoDisplay
        | RuntimeError::NoControlSession => HostCapabilityErrorCode::FailedPrecondition,
        RuntimeError::WorkspaceMeshIncomplete
        | RuntimeError::HandoffTimeout
        | RuntimeError::ControlAcquireTimeout => HostCapabilityErrorCode::Unavailable,
        RuntimeError::InvalidInput(_) => HostCapabilityErrorCode::InvalidArgument,
        _ => HostCapabilityErrorCode::Internal,
    };
    HostCapabilityError::new(code, error.to_string())
}

impl ArcInputRuntime {
    pub fn workspace_input_adapter(self: &Arc<Self>) -> Arc<dyn WorkspaceInputRouter> {
        Arc::new(WorkspaceInputRouterAdapter::new(self.clone()))
    }

    pub fn workspace_input_available(&self) -> bool {
        self.input_sharing_enabled()
            && self.store.snapshot().layout.is_some()
            && self.workspace_mesh_connected()
            && InputInjectionPort::capabilities(self.platform.as_ref()).can_target()
    }

    pub fn subscribe_workspace_input(&self) -> broadcast::Receiver<WorkspaceInputSnapshot> {
        self.workspace_input_events.subscribe()
    }

    pub(super) fn external_input_owner(&self) -> Option<String> {
        lock(&self.external_input_owner).clone()
    }

    pub async fn begin_external_input(
        self: &Arc<Self>,
        owner_device_id: &str,
    ) -> Result<WorkspaceInputSnapshot, RuntimeError> {
        let _external_gate = self.external_input_gate.lock().await;
        if owner_device_id.trim().is_empty() {
            return Err(RuntimeError::InvalidInput(
                "external input owner is empty".into(),
            ));
        }
        let already_owned = {
            let mut owner = lock(&self.external_input_owner);
            match owner.as_deref() {
                Some(current) if current == owner_device_id => true,
                Some(_) => {
                    return Err(RuntimeError::InvalidInput(
                        "workspace input is already controlled by another mobile device".into(),
                    ));
                }
                None => {
                    *owner = Some(owner_device_id.to_string());
                    false
                }
            }
        };
        if already_owned {
            return self
                .workspace_snapshot(
                    WorkspaceInputRouteState::Active,
                    WorkspaceInputFocusCause::SessionStarted,
                    String::new(),
                )
                .ok_or(RuntimeError::NoControlSession);
        }
        if let Err(error) = self.take_control().await {
            *lock(&self.external_input_owner) = None;
            return Err(error);
        }
        if self.external_input_owner().as_deref() != Some(owner_device_id) {
            if lock(&self.session)
                .as_ref()
                .is_some_and(|session| session.controller == self.identity.service_instance_id)
            {
                self.release_control()?;
            }
            return Err(RuntimeError::NoControlSession);
        }
        // A previous local controller can carry physical held state. The
        // mobile session starts at a clean ordering boundary.
        if let Err(error) = self.release_external_held().await {
            *lock(&self.external_input_owner) = None;
            let _ = self.release_control();
            return Err(error);
        }
        if self.external_input_owner().as_deref() != Some(owner_device_id) {
            if lock(&self.session)
                .as_ref()
                .is_some_and(|session| session.controller == self.identity.service_instance_id)
            {
                self.release_control()?;
            }
            return Err(RuntimeError::NoControlSession);
        }
        let snapshot = self
            .workspace_snapshot(
                WorkspaceInputRouteState::Active,
                WorkspaceInputFocusCause::SessionStarted,
                String::new(),
            )
            .ok_or(RuntimeError::NoControlSession)?;
        let _ = self.workspace_input_events.send(snapshot.clone());
        Ok(snapshot)
    }

    pub async fn apply_external_input(
        self: &Arc<Self>,
        owner_device_id: &str,
        events: &[DomainInputEvent],
    ) -> Result<WorkspaceInputSnapshot, RuntimeError> {
        let _external_gate = self.external_input_gate.lock().await;
        if lock(&self.external_input_owner).as_deref() != Some(owner_device_id) {
            return Err(RuntimeError::NoControlSession);
        }
        if !lock(&self.session)
            .as_ref()
            .is_some_and(|session| session.controller == self.identity.service_instance_id)
        {
            return Err(RuntimeError::NoControlSession);
        }
        for event in events {
            self.route_external_domain_event(event.clone()).await?;
        }
        self.workspace_snapshot(
            WorkspaceInputRouteState::Active,
            WorkspaceInputFocusCause::SessionStarted,
            String::new(),
        )
        .ok_or(RuntimeError::NoControlSession)
    }

    pub async fn end_external_input(
        &self,
        owner_device_id: &str,
        cause: WorkspaceInputFocusCause,
    ) -> Result<(), RuntimeError> {
        let _external_gate = self.external_input_gate.lock().await;
        let owner = {
            let mut current = lock(&self.external_input_owner);
            if current.as_deref() != Some(owner_device_id) {
                return Ok(());
            }
            current.take()
        };
        let snapshot =
            self.workspace_snapshot(WorkspaceInputRouteState::Ended, cause, String::new());
        self.release_control()?;
        if let Some(mut snapshot) = snapshot {
            snapshot.owner_device_id = owner;
            let _ = self.workspace_input_events.send(snapshot);
        }
        Ok(())
    }

    pub(super) fn suspend_external_input_for_physical(
        &self,
        target_device_id: ServiceInstanceId,
        target_display_id: DisplayId,
        control_epoch: u64,
        release_control: bool,
    ) -> Result<bool, RuntimeError> {
        let owner = lock(&self.external_input_owner).take();
        let Some(owner) = owner else { return Ok(false) };
        if release_control {
            self.release_control()?;
        }
        let supports_system_gestures =
            self.external_system_gesture_target_version(&target_device_id) != 0;
        let snapshot = WorkspaceInputSnapshot {
            owner_device_id: Some(owner),
            controller_device_id: target_device_id.to_string(),
            logical_target_device_id: target_device_id.to_string(),
            target_display_id: target_display_id.to_string(),
            control_epoch,
            supports_system_gestures,
            state: WorkspaceInputRouteState::Suspended,
            cause: WorkspaceInputFocusCause::PhysicalActivity,
            message: "physical keyboard or pointer activity took control".into(),
        };
        let _ = self.workspace_input_events.send(snapshot);
        Ok(true)
    }

    pub(super) fn emit_workspace_focus(&self, cause: WorkspaceInputFocusCause) {
        if let Some(snapshot) =
            self.workspace_snapshot(WorkspaceInputRouteState::Active, cause, String::new())
        {
            let _ = self.workspace_input_events.send(snapshot);
        }
    }

    pub(super) fn emit_physical_focus_for_target(
        &self,
        target_device_id: &ServiceInstanceId,
        target_display_id: &DisplayId,
        control_epoch: u64,
    ) {
        let supports_system_gestures =
            self.external_system_gesture_target_version(target_device_id) != 0;
        let _ = self.workspace_input_events.send(WorkspaceInputSnapshot {
            owner_device_id: None,
            controller_device_id: target_device_id.to_string(),
            logical_target_device_id: target_device_id.to_string(),
            target_display_id: target_display_id.to_string(),
            control_epoch,
            supports_system_gestures,
            state: WorkspaceInputRouteState::Active,
            cause: WorkspaceInputFocusCause::PhysicalActivity,
            message: String::new(),
        });
    }

    pub(super) fn emit_idle_physical_focus(&self) -> Result<(), RuntimeError> {
        const MIN_INTERVAL: Duration = Duration::from_millis(750);
        {
            let mut last = lock(&self.last_physical_focus_emitted);
            if last.is_some_and(|last| last.elapsed() < MIN_INTERVAL) {
                return Ok(());
            }
            *last = Some(Instant::now());
        }
        let logical = self.platform.current_pointer_position()?;
        let inventory = read(&self.local_inventory)
            .clone()
            .map(Ok)
            .unwrap_or_else(|| self.platform.inventory())?;
        let display = inventory
            .displays
            .iter()
            .find(|display| display.contains_logical_point(logical))
            .or_else(|| inventory.displays.first())
            .ok_or(RuntimeError::NoDisplay)?;
        let epoch = self
            .control_epoch_clock
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1)
            .max(1);
        self.emit_physical_focus_for_target(
            &self.identity.service_instance_id,
            &display.display_id,
            epoch,
        );
        Ok(())
    }

    fn workspace_snapshot(
        &self,
        state: WorkspaceInputRouteState,
        cause: WorkspaceInputFocusCause,
        message: String,
    ) -> Option<WorkspaceInputSnapshot> {
        let session = lock(&self.session).clone()?;
        Some(WorkspaceInputSnapshot {
            owner_device_id: lock(&self.external_input_owner).clone(),
            controller_device_id: session.controller.to_string(),
            logical_target_device_id: session.current_target.to_string(),
            target_display_id: session.current_display.to_string(),
            control_epoch: session.control_epoch.0,
            supports_system_gestures: self
                .external_system_gesture_target_version(&session.current_target)
                != 0,
            state,
            cause,
            message,
        })
    }
}
