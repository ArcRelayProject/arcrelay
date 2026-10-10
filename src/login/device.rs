use zeroize::Zeroizing;
#[cfg(target_os = "macos")]
mod macos;

pub fn available() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::available()
    }
    #[cfg(target_os = "windows")]
    {
        let Ok(_apartment) = Apartment::new() else {
            return false;
        };
        use windows::Security::Credentials::UI::{
            UserConsentVerifier, UserConsentVerifierAvailability,
        };
        UserConsentVerifier::CheckAvailabilityAsync()
            .and_then(|o| o.get())
            .is_ok_and(|v| v == UserConsentVerifierAvailability::Available)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}
pub fn store(id: &str, key: &[u8; 32], window: isize) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let _ = window;
        macos::store(id, key)
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Security::Cryptography::{
            CryptographicBuffer, DataProtection::DataProtectionProvider,
        };
        if !available() {
            return Err("Windows Hello is unavailable".into());
        }
        let _apartment = Apartment::new()?;
        verify(window)?;
        let source =
            CryptographicBuffer::CreateFromByteArray(key).map_err(|_| "Key protection failed")?;
        let protected = DataProtectionProvider::CreateOverloadExplicit(&"LOCAL=user".into())
            .and_then(|p| p.ProtectAsync(&source))
            .and_then(|o| o.get())
            .map_err(|_| "Key protection failed")?;
        let mut bytes = windows_core::Array::new();
        CryptographicBuffer::CopyToByteArray(&protected, &mut bytes)
            .map_err(|_| "Key protection failed")?;
        std::fs::write(path(id), bytes.as_slice())
            .map_err(|_| "Cannot save the system-protected key".into())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (id, key, window);
        Err("Quick unlock is not supported on this system".into())
    }
}
pub fn load(id: &str, window: isize) -> Result<Zeroizing<[u8; 32]>, String> {
    #[cfg(target_os = "macos")]
    let bytes = {
        let _ = window;
        macos::load(id)?
    };
    #[cfg(target_os = "windows")]
    let bytes = {
        use windows::Security::Cryptography::{
            CryptographicBuffer, DataProtection::DataProtectionProvider,
        };
        let _apartment = Apartment::new()?;
        verify(window)?;
        let encrypted = std::fs::read(path(id)).map_err(|_| "Quick unlock key is unavailable")?;
        let source = CryptographicBuffer::CreateFromByteArray(&encrypted)
            .map_err(|_| "Key reading failed")?;
        let plain = DataProtectionProvider::new()
            .and_then(|p| p.UnprotectAsync(&source))
            .and_then(|o| o.get())
            .map_err(|_| "Key reading failed")?;
        let mut bytes = windows_core::Array::new();
        CryptographicBuffer::CopyToByteArray(&plain, &mut bytes)
            .map_err(|_| "Key reading failed")?;
        let result = Zeroizing::new(bytes.to_vec());
        zeroize::Zeroize::zeroize(&mut bytes[..]);
        result
    };
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        if bytes.len() != 32 {
            return Err("Invalid quick unlock key".into());
        }
        let mut key = Zeroizing::new([0; 32]);
        key.copy_from_slice(&bytes);
        Ok(key)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (id, window);
        Err("Quick unlock is not supported on this system".into())
    }
}
pub fn remove(id: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::remove(id)
    }
    #[cfg(target_os = "windows")]
    {
        match std::fs::remove_file(path(id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("Cannot remove the system quick unlock key".into()),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = id;
        Ok(())
    }
}
#[cfg(target_os = "windows")]
fn path(id: &str) -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ArcRelay")
        .join(format!("login-hello-{id}.key"))
}
#[cfg(target_os = "windows")]
fn verify(window: isize) -> Result<(), String> {
    use windows::Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier};
    use windows::Win32::{Foundation::HWND, System::WinRT::IUserConsentVerifierInterop};
    let factory: IUserConsentVerifierInterop =
        windows_core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>()
            .map_err(|_| "This Windows version does not support desktop quick unlock")?;
    let operation: windows_future::IAsyncOperation<UserConsentVerificationResult> = unsafe {
        factory.RequestVerificationForWindowAsync(
            HWND(window as *mut _),
            &"解锁 ArcRelay 登录信息".into(),
        )
    }
    .map_err(|_| "Windows Hello verification failed")?;
    let result = operation
        .get()
        .map_err(|_| "Windows Hello verification failed")?;
    if result == UserConsentVerificationResult::Verified {
        Ok(())
    } else {
        Err("Windows Hello verification was not completed".into())
    }
}

#[cfg(target_os = "windows")]
pub fn observe_sleep(login: std::sync::Arc<super::LoginService>, app: tauri::AppHandle) {
    use tauri::Emitter;
    use windows::Win32::{
        Foundation::HANDLE,
        System::Power::{
            PowerRegisterSuspendResumeNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS,
        },
        UI::WindowsAndMessaging::DEVICE_NOTIFY_CALLBACK,
    };
    struct Observer {
        login: std::sync::Arc<super::LoginService>,
        app: tauri::AppHandle,
    }
    unsafe extern "system" fn callback(
        context: *const core::ffi::c_void,
        kind: u32,
        _: *const core::ffi::c_void,
    ) -> u32 {
        if kind == 4 {
            // PBT_APMSUSPEND
            let observer = unsafe { &*context.cast::<Observer>() };
            observer.login.lock();
            let _ = observer.app.emit("login-vault-locked", ());
        }
        0
    }
    let observer = Box::into_raw(Box::new(Observer { login, app }));
    let parameters = Box::into_raw(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
        Callback: Some(callback),
        Context: observer.cast(),
    }));
    let mut handle = std::ptr::null_mut();
    let result = unsafe {
        PowerRegisterSuspendResumeNotification(
            DEVICE_NOTIFY_CALLBACK,
            HANDLE(parameters.cast()),
            &mut handle,
        )
    };
    if result.0 != 0 {
        unsafe {
            drop(Box::from_raw(parameters));
            drop(Box::from_raw(observer));
        }
        tracing::warn!(
            "system suspend notification unavailable; clock and session guard remain active"
        );
    }
    // The OS retains the callbacks for this process lifetime; no runtime or UI object is captured.
}

#[cfg(target_os = "windows")]
struct Apartment(bool);
#[cfg(target_os = "windows")]
impl Apartment {
    fn new() -> Result<Self, String> {
        use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
        match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(()) => Ok(Self(true)),
            Err(e) if e.code().0 == 0x80010106u32 as i32 => Ok(Self(false)), // existing STA
            Err(_) => Err("System verification service initialization failed".into()),
        }
    }
}
#[cfg(target_os = "windows")]
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                windows::Win32::System::WinRT::RoUninitialize();
            }
        }
    }
}
