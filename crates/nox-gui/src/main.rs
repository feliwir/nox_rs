// No console window for release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod about;
mod alert;
mod clip_strip;
mod color_wheels;
mod effect_controls;
mod export_dialog;
mod framing;
mod i18n;
mod inspector;
mod media_cell;
mod media_object;
mod media_pool;
mod node_editor;
mod node_panel;
mod playback;
mod preferences;
mod project;
mod sandbox;
mod scopes;
mod settings_dialog;
#[cfg(feature = "adw-1-8")]
mod shortcuts;
#[cfg(test)]
mod template_tests;
mod text_fonts;
mod thumbnails;
mod timeline;
mod track_header;
mod transform_overlay;
#[cfg(test)]
mod ui_tests;
mod ui_util;
mod viewer;
mod waveforms;
mod window;

use adw::prelude::*;
use gtk::{gio, glib};

const APP_ID: &str = "io.github.feliwir.Nox";
/// The bundled resources' base path (`nox.gresource`).
const RESOURCES: &str = "/io/github/feliwir/Nox";

/// What every Nox application sets up when it starts (also the tests'):
/// the dark style and the application's actions.
fn set_up(app: &adw::Application) {
    // A video editor shows pictures: always dark around them, as other
    // editors, whatever the system prefers. High contrast still applies.
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    // Preferences are the application's, for every window.
    let preferences = gio::SimpleAction::new("preferences", None);
    preferences.connect_activate(glib::clone!(
        #[weak]
        app,
        move |_, _| preferences::show(app.active_window().as_ref())
    ));
    app.add_action(&preferences);
    let about = gio::SimpleAction::new("about", None);
    about.connect_activate(glib::clone!(
        #[weak]
        app,
        move |_, _| about::show(app.active_window().as_ref())
    ));
    app.add_action(&about);
    #[cfg(feature = "adw-1-8")]
    {
        let shortcuts = gio::SimpleAction::new("shortcuts", None);
        shortcuts.connect_activate(glib::clone!(
            #[weak]
            app,
            move |_, _| shortcuts::show(app.active_window().as_ref())
        ));
        app.add_action(&shortcuts);
        app.set_accels_for_action("app.shortcuts", &["<Control>question"]);
    }
    // Quitting closes each window, so each asks about unsaved changes.
    let quit = gio::SimpleAction::new("quit", None);
    quit.connect_activate(glib::clone!(
        #[weak]
        app,
        move |_, _| {
            for window in app.windows() {
                window.close();
            }
        }
    ));
    app.add_action(&quit);
}

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("NOX_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    i18n::init();
    gio::resources_register_include!("nox.gresource").expect("bundled resources");
    // Which AI models there are and which tasks use them (read once).
    let models = nox_ai::models::registry();
    tracing::debug!("{} AI models known", models.models().len());

    let app = adw::Application::builder()
        .application_id(APP_ID)
        // CAN_OVERRIDE_APP_ID allows `--gapplication-app-id=…` to run a
        // separate instance (e.g. for testing next to a running Nox).
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::CAN_OVERRIDE_APP_ID)
        // Where libadwaita finds style.css and the app's icons, also under
        // another app id.
        .resource_base_path(RESOURCES)
        .build();
    app.connect_startup(|app| {
        set_up(app);
        gtk::Window::set_default_icon_name(APP_ID);
    });
    // Only shortcuts with Ctrl or Alt here: GTK runs application
    // accelerators before the focused widget, so a bare key would be taken
    // from it (Space from a focused button, arrows from a spin button).
    // Those keys are in a bubble-phase controller in window.ui.
    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("window.close", &["<Control>w"]);
    app.set_accels_for_action("win.new", &["<Control>n"]);
    app.set_accels_for_action("win.open", &["<Control>o"]);
    app.set_accels_for_action("win.save", &["<Control>s"]);
    app.set_accels_for_action("win.save-as", &["<Control><Shift>s"]);
    app.set_accels_for_action("win.undo", &["<Control>z"]);
    app.set_accels_for_action("win.split", &["<Control>b"]);
    app.set_accels_for_action("win.add-dissolve", &["<Control>t"]);
    app.set_accels_for_action("win.add-text", &["<Control><Shift>t"]);
    app.set_accels_for_action("win.export", &["<Control>e"]);
    app.set_accels_for_action("app.preferences", &["<Control>comma"]);
    app.set_accels_for_action("win.redo", &["<Control><Shift>z", "<Control>y"]);
    app.connect_activate(|app| window::build(app.upcast_ref(), &[]).present());
    // `nox clip.mp4 …` imports the files into the media pool; `nox
    // edit.otio` opens a project.
    app.connect_open(|app, files, _| window::build(app.upcast_ref(), files).present());
    app.run()
}
