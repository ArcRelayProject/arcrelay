//! Compile-time checked IPC reflection. build.rs extracts signatures; ts-rs
//! derives DTOs from their Rust definitions, including serde field/variant names.
use std::{
    any::TypeId,
    collections::{BTreeMap, HashSet},
};
use ts_rs::{TypeVisitor, TS};

// Keep desktop contract registration out of the Arc Input modules reused by
// standalone examples, whose test crates do not contain this registry.
mod arc_input_commands {
    use crate::arc_input::{
        commands::{EdgeTestRequest, EdgeTestResult},
        runtime::RuntimeSnapshot,
        store::WorkspaceConfiguration,
    };

    include!(concat!(env!("OUT_DIR"), "/src_arc_input_commands_ipc.rs"));
}

#[derive(Default)]
pub(crate) struct Registry {
    visited: HashSet<TypeId>,
    types: BTreeMap<String, String>,
    events: BTreeMap<String, String>,
    commands: BTreeMap<String, (BTreeMap<String, String>, String)>,
    config: ts_rs::Config,
}
impl TypeVisitor for Registry {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if !self.visited.insert(TypeId::of::<T>()) {
            return;
        }
        if T::output_path().is_some() {
            let name = T::ident(&self.config);
            let declaration = T::decl(&self.config);
            if let Some(previous) = self.types.insert(name.clone(), declaration.clone()) {
                assert_eq!(previous, declaration, "ambiguous IPC type: {name}");
            }
        }
        T::visit_dependencies(self);
        T::visit_generics(self);
    }
}
impl Registry {
    pub fn ty<T: TS + 'static + ?Sized>(&mut self) -> String {
        self.visit::<T>();
        T::name(&self.config)
    }
    pub fn command(&mut self, name: &str, args: BTreeMap<String, String>, result: String) {
        assert!(
            self.commands.insert(name.into(), (args, result)).is_none(),
            "duplicate command {name}"
        );
    }
    fn event<T: TS + 'static>(&mut self, name: &str) {
        let ty = self.ty::<T>();
        self.events.insert(name.into(), ty);
    }
    fn render(&self) -> String {
        let mut output =
            String::from("// Generated from Rust. Run npm run generate:ipc; do not edit.\n\n");
        for definition in self.types.values() {
            output.push_str("export ");
            output.push_str(definition);
            output.push_str("\n\n");
        }
        output.push_str("export interface CommandMap {\n");
        for (name, (args, result)) in &self.commands {
            output.push_str(&format!("  {name}: {{ args: {{ "));
            for (name, ty) in args {
                output.push_str(&format!("{name}: {ty}; "));
            }
            output.push_str(&format!("}}; result: {result} }};\n"));
        }
        output.push_str("}\n\nexport interface EventMap {\n");
        for (name, ty) in &self.events {
            output.push_str(&format!("  {name:?}: {ty};\n"));
        }
        output.push_str("}\n");
        output
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }
}
#[test]
fn ipc_contract_matches_rust() {
    let mut registry = Registry {
        config: ts_rs::Config::default().with_large_int("number"),
        ..Registry::default()
    };
    crate::commands::register_ipc_contract(&mut registry);
    crate::app_update::register_ipc_contract(&mut registry);
    crate::gesture_debug::register_ipc_contract(&mut registry);
    crate::gaze::register_ipc_contract(&mut registry);
    arc_input_commands::register_ipc_contract(&mut registry);
    registry.ty::<crate::ipc::IpcError>();
    registry.event::<crate::ipc::IpcError>("print-job-activity-error");
    registry.event::<crate::backend::BootstrapState>("desktop-state");
    registry.event::<crate::backend::DesktopRuntimeState>("desktop-runtime");
    registry.event::<crate::settings::AppSettings>("app-settings-changed");
    registry.event::<Option<i64>>("sound-mute-changed");
    registry.event::<crate::desktop_notification::InAppNotification>("desktop-notification");
    registry.event::<crate::application::output_manager::ActionOutputSnapshot>("action-output");
    registry.event::<arcrelay_transfer::TransferSnapshot>("transfer-state");
    registry.event::<arcrelay_transfer::TransferProgress>("transfer-progress");
    registry.event::<String>("open-transfer-request");
    registry.event::<()>("tray-navigation-pending");
    registry.event::<()>("tray-transfer-drop-changed");
    registry.event::<crate::system_share::SystemShareRequest>(
        crate::system_share::OPEN_SYSTEM_SHARE_EVENT,
    );
    registry.event::<Vec<crate::application::runtime_modules::ModuleStatus>>("runtime-modules");
    registry.event::<crate::arc_input::runtime::RuntimeSnapshot>("arc-input-state");
    registry.event::<crate::gaze::GazeStatusView>("gaze-state");
    registry.event::<crate::gaze::GazePreviewView>("gaze-preview");
    registry.event::<serde_json::Value>("gaze-calibration-flow");
    registry.event::<String>("gaze-calibration-cancel");
    registry.event::<crate::commands::PrintJobActivitySnapshot>("print-job-activity");
    registry.event::<arcrelay_automation::AutomationActivity>("automation-activity");
    registry.event::<()>("automation-configuration");
    registry.event::<crate::privacy::PrivacySnapshot>("privacy-state");
    registry.event::<crate::app_update::AppUpdateCheckResult>("app-update-checked");
    registry.event::<crate::app_update::AppUpdateProgress>("app-update-progress");
    registry.event::<arcrelay_web_gateway::WebGatewayStatus>("web-gateway-status");
    registry.event::<Option<crate::backend::InputMetricsView>>("input-metrics");
    registry.event::<usize>("notification-count");
    registry.event::<()>("notifications-changed");
    registry.event::<()>("clipboard-changed");
    registry.event::<u64>("clipboard-ocr-changed");
    registry.event::<()>("clipboard-window-shown");
    registry.event::<()>("clipboard-window-hidden");
    registry.event::<bool>("clipboard-window-pin-changed");
    registry.event::<String>("clipboard-continuous-paste-error");
    let main = include_str!("main.rs");
    let registered: std::collections::BTreeSet<_> = main
        .split("generate_handler![")
        .nth(1)
        .unwrap()
        .split("])")
        .next()
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.trim_end_matches(',')
                .rsplit("::")
                .next()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        registered,
        registry.commands.keys().cloned().collect(),
        "every registered command must participate in the generated contract"
    );
    let rendered = registry.render();
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/src/ipc/generated.ts");
    if std::env::var_os("ARCRELAY_GENERATE_IPC").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, rendered).unwrap();
    } else {
        let existing = std::fs::read_to_string(path).expect("generate IPC bindings first");
        assert!(
            existing == rendered,
            "Rust IPC contract changed: run npm run generate:ipc"
        );
    }
}
