#[cfg(any(target_os = "macos", target_os = "windows"))]
mod brightness;
#[cfg(any(target_os = "windows", test))]
mod consumer;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::NativePlatform;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::NativePlatform;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::NativePlatform;

#[cfg(any(target_os = "windows", test))]
fn decode_windows_wheel(mouse_data: u32) -> (f64, bool) {
    const WHEEL_DELTA: f64 = 120.0;
    let delta = f64::from((mouse_data >> 16) as u16 as i16) / WHEEL_DELTA;
    // Windows wheel messages express rotation in detents, not pixels.
    // Sending these as precise scroll deltas makes macOS interpret one
    // ordinary detent as one pixel instead of one line.
    (delta, false)
}

#[cfg(any(target_os = "windows", test))]
const COMPATIBLE_WHEEL_DELTA: i32 = 120;
#[cfg(any(target_os = "windows", test))]
const COMPATIBLE_WHEEL_END_FLUSH_THRESHOLD: f64 = 60.0;

#[cfg(any(target_os = "windows", test))]
fn take_compatible_wheel_delta(residual: &mut f64, flush: bool) -> i32 {
    let detents = (*residual / f64::from(COMPATIBLE_WHEEL_DELTA)).trunc() as i32;
    let mut emitted = detents.saturating_mul(COMPATIBLE_WHEEL_DELTA);
    if emitted == 0 && flush && residual.abs() >= COMPATIBLE_WHEEL_END_FLUSH_THRESHOLD {
        emitted = residual.signum() as i32 * COMPATIBLE_WHEEL_DELTA;
    }
    *residual -= f64::from(emitted);
    if flush {
        *residual = 0.0;
    }
    emitted
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod unsupported;
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub use unsupported::NativePlatform;

#[cfg(test)]
mod tests {
    use super::{
        decode_windows_wheel, take_compatible_wheel_delta, COMPATIBLE_WHEEL_DELTA,
        COMPATIBLE_WHEEL_END_FLUSH_THRESHOLD,
    };

    #[test]
    fn windows_wheel_delta_is_line_based_and_preserves_direction() {
        assert_eq!(decode_windows_wheel(120_u32 << 16), (1.0, false));
        assert_eq!(
            decode_windows_wheel(u32::from((-120_i16) as u16) << 16),
            (-1.0, false)
        );
        assert_eq!(decode_windows_wheel(30_u32 << 16), (0.25, false));
    }

    #[test]
    fn compatible_wheel_accumulates_complete_detents() {
        let mut residual = 0.0;
        let mut emitted_total = 0;
        for _ in 0..40 {
            residual += 0.25 * 12.0;
            emitted_total += take_compatible_wheel_delta(&mut residual, false);
        }
        assert_eq!(emitted_total, COMPATIBLE_WHEEL_DELTA);
        assert_eq!(residual, 0.0);
    }

    #[test]
    fn compatible_wheel_flushes_a_deliberate_short_gesture() {
        let mut residual = COMPATIBLE_WHEEL_END_FLUSH_THRESHOLD;
        assert_eq!(
            take_compatible_wheel_delta(&mut residual, true),
            COMPATIBLE_WHEEL_DELTA
        );
        assert_eq!(residual, 0.0);
    }
}
