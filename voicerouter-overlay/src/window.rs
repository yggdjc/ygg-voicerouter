//! Capsule overlay window using GTK4 + layer-shell.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{ApplicationWindow, CssProvider, Label};
use gtk4_layer_shell::LayerShell;

use crate::waveform::{self, WaveformState};

const CAPSULE_HEIGHT: i32 = 56;
const CAPSULE_RADIUS: i32 = 28;
const MARGIN_BOTTOM: i32 = 48;

pub fn build_window(
    app: &gtk4::Application,
) -> (ApplicationWindow, Label, Rc<WaveformState>) {
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(220)
        .default_height(CAPSULE_HEIGHT)
        .decorated(false)
        .resizable(false)
        .build();

    let use_layer_shell = gtk4_layer_shell::is_supported();

    if use_layer_shell {
        window.init_layer_shell();
        window.set_layer(gtk4_layer_shell::Layer::Overlay);
        window.set_anchor(gtk4_layer_shell::Edge::Bottom, true);
        window.set_margin(gtk4_layer_shell::Edge::Bottom, MARGIN_BOTTOM);
        window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
    } else {
        log::warn!(
            "wlr-layer-shell not supported (GNOME); \
             window position is compositor-controlled, focus may be grabbed"
        );
        // Prevent keyboard focus grab as much as possible.
        window.set_focusable(false);
        window.set_can_focus(false);
        window.set_focus_on_click(false);
    }

    // Outer container provides margin so box-shadow is not clipped.
    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    outer.set_margin_start(16);
    outer.set_margin_end(16);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);

    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    hbox.set_widget_name("overlay-box");
    hbox.set_valign(gtk4::Align::Center);
    outer.append(&hbox);

    let wave_state = Rc::new(WaveformState::new());
    let waveform_area = waveform::create_waveform_widget(&wave_state);
    hbox.append(&waveform_area);

    let label = Label::new(Some("Listening..."));
    label.set_widget_name("status-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_max_width_chars(60);
    hbox.append(&label);

    window.set_child(Some(&outer));

    // Remove GNOME's "background" CSS class — it forces an opaque theme
    // color that cannot be overridden by CSS alone.
    window.remove_css_class("background");

    let css = CssProvider::new();
    css.load_from_data(
        "#overlay-box { \
             background-color: rgba(40, 40, 40, 0.80); \
             border-radius: 28px; \
             padding: 12px 16px 12px 12px; \
             box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4); \
         } \
         #status-label { \
             color: #ffffff; \
             font-family: sans-serif; \
             font-size: 14px; \
         }",
    );
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("no display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_USER,
    );

    window.set_visible(false);

    let wave_state_tick = wave_state.clone();
    let area_clone = waveform_area.clone();
    gtk4::glib::timeout_add_local(
        std::time::Duration::from_millis(33),
        move || {
            wave_state_tick.tick();
            area_clone.queue_draw();
            gtk4::glib::ControlFlow::Continue
        },
    );

    (window, label, wave_state)
}
