use super::*;

impl ArcInputRuntime {
    pub(super) fn display_availability(
        &self,
        configuration: &WorkspaceConfiguration,
    ) -> BTreeMap<DisplayId, DisplayAvailability> {
        let local = &self.identity.service_instance_id;
        let connected = self.network.connected_peers();
        let capabilities = read(&self.remote_capabilities);
        let enabled = read(&self.remote_sharing_enabled);
        let inventories = read(&self.remote_inventories);
        let local_inventory = read(&self.local_inventory);
        let local_capabilities = InputCapturePort::capabilities(self.platform.as_ref());
        // Network convergence tests supply permission states explicitly; CI
        // must not need access to the operator's physical keyboard or mouse.
        #[cfg(test)]
        let local_capabilities = read(&self.availability_capabilities)
            .clone()
            .unwrap_or(local_capabilities);
        configuration
            .remembered_displays
            .values()
            .chain(
                configuration
                    .layout
                    .iter()
                    .flat_map(|layout| layout.displays.values()),
            )
            .map(|display| {
                let device = &display.device_id;
                let status = if device != local && !connected.contains(device) {
                    DisplayAvailability::Offline
                } else if (device == local && !configuration.input_sharing_enabled)
                    || (device != local && enabled.get(device) == Some(&false))
                {
                    DisplayAvailability::SharingDisabled
                } else {
                    let capability = if device == local {
                        Some(&local_capabilities)
                    } else {
                        capabilities.get(device)
                    };
                    let inventory = if device == local {
                        local_inventory.as_ref()
                    } else {
                        inventories.get(device)
                    };
                    if capability.is_none() || inventory.is_none() {
                        DisplayAvailability::Connecting
                    } else if capability.is_some_and(|value| !value.can_target()) {
                        DisplayAvailability::PermissionRequired
                    } else if inventory.is_some_and(|value| {
                        !value
                            .displays
                            .iter()
                            .any(|actual| actual.display_id == display.display_id)
                    }) {
                        DisplayAvailability::DisplayDisconnected
                    } else {
                        DisplayAvailability::Ready
                    }
                };
                (display.display_id.clone(), status)
            })
            .collect()
    }

    pub(super) fn refresh_portal_availability(&self, configuration: &mut WorkspaceConfiguration) {
        let availability = self.display_availability(configuration);
        if let Some(layout) = configuration.layout.as_mut() {
            for portal in &mut layout.portals {
                let states = [&portal.source_display, &portal.target_display].map(|id| {
                    availability
                        .get(id)
                        .copied()
                        .unwrap_or(DisplayAvailability::Offline)
                });
                portal.status = if states
                    .iter()
                    .all(|state| *state == DisplayAvailability::Ready)
                {
                    PortalStatus::Active
                } else if states.contains(&DisplayAvailability::PermissionRequired) {
                    PortalStatus::UnsupportedCapability
                } else {
                    PortalStatus::SuspendedOffline
                };
            }
        }
    }

    pub(super) fn input_component(&self) -> BTreeSet<ServiceInstanceId> {
        // Portals are refreshed on connection/capability/inventory changes.
        // Do not query native permission APIs for every captured mouse sample.
        let configuration = self.store.snapshot();
        configuration
            .layout
            .as_ref()
            .map(|layout| {
                arcrelay_input::active_component_devices(layout, &self.identity.service_instance_id)
            })
            .unwrap_or_default()
    }

    pub(super) fn is_application_receiver(&self, peer: &ServiceInstanceId) -> bool {
        read(&self.remote_capabilities)
            .get(peer)
            .is_some_and(|caps| caps.can_inject_app_pointer && !caps.can_source())
    }

    /// Application receivers do not arbitrate desktop ownership. Each computer
    /// can have its own paired application targets without changing the set of
    /// desktop voters seen by the other computers.
    pub(super) fn input_control_component(&self) -> BTreeSet<ServiceInstanceId> {
        self.input_component()
            .into_iter()
            .filter(|peer| !self.is_application_receiver(peer))
            .collect()
    }

    /// A split/merge never extends an old token to new participants. Releasing
    /// first also cancels held input and in-flight native gestures on removal.
    pub(super) fn reconcile_control_component(&self) -> Result<(), RuntimeError> {
        let locally_controlled = lock(&self.session)
            .as_ref()
            .is_none_or(|session| session.controller == self.identity.service_instance_id);
        let component = if locally_controlled {
            self.input_component()
        } else {
            self.input_control_component()
        };
        let invalid =
            lock(&self.session).as_ref().is_some_and(|session| {
                *lock(&self.control_participants) != component
                    || !component.contains(&session.controller)
                    || !component.contains(&session.current_target)
                    || self.store.snapshot().layout.as_ref().is_none_or(|layout| {
                        !layout.displays.contains_key(&session.current_display)
                    })
            });
        if invalid {
            tracing::info!(
                event = "input.control.component_changed",
                previous_participant_count = lock(&self.control_participants).len(),
                available_participant_count = component.len(),
                "Arc Input releases ownership before changing its online screen component"
            );
            self.release_control()?;
        }
        let mut observed = lock(&self.observed_control);
        if observed
            .active
            .as_ref()
            .is_some_and(|(controller, _)| !component.contains(controller))
        {
            observed.active = None;
        }
        Ok(())
    }
}
