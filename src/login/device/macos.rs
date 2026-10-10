//! Developer ID builds have no provisioned data-protection keychain access group.
//! Authenticate each operation with LocalAuthentication, then use the file-based
//! Keychain's application ACL. Never retry a failed verification as a plain read.
use objc2_local_authentication::{LAContext, LAPolicy};
use security_framework::passwords::{self, PasswordOptions};
use zeroize::Zeroizing;

const SERVICE: &str = "cn.arcrelay.login.system-verification";
const LEGACY_SERVICE: &str = "cn.arcrelay.login.user-presence";
const VERIFICATION_FAILED: &str = "System verification was not completed; use the master password";

pub(super) fn available() -> bool {
    // No prompt, runtime, or cached authentication context is created here.
    unsafe {
        LAContext::new()
            .canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication)
            .is_ok()
    }
}

fn verify() -> Result<(), String> {
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use std::{sync::mpsc, time::Duration};

    // A fresh context prevents an earlier successful verification being reused.
    let context = unsafe { LAContext::new() };
    unsafe { context.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication) }.map_err(
        |e| {
            tracing::warn!(
                event = "login.system_verification_unavailable",
                code = e.code()
            );
            "System user verification is unavailable".to_string()
        },
    )?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let reply = block2::RcBlock::new(move |success: Bool, error: *mut NSError| {
        if !success.as_bool() {
            // Log only the OS error code, never credentials or identifying data.
            let code = unsafe { error.as_ref() }.map(NSError::code);
            tracing::warn!(event = "login.system_verification_failed", code);
        }
        let _ = sender.try_send(success.as_bool());
    });
    unsafe {
        context.evaluatePolicy_localizedReason_reply(
            LAPolicy::DeviceOwnerAuthentication,
            &NSString::from_str("解锁登录信息"),
            &reply,
        );
    }
    let verified = receiver.recv_timeout(Duration::from_secs(120));
    unsafe { context.invalidate() };
    if verified == Ok(true) {
        Ok(())
    } else {
        Err(VERIFICATION_FAILED.into())
    }
}

fn after_verification<T>(
    verification: impl FnOnce() -> Result<(), String>,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    verification()?;
    operation()
}

pub(super) fn store(id: &str, key: &[u8; 32]) -> Result<(), String> {
    after_verification(verify, || {
        // No synchronizable/access-control attributes: those select the
        // provisioned data-protection store rather than the login Keychain.
        passwords::set_generic_password(SERVICE, id, key)
            .map_err(|e| keychain_error("store", e, "Cannot save the system quick unlock key"))
    })
}

pub(super) fn load(id: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    after_verification(verify, || {
        let bytes =
            match passwords::generic_password(PasswordOptions::new_generic_password(SERVICE, id)) {
                Ok(bytes) => bytes,
                Err(e) if e.code() == -25300 => {
                    // Keep previously provisioned builds readable. Their native
                    // user-presence ACL remains enforced by Security.framework.
                    passwords::generic_password(legacy_options(id))
                        .map_err(|e| keychain_error("load", e, "Quick unlock key is unavailable"))?
                }
                Err(e) => return Err(keychain_error("load", e, "Quick unlock key is unavailable")),
            };
        Ok(Zeroizing::new(bytes))
    })
}

fn legacy_options(id: &str) -> PasswordOptions {
    let mut options = PasswordOptions::new_generic_password(LEGACY_SERVICE, id);
    options.use_protected_keychain();
    options.set_access_synchronized(Some(false));
    options
}

pub(super) fn remove(id: &str) -> Result<(), String> {
    match passwords::delete_generic_password(SERVICE, id) {
        Ok(()) => {}
        Err(e) if e.code() == -25300 => {}
        Err(e) => {
            return Err(keychain_error(
                "remove",
                e,
                "Cannot remove the system quick unlock key",
            ))
        }
    }
    match passwords::delete_generic_password_options(legacy_options(id)) {
        Ok(()) => Ok(()),
        // Unprovisioned builds could never create the legacy protected item.
        Err(e) if matches!(e.code(), -25300 | -34018) => Ok(()),
        Err(e) => Err(keychain_error(
            "remove_legacy",
            e,
            "Cannot remove the system quick unlock key",
        )),
    }
}

fn keychain_error(
    operation: &'static str,
    error: security_framework::base::Error,
    message: &str,
) -> String {
    tracing::warn!(
        event = "login.keychain_failed",
        operation,
        code = error.code()
    );
    match error.code() {
        -34018 => "The application is not authorized to access the system Keychain".into(),
        -25308 => "Keychain access is unavailable; unlock the login Keychain and retry".into(),
        -128 | -25293 => VERIFICATION_FAILED.into(),
        _ => message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn failed_verification_never_reads_or_writes_a_key() {
        for failure in ["cancelled", "unavailable", "timed out", "failed"] {
            let accessed = Cell::new(false);
            let result = after_verification(
                || Err(failure.to_string()),
                || {
                    accessed.set(true);
                    Ok(())
                },
            );
            assert_eq!(result, Err(failure.to_string()));
            assert!(!accessed.get());
        }
    }

    #[test]
    fn every_key_operation_requires_a_new_verification() {
        let verifications = Cell::new(0);
        for _ in 0..2 {
            after_verification(
                || {
                    verifications.set(verifications.get() + 1);
                    Ok(())
                },
                || Ok(()),
            )
            .unwrap();
        }
        assert_eq!(verifications.get(), 2);
        assert_eq!(
            after_verification(|| Ok(()), || Err::<(), _>("Keychain locked".into())),
            Err("Keychain locked".into())
        );
    }

    #[test]
    fn availability_does_not_require_a_tokio_runtime() {
        assert!(tokio::runtime::Handle::try_current().is_err());
        let _ = available();
    }

    #[test]
    fn missing_entitlement_is_reported_as_an_application_error() {
        let message = keychain_error(
            "store",
            security_framework::base::Error::from_code(-34018),
            "Cannot save the system quick unlock key",
        );
        assert_eq!(
            message,
            "The application is not authorized to access the system Keychain"
        );
    }
}
