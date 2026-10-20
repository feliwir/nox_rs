//! The main window driven as a user drives it: through its actions (which
//! the toolbar's buttons, menus and shortcuts activate) and its controls,
//! checking the project and the widgets after each step. `#[gtk::test]`
//! runs these in turn on one GTK thread. They need a display (CI uses
//! Broadway, `.github/scripts/with-display.sh`); the engine renders on the
//! GPU if there is one (CI: lavapipe), else on the CPU.

use gtk::{gio, glib, prelude::*, subclass::prelude::*};
use nox_project::{
    Clip, Composable, Item, ItemRef, MediaReference, Parameters, Rational, Time, TimeRange,
    Timeline,
};
use std::cell::{Cell, OnceCell};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::timeline::TimelineView;
use crate::window::Window;

const FPS: Rational = Rational::new(25, 1);

fn f(n: i64) -> Time {
    Time::from_frames(n, FPS)
}

/// GTK set up as Nox sets it up, once: translations, the bundled resources
/// and an application for windows to join. Also used by `template_tests`.
pub(crate) fn app() -> adw::Application {
    thread_local! {
        static APP: OnceCell<adw::Application> = const { OnceCell::new() };
    }
    APP.with(|app| {
        app.get_or_init(|| {
            crate::i18n::init();
            gio::resources_register_include!("nox.gresource").expect("bundled resources");
            // Not unique: no D-Bus needed, and Nox itself may be running.
            let app = adw::Application::builder()
                .application_id("io.github.feliwir.Nox.Tests")
                .flags(gio::ApplicationFlags::NON_UNIQUE)
                .resource_base_path(crate::RESOURCES)
                .build();
            app.connect_startup(crate::set_up);
            app.register(None::<&gio::Cancellable>)
                .expect("application registered");
            app
        })
        .clone()
    })
}

/// A directory of this test run's files.
fn dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nox-gui-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("test directory");
    dir
}

/// A clip made by the `ffmpeg` CLI with `args` (tests share them; they run
/// in turn); `None`, after a note, without the CLI.
fn fixture(name: &str, args: &[&str]) -> Option<PathBuf> {
    let path = dir().join(name);
    if !path.exists() {
        let status = std::process::Command::new("ffmpeg")
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args(args)
            .arg(&path)
            .status();
        match status {
            Ok(status) => assert!(status.success(), "ffmpeg made {name}"),
            Err(err) => {
                eprintln!("skipping: no ffmpeg CLI to make test clips ({err})");
                return None;
            }
        }
    }
    Some(path)
}

/// Four seconds (100 frames) of one color.
fn solid(name: &str, color: &str) -> Option<PathBuf> {
    let input = format!("color=c={color}:size=320x180:rate=25:duration=4,format=yuv444p");
    fixture(name, &["-f", "lavfi", "-i", &input, "-c:v", "ffv1"])
}

/// A clip of `path` using frames `start..start + length` of 100.
fn clip(path: &Path, start: i64, length: i64) -> Composable {
    let reference = MediaReference::from_path(path, Some(TimeRange::new(f(0), f(100))));
    Composable::Clip(Clip::new(
        "c",
        reference,
        TimeRange::new(f(start), f(length)),
    ))
}

/// The window, shown, with `timeline` opened as its project (from a file,
/// as `nox edit.otio` opens it).
fn open(timeline: &Timeline) -> Window {
    thread_local! {
        static OPENED: Cell<u32> = const { Cell::new(0) };
    }
    let n = OPENED.with(|n| {
        n.set(n.get() + 1);
        n.get()
    });
    let path = dir().join(format!("project-{n}.otio"));
    nox_otio::write_file(&path, timeline).expect("project written");
    let window = crate::window::build(app().upcast_ref(), &[gio::File::for_path(&path)]);
    window.present();
    window
}

/// Closes `window` without asking about changes, and lets its playback
/// stop.
fn close(window: Window) {
    window.destroy();
    let end = Instant::now() + Duration::from_millis(200);
    while Instant::now() < end {
        glib::MainContext::default().iteration(false);
    }
}

/// Runs GTK's main loop until `done`, at most a minute.
fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let context = glib::MainContext::default();
    let end = Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        if !context.iteration(false) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Activates `window`'s action `name` (e.g. `win.split`), as its button or
/// shortcut does.
fn act(window: &Window, name: &str, parameter: Option<glib::Variant>) {
    WidgetExt::activate_action(window, name, parameter.as_ref())
        .unwrap_or_else(|_| panic!("no action {name}"));
}

/// The first widget of type `T` inside `widget`, depth first.
fn descendant<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Option<T> {
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Ok(found) = current.clone().downcast::<T>() {
            return Some(found);
        }
        if let Some(found) = descendant(&current) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

/// The clips (and transitions) of `track`, as the timeline has them now.
fn track(timeline: &TimelineView, track: usize) -> Vec<Composable> {
    let project = timeline.project();
    let model = project.timeline();
    model
        .track(track)
        .map(|t| t.children.clone())
        .unwrap_or_default()
}

fn clips(timeline: &TimelineView) -> usize {
    track(timeline, 0)
        .iter()
        .filter(|c| matches!(c, Composable::Clip(_)))
        .count()
}

fn transitions(timeline: &TimelineView) -> usize {
    track(timeline, 0)
        .iter()
        .filter(|c| matches!(c, Composable::Transition(_)))
        .count()
}

/// The text a text clip shows.
fn text_of(timeline: &TimelineView, item: ItemRef) -> String {
    let project = timeline.project();
    let model = project.timeline();
    let Ok(Composable::Clip(clip)) = model.item(item) else {
        panic!("no clip at {item:?}");
    };
    let parameters = clip.text_parameters().expect("a text clip");
    nox_text::Document::from_parameters(parameters).text
}

/// Add Text puts a title at the playhead, the inspector edits it (typing
/// in a row is one undo step), and it shows in the clip strip and in the
/// viewer's transform box.
#[gtk::test]
#[cfg_attr(
    target_os = "macos",
    ignore = "GTK must be initialized on the main thread on macOS"
)]
fn add_text_and_type() {
    let window = open(&Timeline::new("t", FPS));
    let imp = window.imp();
    let timeline = imp.timeline.get();
    act(&window, "win.add-text", None);
    let item = timeline.selection().expect("the new text clip selected");
    // "Title", in the user's language.
    let title = text_of(&timeline, item);
    assert!(!title.is_empty());

    let inspector = imp.inspector.get();
    let section = inspector.imp().text_section.get();
    assert!(section.is_visible(), "the inspector's Text section");
    let view: gtk::TextView = descendant(section.upcast_ref()).expect("the text field");
    let buffer = view.buffer();
    let shown = |buffer: &gtk::TextBuffer| {
        let (start, end) = buffer.bounds();
        buffer.text(&start, &end, false).to_string()
    };
    assert_eq!(shown(&buffer), title);
    buffer.insert(&mut buffer.end_iter(), " one");
    buffer.insert(&mut buffer.end_iter(), " two");
    let typed = format!("{title} one two");
    assert_eq!(text_of(&timeline, item), typed);
    act(&window, "win.undo", None);
    assert_eq!(text_of(&timeline, item), title, "one undo step");
    assert_eq!(shown(&buffer), title, "the field follows the undo");
    act(&window, "win.redo", None);
    assert_eq!(text_of(&timeline, item), typed);

    // The Effects page's clip strip shows the text rendered.
    let strip = imp.effects_clip_strip.get();
    let store = strip.imp().store.get();
    let row = (0..store.n_items())
        .filter_map(|i| store.item(i).and_downcast::<crate::clip_strip::ClipRow>())
        .find(|row| row.preview().is_some())
        .expect("a text preview in the clip strip");
    let preview = row.preview().unwrap();
    assert_eq!((preview.width(), preview.height()), (224, 126));

    // The viewer's transform box: wider than tall, around the middle.
    let overlay = imp.viewer.get().transform_overlay();
    // Once the viewer has its size.
    wait_for("the transform box", || {
        overlay
            .box_corners()
            .is_some_and(|c| c[1].0 > c[0].0 && overlay.width() > 0)
    });
    let corners = overlay.box_corners().unwrap();
    let (width, height) = (corners[1].0 - corners[0].0, corners[3].1 - corners[0].1);
    assert!(width > height && height > 0.0, "{corners:?}");
    let middle = (corners[0].0 + corners[1].0) / 2.0;
    assert!(
        (middle - f64::from(overlay.width()) / 2.0).abs() < 2.0,
        "{corners:?} in {}",
        overlay.width()
    );
    close(window);
}

/// Import Subtitles (by path, as the dialog's choice does) with a clip
/// selected: a subtitle track whose clip is timed as the clip is (it
/// shows its file from 0.4 s, from frame 20), linked with it; Convert to
/// Text Clips then makes text clips where the events show, and hides the
/// subtitle track. One undo step each.
#[gtk::test]
#[cfg_attr(
    target_os = "macos",
    ignore = "GTK must be initialized on the main thread on macOS"
)]
fn import_subtitles() {
    let Some(red) = solid("red.mkv", "0xC03020") else {
        return;
    };
    let srt = dir().join("subtitles.srt");
    std::fs::write(
        &srt,
        "1\n00:00:00,500 --> 00:00:01,750\nHello <b>world</b>\n\n\
         2\n00:00:02,000 --> 00:00:03,000\nTwo\nlines\n\n\
         3\n00:00:09,000 --> 00:00:10,000\nAfter the clip\n",
    )
    .unwrap();
    let mut model = Timeline::new("t", FPS);
    model.place(0, clip(&red, 10, 80), f(20)).unwrap();
    let window = open(&model);
    let timeline = window.imp().timeline.get();
    let tracks = || timeline.project().timeline().tracks().count();
    let before = tracks();
    timeline.set_selection(Some(ItemRef { track: 0, index: 1 }));
    act(
        &window,
        "win.import-subtitles-path",
        Some(srt.to_str().unwrap().to_variant()),
    );
    wait_for("a subtitle track", || tracks() > before);
    let index = {
        let project = timeline.project();
        let model = project.timeline();
        let (index, track) = model
            .tracks()
            .find(|(_, t)| t.kind == nox_project::TrackKind::Subtitle)
            .expect("the subtitle track");
        assert!(track.shown());
        let Composable::Clip(subtitles) = &track.children[1] else {
            panic!("a gap, then the clip");
        };
        let Ok(Composable::Clip(video)) = model.item(ItemRef { track: 0, index: 1 }) else {
            panic!("the video clip");
        };
        assert_eq!(subtitles.trimmed_range(), video.trimmed_range());
        assert_eq!(subtitles.stream(), Some(0));
        assert!(video.item.link().is_some());
        assert_eq!(subtitles.item.link(), video.item.link());
        assert_eq!(
            model
                .item_range(ItemRef {
                    track: index,
                    index: 1
                })
                .unwrap(),
            TimeRange::new(f(20), f(80))
        );
        index
    };

    // As text clips, where the events show (cut to the clip).
    act(
        &window,
        "win.convert-to-text-clips",
        Some((index as i64).to_variant()),
    );
    let texts = || {
        let project = timeline.project();
        let model = project.timeline();
        model
            .tracks()
            .flat_map(|(_, t)| t.child_ranges())
            .filter_map(|(range, child)| {
                let Composable::Clip(clip) = child else {
                    return None;
                };
                let text = nox_text::Document::from_parameters(clip.text_parameters()?).text;
                Some((range.start.frame(FPS), range.duration.frame(FPS), text))
            })
            .collect::<Vec<_>>()
    };
    wait_for("text clips", || !texts().is_empty());
    assert_eq!(
        texts(),
        [
            (23, 31, "Hello world".to_owned()),
            (60, 25, "Two\nlines".to_owned()),
        ]
    );
    assert!(
        !timeline
            .project()
            .timeline()
            .track(index)
            .unwrap()
            .item
            .enabled
    );
    act(&window, "win.undo", None);
    assert!(texts().is_empty(), "one undo step");
    act(&window, "win.undo", None);
    assert_eq!(tracks(), before, "one undo step");
    close(window);
}

/// Adding a file with several streams, the ones chosen in the checklist:
/// the commentary on a second audio track tagged as in the file, the
/// subtitles on subtitle tracks, all linked.
#[gtk::test]
#[cfg_attr(
    target_os = "macos",
    ignore = "GTK must be initialized on the main thread on macOS"
)]
fn adds_chosen_streams() {
    let srt = dir().join("streams.srt");
    std::fs::write(&srt, "1\n00:00:00,500 --> 00:00:01,000\nHi\n").unwrap();
    let Some(file) = fixture(
        "streams.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=25:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:duration=2",
            "-i",
            srt.to_str().unwrap(),
            "-map",
            "0",
            "-map",
            "1",
            "-map",
            "2",
            "-map",
            "3",
            "-map",
            "3",
            "-c:v",
            "ffv1",
            "-c:a",
            "flac",
            "-c:s",
            "srt",
            "-metadata:s:a:1",
            "title=Commentary",
            "-metadata:s:s:0",
            "language=eng",
            "-metadata:s:s:1",
            "language=ger",
            "-disposition:s:0",
            "0",
            "-disposition:s:1",
            "default",
        ],
    ) else {
        return;
    };
    let info = nox_media::probe(&file).unwrap();
    assert!(crate::project::Streams::worth_asking(&info));
    let main = crate::project::Streams::main(&info);
    assert_eq!((main.video, &main.audio[..]), (Some(0), &[1][..]));
    let window = open(&Timeline::new("t", FPS));
    let project = window.imp().timeline.get().project();
    let streams = crate::project::Streams {
        video: Some(0),
        audio: vec![1, 2],
        subtitles: vec![3, 4],
    };
    project
        .add_media(
            &info,
            Some(&streams),
            None,
            0,
            false,
            crate::project::Framing::Keep,
        )
        .unwrap();
    let model = project.timeline();
    // Each clip's track kind, stream, link and track title or language.
    type Added = (
        nox_project::TrackKind,
        Option<usize>,
        Option<i64>,
        Option<String>,
    );
    let clips: Vec<Added> = model
        .tracks()
        .flat_map(|(_, t)| {
            t.children.iter().filter_map(move |c| match c {
                Composable::Clip(clip) => Some((
                    t.kind.clone(),
                    clip.stream(),
                    clip.item.link(),
                    t.stream_tags().title.or(t.stream_tags().language),
                )),
                _ => None,
            })
        })
        .collect();
    use nox_project::TrackKind::*;
    let link = clips[0].2;
    assert!(link.is_some());
    assert_eq!(
        clips,
        [
            (Video, Some(0), link, None),
            (Audio, Some(1), link, None),
            (Audio, Some(2), link, Some("Commentary".to_owned())),
            (Subtitle, Some(3), link, Some("eng".to_owned())),
            (Subtitle, Some(4), link, Some("ger".to_owned())),
        ]
    );
    // The viewer shows the file's default subtitles, not the first.
    let shown: Vec<Option<String>> = model
        .tracks()
        .filter(|(_, t)| t.shown())
        .map(|(_, t)| t.stream_tags().language)
        .collect();
    assert_eq!(shown, [Some("ger".to_owned())]);
    drop(model);
    close(window);
}

/// More tracks than fit scroll: the scroll bar shows, rows and their
/// headers move up under the fixed ruler, the pointer finds the rows
/// where they are drawn, and selecting a clip off screen scrolls to it.
#[gtk::test]
#[cfg_attr(
    target_os = "macos",
    ignore = "GTK must be initialized on the main thread on macOS"
)]
fn many_tracks_scroll() {
    let Some(red) = solid("red.mkv", "0xC03020") else {
        return;
    };
    let mut model = Timeline::new("t", FPS);
    let mut last = 0;
    for _ in 0..20 {
        last = model.add_track(nox_project::TrackKind::Video);
        model.place(last, clip(&red, 0, 10), f(0)).unwrap();
    }
    let window = open(&model);
    let timeline = window.imp().timeline.get();
    wait_for("the timeline laid out", || timeline.height() > 0);
    let imp = timeline.imp();
    let bar = imp.vbar.get().unwrap().clone();
    assert!(
        bar.is_visible(),
        "22 rows don't fit in {} px",
        timeline.height()
    );
    // The first row under the ruler, until scrolled.
    let top = imp.track_at_y(25.0 + 1.0);
    bar.adjustment().set_value(44.0 * 3.0);
    let below = imp.track_at_y(25.0 + 1.0);
    assert_ne!(top, below);
    assert_eq!(imp.vscroll.get(), 132.0);
    // Never past the end.
    bar.adjustment().set_value(1e6);
    imp.scroll_rows(1e6);
    let max = imp.vscroll.get();
    assert!(max > 0.0 && max <= 44.0 * 22.0, "{max}");
    // The bottom track (V1, last row) in view, then the top one by
    // selecting a clip on it.
    let rows_top = imp.row_y(0);
    assert!(rows_top < 25.0);
    timeline.set_selection(Some(ItemRef {
        track: last,
        index: 0,
    }));
    assert!(imp.vscroll.get() < max);
    let shown = (0..30)
        .find(|&row| imp.track_at_y(25.0 + 44.0 * f64::from(row) + 1.0) == Some(last))
        .is_some();
    assert!(shown, "the selected clip's track scrolled into view");
    close(window);
}

/// During playback the time pages along with the playhead: off the right
/// edge, it shows near the left edge again; while in view, nothing moves.
#[gtk::test]
#[cfg_attr(
    target_os = "macos",
    ignore = "GTK must be initialized on the main thread on macOS"
)]
fn timeline_follows_the_playhead() {
    let window = open(&Timeline::new("t", FPS));
    let timeline = window.imp().timeline.get();
    wait_for("the timeline laid out", || timeline.width() > 300);
    let imp = timeline.imp();
    imp.pixels_per_frame.set(4.0);
    timeline.set_playhead(10);
    timeline.follow_playhead();
    assert_eq!(imp.scroll.get(), 0.0, "in view: no scrolling");
    // Far past the right edge.
    let past = (f64::from(timeline.width()) / 4.0) as i64 + 100;
    timeline.set_playhead(past);
    timeline.follow_playhead();
    let at = imp.frame_to_x(timeline.playhead() as f64);
    let header = 112.0;
    assert!(
        at > header && at < header + 0.1 * f64::from(timeline.width()),
        "near the left edge: {at}"
    );
    // Playing on within the view: no jump.
    let scroll = imp.scroll.get();
    timeline.set_playhead(past + 20);
    timeline.follow_playhead();
    assert_eq!(imp.scroll.get(), scroll);
    // Back before the view.
    timeline.set_playhead(0);
    timeline.follow_playhead();
    assert_eq!(imp.scroll.get(), 0.0);
    close(window);
}

/// Split, Add Dissolve, Undo and Redo from the toolbar, and frames in the
/// viewer.
#[gtk::test]
#[cfg_attr(
    target_os = "macos",
    ignore = "GTK must be initialized on the main thread on macOS"
)]
fn edits_from_the_toolbar() {
    let Some(red) = solid("red.mkv", "0xC03020") else {
        return;
    };
    let mut model = Timeline::new("t", FPS);
    model.place(0, clip(&red, 0, 50), f(0)).unwrap();
    model.place(0, clip(&red, 50, 50), f(50)).unwrap();
    let window = open(&model);
    let imp = window.imp();
    let timeline = imp.timeline.get();
    let picture = imp.viewer.get().picture();
    wait_for("a frame in the viewer", || picture.paintable().is_some());

    // Split the first clip in the middle.
    act(&window, "win.seek", Some(25i64.to_variant()));
    act(&window, "win.select-clip-at-playhead", None);
    assert_eq!(timeline.selection(), Some(ItemRef { track: 0, index: 0 }));
    act(&window, "win.split", None);
    assert_eq!(clips(&timeline), 3);

    // A dissolve over the cut at 2 s (both clips have media past it).
    act(&window, "win.seek", Some(52i64.to_variant()));
    act(&window, "win.select-clip-at-playhead", None);
    act(&window, "win.add-dissolve", None);
    assert_eq!(transitions(&timeline), 1);

    act(&window, "win.undo", None);
    assert_eq!((clips(&timeline), transitions(&timeline)), (3, 0));
    act(&window, "win.undo", None);
    assert_eq!(clips(&timeline), 2);
    act(&window, "win.redo", None);
    assert_eq!(clips(&timeline), 3);
    close(window);
}

/// Clips by keyboard: nudging, moving to another track and trimming to
/// the playhead, through the actions their keys activate.
#[gtk::test]
#[cfg_attr(
    target_os = "macos",
    ignore = "GTK must be initialized on the main thread on macOS"
)]
fn edits_by_keyboard() {
    let Some(red) = solid("red.mkv", "0xC03020") else {
        return;
    };
    let mut model = Timeline::new("t", FPS);
    model.place(0, clip(&red, 10, 50), f(40)).unwrap();
    model.add_track(nox_project::TrackKind::Video);
    let window = open(&model);
    let timeline = window.imp().timeline.get();
    let range = |item: ItemRef| timeline.project().timeline().item_range(item).unwrap();
    act(&window, "win.seek", Some(45i64.to_variant()));
    act(&window, "win.select-clip-at-playhead", None);
    let first = timeline.selection().expect("the clip");

    act(&window, "win.nudge-right", None);
    act(&window, "win.nudge-right", None);
    let moved = timeline.selection().unwrap();
    assert_eq!(range(moved).start, f(42));
    act(&window, "win.undo", None);
    assert_eq!(
        range(first).start,
        f(40),
        "nudges in a row are one undo step"
    );
    act(&window, "win.redo", None);
    act(&window, "win.nudge-left-second", None);
    assert_eq!(range(timeline.selection().unwrap()).start, f(17));

    // Up a track, then trimmed to the playhead at both ends.
    act(&window, "win.move-to-track-above", None);
    let up = timeline.selection().unwrap();
    assert_ne!(up.track, first.track);
    act(&window, "win.seek", Some(20i64.to_variant()));
    act(&window, "win.trim-start-to-playhead", None);
    act(&window, "win.seek", Some(50i64.to_variant()));
    act(&window, "win.trim-end-to-playhead", None);
    let up = timeline.selection().unwrap();
    assert_eq!((range(up).start, range(up).end()), (f(20), f(50)));
    close(window);
}

/// A graph on the Effects page: a key of a still's blue half masks a blur.
/// The page shows the clip's nodes with a thumbnail each, and the viewer a
/// frame. Needs the GPU (effects run there).
#[gtk::test]
#[cfg_attr(
    target_os = "macos",
    ignore = "GTK must be initialized on the main thread on macOS"
)]
fn effects_page_shows_the_graph() {
    if nox_media::Accel::new().is_err() {
        assert!(
            std::env::var_os("NOX_REQUIRE_GPU").is_none(),
            "no Vulkan GPU, but NOX_REQUIRE_GPU is set"
        );
        eprintln!("skipping: needs a GPU");
        return;
    }
    let Some(photo) = fixture(
        "red-blue.png",
        &[
            "-f",
            "lavfi",
            "-i",
            "color=c=red:size=320x180,drawbox=x=160:y=0:w=160:h=180:color=blue:t=fill,format=rgb24",
            "-frames:v",
            "1",
        ],
    ) else {
        return;
    };
    let mut still = Composable::Clip(Clip::new(
        "p",
        MediaReference::from_path(&photo, None),
        TimeRange::new(f(0), f(50)),
    ));
    let (key, blur) = masked_blur(still.item_mut().unwrap());
    let mut model = Timeline::new("t", FPS);
    model.place(0, still, f(0)).unwrap();
    let window = open(&model);
    let imp = window.imp();

    act(&window, "win.page", Some("effects".to_variant()));
    act(&window, "win.select-clip-at-playhead", None);
    let editor = imp.node_editor.get();
    let nodes = [nox_project::SOURCE, &key, &blur, nox_fx::OUTPUT];
    wait_for("a thumbnail on every node", || {
        let thumbnails = editor.imp().thumbnails.borrow();
        nodes.iter().all(|node| thumbnails.contains_key(*node))
    });
    let picture = imp.viewer.get().picture();
    wait_for("a frame in the viewer", || picture.paintable().is_some());
    let paintable = picture.paintable().unwrap();
    assert!(paintable.intrinsic_width() > 0, "a frame, not an empty one");
    close(window);
}

/// Adds a chroma key on blue and a blur masked by its matte to `item`;
/// returns their keys.
fn masked_blur(item: &mut Item) -> (String, String) {
    let blur = item.add_effect(nox_project::Effect::rendering("nox.gaussian_blur", "Blur"));
    item.effect_mut(&blur)
        .unwrap()
        .set_number_at("radius", 0.0, 5.0);
    let key = item.add_effect(nox_project::Effect::rendering("nox.chroma_key", "Key"));
    item.effect_mut(&key).unwrap().set_param(
        "color",
        Some(nox_project::Value::Array(vec![
            nox_project::Value::Float(0.0),
            nox_project::Value::Float(0.0),
            nox_project::Value::Float(1.0),
        ])),
    );
    nox_fx::materialize(item);
    item.connect(nox_project::SOURCE, "picture", &key, "picture")
        .unwrap();
    item.connect(&key, "matte", &blur, "mask").unwrap();
    item.set_effects_output(Some(nox_project::Connection::new(&blur, "picture")));
    (key, blur)
}

/// Ctrl+? (app.shortcuts) shows libadwaita's shortcuts dialog over the
/// window, and the main menu has its item.
#[cfg(feature = "adw-1-8")]
#[gtk::test]
#[cfg_attr(
    target_os = "macos",
    ignore = "GTK must be initialized on the main thread on macOS"
)]
fn keyboard_shortcuts_dialog() {
    use adw::prelude::*;
    let window = open(&Timeline::new("t", FPS));
    let items = window.imp().standard_items.get();
    let actions: Vec<String> = (0..items.n_items())
        .filter_map(|i| {
            items
                .item_attribute_value(i, "action", None)
                .and_then(|v| v.get::<String>())
        })
        .collect();
    assert_eq!(actions, ["app.preferences", "app.shortcuts", "app.about"]);
    act(&window, "app.shortcuts", None);
    wait_for("the shortcuts dialog", || {
        window
            .visible_dialog()
            .is_some_and(|d| d.is::<adw::ShortcutsDialog>())
    });
    close(window);
}
