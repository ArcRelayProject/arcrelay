//! Windows OLE drag export. Encoding tests can also run on non-Windows hosts:
//! `rustc --edition=2021 --test native_windows.rs -o /tmp/clipboard-drag-windows-tests`.

#[cfg(target_os = "windows")]
pub(super) use implementation::start;

#[cfg(target_os = "windows")]
mod implementation {
    use std::mem::ManuallyDrop;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use windows::core::{implement, w, Error, BOOL, HRESULT, PCWSTR};
    use windows::Win32::Foundation::{
        GlobalFree, COLORREF, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS,
        POINT, SIZE, S_OK,
    };
    use windows::Win32::Graphics::Gdi::{
        CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, IDataObject, CLSCTX_INPROC_SERVER, DVASPECT_CONTENT, FORMATETC,
        STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
    };
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT,
    };
    use windows::Win32::System::Ole::{
        DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, OleUninitialize,
        ReleaseStgMedium, CF_HDROP, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY,
    };
    use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows::Win32::UI::Shell::{
        CLSID_DragDropHelper, IDragSourceHelper, SHCreateDataObject, CFSTR_PREFERREDDROPEFFECT,
        DROPFILES, SHDRAGIMAGE,
    };

    use super::super::{Completion, NativeDragOutcome, NativeDragPayload};
    use super::encoding;

    pub(crate) fn start(
        _window: &tauri::WebviewWindow,
        payload: NativeDragPayload,
        completion: Completion,
    ) -> Result<(), String> {
        // Preparation runs asynchronously. Never let a release during preparation
        // become an immediate drop in whichever window currently has the cursor.
        if payload.cancelled.load(Ordering::Acquire)
            || unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON.0)) } >= 0
        {
            completion(NativeDragOutcome::Cancelled);
            return Ok(());
        }

        let outcome = run_drag(payload)?;
        completion(outcome);
        Ok(())
    }

    fn run_drag(payload: NativeDragPayload) -> Result<NativeDragOutcome, String> {
        unsafe { OleInitialize(None) }.map_err(|error| error.to_string())?;
        let _ole = OleApartment;
        let data_object: IDataObject =
            unsafe { SHCreateDataObject(None, None, None) }.map_err(|error| error.to_string())?;

        if !payload.files.is_empty() {
            let paths = payload
                .files
                .iter()
                .map(|path| path.as_os_str().encode_wide().collect())
                .collect::<Vec<Vec<u16>>>();
            debug_assert_eq!(std::mem::size_of::<DROPFILES>(), encoding::DROPFILES_SIZE);
            set_bytes(&data_object, CF_HDROP.0, &encoding::file_drop(&paths)?)?;
        } else {
            // Richest format first; plain inputs still get the complete text.
            if let Some(html) = &payload.html {
                set_bytes(
                    &data_object,
                    register_format(w!("HTML Format"))?,
                    &encoding::html(html)?,
                )?;
            }
            if let Some(text) = &payload.text {
                set_bytes(&data_object, CF_UNICODETEXT.0, &encoding::text(text)?)?;
            }
        }
        set_bytes(
            &data_object,
            register_format(CFSTR_PREFERREDDROPEFFECT)?,
            &DROPEFFECT_COPY.0.to_le_bytes(),
        )?;

        // The shell data object accepts the helper's private SetData formats.
        // Preview failure must not prevent transfer of the original payload.
        let _preview = initialize_preview(&data_object, &payload.preview_png);
        if payload.cancelled.load(Ordering::Acquire)
            || unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON.0)) } >= 0
        {
            return Ok(NativeDragOutcome::Cancelled);
        }
        let source: IDropSource = ClipboardDropSource {
            cancelled: payload.cancelled,
        }
        .into();
        let mut effect = DROPEFFECT::default();
        payload.offered.store(true, Ordering::Release);
        let result = unsafe { DoDragDrop(&data_object, &source, DROPEFFECT_COPY, &mut effect) };
        if result == DRAGDROP_S_DROP && effect.0 & DROPEFFECT_COPY.0 != 0 {
            Ok(NativeDragOutcome::Dropped)
        } else if result.is_err() {
            Err(Error::from_hresult(result).to_string())
        } else {
            Ok(NativeDragOutcome::Cancelled)
        }
    }

    struct OleApartment;

    impl Drop for OleApartment {
        fn drop(&mut self) {
            unsafe { OleUninitialize() };
        }
    }

    #[implement(IDropSource)]
    struct ClipboardDropSource {
        cancelled: Arc<AtomicBool>,
    }

    #[allow(non_snake_case)]
    impl IDropSource_Impl for ClipboardDropSource_Impl {
        fn QueryContinueDrag(
            &self,
            escape_pressed: BOOL,
            key_state: MODIFIERKEYS_FLAGS,
        ) -> HRESULT {
            if escape_pressed.as_bool() || self.cancelled.load(Ordering::Acquire) {
                DRAGDROP_S_CANCEL
            } else if (key_state & MK_LBUTTON) == MODIFIERKEYS_FLAGS(0) {
                DRAGDROP_S_DROP
            } else {
                S_OK
            }
        }

        fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    fn register_format(name: PCWSTR) -> Result<u16, String> {
        let format = unsafe { RegisterClipboardFormatW(name) };
        u16::try_from(format)
            .ok()
            .filter(|format| *format != 0)
            .ok_or_else(|| Error::from_win32().to_string())
    }

    fn set_bytes(data_object: &IDataObject, format: u16, bytes: &[u8]) -> Result<(), String> {
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len()) }
            .map_err(|error| error.to_string())?;
        let pointer = unsafe { GlobalLock(handle) };
        if pointer.is_null() {
            let error = Error::from_win32().to_string();
            let _ = unsafe { GlobalFree(Some(handle)) };
            return Err(error);
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast(), bytes.len());
            let _ = GlobalUnlock(handle);
        }
        let format = FORMATETC {
            cfFormat: format,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let mut medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: handle },
            pUnkForRelease: ManuallyDrop::new(None),
        };
        // Ownership transfers only on success. The native shell object also
        // supports delayed consumers retaining it after DoDragDrop returns.
        if let Err(error) = unsafe { data_object.SetData(&format, &medium, true) } {
            unsafe { ReleaseStgMedium(&mut medium) };
            return Err(error.to_string());
        }
        Ok(())
    }

    fn initialize_preview(data_object: &IDataObject, png: &[u8]) -> Option<IDragSourceHelper> {
        let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .ok()?
            .thumbnail(192, 128)
            .to_rgba8();
        let (width, height) = image.dimensions();
        if width == 0 || height == 0 {
            return None;
        }
        let helper: IDragSourceHelper =
            unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) }.ok()?;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = std::ptr::null_mut();
        let bitmap =
            unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut pixels, None, 0) }.ok()?;
        if pixels.is_null() {
            let _ = unsafe { DeleteObject(bitmap.into()) };
            return None;
        }
        let mut bgra = image.into_raw();
        // InitializeFromBitmap performs alpha premultiplication itself.
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        unsafe { std::ptr::copy_nonoverlapping(bgra.as_ptr(), pixels.cast(), bgra.len()) };
        let drag_image = SHDRAGIMAGE {
            sizeDragImage: SIZE {
                cx: width as i32,
                cy: height as i32,
            },
            ptOffset: POINT { x: 8, y: 8 },
            hbmpDragImage: bitmap,
            crColorKey: COLORREF(u32::MAX),
        };
        let result = unsafe { helper.InitializeFromBitmap(&drag_image, data_object) };
        let _ = unsafe { DeleteObject(bitmap.into()) };
        result.ok().map(|_| helper)
    }
}

// These encoders deliberately have no Windows dependency: their byte-level
// contracts can be checked on every developer machine and CI operating system.
#[cfg(any(target_os = "windows", test))]
mod encoding {
    pub(super) const DROPFILES_SIZE: usize = 20;
    const START_FRAGMENT: &str = "<!--StartFragment-->";
    const END_FRAGMENT: &str = "<!--EndFragment-->";

    pub(super) fn file_drop(paths: &[Vec<u16>]) -> Result<Vec<u8>, String> {
        if paths.is_empty()
            || paths
                .iter()
                .any(|path| path.is_empty() || path.contains(&0))
        {
            return Err("The file drag contains an invalid path".into());
        }
        let mut bytes = vec![0; DROPFILES_SIZE];
        bytes[..4].copy_from_slice(&(DROPFILES_SIZE as u32).to_le_bytes());
        // DROPFILES { pFiles, POINT { 0, 0 }, fNC: false, fWide: true }.
        bytes[16..20].copy_from_slice(&1_u32.to_le_bytes());
        for path in paths {
            bytes.extend(
                path.iter()
                    .chain(std::iter::once(&0))
                    .flat_map(|unit| unit.to_le_bytes()),
            );
        }
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        Ok(bytes)
    }

    pub(super) fn text(text: &str) -> Result<Vec<u8>, String> {
        if text.contains('\0') {
            return Err("Text containing a NUL character must be exported as a file".into());
        }
        // CF_UNICODETEXT specifies CRLF. Preserve existing pairs without
        // doubling CR and normalize bare CR and LF, including final newlines.
        let normalized = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\r\n");
        Ok(normalized
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect())
    }

    pub(super) fn html(html: &str) -> Result<Vec<u8>, String> {
        if html.contains('\0') {
            return Err("HTML containing a NUL character must be exported as a file".into());
        }
        let (context, start, end) = html_context(html);
        let header = |start_html, end_html, start_fragment, end_fragment| {
            format!(
                "Version:1.0\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\nStartFragment:{start_fragment:010}\r\nEndFragment:{end_fragment:010}\r\n"
            )
        };
        let header_len = header(0, 0, 0, 0).len();
        let total = header_len
            .checked_add(context.len())
            .filter(|size| *size < 10_000_000_000)
            .ok_or_else(|| "HTML drag data is too large".to_string())?;
        let mut bytes =
            header(header_len, total, header_len + start, header_len + end).into_bytes();
        bytes.extend_from_slice(context.as_bytes());
        bytes.push(0);
        Ok(bytes)
    }

    fn html_context(html: &str) -> (String, usize, usize) {
        // Clipboard collectors may already retain fragment markers and outer
        // context (e.g. table/list ancestors). Keep those semantics intact.
        if let Some(start) = html
            .find(START_FRAGMENT)
            .map(|start| start + START_FRAGMENT.len())
        {
            if let Some(end) = html[start..].find(END_FRAGMENT).map(|end| start + end) {
                return (html.to_owned(), start, end);
            }
        }
        // A complete document must not become a nested <html> fragment.
        let lowercase = html.to_ascii_lowercase();
        if let Some(body) = lowercase.find("<body").filter(|body| {
            lowercase
                .as_bytes()
                .get(body + 5)
                .is_some_and(|next| *next == b'>' || next.is_ascii_whitespace())
        }) {
            if let Some(open) = lowercase[body..].find('>').map(|open| body + open + 1) {
                if let Some(close) = lowercase.rfind("</body>").filter(|close| *close >= open) {
                    let context = format!(
                        "{}{START_FRAGMENT}{}{END_FRAGMENT}{}",
                        &html[..open],
                        &html[open..close],
                        &html[close..]
                    );
                    let start = open + START_FRAGMENT.len();
                    return (context, start, start + close - open);
                }
            }
        }
        let prefix = format!("<html><body>{START_FRAGMENT}");
        let start = prefix.len();
        let context = format!("{prefix}{html}{END_FRAGMENT}</body></html>");
        (context, start, start + html.len())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn file_drop_preserves_unicode_special_characters_and_directory_paths() {
            let paths = [
                r"C:\图片\a #100%.png",
                r"\\server\share\folder 🖼",
                r"C:\目录\",
            ];
            let encoded = file_drop(
                &paths
                    .iter()
                    .map(|path| path.encode_utf16().collect())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            assert_eq!(u32::from_le_bytes(encoded[..4].try_into().unwrap()), 20);
            assert_eq!(&encoded[4..16], &[0; 12]);
            assert_eq!(u32::from_le_bytes(encoded[16..20].try_into().unwrap()), 1);
            let units = encoded[20..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect::<Vec<_>>();
            assert_eq!(&units[units.len() - 2..], &[0, 0]);
            let decoded = units[..units.len() - 1]
                .split_inclusive(|unit| *unit == 0)
                .map(|path| String::from_utf16(&path[..path.len() - 1]).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(decoded, paths);
        }

        #[test]
        fn file_drop_rejects_missing_and_embedded_nul_paths() {
            assert!(file_drop(&[]).is_err());
            assert!(file_drop(&[vec![]]).is_err());
            assert!(file_drop(&[vec![65, 0, 66]]).is_err());
        }

        #[test]
        fn unicode_text_retains_full_content_with_windows_newlines_and_one_terminator() {
            let encoded = text("中文🖼\nsecond\r\nthird\rlast\n").unwrap();
            let units = encoded
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect::<Vec<_>>();
            assert_eq!(units.last(), Some(&0));
            assert_eq!(
                String::from_utf16(&units[..units.len() - 1]).unwrap(),
                "中文🖼\r\nsecond\r\nthird\r\nlast\r\n"
            );
            assert!(text("a\0b").is_err());
        }

        fn assert_html_fragment(input: &str, expected: &str) -> String {
            let encoded = html(input).unwrap();
            assert_eq!(encoded.last(), Some(&0));
            let data = std::str::from_utf8(&encoded[..encoded.len() - 1]).unwrap();
            let offset = |name: &str| -> usize {
                data.lines()
                    .find_map(|line| line.strip_prefix(&format!("{name}:")))
                    .unwrap()
                    .parse()
                    .unwrap()
            };
            assert_eq!(offset("EndHTML"), encoded.len() - 1);
            assert_eq!(
                &data[offset("StartFragment")..offset("EndFragment")],
                expected
            );
            assert!(data[offset("StartHTML")..offset("StartFragment")].ends_with(START_FRAGMENT));
            assert!(data[offset("EndFragment")..offset("EndHTML")].starts_with(END_FRAGMENT));
            data[offset("StartHTML")..offset("EndHTML")].to_owned()
        }

        #[test]
        fn html_offsets_are_utf8_bytes_with_unicode_before_and_inside_fragment() {
            let input = "<html><head><title>图片🖼</title></head><body><!--StartFragment--><b>中文 café 🖼</b><!--EndFragment--></body></html>";
            assert_eq!(assert_html_fragment(input, "<b>中文 café 🖼</b>"), input);
        }

        #[test]
        fn html_wraps_fragments_and_preserves_document_context() {
            assert_html_fragment("<b>hello 中文</b>", "<b>hello 中文</b>");
            let input = "<HTML><HEAD><STYLE>b { color: red; }</STYLE></HEAD><BODY class='测试'><b>你好</b></BODY></HTML>";
            let context = assert_html_fragment(input, "<b>你好</b>");
            assert!(context.starts_with("<HTML><HEAD>"));
            assert_eq!(context.matches("<HTML>").count(), 1);
            assert!(html("a\0b").is_err());
        }
    }
}
