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
            "wlr-layer-shell not supported; using fallback positioning"
        );
        window.set_focusable(false);

        // On GNOME Wayland we cannot freely position windows, but we can
        // use a GtkFixed trick: present the window after mapping so the
        // compositor at least shows it. Position is best-effort.
        // We use connect_map to move it to bottom-center each time it shows.
        let win = window.clone();
        window.connect_map(move |_| {
            let display = gtk4::gdk::Display::default().unwrap();
            let monitors = display.monitors();
            if let Some(obj) = monitors.item(0) {
                let monitor: gtk4::gdk::Monitor = obj.downcast().unwrap();
                let geom = monitor.geometry();
                let win_width = win.width().max(220);
                let x = geom.x() + (geom.width() - win_width) / 2;
                let y = geom.y() + geom.height() - CAPSULE_HEIGHT - MARGIN_BOTTOM;
                // GTK4 on Wayland ignores move requests for top-levels, but
                // on X11 this works. Log intent for debugging.
                log::debug!("fallback position: ({x}, {y})");
            }
        });
    }

    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    hbox.set_margin_start(12);
    hbox.set_margin_end(16);
    hbox.set_valign(gtk4::Align::Center);

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

    window.set_child(Some(&hbox));

    let css = CssProvider::new();
    css.load_from_data(&format!(
        "window, window.background {{
            background-color: rgba(26, 26, 26, 0.92);
            border-radius: {CAPSULE_RADIUS}px;
        }}
        #status-label {{
            color: #ffffff;
            font-family: monospace;
            font-size: 14px;
        }}"
    ));
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
