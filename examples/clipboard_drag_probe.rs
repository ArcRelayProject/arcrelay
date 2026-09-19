//! Isolated macOS native-to-browser drag probe, using generated fixtures only.
//!
//! This reuses the production native adapter with an NSView-backed handle shim.
//! It exercises AppKit and real external browser drops, not Tauri IPC/window
//! policy. See docs/clipboard-drag-probe.md for commands and coverage limits.

#[cfg(target_os = "macos")]
extern crate self as tauri;

#[cfg(target_os = "macos")]
pub struct WebviewWindow(objc2::rc::Retained<objc2_app_kit::NSView>);

#[cfg(target_os = "macos")]
impl WebviewWindow {
    pub fn ns_view(&self) -> Result<*mut std::ffi::c_void, String> {
        Ok(objc2::rc::Retained::as_ptr(&self.0).cast_mut().cast())
    }
}

#[cfg(target_os = "macos")]
#[path = "../src/clipboard_drag/native.rs"]
mod native;

#[cfg(target_os = "macos")]
mod probe {
    use std::cell::RefCell;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    use image::ImageEncoder;
    use objc2::rc::Retained;
    use objc2::runtime::NSObjectProtocol;
    use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSButtonType,
        NSEvent, NSEventMask, NSEventType, NSTextField, NSView, NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use sha2::{Digest, Sha256};

    use crate::native::{NativeDragOutcome, NativeDragPayload};

    const TEXT: &str = "你好，ArcRelay 👋\nUnicode text: café & <tags>";
    const HTML: &str =
        "<p><strong>你好，ArcRelay 👋</strong></p><p>Unicode text: café &amp; &lt;tags&gt;</p>";
    const FILE_TEXT: &str = "ArcRelay generated drag fixture\n你好，文件 👋\n";
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);

    #[link(name = "WebKit", kind = "framework")]
    extern "C" {}

    fn webkit_target(frame: NSRect, url: &str) -> Retained<NSView> {
        // Keep the probe independent of the product's Tauri runtime while
        // exercising an actual WKWebView destination. All methods are public
        // WebKit APIs; the view is retained by the source window's view tree.
        let class = objc2::runtime::AnyClass::get(c"WKWebView").expect("WebKit framework loaded");
        unsafe {
            let allocated: objc2::rc::Allocated<NSView> = msg_send![class, alloc];
            let webview: Retained<NSView> = msg_send![allocated, initWithFrame: frame];
            let url = objc2_foundation::NSURL::URLWithString(&NSString::from_str(url)).unwrap();
            let request = objc2_foundation::NSURLRequest::requestWithURL(&url);
            let _: *mut objc2::runtime::AnyObject = msg_send![&*webview, loadRequest: &*request];
            webview
        }
    }

    #[derive(Clone)]
    struct Fixtures {
        directory: PathBuf,
        image: PathBuf,
        file: PathBuf,
        png: Vec<u8>,
    }

    thread_local! {
        static FIXTURES: RefCell<Option<Fixtures>> = const { RefCell::new(None) };
    }

    struct ButtonIvars {
        kind: u8,
    }

    define_class!(
        #[unsafe(super(NSButton))]
        #[thread_kind = MainThreadOnly]
        #[name = "ArcRelayClipboardDragProbeButton"]
        #[ivars = ButtonIvars]
        struct ProbeButton;

        unsafe impl NSObjectProtocol for ProbeButton {}

        impl ProbeButton {
            #[unsafe(method(mouseDown:))]
            fn mouse_down(&self, _event: &NSEvent) {
                let Some(window) = self.window() else { return };
                // Wait for real AppKit input. No synthetic DOM or OS events are
                // produced by the probe; merely clicking does not start a drag.
                let Some(event) = window.nextEventMatchingMask(
                    NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp,
                ) else { return };
                if event.r#type() != NSEventType::LeftMouseDragged {
                    return;
                }
                let Some(view) = window.contentView() else { return };
                let fixtures = FIXTURES.with(|fixtures| fixtures.borrow().clone()).unwrap();
                let mut payload = NativeDragPayload {
                    files: Vec::new(),
                    text: None,
                    html: None,
                    preview_png: fixtures.png,
                    cancelled: Arc::new(AtomicBool::new(false)),
                    offered: Arc::new(AtomicBool::new(false)),
                };
                match self.ivars().kind {
                    0 => payload.files.push(fixtures.image),
                    1 => payload.files.push(fixtures.file),
                    2 => payload.files.extend([fixtures.image, fixtures.file]),
                    3 => payload.text = Some(TEXT.into()),
                    4 => {
                        payload.text = Some(TEXT.into());
                        payload.html = Some(HTML.into());
                    }
                    _ => unreachable!(),
                }
                let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
                let kind = self.ivars().kind;
                println!("NATIVE_START sequence={sequence} kind={kind} mouse_buttons={}", NSEvent::pressedMouseButtons());
                let completion = Box::new(move |outcome: NativeDragOutcome| {
                    println!("NATIVE_END sequence={sequence} outcome={outcome:?}");
                });
                if let Err(error) = crate::native::start(&crate::WebviewWindow(view), payload, completion) {
                    eprintln!("NATIVE_ERROR sequence={sequence} error={error}");
                }
            }
        }
    );

    impl ProbeButton {
        fn new(kind: u8, title: &str, y: f64, mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(ButtonIvars { kind });
            let button: Retained<Self> = unsafe {
                msg_send![super(this), initWithFrame: NSRect::new(NSPoint::new(20.0, y), NSSize::new(290.0, 56.0))]
            };
            button.setTitle(&NSString::from_str(title));
            button.setButtonType(NSButtonType::MomentaryPushIn);
            button
        }
    }

    fn fixtures() -> Result<Fixtures, Box<dyn std::error::Error>> {
        let directory = std::env::temp_dir().join(format!(
            "arcrelay-clipboard-drag-probe-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory)?;
        let image = directory.join("你好 image #1%.png");
        let file = directory.join("说明 notes #2%.txt");
        let mut pixels = image::RgbaImage::new(160, 100);
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            *pixel = image::Rgba([20 + (x / 2) as u8, 90 + y as u8, 180, 255]);
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png).write_image(
            pixels.as_raw(),
            160,
            100,
            image::ExtendedColorType::Rgba8,
        )?;
        std::fs::write(&image, &png)?;
        std::fs::write(&file, FILE_TEXT)?;
        let expected = serde_json::json!({
            "text": TEXT,
            "html": HTML,
            "files": [
                { "name": image.file_name().unwrap().to_str().unwrap(), "type": "image/png", "size": png.len(), "sha256": format!("{:x}", Sha256::digest(&png)) },
                { "name": file.file_name().unwrap().to_str().unwrap(), "type": "text/plain", "size": FILE_TEXT.len(), "sha256": format!("{:x}", Sha256::digest(FILE_TEXT.as_bytes())) }
            ]
        });
        std::fs::write(
            directory.join("expected.json"),
            serde_json::to_vec_pretty(&expected)?,
        )?;
        std::fs::write(
            directory.join("index.html"),
            include_str!("fixtures/clipboard-drag-drop.html"),
        )?;
        Ok(Fixtures {
            directory,
            image,
            file,
            png,
        })
    }

    fn serve_one(mut stream: TcpStream, directory: &std::path::Path) -> std::io::Result<()> {
        stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
        let mut data = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let count = stream.read(&mut buffer)?;
            if count == 0 {
                return Ok(());
            }
            data.extend_from_slice(&buffer[..count]);
            if data.len() > 65_536 {
                return Ok(());
            }
            if let Some(offset) = data.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                break offset + 4;
            }
        };
        let headers = String::from_utf8_lossy(&data[..header_end]).into_owned();
        let request = headers.lines().next().unwrap_or_default();
        let (status, content_type, body) = if request.starts_with("GET /expected.json ") {
            (
                "200 OK",
                "application/json",
                std::fs::read(directory.join("expected.json"))?,
            )
        } else if request.starts_with("GET / ") {
            (
                "200 OK",
                "text/html; charset=utf-8",
                std::fs::read(directory.join("index.html"))?,
            )
        } else if request.starts_with("POST /report ") {
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0)
                .min(65_536);
            while data.len() < header_end + content_length {
                let count = stream.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                data.extend_from_slice(&buffer[..count]);
            }
            if data.len() >= header_end + content_length {
                let body = &data[header_end..header_end + content_length];
                let mut report = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(directory.join("reports.jsonl"))?;
                report.write_all(body)?;
                report.write_all(b"\n")?;
                println!("BROWSER_REPORT {}", String::from_utf8_lossy(body));
            }
            ("200 OK", "text/plain", b"ok".to_vec())
        } else {
            ("404 Not Found", "text/plain", b"not found".to_vec())
        };
        write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
        stream.write_all(&body)
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let fixtures = fixtures()?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let browser_url = format!("http://{}/", listener.local_addr()?);
        println!("BROWSER_URL {browser_url}");
        println!("FIXTURES {}", fixtures.directory.display());
        println!("Drag generated sources into the local browser fixture. Ctrl-C stops this isolated probe.");
        let directory = fixtures.directory.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if let Err(error) = serve_one(stream, &directory) {
                    eprintln!("probe HTTP: {error}");
                }
            }
        });
        FIXTURES.with(|slot| *slot.borrow_mut() = Some(fixtures));
        let mtm = MainThreadMarker::new().expect("probe runs on main thread");
        let application = NSApplication::sharedApplication(mtm);
        application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        let embedded = std::env::args().any(|argument| argument == "--embedded-webkit");
        let size = if embedded {
            NSSize::new(1360.0, 740.0)
        } else {
            NSSize::new(330.0, 440.0)
        };
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(40.0, 100.0), size),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(
            "ArcRelay Drag Probe — generated fixtures",
        ));
        let content = NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::ZERO, size));
        let label = NSTextField::labelWithString(
            &NSString::from_str("Drag to the localhost browser page"),
            mtm,
        );
        label.setFrame(NSRect::new(
            NSPoint::new(20.0, size.height - 48.0),
            NSSize::new(295.0, 25.0),
        ));
        content.addSubview(&label);
        for (kind, title) in [
            "PNG image · 中文 # %",
            "Text file · 中文 # %",
            "Two files together",
            "Plain Unicode text",
            "HTML + plain text",
        ]
        .into_iter()
        .enumerate()
        {
            content.addSubview(&ProbeButton::new(
                kind as u8,
                title,
                size.height - 120.0 - kind as f64 * 70.0,
                mtm,
            ));
        }
        if embedded {
            content.addSubview(&webkit_target(
                NSRect::new(NSPoint::new(350.0, 0.0), NSSize::new(1010.0, size.height)),
                &browser_url,
            ));
        }
        window.setContentView(Some(&content));
        window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        application.activateIgnoringOtherApps(true);
        application.run();
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    probe::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This Cocoa probe runs on macOS; the HTML drop fixture is browser-portable.");
}
