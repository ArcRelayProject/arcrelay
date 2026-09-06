use super::super::windows::enumerate_monitors;
use ::windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
};
use ::windows::Win32::Graphics::Gdi::HMONITOR;
use arcrelay_input::PlatformError;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

thread_local! { static WMI: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) }; }
fn wmi(mode: &str, instance: &str, steps: i32) -> Result<Vec<u8>, PlatformError> {
    let executable = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
    )
    .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut child = Command::new(executable)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            include_str!("brightness.ps1"),
        ])
        .env("ARCRELAY_BRIGHTNESS_MODE", mode)
        .env("ARCRELAY_BRIGHTNESS_INSTANCE", instance)
        .env(
            "ARCRELAY_BRIGHTNESS_STEPS",
            steps.clamp(-20, 20).to_string(),
        )
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| PlatformError::Operation(error.to_string()))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| PlatformError::Operation(error.to_string()))?;
                return if status.success() {
                    Ok(output.stdout)
                } else {
                    Err(PlatformError::Unsupported(
                        "WMI brightness operation failed".into(),
                    ))
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PlatformError::Operation("WMI brightness timed out".into()));
            }
        }
    }
}
fn normalize(value: &str) -> String {
    let value = value
        .trim_start_matches(r"\\?\")
        .replace('#', "\\")
        .to_ascii_uppercase();
    let value = value.split("\\{").next().unwrap_or(&value);
    value.strip_suffix("_0").unwrap_or(value).to_string()
}
struct Physical(Vec<PHYSICAL_MONITOR>);
impl Drop for Physical {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyPhysicalMonitors(&self.0);
        }
    }
}
fn physical(handle: isize) -> Option<Physical> {
    unsafe {
        let monitor = HMONITOR(handle as *mut _);
        let mut count = 0;
        GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut count).ok()?;
        // Ambiguous mirrored/tiled targets are not silently mapped to a panel.
        if count != 1 {
            return None;
        }
        let mut result = Physical(vec![PHYSICAL_MONITOR::default(); count as usize]);
        GetPhysicalMonitorsFromHMONITOR(monitor, &mut result.0).ok()?;
        Some(result)
    }
}
fn read_brightness(monitors: &Physical) -> Option<(u32, u32, u32)> {
    let (mut min, mut current, mut max) = (0, 0, 0);
    let ok = unsafe {
        GetMonitorBrightness(
            monitors.0[0].hPhysicalMonitor,
            &mut min,
            &mut current,
            &mut max,
        )
    };
    (ok != 0 && min < max && (min..=max).contains(&current)).then_some((min, current, max))
}
pub(super) fn probe() -> Vec<String> {
    let names: Vec<String> = wmi("probe", "", 0)
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default();
    let mut result = Vec::new();
    let mut mapping = BTreeMap::new();
    let monitors = enumerate_monitors().unwrap_or_default();
    for monitor in &monitors {
        if monitors
            .iter()
            .filter(|other| other.display_id == monitor.display_id)
            .count()
            != 1
        {
            continue;
        }
        let matches: Vec<_> = names
            .iter()
            .filter(|name| normalize(name) == normalize(&monitor.device_instance))
            .collect();
        if matches.len() == 1 {
            mapping.insert(monitor.display_id.to_string(), matches[0].clone());
            result.push(monitor.display_id.to_string());
        } else if physical(monitor.native_handle)
            .as_ref()
            .and_then(read_brightness)
            .is_some()
        {
            result.push(monitor.display_id.to_string());
        }
    }
    WMI.with(|cache| *cache.borrow_mut() = mapping);
    result
}
pub(super) fn adjust(target: &str, steps: i32) -> Result<(), PlatformError> {
    let monitors: Vec<_> = enumerate_monitors()?
        .into_iter()
        .filter(|m| m.display_id.as_str() == target)
        .collect();
    let [monitor] = monitors.as_slice() else {
        return Err(PlatformError::Unsupported(
            "brightness display is offline or ambiguous".into(),
        ));
    };
    if let Some(name) = WMI.with(|cache| cache.borrow().get(target).cloned()) {
        if normalize(&name) != normalize(&monitor.device_instance) {
            return Err(PlatformError::Unsupported(
                "brightness display identity changed".into(),
            ));
        }
        wmi("adjust", &name, steps)?;
        return Ok(());
    }
    let physical = physical(monitor.native_handle).ok_or_else(|| {
        PlatformError::Unsupported("display has no unambiguous physical monitor".into())
    })?;
    let (min, current, max) = read_brightness(&physical)
        .ok_or_else(|| PlatformError::Unsupported("display brightness cannot be read".into()))?;
    let mut delta = i64::from(max - min).saturating_mul(i64::from(steps)) / 20;
    if delta == 0 {
        delta = i64::from(steps.signum());
    }
    let next = (i64::from(current) + delta).clamp(i64::from(min), i64::from(max)) as u32;
    if unsafe { SetMonitorBrightness(physical.0[0].hPhysicalMonitor, next) } == 0 {
        return Err(PlatformError::Operation(
            "display brightness write failed".into(),
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wmi_and_display_interface_paths_match_only_the_same_monitor() {
        assert_eq!(
            normalize(r"\\?\DISPLAY#BOE1234#4&ABC&0&UID1#{guid}"),
            normalize(r"DISPLAY\BOE1234\4&ABC&0&UID1_0")
        );
        assert_ne!(
            normalize(r"DISPLAY\BOE1234\A_0"),
            normalize(r"DISPLAY\BOE1234\B_0")
        );
    }
}
