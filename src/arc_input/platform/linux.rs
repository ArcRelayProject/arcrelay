use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, MutexGuard};

use arcrelay_input::{
    CaptureOptions, CapturedInputEvent, DisplayFingerprint, DisplayId, DisplayInventory,
    DisplayInventoryPort, DisplayRotation, DisplaySurface, GeometryConfidence, InputCapturePort,
    InputInjectionPort, InventoryRevision, LogicalPoint, LogicalRect, MappedKeyboardEvent,
    OsFamily, PlatformCapabilities, PlatformError, ScaleFactor, ScrollEvent, SizeI64, SizeU32,
    HID_KEY_FUNCTION,
};
use arcrelay_peer::ServiceInstanceId;

pub struct NativePlatform {
    device_id: ServiceInstanceId,
    capture: Mutex<Option<Child>>,
    wayland: bool,
    has_xinput: bool,
    has_xdotool: bool,
    has_libei: bool,
}

impl NativePlatform {
    pub fn new(device_id: ServiceInstanceId) -> Self {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        Self {
            device_id,
            capture: Mutex::new(None),
            wayland,
            has_xinput: command_exists("xinput"),
            has_xdotool: command_exists("xdotool"),
            has_libei: command_exists("libei-debug") || command_exists("ei-debug-events"),
        }
    }

    fn run_xdotool(&self, arguments: &[String]) -> Result<(), PlatformError> {
        if self.wayland || !self.has_xdotool {
            return Err(self.unsupported("X11 xdotool injection"));
        }
        let status = Command::new("xdotool")
            .args(arguments)
            .status()
            .map_err(|error| PlatformError::Operation(error.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError::Operation(format!(
                "xdotool exited with {status}"
            )))
        }
    }

    fn unsupported(&self, operation: &str) -> PlatformError {
        let message = if self.wayland {
            if self.has_libei {
                format!(
                    "{operation} requires compositor approval through XDG InputCapture/RemoteDesktop and EIS"
                )
            } else {
                format!(
                    "{operation} is unavailable: this Wayland compositor/session has no usable libei/EIS channel"
                )
            }
        } else {
            format!("{operation} requires xinput and xdotool")
        };
        PlatformError::Unsupported(message)
    }
}

impl InputCapturePort for NativePlatform {
    fn capabilities(&self) -> PlatformCapabilities {
        let x11_ready = !self.wayland && self.has_xinput && self.has_xdotool;
        PlatformCapabilities {
            can_capture_pointer: x11_ready,
            can_capture_keyboard: x11_ready,
            can_suppress_local_input: false,
            can_place_internal_barrier: false,
            can_inject_absolute_pointer: x11_ready,
            can_inject_keyboard: x11_ready,
            can_inject_app_pointer: false,
            can_control_elevated_apps: false,
            can_persist_permission: !self.wayland,
            can_capture_native_quartz_events: false,
            can_inject_native_quartz_events: false,
            can_capture_precision_touchpad_events: false,
            can_inject_precision_touchpad_events: false,
            can_capture_system_gestures: false,
            can_inject_system_gestures: false,
            system_gesture_format_version: 0,
            consumer_capture_mask: 0,
            consumer_inject_mask: 0,
            brightness_display_ids: Vec::new(),
            limitation: (!x11_ready).then(|| self.unsupported("Linux input sharing").to_string()),
        }
    }

    fn start(
        &self,
        options: CaptureOptions,
    ) -> Result<Receiver<CapturedInputEvent>, PlatformError> {
        if options.suppress_local {
            return Err(self.unsupported("local input suppression"));
        }
        if self.wayland || !self.has_xinput {
            return Err(self.unsupported("global input capture"));
        }
        let mut active = lock(&self.capture);
        if active.is_some() {
            return Err(PlatformError::Operation("capture is already active".into()));
        }
        let mut child = Command::new("xinput")
            .args(["test-xi2", "--root"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| PlatformError::Operation(error.to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PlatformError::Operation("xinput stdout is unavailable".into()))?;
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("arc-input-xinput-capture".into())
            .spawn(move || {
                let mut event_type = String::new();
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let line = line.trim();
                    if let Some(value) = line.strip_prefix("EVENT type ") {
                        event_type = value.to_string();
                    } else if let Some(value) = line.strip_prefix("detail:") {
                        let detail = value.trim().parse::<u16>().ok();
                        let down =
                            event_type.contains("ButtonPress") || event_type.contains("KeyPress");
                        let up = event_type.contains("ButtonRelease")
                            || event_type.contains("KeyRelease");
                        if down || up {
                            if event_type.contains("Button") {
                                if let Some(button) = detail {
                                    let _ = sender.send(CapturedInputEvent::PointerButton {
                                        hid_usage: x11_button_to_hid(button),
                                        down,
                                    });
                                }
                            } else if let Some(keycode) = detail.and_then(x11_keycode_to_hid) {
                                let _ = sender.send(CapturedInputEvent::Keyboard(
                                    MappedKeyboardEvent::Physical {
                                        hid_usage: keycode,
                                        down,
                                    },
                                ));
                            }
                        }
                    }
                }
            })
            .map_err(|error| PlatformError::Operation(error.to_string()))?;
        *active = Some(child);
        Ok(receiver)
    }

    fn set_suppress_local(&self, suppress: bool) -> Result<(), PlatformError> {
        if suppress {
            Err(self.unsupported("local input suppression"))
        } else {
            Ok(())
        }
    }

    fn current_pointer_position(&self) -> Result<LogicalPoint, PlatformError> {
        Err(self.unsupported("reading the global pointer position"))
    }

    fn stop(&self) -> Result<(), PlatformError> {
        if let Some(mut child) = lock(&self.capture).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}

impl InputInjectionPort for NativePlatform {
    fn capabilities(&self) -> PlatformCapabilities {
        InputCapturePort::capabilities(self)
    }

    fn place_pointer(&self, _: &DisplayId, point: LogicalPoint) -> Result<(), PlatformError> {
        self.run_xdotool(&[
            "mousemove".into(),
            "--sync".into(),
            point.x.round().to_string(),
            point.y.round().to_string(),
        ])
    }

    fn apply_keyboard(&self, event: &MappedKeyboardEvent) -> Result<(), PlatformError> {
        match event {
            MappedKeyboardEvent::Physical { hid_usage, .. } if *hid_usage == HID_KEY_FUNCTION => {
                Ok(())
            }
            MappedKeyboardEvent::Physical { hid_usage, down } => {
                let key = hid_to_x11_key(*hid_usage).ok_or_else(|| {
                    PlatformError::Unsupported(format!("HID key 0x{hid_usage:02x}"))
                })?;
                self.run_xdotool(&[(if *down { "keydown" } else { "keyup" }).into(), key.into()])
            }
            MappedKeyboardEvent::Semantic { action, down } => {
                let chord = arcrelay_input::target_chord(*action, OsFamily::LinuxX11);
                if *down {
                    for modifier in &chord.modifiers {
                        self.apply_keyboard(&MappedKeyboardEvent::Physical {
                            hid_usage: *modifier,
                            down: true,
                        })?;
                    }
                }
                self.apply_keyboard(&MappedKeyboardEvent::Physical {
                    hid_usage: chord.key,
                    down: *down,
                })?;
                if !down {
                    for modifier in chord.modifiers.iter().rev() {
                        self.apply_keyboard(&MappedKeyboardEvent::Physical {
                            hid_usage: *modifier,
                            down: false,
                        })?;
                    }
                }
                Ok(())
            }
            MappedKeyboardEvent::TextCommit(text) => {
                self.run_xdotool(&["type".into(), "--clearmodifiers".into(), text.clone()])
            }
        }
    }

    fn pointer_button(&self, hid_usage: u16, down: bool) -> Result<(), PlatformError> {
        let button = hid_to_x11_button(hid_usage).to_string();
        self.run_xdotool(&[(if down { "mousedown" } else { "mouseup" }).into(), button])
    }

    fn scroll(&self, event: ScrollEvent) -> Result<(), PlatformError> {
        if !event.is_finite() {
            return Err(PlatformError::Operation(
                "scroll delta is not finite".into(),
            ));
        }
        if !event.has_delta() {
            return Ok(());
        }
        let mut arguments = vec!["click".to_string(), "--repeat".into()];
        let (count, button) = if event.delta_y.abs() >= event.delta_x.abs() {
            (
                event.delta_y.abs().round().max(1.0) as u32,
                if event.delta_y > 0.0 { 4 } else { 5 },
            )
        } else {
            (
                event.delta_x.abs().round().max(1.0) as u32,
                if event.delta_x > 0.0 { 6 } else { 7 },
            )
        };
        arguments.push(count.min(32).to_string());
        arguments.push(button.to_string());
        self.run_xdotool(&arguments)
    }

    fn release_all(&self) -> Result<(), PlatformError> {
        self.run_xdotool(&["keyup".into(), "Control_L".into()]).ok();
        self.run_xdotool(&["keyup".into(), "Shift_L".into()]).ok();
        self.run_xdotool(&["keyup".into(), "Alt_L".into()]).ok();
        self.run_xdotool(&["keyup".into(), "Super_L".into()]).ok();
        Ok(())
    }
}

impl DisplayInventoryPort for NativePlatform {
    fn inventory(&self) -> Result<DisplayInventory, PlatformError> {
        if self.wayland {
            return Err(self.unsupported("Wayland display inventory"));
        }
        let output = Command::new("xrandr")
            .arg("--query")
            .output()
            .map_err(|error| PlatformError::Operation(error.to_string()))?;
        if !output.status.success() {
            return Err(PlatformError::Operation("xrandr query failed".into()));
        }
        let revision = InventoryRevision(1);
        let mut logical_outputs = BTreeMap::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.get(1) != Some(&"connected") {
                continue;
            }
            let geometry = fields.iter().find_map(|field| parse_xrandr_geometry(field));
            let Some((width, height, x, y)) = geometry else {
                continue;
            };
            let millimetres = fields.windows(3).find_map(parse_xrandr_mm);
            let (width_mm, height_mm) = millimetres.unwrap_or((
                (width as f64 * 25.4 / 96.0).round() as i64,
                (height as f64 * 25.4 / 96.0).round() as i64,
            ));
            let connector = fields[0];
            logical_outputs
                .entry((x, y, width, height))
                .and_modify(|current: &mut (String, i64, i64, bool)| {
                    if connector < current.0.as_str() {
                        current.0 = connector.to_string();
                        current.1 = width_mm;
                        current.2 = height_mm;
                        current.3 = millimetres.is_some();
                    }
                })
                .or_insert_with(|| {
                    (
                        connector.to_string(),
                        width_mm,
                        height_mm,
                        millimetres.is_some(),
                    )
                });
        }
        let mut displays = Vec::new();
        for ((x, y, width, height), (connector, width_mm, height_mm, physical_known)) in
            logical_outputs
        {
            let fingerprint =
                DisplayFingerprint::parse(format!("x11-{connector}-{width}x{height}"))
                    .map_err(|error| PlatformError::Operation(error.to_string()))?;
            let display_id = DisplayId::parse(format!("x11-{connector}"))
                .map_err(|error| PlatformError::Operation(error.to_string()))?;
            displays.push(DisplaySurface {
                display_id,
                device_id: self.device_id.clone(),
                fingerprint,
                name: connector,
                pixel_size: SizeU32 { width, height },
                logical_bounds: LogicalRect {
                    x: x as f64,
                    y: y as f64,
                    width: width as f64,
                    height: height as f64,
                },
                scale_factor: ScaleFactor(1.0),
                physical_size_um: SizeI64 {
                    width: width_mm.max(1) * 1000,
                    height: height_mm.max(1) * 1000,
                },
                rotation: DisplayRotation::Degrees0,
                desk_rect_um: arcrelay_input::DeskRectUm {
                    x: (f64::from(x) / 96.0 * 25_400.0).round() as i64,
                    y: (f64::from(y) / 96.0 * 25_400.0).round() as i64,
                    width: width_mm.max(1) * 1000,
                    height: height_mm.max(1) * 1000,
                },
                geometry_confidence: if physical_known {
                    GeometryConfidence::HardwareReported
                } else {
                    GeometryConfidence::Estimated
                },
                inventory_revision: revision,
            });
        }
        let inventory = DisplayInventory {
            device_id: self.device_id.clone(),
            revision,
            displays,
        };
        inventory
            .validate()
            .map_err(|error| PlatformError::Operation(error.to_string()))?;
        Ok(inventory)
    }
}

impl Drop for NativePlatform {
    fn drop(&mut self) {
        let _ = self.stop();
        let _ = self.release_all();
    }
}

fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {command} >/dev/null 2>&1")])
        .status()
        .is_ok_and(|status| status.success())
}

fn parse_xrandr_geometry(value: &str) -> Option<(u32, u32, i32, i32)> {
    let (size, position) = value.split_once('+')?;
    let (width, height) = size.split_once('x')?;
    let (x, y) = position.split_once('+')?;
    Some((
        width.parse().ok()?,
        height.parse().ok()?,
        x.parse().ok()?,
        y.parse().ok()?,
    ))
}

fn parse_xrandr_mm(values: &[&str]) -> Option<(i64, i64)> {
    let width = values[0].strip_suffix("mm")?.parse().ok()?;
    if values[1] != "x" {
        return None;
    }
    let height = values[2].strip_suffix("mm")?.parse().ok()?;
    Some((width, height))
}

fn x11_button_to_hid(button: u16) -> u16 {
    match button {
        1 => 1,
        2 => 3,
        3 => 2,
        value => value,
    }
}
fn hid_to_x11_button(button: u16) -> u16 {
    match button {
        1 => 1,
        2 => 3,
        3 => 2,
        value => value,
    }
}
fn x11_keycode_to_hid(keycode: u16) -> Option<u16> {
    keycode.checked_sub(8)
}

fn hid_to_x11_key(usage: u16) -> Option<&'static str> {
    Some(match usage {
        0x04 => "a",
        0x05 => "b",
        0x06 => "c",
        0x07 => "d",
        0x08 => "e",
        0x09 => "f",
        0x0a => "g",
        0x0b => "h",
        0x0c => "i",
        0x0d => "j",
        0x0e => "k",
        0x0f => "l",
        0x10 => "m",
        0x11 => "n",
        0x12 => "o",
        0x13 => "p",
        0x14 => "q",
        0x15 => "r",
        0x16 => "s",
        0x17 => "t",
        0x18 => "u",
        0x19 => "v",
        0x1a => "w",
        0x1b => "x",
        0x1c => "y",
        0x1d => "z",
        0x28 => "Return",
        0x29 => "Escape",
        0x2a => "BackSpace",
        0x2b => "Tab",
        0x2c => "space",
        0x4f => "Right",
        0x50 => "Left",
        0x51 => "Down",
        0x52 => "Up",
        0xe0 => "Control_L",
        0xe1 => "Shift_L",
        0xe2 => "Alt_L",
        0xe3 => "Super_L",
        0xe4 => "Control_R",
        0xe5 => "Shift_R",
        0xe6 => "Alt_R",
        0xe7 => "Super_R",
        _ => return None,
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
