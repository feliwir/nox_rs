//! The main window (`resources/ui/window.ui`): media pool and viewer on
//! top, the timeline below. The template holds the layout; `Ui` holds the
//! project and implements the window's actions.

use adw::prelude::AdwApplicationWindowExt;
use adw::subclass::prelude::*;
use gtk::{CompositeTemplate, gdk, gio, glib, prelude::*};
use nox_project::{BlendMode, Edge, ItemRef, Rational, Time, TrackKind, supports_drop_frame};
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::alert::Look;
use crate::clip_strip::ClipStrip;
use crate::color_wheels::ColorWheels;
use crate::i18n::{fill, gettext, ngettext};
use crate::inspector::Inspector;
use crate::media_object::MediaObject;
use crate::node_editor::NodeEditor;
use crate::node_panel::NodePanel;
use crate::playback::Playback;
use crate::project::Framing;
use crate::project::Project;
use crate::scopes::ScopesPanel;
use crate::{media_pool::MediaPool, timeline::TimelineView, viewer::Viewer};

mod imp {
    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(resource = "/io/github/feliwir/Nox/ui/window.ui")]
    pub struct Window {
        #[template_child]
        pub media_pool: TemplateChild<MediaPool>,
        #[template_child]
        pub viewer: TemplateChild<Viewer>,
        #[template_child]
        pub timeline: TemplateChild<TimelineView>,
        #[template_child]
        pub pages: TemplateChild<gtk::Stack>,
        #[template_child]
        pub edit_viewer_slot: TemplateChild<gtk::Box>,
        #[template_child]
        pub color_viewer_slot: TemplateChild<gtk::Box>,
        #[template_child]
        pub effects_viewer_slot: TemplateChild<gtk::Box>,
        #[template_child]
        pub effects_paned: TemplateChild<gtk::Paned>,
        #[template_child]
        pub effects_clip_strip: TemplateChild<ClipStrip>,
        #[template_child]
        pub node_editor: TemplateChild<NodeEditor>,
        #[template_child]
        pub node_panel: TemplateChild<NodePanel>,
        #[template_child]
        pub inspector: TemplateChild<Inspector>,
        #[template_child]
        pub inspector_toggle: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub window_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub offline_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub toasts: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub standard_items: TemplateChild<gio::Menu>,
        #[template_child]
        pub clip_strip: TemplateChild<ClipStrip>,
        #[template_child]
        pub color_paned: TemplateChild<gtk::Paned>,
        #[template_child]
        pub scopes: TemplateChild<ScopesPanel>,
        #[template_child]
        pub color_wheels: TemplateChild<ColorWheels>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "NoxWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            MediaPool::ensure_type();
            Viewer::ensure_type();
            TimelineView::ensure_type();
            Inspector::ensure_type();
            ClipStrip::ensure_type();
            ScopesPanel::ensure_type();
            ColorWheels::ensure_type();
            NodeEditor::ensure_type();
            NodePanel::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Window {}
    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    /// The pool, viewer and timeline of the template.
    #[cfg(test)]
    pub(crate) fn imp_children(&self) -> (MediaPool, Viewer, TimelineView) {
        let imp = self.imp();
        (imp.media_pool.get(), imp.viewer.get(), imp.timeline.get())
    }
}

/// Builds the main window. `.otio` files open as the project, other files
/// are imported into the media pool.
pub fn build(app: &gtk::Application, files: &[gio::File]) -> Window {
    let window: Window = glib::Object::builder().property("application", app).build();
    let project = Project::new();
    let pool = window.imp().media_pool.get();
    let viewer = window.imp().viewer.get();
    let timeline = window.imp().timeline.get();
    timeline.set_project(project.clone());
    let playback = Playback::new(&project, &viewer, &timeline);

    let ui = Rc::new(Ui {
        window: window.clone().upcast(),
        project: project.clone(),
        pool: pool.clone(),
        timeline: timeline.clone(),
        playback,
    });
    // The window owns the controller; handlers only hold weak references.
    // The strong one is released when the window leaves the application
    // (closed, or destroyed after "discard changes"). Not on `destroy`:
    // GTK 4 emits that only once the last reference is gone, which the
    // window ↔ ui cycle prevents, so the controller and its render thread
    // outlived the window. The thread then kept decoding while the GPU
    // driver was unloaded at exit, and crashed in the driver.
    let owner = RefCell::new(Some(ui.clone()));
    app.connect_window_removed(glib::clone!(
        #[weak]
        window,
        move |_, removed| {
            if removed != window.upcast_ref::<gtk::Window>() {
                return;
            }
            if let Some(ui) = owner.take() {
                // Other closures may still hold the playback; stop it anyway.
                ui.playback.shutdown();
            }
        }
    ));
    #[cfg(feature = "adw-1-8")]
    window.imp().standard_items.insert(
        1,
        Some(&gettext("_Keyboard Shortcuts")),
        Some("app.shortcuts"),
    );
    window.imp().inspector.set_timeline(&timeline);
    window.imp().inspector.set_playback(&ui.playback);
    window.imp().clip_strip.set_up(&timeline, &pool);
    window.imp().color_wheels.set_timeline(&timeline);
    window.imp().effects_clip_strip.set_up(&timeline, &pool);
    window
        .imp()
        .node_editor
        .set_up(&timeline, &window.imp().inspector);
    window.imp().node_editor.set_playback(&ui.playback);
    window.imp().node_panel.set_up(&timeline);
    let panel = window.imp().node_panel.downgrade();
    window
        .imp()
        .node_editor
        .connect_selection_changed(move |key| {
            if let Some(panel) = panel.upgrade() {
                panel.show(key);
            }
        });
    window
        .imp()
        .viewer
        .transform_overlay()
        .set_up(&timeline, &pool);
    add_page_action(&window, &ui.playback);
    add_shift_digit_keys(&window);
    add_transport_actions(window.upcast_ref(), &timeline, &project);
    add_project_actions(&ui);
    add_clip_actions(&ui);
    add_track_actions(&ui);
    pool.connect_activate(glib::clone!(
        #[weak]
        ui,
        move |media| ui.append_media(&media)
    ));
    project.connect_changed(glib::clone!(
        #[weak]
        ui,
        move |_| ui.update()
    ));
    window.connect_close_request(glib::clone!(
        #[weak]
        ui,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |window| {
            if !ui.project.is_modified() {
                return glib::Propagation::Proceed;
            }
            ui.confirm_discard(glib::clone!(
                #[weak]
                window,
                move || window.destroy()
            ));
            glib::Propagation::Stop
        }
    ));

    for file in files {
        match file.path() {
            Some(path) if is_project_file(&path) => ui.open(&path),
            _ => pool.import(file),
        }
    }
    ui.update();
    window
}

fn is_project_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("otio"))
}

struct Ui {
    window: gtk::ApplicationWindow,
    project: Rc<Project>,
    pool: MediaPool,
    timeline: TimelineView,
    playback: Rc<Playback>,
}

impl Ui {
    fn action(&self, name: &str) -> gio::SimpleAction {
        self.window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
            .expect("window action")
    }

    /// Syncs the title and action states with the project.
    fn update(&self) {
        let project = &self.project;
        self.window.set_title(Some(&project.title()));
        // The header bar: the name (marked when there are unsaved
        // changes) over the project's folder.
        if let Some(window) = self.window.downcast_ref::<Window>() {
            let title = &window.imp().window_title;
            let name = project.name();
            title.set_title(&if project.is_modified() {
                format!("• {name}") // no-i18n: a mark and a name
            } else {
                name
            });
            let folder = project.path().and_then(|p| p.parent().map(display_folder));
            title.set_subtitle(folder.as_deref().unwrap_or_default());
        }
        let undo = self.action("undo");
        undo.set_enabled(project.undo_label().is_some());
        let redo = self.action("redo");
        redo.set_enabled(project.redo_label().is_some());
        let offline = project.timeline().offline_files().len();
        self.action("relink-offline").set_enabled(offline > 0);
        if let Some(window) = self.window.downcast_ref::<Window>() {
            let banner = &window.imp().offline_banner;
            if offline > 0 {
                banner.set_title(&fill(
                    &ngettext(
                        "{n} media file is offline; its clips show black",
                        "{n} media files are offline; their clips show black",
                        offline as u32,
                    ),
                    &[("n", &offline)],
                ));
            }
            banner.set_revealed(offline > 0);
        }

        let fps = project.frame_rate();
        self.action("frame-rate")
            .set_state(&format!("{}/{}", fps.num, fps.den).to_variant());
        let drop_frame = self.action("drop-frame");
        drop_frame.set_enabled(supports_drop_frame(fps));
        drop_frame.set_state(&project.timeline().settings.drop_frame.to_variant());
        let settings = project.timeline().settings.clone();
        self.action("resolution")
            .set_state(&format!("{}x{}", settings.width, settings.height).to_variant());

        // Keep the playhead inside the sequence (plus one frame to see the end).
        let end = project.duration_frames();
        if self.timeline.playhead() > end {
            self.timeline.set_playhead(end);
        }
    }

    /// An error about the project's file (opening, saving): told in a
    /// dialog.
    fn show_error(&self, message: &str, detail: &str) {
        tracing::warn!("{message}: {detail}");
        crate::alert::tell(&self.window, message, detail);
    }

    /// Feedback that needs no answer, such as an edit that couldn't be
    /// made: a toast at the bottom of the window.
    fn toast(&self, message: &str) {
        if let Some(window) = self.window.downcast_ref::<Window>() {
            window.imp().toasts.add_toast(adw::Toast::new(message));
        }
    }

    /// An edit that couldn't be made, and why: a toast.
    fn toast_error(&self, message: &str, reason: &str) {
        tracing::warn!("{message}: {reason}");
        self.toast(&fill(
            &gettext("{message}: {reason}"),
            &[("message", &message), ("reason", &reason)],
        ));
    }

    fn open(self: &Rc<Self>, path: &Path) {
        if let Err(err) = self.project.open(path) {
            self.show_error(
                &fill(&gettext("Cannot open {path}"), &[("path", &path.display())]),
                &err.to_string(),
            );
            return;
        }
        // Import the media the timeline uses, so it can be added again.
        let timeline = self.project.timeline();
        let paths: HashSet<PathBuf> = timeline
            .clips()
            .filter_map(|c| timeline.media_path(c.media_reference()?))
            .collect();
        drop(timeline);
        let mut missing = 0;
        for path in paths {
            if path.exists() {
                self.pool.import(&gio::File::for_path(&path));
            } else {
                tracing::warn!("media offline: {}", path.display());
                missing += 1;
            }
        }
        self.timeline.set_selection(None);
        self.timeline.set_playhead(0);
        if missing > 0 {
            // The banner (update) tells it and offers to relink.
            tracing::warn!("{missing} media files of {} are missing", path.display());
        }
    }

    /// Asks for a folder and relinks the offline media found in it.
    fn choose_relink_folder(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Relink Offline Media"))
            .modal(true)
            .build();
        let ui = self.clone();
        dialog.select_folder(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if let Some(folder) = dialog_path(result) {
                ui.relink_from_folder(&folder);
            }
        });
    }

    /// Relinks every offline file that has a namesake in `folder` (or
    /// below), and tells how many were found.
    fn relink_from_folder(&self, folder: &Path) {
        let offline = self.project.timeline().offline_files();
        if offline.is_empty() {
            self.toast(&gettext("No media is offline"));
            return;
        }
        let found = nox_project::relink::find_in_folder(
            &offline,
            folder,
            nox_project::relink::SEARCH_DEPTH,
        );
        self.relink(&found);
        let message = fill(
            &ngettext(
                "Found {found} of {n} offline file",
                "Found {found} of {n} offline files",
                offline.len() as u32,
            ),
            &[("found", &found.len()), ("n", &offline.len())],
        );
        self.toast(&message);
    }

    /// Asks for a new file for the selected clip's media. Other offline
    /// files that are in the chosen file's folder are relinked too.
    fn choose_relink_file(self: &Rc<Self>) {
        let old = self.timeline.selection().and_then(|item| {
            let timeline = self.project.timeline();
            match timeline.item(item).ok()? {
                nox_project::Composable::Clip(clip) => timeline.media_path(clip.media_reference()?),
                _ => None,
            }
        });
        let Some(old) = old else {
            tracing::info!("select a clip to relink");
            return;
        };
        let name = old
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let dialog = gtk::FileDialog::builder()
            .title(fill(&gettext("Relink {name}"), &[("name", &name)]))
            .modal(true)
            .initial_name(name)
            .build();
        if let Some(folder) = old.parent().filter(|f| f.is_dir()) {
            dialog.set_initial_folder(Some(&gio::File::for_path(folder)));
        }
        let ui = self.clone();
        dialog.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if let Some(new) = dialog_path(result) {
                ui.relink_file(&old, &new);
            }
        });
    }

    /// Relinks `old` to `new`, and other offline files to namesakes in
    /// `new`'s folder.
    fn relink_file(&self, old: &Path, new: &Path) {
        let mut pairs = vec![(old.to_owned(), new.to_owned())];
        if let Some(folder) = new.parent() {
            let mut others = self.project.timeline().offline_files();
            others.remove(old);
            pairs.extend(nox_project::relink::find_in_folder(&others, folder, 0));
        }
        self.relink(&pairs);
    }

    /// Points the clips using each first path at the second, as one edit,
    /// and imports the new files into the media pool.
    fn relink(&self, pairs: &[(PathBuf, PathBuf)]) {
        let result = self.project.edit(&gettext("Relink Media"), |t| {
            match pairs
                .iter()
                .map(|(old, new)| t.relink(old, new))
                .sum::<usize>()
            {
                0 => Err("nothing to relink"),
                _ => Ok(()),
            }
        });
        if result.is_ok() {
            for (_, new) in pairs {
                if self.pool.find(new).is_none() {
                    self.pool.import(&gio::File::for_path(new));
                }
            }
        }
    }

    /// Saves to the project's file, or asks for one. Calls `then` after a
    /// successful save.
    fn save(self: &Rc<Self>, then: Option<Box<dyn FnOnce()>>) {
        match self.project.path() {
            Some(path) => {
                if self.save_to(&path)
                    && let Some(then) = then
                {
                    then();
                }
            }
            None => self.save_as(then),
        }
    }

    fn save_to(&self, path: &Path) -> bool {
        match self.project.save_as(path) {
            Ok(()) => true,
            Err(err) => {
                self.show_error(
                    &fill(&gettext("Cannot save {path}"), &[("path", &path.display())]),
                    &err.to_string(),
                );
                false
            }
        }
    }

    fn save_as(self: &Rc<Self>, then: Option<Box<dyn FnOnce()>>) {
        let name = format!("{}.otio", self.project.timeline().name);
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Save Project"))
            .modal(true)
            .initial_name(name)
            .filters(&otio_filters())
            .build();
        let ui = self.clone();
        dialog.save(Some(&self.window), gio::Cancellable::NONE, move |result| {
            let Some(mut path) = dialog_path(result) else {
                return;
            };
            if !is_project_file(&path) {
                path.set_extension("otio");
            }
            if ui.save_to(&path)
                && let Some(then) = then
            {
                then();
            }
        });
    }

    fn choose_and_open(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Open Project"))
            .modal(true)
            .filters(&otio_filters())
            .build();
        let ui = self.clone();
        dialog.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if let Some(path) = dialog_path(result) {
                ui.open(&path);
            }
        });
    }

    /// Runs `proceed` unless the user cancels, offering to save first.
    fn confirm_discard(self: &Rc<Self>, proceed: impl FnOnce() + 'static) {
        if !self.project.is_modified() {
            proceed();
            return;
        }
        let ui = self.clone();
        crate::alert::ask(
            &self.window,
            &gettext("Save changes to the project?"),
            &gettext("Unsaved changes will be lost otherwise."),
            &[
                ("cancel", gettext("Cancel"), Look::Plain),
                ("discard", gettext("Discard"), Look::Destructive),
                ("save", gettext("Save"), Look::Suggested),
            ],
            move |answer| match answer {
                "discard" => proceed(),
                _ => ui.save(Some(Box::new(proceed))),
            },
        );
    }

    fn add_track(&self, kind: TrackKind) {
        self.project.change(&gettext("Add Track"), |t| {
            t.add_track(kind);
        });
    }

    /// Applies `change` to the selected clip as one undoable edit.
    fn edit_selected_clip(&self, label: &str, change: impl Fn(&mut nox_project::Item)) {
        self.timeline.edit_selected_clip(label, None, change);
    }

    /// A one-second dissolve at the selected clip's edge nearest the
    /// playhead. If the clips lack media beyond the cut for it, offers to
    /// trim them to make room (as Resolve does).
    /// Adds a five-second text clip at the playhead (on the top video
    /// track, or a new one above it) and selects it.
    fn add_text(&self) {
        let fps = self.project.frame_rate();
        let at = Time::from_frames(self.timeline.playhead(), fps);
        let duration = Time::from_frames(5 * nox_project::nominal_fps(fps) as i64, fps);
        let parameters = nox_text::Document::new(&gettext("Title")).to_parameters();
        match self.project.edit(&gettext("Add Text"), |t| {
            t.add_text_clip(at, duration, parameters)
        }) {
            Ok(item) => self.timeline.set_selection(Some(item)),
            Err(err) => self.toast_error(&gettext("Cannot add text"), &err.to_string()),
        }
    }

    /// Imports subtitles onto a subtitle track: with a video clip
    /// selected, one of its file's subtitle streams or a subtitle file,
    /// timed and linked with the clip; else a subtitle file from the start
    /// of the sequence.
    fn import_subtitles(self: &Rc<Self>) {
        let target = self.subtitle_target();
        let media = target.and_then(|item| {
            let timeline = self.project.timeline();
            match timeline.item(item).ok()? {
                nox_project::Composable::Clip(clip) => timeline.media_path(clip.media_reference()?),
                _ => None,
            }
        });
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let streams = match media.clone() {
                Some(path) => gio::spawn_blocking(move || {
                    nox_media::probe(&path)
                        .map(|info| info.subtitles)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default(),
                None => Vec::new(),
            };
            match media {
                Some(path) if !streams.is_empty() => {
                    ui.choose_subtitle_stream(path, streams, target, true);
                }
                _ => ui.choose_subtitle_file(target),
            }
        });
    }

    /// The selected video clip of a file, which imported subtitles are
    /// timed with.
    fn subtitle_target(&self) -> Option<ItemRef> {
        let item = self.timeline.selection()?;
        let timeline = self.project.timeline();
        if timeline.track(item.track)?.kind != TrackKind::Video {
            return None;
        }
        match timeline.item(item).ok()? {
            nox_project::Composable::Clip(clip)
                if clip.text_parameters().is_none() && clip.media_reference()?.path().is_some() =>
            {
                Some(item)
            }
            _ => None,
        }
    }

    /// Asks which of `streams` of `path` to import (or, with `offer_file`,
    /// a subtitle file instead); one stream of a chosen file is imported
    /// without asking.
    fn choose_subtitle_stream(
        self: &Rc<Self>,
        path: PathBuf,
        streams: Vec<nox_media::SubtitleInfo>,
        target: Option<ItemRef>,
        offer_file: bool,
    ) {
        if let [stream] = &streams[..]
            && !offer_file
        {
            self.import_subtitle_stream(path, stream.clone(), target);
            return;
        }
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let mut answers = vec![("cancel".to_owned(), gettext("Cancel"), Look::Plain)];
        if offer_file {
            answers.push(("file".to_owned(), gettext("From File…"), Look::Plain));
        }
        let suggested = streams
            .iter()
            .position(|s| s.tags.default)
            .unwrap_or_default();
        for (i, stream) in streams.iter().enumerate() {
            let look = if i == suggested {
                Look::Suggested
            } else {
                Look::Plain
            };
            let mut label = stream.label();
            if stream.tags.forced {
                label = fill(&gettext("{subtitles}, forced"), &[("subtitles", &label)]);
            }
            answers.push((format!("stream-{i}"), label, look));
        }
        let answers: Vec<(&str, String, Look)> = answers
            .iter()
            .map(|(id, label, look)| (id.as_str(), label.clone(), *look))
            .collect();
        let ui = self.clone();
        crate::alert::ask(
            &self.window,
            &gettext("Import Subtitles"),
            &fill(
                &gettext("Which subtitles of “{file}” go on a new subtitle track?"),
                &[("file", &name)],
            ),
            &answers,
            move |answer| {
                if answer == "file" {
                    ui.choose_subtitle_file(target);
                } else if let Some(stream) = answer
                    .strip_prefix("stream-")
                    .and_then(|i| i.parse::<usize>().ok())
                    .and_then(|i| streams.get(i))
                {
                    ui.import_subtitle_stream(path, stream.clone(), target);
                }
            },
        );
    }

    /// Asks for a subtitle file (or a video with subtitles) to import.
    fn choose_subtitle_file(self: &Rc<Self>, target: Option<ItemRef>) {
        let subtitles = gtk::FileFilter::new();
        subtitles.set_name(Some(&gettext("Subtitles")));
        for suffix in [
            "srt", "ass", "ssa", "vtt", "sup", "idx", "mks", "mkv", "mp4", "m4v", "mov", "ts",
            "m2ts",
        ] {
            subtitles.add_suffix(suffix);
        }
        let all = gtk::FileFilter::new();
        all.set_name(Some(&gettext("All Files")));
        all.add_pattern("*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&subtitles);
        filters.append(&all);
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Import Subtitles"))
            .modal(true)
            .filters(&filters)
            .build();
        let ui = self.clone();
        dialog.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if let Some(path) = dialog_path(result) {
                ui.import_subtitle_file(path, target, true);
            }
        });
    }

    /// Imports the subtitles of the file at `path`: its only stream, else
    /// the one chosen (with `ask`) or its default (or first) one.
    fn import_subtitle_file(self: &Rc<Self>, path: PathBuf, target: Option<ItemRef>, ask: bool) {
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let probed = path.clone();
            let streams =
                gio::spawn_blocking(move || nox_media::probe(&probed).map(|info| info.subtitles))
                    .await;
            match streams {
                Ok(Ok(streams)) if ask && !streams.is_empty() => {
                    ui.choose_subtitle_stream(path, streams, target, false);
                }
                Ok(Ok(streams)) if !streams.is_empty() => {
                    let stream = streams
                        .iter()
                        .find(|s| s.tags.default)
                        .unwrap_or(&streams[0]);
                    ui.import_subtitle_stream(path, stream.clone(), target);
                }
                Ok(Err(err)) => {
                    ui.toast_error(&gettext("Cannot import subtitles"), &err.to_string());
                }
                _ => ui.toast(&fill(
                    &gettext("No subtitles in “{file}”"),
                    &[("file", &path.display())],
                )),
            }
        });
    }

    /// Reads `stream` of `path` (in the background) and adds it on a new
    /// subtitle track, timed and linked with `target` if it is still
    /// there, as one edit.
    fn import_subtitle_stream(
        self: &Rc<Self>,
        path: PathBuf,
        stream: nox_media::SubtitleInfo,
        target: Option<ItemRef>,
    ) {
        let fps = self.project.frame_rate();
        let name = stream.label();
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let index = stream.stream_index;
            let file = path.clone();
            let read =
                gio::spawn_blocking(move || crate::subtitle_import::read(&file, index)).await;
            let cannot = gettext("Cannot import subtitles");
            let end = match read {
                Ok(Ok(Some(end))) => end,
                Ok(Ok(None)) => return ui.toast(&gettext("No subtitles to import there")),
                Ok(Err(err)) => return ui.toast_error(&cannot, &err.to_string()),
                Err(_) => return,
            };
            let added = ui.project.edit(&gettext("Import Subtitles"), |t| {
                let like = target.and_then(|item| match t.item(item).ok()? {
                    nox_project::Composable::Clip(clip) => {
                        Some((item, clip.clone(), t.item_range(item).ok()?.start))
                    }
                    _ => None,
                });
                let (clip, at) = match like {
                    Some((item, video, at)) => {
                        let mut clip =
                            crate::subtitle_import::clip_like(&video, &path, index, &name);
                        // Linked with the clip (and what it is linked with).
                        let link = video.item.link().unwrap_or_else(|| t.new_link_id());
                        if let Some(nox_project::Composable::Clip(video)) = t
                            .track_mut(item.track)
                            .and_then(|track| track.children.get_mut(item.index))
                        {
                            video.item.set_link(Some(link));
                        }
                        clip.item.set_link(Some(link));
                        (clip, at)
                    }
                    None => (
                        crate::subtitle_import::clip_from_start(&path, index, &name, end, fps),
                        Time::zero(fps.inverse()),
                    ),
                };
                t.add_subtitle_track(&name, &stream.tags, vec![(at, clip)])
            });
            match added {
                Ok(_) => ui.toast(&fill(
                    &gettext("Added the subtitle track “{name}”"),
                    &[("name", &name)],
                )),
                Err(err) => ui.toast_error(&cannot, &err.to_string()),
            }
        });
    }

    /// Turns the text subtitles of subtitle track `index` into text clips
    /// on caption tracks (video tracks above the others), styled and
    /// placed as they show, to change them like titles; the subtitle
    /// track is hidden.
    pub(crate) fn convert_to_text_clips(self: &Rc<Self>, index: usize) {
        let fps = self.project.frame_rate();
        let (clips, name) = {
            let timeline = self.project.timeline();
            let Some(track) = timeline.track(index) else {
                return;
            };
            (
                crate::subtitle_import::to_convert(&timeline, index),
                track.item.name.clone(),
            )
        };
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let converted =
                gio::spawn_blocking(move || crate::subtitle_import::text_clips(&clips, fps)).await;
            let cannot = gettext("Cannot convert to text clips");
            let clips = match converted {
                Ok(Ok(Some(clips))) if !clips.is_empty() => clips,
                Ok(Ok(Some(_))) => return ui.toast(&gettext("No subtitles to convert")),
                Ok(Ok(None)) => {
                    return ui.toast(&gettext("Bitmap subtitles have no text to convert"));
                }
                Ok(Err(err)) => return ui.toast_error(&cannot, &err.to_string()),
                Err(_) => return,
            };
            let result = ui.project.edit(&gettext("Convert to Text Clips"), |t| {
                t.add_caption_tracks(&name, clips)?;
                if let Some(track) = t.track_mut(index) {
                    track.item.enabled = false;
                }
                Ok::<_, nox_project::EditError>(())
            });
            if let Err(err) = result {
                ui.toast_error(&cannot, &err.to_string());
            }
        });
    }

    fn add_dissolve(self: &Rc<Self>) {
        let Some(item) = self.timeline.selection() else {
            tracing::info!("select a clip to add a dissolve to");
            return;
        };
        let fps = self.project.frame_rate();
        let playhead = Time::from_frames(self.timeline.playhead(), fps);
        let Ok(range) = self.project.timeline().item_range(item) else {
            return;
        };
        let edge = if (playhead - range.start).as_seconds().abs()
            <= (range.end() - playhead).as_seconds().abs()
        {
            Edge::Start
        } else {
            Edge::End
        };
        let one_second = Time::from_frames(nox_project::nominal_fps(fps) as i64, fps);
        let label = gettext("Add Dissolve");
        let cannot = gettext("Cannot add a dissolve");
        let room = {
            let timeline = self.project.timeline();
            timeline
                .transition_fits(item, edge, one_second)
                .and_then(|fits| Ok((fits, timeline.transition_trims(item, edge, one_second)?)))
        };
        let (trim_end, trim_start) = match room {
            Ok((true, _)) => {
                let _ = self
                    .project
                    .edit(&label, |t| t.add_dissolve(item, edge, one_second));
                return;
            }
            Ok((false, trims)) => trims,
            Err(err) => {
                self.toast_error(&cannot, &err.to_string());
                return;
            }
        };
        let detail = fill(
            &gettext(
                "The clips have too little media beyond the cut for a dissolve of {length}. \
                 Trim {trim} off the clips at the cut to make room? What follows moves up.",
            ),
            &[
                ("length", &self.project.timecode(one_second.frame(fps))),
                (
                    "trim",
                    &self.project.timecode((trim_end + trim_start).frame(fps)),
                ),
            ],
        );
        let ui = self.clone();
        crate::alert::ask(
            &self.window,
            &gettext("Not enough media for the dissolve"),
            &detail,
            &[
                ("cancel", gettext("Cancel"), Look::Plain),
                ("trim", gettext("Trim Clips"), Look::Suggested),
            ],
            move |_| {
                let zero = Time::zero(one_second.timebase);
                let dissolve = nox_project::Transition::new(
                    nox_project::DISSOLVE,
                    "Cross Dissolve",
                    zero,
                    zero,
                );
                if let Err(err) = ui.project.edit(&label, |t| {
                    t.add_transition_making_room(item, edge, one_second, &dissolve)
                }) {
                    ui.toast_error(&cannot, &err.to_string());
                }
            },
        );
    }

    /// Changes the In/Out marks as an undoable edit; `change` gets the
    /// settings and the playhead's frame as a time.
    fn mark(&self, label: &str, change: impl FnOnce(&mut nox_project::Settings, Time)) {
        let fps = self.project.frame_rate();
        let playhead = Time::from_frames(self.timeline.playhead(), fps);
        self.project
            .change(label, |t| change(&mut t.settings, playhead));
    }

    /// Edits the media pool's selection into the sequence at the playhead,
    /// one after another, inserting (pushing what follows) or overwriting,
    /// as F9/F10 in Resolve; the playhead moves past the last clip.
    fn edit_selected_media(self: &Rc<Self>, insert: bool) {
        let mut queue = VecDeque::new();
        for media in self.pool.selected() {
            match media.info() {
                Some(info) => queue.push_back(info),
                None => tracing::info!("{} is still loading", media.name()),
            }
        }
        self.add_media_batch(queue, self.timeline.playhead(), insert, None);
    }

    /// Adds `queue` from `frame` on, one clip after another. A clip that
    /// doesn't fit the timeline's orientation asks how to fit it; the answer
    /// holds for the rest of the batch.
    fn add_media_batch(
        self: &Rc<Self>,
        mut queue: VecDeque<nox_media::MediaInfo>,
        frame: i64,
        insert: bool,
        choice: Option<Framing>,
    ) {
        let Some(info) = queue.pop_front() else {
            self.timeline.set_playhead(frame);
            return;
        };
        let ui = self.clone();
        let name = info.path.display().to_string();
        crate::framing::add_media(
            &self.window,
            &self.project,
            info,
            (None, frame, insert),
            choice,
            move |result, asked| {
                let frame = match result {
                    Ok(length) => frame + length,
                    Err(err) => {
                        tracing::info!("cannot add {name}: {err}");
                        frame
                    }
                };
                ui.add_media_batch(queue, frame, insert, asked.or(choice));
            },
        );
    }

    /// Adds `media` at the end of the sequence.
    fn append_media(self: &Rc<Self>, media: &MediaObject) {
        let Some(info) = media.info() else {
            tracing::info!("{} is still loading", media.name());
            return;
        };
        let end = self.project.duration_frames();
        crate::framing::add_media(
            &self.window,
            &self.project,
            info,
            (None, end, false),
            None,
            |_, _| {},
        );
    }
}

fn otio_filters() -> gio::ListStore {
    let otio = gtk::FileFilter::new();
    otio.set_name(Some("OpenTimelineIO")); // no-i18n: a format name
    otio.add_suffix("otio");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&otio);
    filters
}

fn dialog_path(result: Result<gio::File, glib::Error>) -> Option<PathBuf> {
    match result {
        Ok(file) => file.path(),
        Err(err) => {
            if !err.matches(gtk::DialogError::Dismissed) {
                tracing::warn!("file dialog failed: {err}");
            }
            None
        }
    }
}

/// File, edit and sequence actions. The `*-path` variants take a path
/// instead of showing a dialog, for scripting over D-Bus.
/// `win.page` ("edit", "effects" or "color"; Shift+4, Shift+5 and Shift+6
/// as in Resolve's Edit, Fusion and Color pages)
/// switches pages. The one viewer moves to the visible page, so there is
/// one engine and nothing renders twice.
/// Shift+4, Shift+5, Shift+6 and Shift+9 as in DaVinci Resolve: the Edit,
/// Effects and Color pages and Project Settings. Matched by the physical key, since Shift
/// plus a digit key types a different character on most layouts (`^` for
/// Shift+6 on US English, `&` on German), which a `<Shift>6` trigger
/// misses. A key counts as digit `d` if it types `d` without Shift (US,
/// German) or with it (French). Runs after the focused widget, like the
/// window's other single-key shortcuts.
fn add_shift_digit_keys(window: &Window) {
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        window,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, keyval, keycode, state| {
            let mods = state & gtk::accelerator_get_default_mod_mask();
            if mods != gdk::ModifierType::SHIFT_MASK {
                return glib::Propagation::Proceed;
            }
            let digit = |k: gdk::Key| k.to_unicode().and_then(|c| c.to_digit(10));
            let unshifted = WidgetExt::display(&window)
                .map_keycode(keycode)
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|(key, _)| key.level() == 0 && key.group() == 0)
                        .and_then(|(_, k)| digit(*k))
                });
            let action = match digit(keyval).or(unshifted) {
                Some(4) => ("win.page", Some("edit".to_variant())),
                Some(5) => ("win.page", Some("effects".to_variant())),
                Some(6) => ("win.page", Some("color".to_variant())),
                Some(9) => ("win.project-settings", None),
                _ => return glib::Propagation::Proceed,
            };
            let _ = WidgetExt::activate_action(&window, action.0, action.1.as_ref());
            glib::Propagation::Stop
        }
    ));
    window.add_controller(keys);
}

fn add_page_action(window: &Window, playback: &Rc<Playback>) {
    let imp = window.imp();
    let page = gio::SimpleAction::new_stateful(
        "page",
        Some(glib::VariantTy::STRING),
        &"edit".to_variant(),
    );
    page.connect_activate(glib::clone!(
        #[weak]
        window,
        move |_, name| {
            if let Some(name) = name.and_then(|n| n.get::<String>())
                && window.imp().pages.child_by_name(&name).is_some()
            {
                window.imp().pages.set_visible_child_name(&name);
            }
        }
    ));
    let playback = Rc::downgrade(playback);
    imp.pages.connect_visible_child_name_notify(glib::clone!(
        #[weak]
        window,
        #[weak]
        page,
        move |pages| {
            let imp = window.imp();
            let name = pages.visible_child_name().unwrap_or_default();
            page.set_state(&name.to_variant());
            imp.inspector_toggle.set_visible(name == "edit");
            imp.viewer.transform_overlay().set_enabled(name == "edit");
            imp.node_editor.set_active(name == "effects");
            let slot = match name.as_str() {
                "color" => &imp.color_viewer_slot,
                "effects" => &imp.effects_viewer_slot,
                _ => &imp.edit_viewer_slot,
            };
            // Scopes are measured only while they are visible.
            if let Some(playback) = playback.upgrade() {
                let panel = (name == "color").then(|| imp.scopes.get());
                playback.set_scopes(panel);
            }
            let viewer = imp.viewer.get();
            if viewer.parent().as_ref() != Some(slot.upcast_ref()) {
                // The slots are boxes; the viewer is their only child.
                if let Some(parent) = viewer.parent().and_downcast::<gtk::Box>() {
                    parent.remove(&viewer);
                }
                slot.append(&viewer);
            }
        }
    ));
    window.add_action(&page);

    // The Color page's splitter starts with room for the clip strip and the
    // wheels (380 px) below the viewer (at most half the page); a fixed position
    // hid them on small screens. Once set, the user's position is kept.
    let paned = imp.color_paned.get();
    paned.connect_max_position_notify(|paned| {
        let max = paned.max_position();
        if max > 0 && !paned.is_position_set() {
            paned.set_position((max - 380).max(max / 2));
        }
    });
    // The same for the Effects page: room for the clip strip and the node
    // editor.
    imp.effects_paned.connect_max_position_notify(|paned| {
        let max = paned.max_position();
        if max > 0 && !paned.is_position_set() {
            paned.set_position((max - 400).max(max / 2));
        }
    });
}

fn add_project_actions(ui: &Rc<Ui>) {
    let simple = |name: &str, f: fn(&Rc<Ui>)| {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(glib::clone!(
            #[weak]
            ui,
            move |_, _| f(&ui)
        ));
        action
    };
    let with_path = |name: &str, f: fn(&Rc<Ui>, &Path)| {
        let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        action.connect_activate(glib::clone!(
            #[weak]
            ui,
            move |_, parameter| {
                if let Some(path) = parameter.and_then(|p| p.get::<String>()) {
                    f(&ui, Path::new(&path));
                }
            }
        ));
        action
    };
    let actions = [
        simple("new", |ui| {
            let target = ui.clone();
            ui.confirm_discard(move || {
                target.project.reset();
                target.timeline.set_playhead(0);
            });
        }),
        simple("open", |ui| {
            let target = ui.clone();
            ui.confirm_discard(move || target.choose_and_open());
        }),
        simple("save", |ui| ui.save(None)),
        simple("save-as", |ui| ui.save_as(None)),
        simple("undo", |ui| ui.project.undo()),
        simple("project-settings", |ui| {
            crate::settings_dialog::show(ui.window.upcast_ref(), &ui.project)
        }),
        simple("export", |ui| {
            crate::export_dialog::show(ui.window.upcast_ref(), &ui.project)
        }),
        simple("play-pause", |ui| ui.playback.toggle()),
        simple("play", |ui| ui.playback.play()),
        simple("pause", |ui| ui.playback.pause()),
        simple("redo", |ui| ui.project.redo()),
        simple("delete-clip", |ui| ui.timeline.delete_selection()),
        simple("ripple-delete", |ui| ui.timeline.ripple_delete_selection()),
        // Marks as in Resolve: In at the playhead's frame, Out after it, so
        // the frame under the playhead is included.
        simple("set-in", |ui| {
            ui.mark(&gettext("Mark In"), |s, playhead| {
                s.set_mark_in(Some(playhead))
            })
        }),
        simple("set-out", |ui| {
            let frame = Time::from_frames(1, ui.project.frame_rate());
            ui.mark(&gettext("Mark Out"), |s, playhead| {
                s.set_mark_out(Some(playhead + frame))
            })
        }),
        simple("clear-in", |ui| {
            ui.mark(&gettext("Clear In"), |s, _| s.set_mark_in(None))
        }),
        simple("clear-out", |ui| {
            ui.mark(&gettext("Clear Out"), |s, _| s.set_mark_out(None))
        }),
        simple("clear-in-out", |ui| {
            ui.mark(&gettext("Clear In and Out"), |s, _| {
                s.set_mark_in(None);
                s.set_mark_out(None);
            })
        }),
        simple("split", |ui| {
            let time = Time::from_frames(ui.timeline.playhead(), ui.project.frame_rate());
            let _ = ui
                .project
                .edit(&gettext("Split Clips"), |t| match t.split_at(time) {
                    0 => Err("no clip at the playhead"),
                    _ => Ok(()),
                });
        }),
        simple("relink-clip", |ui| ui.choose_relink_file()),
        simple("relink-offline", |ui| ui.choose_relink_folder()),
        with_path("relink-folder", |ui, path| ui.relink_from_folder(path)),
        simple("insert-media", |ui| ui.edit_selected_media(true)),
        simple("overwrite-media", |ui| ui.edit_selected_media(false)),
        simple("add-video-track", |ui| ui.add_track(TrackKind::Video)),
        simple("add-audio-track", |ui| ui.add_track(TrackKind::Audio)),
        with_path("open-path", |ui, path| ui.open(path)),
        with_path("import-subtitles-path", |ui, path| {
            let target = ui.subtitle_target();
            ui.import_subtitle_file(path.to_owned(), target, false);
        }),
        with_path("save-path", |ui, path| {
            ui.save_to(path);
        }),
        with_path("append-media", |ui, path| match ui.pool.find(path) {
            Some(media) => ui.append_media(&media),
            None => tracing::warn!("not in the media pool: {}", path.display()),
        }),
    ];
    for action in actions {
        ui.window.add_action(&action);
    }
    if std::env::var("NOX_TEST_ACTIONS").is_ok_and(|v| v == "1") {
        // The topmost dialog over the window (project settings, export).
        ui.window
            .add_action(&with_path("screenshot-dialog", |ui, path| {
                // libadwaita's dialogs are widgets inside the window.
                let window = ui.window.downcast_ref::<adw::ApplicationWindow>();
                match window.and_then(|w| w.visible_dialog()) {
                    Some(content) => {
                        if let Err(err) = screenshot(content.upcast_ref(), path) {
                            tracing::warn!("screenshot failed: {err}");
                        }
                    }
                    None => tracing::warn!("no dialog open"),
                }
            }));
        ui.window.add_action(&with_path("screenshot", |ui, path| {
            // A paintable of the toplevel itself renders nothing.
            let content = ui
                .window
                .downcast_ref::<adw::ApplicationWindow>()
                .and_then(|w| w.content())
                .expect("window content");
            if let Err(err) = screenshot(&content, path) {
                tracing::warn!("screenshot failed: {err}");
            }
        }));
        ui.window
            .add_action(&with_path("screenshot-titlebar", |ui, path| {
                let titlebar = ui
                    .window
                    .downcast_ref::<adw::ApplicationWindow>()
                    .and_then(|w| w.content())
                    .and_downcast::<adw::ToolbarView>()
                    .and_then(|view| view.first_child())
                    .expect("header bar");
                if let Err(err) = screenshot(&titlebar, path) {
                    tracing::warn!("screenshot failed: {err}");
                }
            }));
        // Like the title bar's close button (close-request and all).
        let close = gio::SimpleAction::new("close", None);
        close.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            ui.window,
            move |_, _| window.close()
        ));
        ui.window.add_action(&close);
    }

    let resolution = gio::SimpleAction::new_stateful(
        "resolution",
        Some(glib::VariantTy::STRING),
        &"1920x1080".to_variant(),
    );
    resolution.connect_change_state(glib::clone!(
        #[weak]
        ui,
        move |_, value| {
            let Some(size) = value
                .and_then(|v| v.get::<String>())
                .and_then(|v| parse_size(&v))
            else {
                return;
            };
            ui.project.change(&gettext("Change Resolution"), |t| {
                (t.settings.width, t.settings.height) = size;
            });
        }
    ));
    ui.window.add_action(&resolution);

    let frame_rate = gio::SimpleAction::new_stateful(
        "frame-rate",
        Some(glib::VariantTy::STRING),
        &"25/1".to_variant(),
    );
    frame_rate.connect_change_state(glib::clone!(
        #[weak]
        ui,
        move |_, value| {
            let Some(rate) = value
                .and_then(|v| v.get::<String>())
                .and_then(|v| parse_rate(&v))
            else {
                return;
            };
            if rate != ui.project.frame_rate() {
                ui.project.change(&gettext("Change Frame Rate"), |t| {
                    t.set_frame_rate(rate);
                });
            }
        }
    ));
    ui.window.add_action(&frame_rate);

    let drop_frame = gio::SimpleAction::new_stateful("drop-frame", None, &false.to_variant());
    drop_frame.connect_activate(|action, _| {
        let enabled = action
            .state()
            .and_then(|s| s.get::<bool>())
            .unwrap_or(false);
        action.change_state(&(!enabled).to_variant());
    });
    drop_frame.connect_change_state(glib::clone!(
        #[weak]
        ui,
        move |_, value| {
            let Some(enabled) = value.and_then(|v| v.get::<bool>()) else {
                return;
            };
            // The state itself follows the project in `Ui::update`.
            ui.project.change(&gettext("Change Timecode"), |t| {
                t.settings.drop_frame = enabled;
            });
        }
    ));
    ui.window.add_action(&drop_frame);

    let snapping = gio::SimpleAction::new_stateful("snapping", None, &true.to_variant());
    snapping.connect_activate(glib::clone!(
        #[weak]
        ui,
        move |action, _| {
            let enabled = !ui.timeline.snapping();
            ui.timeline.set_snapping(enabled);
            action.set_state(&enabled.to_variant());
        }
    ));
    ui.window.add_action(&snapping);

    // The inspector can be hidden, giving the viewer its room. It starts
    // hidden, with nothing to inspect, and the first selection shows it,
    // unless the button was used already (then that choice stays). It
    // never hides by itself, so the viewer doesn't jump.
    let window = ui.window.downcast_ref::<Window>().expect("Nox window");
    window.imp().inspector.set_visible(false);
    let chosen = Rc::new(std::cell::Cell::new(false));
    let inspector = gio::SimpleAction::new_stateful("show-inspector", None, &false.to_variant());
    inspector.connect_activate(glib::clone!(
        #[weak]
        ui,
        #[strong]
        chosen,
        move |action, _| {
            let window = ui.window.downcast_ref::<Window>().expect("Nox window");
            let inspector = window.imp().inspector.get();
            let shown = !inspector.is_visible();
            inspector.set_visible(shown);
            action.set_state(&shown.to_variant());
            chosen.set(true);
        }
    ));
    ui.window.add_action(&inspector);
    ui.timeline.connect_selection_changed(glib::clone!(
        #[weak]
        ui,
        #[weak]
        inspector,
        move |timeline| {
            if chosen.get() || timeline.selection().is_none() {
                return;
            }
            let window = ui.window.downcast_ref::<Window>().expect("Nox window");
            window.imp().inspector.set_visible(true);
            inspector.set_state(&true.to_variant());
            chosen.set(true);
        }
    ));
}

/// Renders `widget` with GTK's renderer and saves it as PNG.
fn screenshot(widget: &gtk::Widget, path: &Path) -> Result<(), glib::BoolError> {
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let snapshot = gtk::Snapshot::new();
    // The window background isn't part of its child; use the dark theme's.
    snapshot.append_color(
        &gdk::RGBA::new(0.14, 0.14, 0.14, 1.0),
        &gtk::graphene::Rect::new(0.0, 0.0, widget.width() as f32, widget.height() as f32),
    );
    paintable.snapshot(&snapshot, widget.width().into(), widget.height().into());
    let node = snapshot
        .to_node()
        .ok_or_else(|| glib::bool_error!("nothing to render"))?;
    let renderer = widget
        .native()
        .ok_or_else(|| glib::bool_error!("not realized"))?
        .renderer()
        .ok_or_else(|| glib::bool_error!("no renderer"))?;
    renderer.render_texture(&node, None).save_to_png(path)?;
    tracing::info!("screenshot saved to {}", path.display());
    Ok(())
}

fn parse_size(text: &str) -> Option<(u32, u32)> {
    let (width, height) = text.split_once('x')?;
    let (width, height) = (width.parse().ok()?, height.parse().ok()?);
    (width >= 16 && height >= 16 && width <= 8192 && height <= 8192).then_some((width, height))
}

/// Actions on the selected clip, for its context menu (and D-Bus):
/// `clip-opacity(d)`, `clip-blend(s)`, `clip-volume(d)` and
/// `add-dissolve`, which adds a one-second dissolve at the selected
/// clip's edge nearest the playhead.
fn add_clip_actions(ui: &Rc<Ui>) {
    let with_param = |name: &str, ty: &glib::VariantTy, f: fn(&Ui, &glib::Variant)| {
        let action = gio::SimpleAction::new(name, Some(ty));
        action.connect_activate(glib::clone!(
            #[weak]
            ui,
            move |_, parameter| {
                if let Some(parameter) = parameter {
                    f(&ui, parameter);
                }
            }
        ));
        ui.window.add_action(&action);
    };
    with_param("clip-opacity", glib::VariantTy::DOUBLE, |ui, value| {
        if let Some(opacity) = value.get::<f64>() {
            ui.edit_selected_clip(&gettext("Change Opacity"), |item| item.set_opacity(opacity));
        }
    });
    with_param("clip-blend", glib::VariantTy::STRING, |ui, value| {
        if let Some(mode) = value.get::<String>().and_then(|m| BlendMode::parse(&m)) {
            ui.edit_selected_clip(&gettext("Change Blend Mode"), |item| {
                item.set_blend_mode(mode)
            });
        }
    });
    with_param("clip-volume", glib::VariantTy::DOUBLE, |ui, value| {
        if let Some(volume) = value.get::<f64>() {
            ui.edit_selected_clip(&gettext("Change Volume"), |item| item.set_volume(volume));
        }
    });
    // By stack and track index, for scripting; `(-1, -1)` clears.
    let select = gio::SimpleAction::new("select-clip", Some(glib::VariantTy::new("(xx)").unwrap()));
    select.connect_activate(glib::clone!(
        #[weak]
        ui,
        move |_, parameter| {
            let item = parameter
                .and_then(|p| p.get::<(i64, i64)>())
                .and_then(|(track, index)| {
                    Some(nox_project::ItemRef {
                        track: usize::try_from(track).ok()?,
                        index: usize::try_from(index).ok()?,
                    })
                })
                .filter(|&item| ui.project.timeline().item(item).is_ok());
            ui.timeline.set_selection(item);
        }
    ));
    ui.window.add_action(&select);
    // Several at once, the first primary: `select-clips([(track, index)])`.
    let select_many =
        gio::SimpleAction::new("select-clips", Some(glib::VariantTy::new("a(xx)").unwrap()));
    select_many.connect_activate(glib::clone!(
        #[weak]
        ui,
        move |_, parameter| {
            let items: Vec<nox_project::ItemRef> = parameter
                .and_then(|p| p.get::<Vec<(i64, i64)>>())
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(track, index)| {
                    Some(nox_project::ItemRef {
                        track: usize::try_from(track).ok()?,
                        index: usize::try_from(index).ok()?,
                    })
                })
                .filter(|&item| ui.project.timeline().item(item).is_ok())
                .collect();
            ui.timeline.select_items(items);
        }
    ));
    ui.window.add_action(&select_many);
    // Slip or slide the selected clip by frames, for scripting.
    for (name, slide) in [("slip-clip", false), ("slide-clip", true)] {
        let action = gio::SimpleAction::new(name, Some(glib::VariantTy::INT64));
        action.connect_activate(glib::clone!(
            #[weak]
            ui,
            move |_, parameter| {
                let (Some(item), Some(frames)) = (
                    ui.timeline.selection(),
                    parameter.and_then(|p| p.get::<i64>()),
                ) else {
                    return;
                };
                let delta = Time::from_frames(frames, ui.project.frame_rate());
                let result = if slide {
                    ui.project
                        .edit(&gettext("Slide Clip"), |t| t.slide(item, delta))
                } else {
                    ui.project
                        .edit(&gettext("Slip Clip"), |t| t.slip(item, delta))
                        .map(|()| item)
                };
                if let Ok(item) = result {
                    ui.timeline.set_selection(Some(item));
                }
            }
        ));
        ui.window.add_action(&action);
    }
    // Timeline marker by index, for scripting; -1 clears.
    let select_marker = gio::SimpleAction::new("select-marker", Some(glib::VariantTy::INT64));
    select_marker.connect_activate(glib::clone!(
        #[weak]
        ui,
        move |_, parameter| {
            let index = parameter
                .and_then(|p| p.get::<i64>())
                .and_then(|i| usize::try_from(i).ok())
                .filter(|&i| i < ui.project.timeline().markers().len());
            ui.timeline.select_marker(index);
        }
    ));
    ui.window.add_action(&select_marker);
    let dissolve = gio::SimpleAction::new("add-dissolve", None);
    dissolve.connect_activate(glib::clone!(
        #[weak]
        ui,
        move |_, _| ui.add_dissolve()
    ));
    ui.window.add_action(&dissolve);
    let text = gio::SimpleAction::new("add-text", None);
    text.connect_activate(glib::clone!(
        #[weak]
        ui,
        move |_, _| ui.add_text()
    ));
    ui.window.add_action(&text);
    let subtitles = gio::SimpleAction::new("import-subtitles", None);
    subtitles.connect_activate(glib::clone!(
        #[weak]
        ui,
        move |_, _| ui.import_subtitles()
    ));
    ui.window.add_action(&subtitles);
}

/// Track actions by stack index, for scripting (the track header menus use
/// the timeline's own `track.*` actions): `delete-track(x)`,
/// `set-track-enabled((xb))` (mute or hide), `set-track-locked((xb))`,
/// `rename-track((xs))`, `set-track-shown((xb))` (subtitles in the viewer)
/// and `convert-to-text-clips(x)`.
fn add_track_actions(ui: &Rc<Ui>) {
    let with_param = |name: &str, ty: &str, f: fn(&Ui, &glib::Variant)| {
        let ty = glib::VariantTy::new(ty).expect("variant type");
        let action = gio::SimpleAction::new(name, Some(ty));
        action.connect_activate(glib::clone!(
            #[weak]
            ui,
            move |_, parameter| {
                if let Some(parameter) = parameter {
                    f(&ui, parameter);
                }
            }
        ));
        ui.window.add_action(&action);
    };
    fn index(i: i64) -> Option<usize> {
        usize::try_from(i).ok()
    }
    with_param("delete-track", "x", |ui, value| {
        if let Some(track) = value.get::<i64>().and_then(index) {
            ui.timeline.delete_track(track);
        }
    });
    with_param("set-track-enabled", "(xb)", |ui, value| {
        if let Some((track, on)) = value.get::<(i64, bool)>()
            && let Some(track) = index(track)
        {
            ui.timeline.set_track_enabled(track, on);
        }
    });
    with_param("set-track-locked", "(xb)", |ui, value| {
        if let Some((track, on)) = value.get::<(i64, bool)>()
            && let Some(track) = index(track)
        {
            ui.timeline.set_track_locked(track, on);
        }
    });
    with_param("rename-track", "(xs)", |ui, value| {
        if let Some((track, name)) = value.get::<(i64, String)>()
            && let Some(track) = index(track)
        {
            ui.timeline.rename_track(track, &name);
        }
    });
    with_param("set-track-shown", "(xb)", |ui, value| {
        if let Some((track, on)) = value.get::<(i64, bool)>()
            && let Some(track) = index(track)
        {
            ui.timeline.set_track_shown(track, on);
        }
    });
    // Subtitle track `x` as text clips (the subtitle track menu's).
    let convert = gio::SimpleAction::new("convert-to-text-clips", Some(glib::VariantTy::INT64));
    convert.connect_activate(glib::clone!(
        #[weak]
        ui,
        move |_, parameter| {
            if let Some(track) = parameter.and_then(|p| p.get::<i64>()).and_then(index) {
                ui.convert_to_text_clips(track);
            }
        }
    ));
    ui.window.add_action(&convert);
}

fn parse_rate(text: &str) -> Option<Rational> {
    let (num, den) = text.split_once('/')?;
    let (num, den) = (num.parse().ok()?, den.parse().ok()?);
    (num > 0 && den > 0).then(|| Rational::new(num, den))
}

/// `win.seek(frame)`, `win.next-frame`, `win.previous-frame`,
/// `win.go-to-start` and `win.go-to-end`, used by the transport buttons and
/// shortcuts (and reachable over D-Bus via org.gtk.Actions, which tests use).
fn add_transport_actions(
    window: &gtk::ApplicationWindow,
    timeline: &TimelineView,
    project: &Rc<Project>,
) {
    let seek = gio::ActionEntry::builder("seek")
        .parameter_type(Some(&i64::static_variant_type()))
        .activate(glib::clone!(
            #[weak]
            timeline,
            move |_: &gtk::ApplicationWindow, _, parameter| {
                if let Some(frame) = parameter.and_then(|p| p.get::<i64>()) {
                    timeline.set_playhead(frame.max(0));
                }
            }
        ))
        .build();
    let step = |name: &str, delta: i64| {
        gio::ActionEntry::builder(name)
            .activate(glib::clone!(
                #[weak]
                timeline,
                move |_: &gtk::ApplicationWindow, _, _| {
                    timeline.set_playhead((timeline.playhead() + delta).max(0));
                }
            ))
            .build()
    };
    let go_to_start = gio::ActionEntry::builder("go-to-start")
        .activate(glib::clone!(
            #[weak]
            timeline,
            move |_: &gtk::ApplicationWindow, _, _| timeline.set_playhead(0)
        ))
        .build();
    let go_to_end = gio::ActionEntry::builder("go-to-end")
        .activate(glib::clone!(
            #[weak]
            timeline,
            #[strong]
            project,
            // The last frame of the sequence.
            move |_: &gtk::ApplicationWindow, _, _| {
                timeline.set_playhead((project.duration_frames() - 1).max(0))
            }
        ))
        .build();
    let timeline_entry = |name: &str, f: fn(&TimelineView)| {
        gio::ActionEntry::builder(name)
            .activate(glib::clone!(
                #[weak]
                timeline,
                move |_: &gtk::ApplicationWindow, _, _| f(&timeline)
            ))
            .build()
    };
    window.add_action_entries([
        seek,
        step("previous-frame", -1),
        step("next-frame", 1),
        go_to_start,
        go_to_end,
        timeline_entry("previous-edit", |t| t.go_to_edit_point(false)),
        timeline_entry("next-edit", |t| t.go_to_edit_point(true)),
        timeline_entry(
            "select-clip-at-playhead",
            TimelineView::select_clip_at_playhead,
        ),
        timeline_entry("add-marker", TimelineView::add_marker_at_playhead),
        timeline_entry("previous-marker", |t| t.go_to_marker(false)),
        timeline_entry("next-marker", |t| t.go_to_marker(true)),
        timeline_entry("delete-marker", TimelineView::delete_selected_marker),
        // Editing clips by keyboard (window.ui has the keys).
        timeline_entry("nudge-left", |t| t.nudge_selection(-1)),
        timeline_entry("nudge-right", |t| t.nudge_selection(1)),
        timeline_entry("nudge-left-second", |t| {
            t.nudge_selection(-(nox_project::nominal_fps(t.project().frame_rate()) as i64))
        }),
        timeline_entry("nudge-right-second", |t| {
            t.nudge_selection(nox_project::nominal_fps(t.project().frame_rate()) as i64)
        }),
        timeline_entry("move-to-track-above", |t| t.move_selection_to_track(true)),
        timeline_entry("move-to-track-below", |t| t.move_selection_to_track(false)),
        timeline_entry("trim-start-to-playhead", |t| {
            t.trim_selection_to_playhead(Edge::Start)
        }),
        timeline_entry("trim-end-to-playhead", |t| {
            t.trim_selection_to_playhead(Edge::End)
        }),
    ]);
}

/// `folder` for people: under the home folder as `~/…`.
fn display_folder(folder: &Path) -> String {
    match folder.strip_prefix(glib::home_dir()) {
        Ok(inside) if inside.as_os_str().is_empty() => "~".to_owned(),
        Ok(inside) => format!("~/{}", inside.display()),
        Err(_) => folder.display().to_string(),
    }
}
