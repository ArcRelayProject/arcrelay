use super::*;
use block2::RcBlock;
use objc2::{
    msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject},
};
use objc2_foundation::{NSError, NSString, NSURL};

fn classes() -> Result<(&'static AnyClass, &'static AnyClass), Error> {
    if objc2_foundation::NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion
        < 13
    {
        return Err(Error::unavailable(
            "Finder cloud folders require macOS 13 or later",
        ));
    }
    // Load on demand so unsupported older systems can still use the rest of ArcRelay.
    static LOADED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let loaded = *LOADED.get_or_init(|| unsafe {
        !libc::dlopen(
            c"/System/Library/Frameworks/FileProvider.framework/FileProvider".as_ptr(),
            libc::RTLD_LAZY,
        )
        .is_null()
    });
    if !loaded {
        return Err(Error::unavailable(
            "File Provider is unavailable on this macOS version",
        ));
    }
    let domain = AnyClass::get(c"NSFileProviderDomain")
        .ok_or_else(|| Error::unavailable("File Provider is unavailable"))?;
    let manager = AnyClass::get(c"NSFileProviderManager")
        .ok_or_else(|| Error::unavailable("File Provider is unavailable"))?;
    Ok((domain, manager))
}

fn domain(view: &SystemFolder) -> Result<Retained<AnyObject>, Error> {
    let (class, _) = classes()?;
    unsafe {
        let allocated: Allocated<AnyObject> = msg_send![class, alloc];
        Ok(
            msg_send![allocated, initWithIdentifier: &*NSString::from_str(&view.id), displayName: &*NSString::from_str(&view.name)],
        )
    }
}

fn error(error: *mut NSError) -> Result<(), Error> {
    if error.is_null() {
        Ok(())
    } else {
        Err(Error::unavailable(
            unsafe { (*error).localizedDescription() }.to_string(),
        ))
    }
}

pub(super) async fn register(view: &SystemFolder) -> Result<(), Error> {
    let receiver = {
        let (_, manager) = classes()?;
        let domain = domain(view)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let sender = std::sync::Mutex::new(Some(sender));
        let completion = RcBlock::new(move |failure: *mut NSError| {
            if let Some(sender) = sender.lock().unwrap().take() {
                let _ = sender.send(error(failure));
            }
        });
        unsafe {
            let _: () = msg_send![manager, addDomain: &*domain, completionHandler: &*completion];
        }
        receiver
    };
    receiver
        .await
        .map_err(|_| Error::unavailable("File Provider registration interrupted"))?
}

pub(super) async fn open(view: &SystemFolder) -> Result<(), Error> {
    let receiver = {
        let (_, class) = classes()?;
        let domain = domain(view)?;
        let manager: Option<Retained<AnyObject>> =
            unsafe { msg_send![class, managerForDomain: &*domain] };
        let manager =
            manager.ok_or_else(|| Error::unavailable("File Provider is not registered"))?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let sender = std::sync::Mutex::new(Some(sender));
        let completion = RcBlock::new(move |url: *mut NSURL, failure: *mut NSError| {
            let result = error(failure).and_then(|_| {
                if url.is_null() {
                    return Err(Error::unavailable("system folder is not ready"));
                }
                unsafe { (*url).path() }
                    .map(|path| path.to_string())
                    .ok_or_else(|| Error::unavailable("system folder has no local path"))
            });
            if let Some(sender) = sender.lock().unwrap().take() {
                let _ = sender.send(result);
            }
        });
        unsafe {
            let _: () = msg_send![&*manager, getUserVisibleURLForItemIdentifier: identifier(c"NSFileProviderRootContainerItemIdentifier")?, completionHandler: &*completion];
        }
        receiver
    };
    let path = receiver
        .await
        .map_err(|_| Error::unavailable("system folder lookup interrupted"))??;
    let status = tokio::process::Command::new("/usr/bin/open")
        .arg("--")
        .arg(path)
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::unavailable(
            "Finder could not open the system folder",
        ))
    }
}

pub(super) async fn remove(view: &SystemFolder) -> Result<(), Error> {
    let receiver = {
        let (_, manager) = classes()?;
        let domain = domain(view)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let sender = std::sync::Mutex::new(Some(sender));
        let completion = RcBlock::new(move |_preserved: *mut NSURL, failure: *mut NSError| {
            if let Some(sender) = sender.lock().unwrap().take() {
                let _ = sender.send(error(failure));
            }
        });
        // PreserveDownloadedUserData: never discard local edits on disconnect.
        unsafe {
            let _: () = msg_send![manager, removeDomain: &*domain, mode: 2isize, completionHandler: &*completion];
        }
        receiver
    };
    receiver
        .await
        .map_err(|_| Error::unavailable("system folder removal interrupted"))?
}

pub(super) fn signal(view: &SystemFolder, reconnected: bool) {
    let Ok((_, class)) = classes() else {
        return;
    };
    let Ok(domain) = domain(view) else {
        return;
    };
    let manager: Option<Retained<AnyObject>> =
        unsafe { msg_send![class, managerForDomain: &*domain] };
    let Some(manager) = manager else {
        return;
    };
    let Ok(identifier) = identifier(c"NSFileProviderWorkingSetContainerItemIdentifier") else {
        return;
    };
    let completion = RcBlock::new(|_failure: *mut NSError| {});
    unsafe {
        let _: () = msg_send![&*manager, signalEnumeratorForContainerItemIdentifier: identifier, completionHandler: &*completion];
        if reconnected {
            let error = NSError::errorWithDomain_code_userInfo(
                &NSString::from_str("NSFileProviderErrorDomain"),
                -1004,
                None,
            );
            let _: () =
                msg_send![&*manager, signalErrorResolved: &*error, completionHandler: &*completion];
        }
    }
}

fn identifier(symbol: &std::ffi::CStr) -> Result<&'static NSString, Error> {
    // These are exported NSString constants, not their C symbol names.
    let address = unsafe { libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr()) };
    if address.is_null() {
        return Err(Error::unavailable("File Provider identifier unavailable"));
    }
    unsafe { (*(address as *const *const NSString)).as_ref() }
        .ok_or_else(|| Error::unavailable("File Provider identifier unavailable"))
}
