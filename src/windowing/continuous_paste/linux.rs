use super::HudModel;
use gtk::gdk;
use gtk::prelude::*;
use gtk::{pango, Align, Inhibit, Orientation};
use tauri::{AppHandle, Theme, Window};

struct NativeHud {
    root: gtk::EventBox,
    icon: gtk::Label,
    title: gtk::Label,
    position: gtk::Label,
    preview: gtk::Label,
    close: gtk::Button,
    segmented: gtk::Box,
    segments: Vec<gtk::DrawingArea>,
    progress: gtk::ProgressBar,
}

thread_local! {
    static HUD: std::cell::RefCell<Option<NativeHud>> = const { std::cell::RefCell::new(None) };
}

const CSS: &str = r#"
.continuous-paste-hud {
  background: rgba(250, 250, 252, 0.98);
  border: 1px solid rgba(21, 24, 35, 0.14);
  border-radius: 14px;
  color: #24262d;
}
.continuous-paste-hud.dark { background: rgba(31, 34, 41, 0.98); color: #f3f4f8; }
.hud-icon { background: #f0f0ff; border-radius: 11px; color: #5b5ff0; font-size: 19px; font-weight: 700; }
.dark .hud-icon { background: #292b52; color: #7c82ff; }
.complete .hud-icon { background: rgba(35, 138, 88, 0.13); color: #238a58; }
.hud-title { font-size: 13px; font-weight: 700; }
.hud-position { color: #5b5ff0; font-size: 11px; font-weight: 700; }
.dark .hud-position { color: #7c82ff; }
.hud-preview { font-family: monospace; font-size: 11px; }
.hud-close { background: transparent; border: 0; box-shadow: none; color: #737681; font-size: 20px; padding: 0; }
.dark .hud-close { color: #b8bbc7; }
.hud-close:hover { background: rgba(36, 38, 45, 0.08); border-radius: 9px; }
.hud-segment { background: #e8e9f0; border-radius: 2px; min-height: 4px; }
.dark .hud-segment { background: #444854; }
.hud-segment.filled { background: #5b5ff0; }
.dark .hud-segment.filled { background: #7c82ff; }
.hud-progress trough { min-height: 4px; border: 0; border-radius: 2px; background: #e8e9f0; }
.dark .hud-progress trough { background: #444854; }
.hud-progress progress { min-height: 4px; border: 0; border-radius: 2px; background: #5b5ff0; }
.dark .hud-progress progress { background: #7c82ff; }
"#;

pub fn install(app: &AppHandle, window: &Window) -> Result<(), String> {
    let native = window.gtk_window().map_err(|error| error.to_string())?;
    native.set_decorated(false);
    native.set_keep_above(true);
    native.set_accept_focus(false);
    native.set_focus_on_map(false);
    native.set_skip_taskbar_hint(true);
    native.set_skip_pager_hint(true);
    native.set_app_paintable(true);

    let provider = gtk::CssProvider::new();
    provider
        .load_from_data(CSS.as_bytes())
        .map_err(|error| error.to_string())?;
    if let Some(screen) = gdk::Screen::default() {
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let root = gtk::EventBox::new();
    root.style_context().add_class("continuous-paste-hud");
    root.set_size_request(320, 92);
    root.add_events(gdk::EventMask::BUTTON_PRESS_MASK);
    let fixed = gtk::Fixed::new();
    root.add(&fixed);

    let icon = gtk::Label::new(Some("▣"));
    icon.style_context().add_class("hud-icon");
    icon.set_size_request(38, 38);
    fixed.put(&icon, 12, 12);

    let title = text_label("hud-title", 208, 18);
    fixed.put(&title, 60, 11);
    let position = text_label("hud-position", 208, 14);
    fixed.put(&position, 60, 29);
    let preview = text_label("hud-preview", 208, 16);
    fixed.put(&preview, 60, 45);

    let close = gtk::Button::with_label("×");
    close.style_context().add_class("hud-close");
    close.set_relief(gtk::ReliefStyle::None);
    close.set_size_request(30, 30);
    close.set_focus_on_click(false);
    fixed.put(&close, 278, 10);

    let segmented = gtk::Box::new(Orientation::Horizontal, 5);
    segmented.set_size_request(208, 4);
    let mut segments = Vec::with_capacity(8);
    for _ in 0..8 {
        let segment = gtk::DrawingArea::new();
        segment.style_context().add_class("hud-segment");
        segment.set_hexpand(true);
        segment.set_size_request(1, 4);
        segmented.pack_start(&segment, true, true, 0);
        segments.push(segment);
    }
    fixed.put(&segmented, 60, 70);
    let progress = gtk::ProgressBar::new();
    progress.style_context().add_class("hud-progress");
    progress.set_size_request(208, 4);
    fixed.put(&progress, 60, 70);

    let close_app = app.clone();
    close.connect_clicked(move |_| {
        let app = close_app.clone();
        tauri::async_runtime::spawn(async move {
            crate::commands::clipboard_stop_continuous_paste(app).await;
        });
    });

    let drag_window = native.clone();
    root.connect_button_press_event(move |_, event| {
        let (x, _) = event.position();
        if event.button() == 1 && x < 276.0 {
            let (root_x, root_y) = event.root();
            drag_window.begin_move_drag(1, root_x as i32, root_y as i32, event.time());
            Inhibit(true)
        } else {
            Inhibit(false)
        }
    });

    native.add(&root);
    root.show_all();
    progress.hide();
    HUD.with(|slot| {
        *slot.borrow_mut() = Some(NativeHud {
            root,
            icon,
            title,
            position,
            preview,
            close,
            segmented,
            segments,
            progress,
        });
    });
    Ok(())
}

pub fn update(window: &Window, model: &HudModel) -> Result<(), String> {
    HUD.with(|slot| {
        let binding = slot.borrow();
        let hud = binding.as_ref().ok_or("native GTK HUD is not installed")?;
        hud.title.set_text(&model.title);
        hud.position.set_text(&model.position);
        hud.position.set_visible(!model.complete);
        hud.preview.set_text(&model.preview);
        hud.close.set_tooltip_text(Some(&model.stop_label));
        let dark = match model.theme {
            crate::settings::ThemePreference::Dark => true,
            crate::settings::ThemePreference::Light => false,
            crate::settings::ThemePreference::System => {
                matches!(window.theme(), Ok(Theme::Dark))
            }
        };
        set_class(&hud.root.style_context(), "dark", dark);
        set_class(&hud.root.style_context(), "complete", model.complete);
        let segmented = model.total <= 8;
        hud.segmented.set_visible(segmented);
        hud.progress.set_visible(!segmented);
        if segmented {
            for (index, segment) in hud.segments.iter().enumerate() {
                segment.set_visible(index < model.total.max(1));
                set_class(&segment.style_context(), "filled", index < model.current);
            }
        } else {
            hud.progress.set_fraction(if model.total == 0 {
                0.0
            } else {
                model.current as f64 / model.total as f64
            });
        }
        Ok(())
    })
}

pub fn show(window: &Window) -> Result<(), String> {
    window.show().map_err(|error| error.to_string())
}

pub fn hide(window: &Window) -> Result<(), String> {
    window.hide().map_err(|error| error.to_string())
}

fn text_label(class: &str, width: i32, height: i32) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.style_context().add_class(class);
    label.set_size_request(width, height);
    label.set_halign(Align::Start);
    label.set_xalign(0.0);
    label.set_ellipsize(pango::EllipsizeMode::End);
    label.set_single_line_mode(true);
    label
}

fn set_class(context: &gtk::StyleContext, class: &str, enabled: bool) {
    if enabled {
        context.add_class(class);
    } else {
        context.remove_class(class);
    }
}
