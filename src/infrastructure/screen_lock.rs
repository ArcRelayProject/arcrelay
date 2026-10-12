//! macOS no longer ships the CGSession command. Resolve the login framework at
//! runtime so an unavailable OS primitive is reported before enabling a rule.
//! This is a private OS API; never substitute display sleep for screen locking.
#[cfg(target_os = "macos")]
fn lock_function() -> Option<unsafe extern "C" fn() -> i32> {
    use std::sync::OnceLock;
    type Lock = unsafe extern "C" fn() -> i32;
    static FUNCTION: OnceLock<Option<Lock>> = OnceLock::new();
    *FUNCTION.get_or_init(|| unsafe {
        let handle = libc::dlopen(
            c"/System/Library/PrivateFrameworks/login.framework/Versions/Current/login".as_ptr(),
            libc::RTLD_LAZY | libc::RTLD_LOCAL,
        );
        if handle.is_null() {
            return None;
        }
        let symbol = libc::dlsym(handle, c"SACLockScreenImmediate".as_ptr());
        if symbol.is_null() {
            libc::dlclose(handle);
            None
        } else {
            // Keep the framework loaded for the lifetime of this function pointer.
            Some(std::mem::transmute::<*mut libc::c_void, Lock>(symbol))
        }
    })
}

pub fn available() -> bool {
    #[cfg(target_os = "macos")]
    return lock_function().is_some();
    #[cfg(not(target_os = "macos"))]
    true
}

#[cfg(target_os = "macos")]
pub fn lock() -> Result<String, String> {
    let function =
        lock_function().ok_or("The screen lock interface is unavailable on this system")?;
    let status = unsafe { function() };
    if status == 0 {
        Ok("已请求系统锁屏".into())
    } else {
        Err(format!("system screen lock failed with status {status}"))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn resolves_without_a_runtime_or_executing_lock() {
        // Only load/resolve the symbol: this test must never lock the developer's
        // or CI runner's session. Do not require availability on every OS release.
        assert_eq!(super::available(), super::lock_function().is_some());
    }
}
