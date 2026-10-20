//! Adding media: for a file with several video or audio streams, or with
//! subtitles, asks which streams to add; for one whose orientation
//! differs from the timeline's (a portrait clip in a landscape timeline or
//! the other way round), asks how to fit it, as Premiere's clip mismatch
//! warning does.

use gtk::prelude::*;
use nox_media::MediaInfo;
use nox_project::EditError;
use std::rc::Rc;

use crate::alert::Look;
use crate::i18n::{fill, gettext};
use crate::project::{Framing, Project, Streams};

/// Adds `media` as `Project::add_media` does. If its orientation differs
/// from the timeline's and `choice` doesn't say how to fit it, asks first
/// (over `parent`); cancelling adds nothing. `done` gets the result and,
/// if a choice was made, the choice, so a batch can reuse it.
pub fn add_media(
    parent: &impl IsA<gtk::Widget>,
    project: &Rc<Project>,
    media: MediaInfo,
    place: (Option<usize>, i64, bool),
    choice: Option<Framing>,
    done: impl FnOnce(Result<i64, EditError>, Option<Framing>) + 'static,
) {
    if !Streams::worth_asking(&media) {
        add_streams(parent, project, media, None, place, choice, done);
        return;
    }
    let parent_widget = parent.as_ref().clone();
    let project = project.clone();
    ask_streams(parent, &media.clone(), move |streams| {
        add_streams(
            &parent_widget,
            &project,
            media,
            Some(streams),
            place,
            choice,
            done,
        );
    });
}

/// Adds `streams` of `media` (see `add_media`), asking how to fit its
/// picture first if needed.
fn add_streams(
    parent: &impl IsA<gtk::Widget>,
    project: &Rc<Project>,
    media: MediaInfo,
    streams: Option<Streams>,
    (track, frame, insert): (Option<usize>, i64, bool),
    choice: Option<Framing>,
    done: impl FnOnce(Result<i64, EditError>, Option<Framing>) + 'static,
) {
    let with_video = streams.as_ref().is_none_or(|s| s.video.is_some());
    if !with_video || !project.orientation_differs(&media) || choice.is_some() {
        let framing = if project.orientation_differs(&media) {
            choice.unwrap_or_default()
        } else {
            Framing::Keep
        };
        let result = project.add_media(&media, streams.as_ref(), track, frame, insert, framing);
        done(result, choice);
        return;
    }
    let Some(video) = media.video.clone() else {
        return;
    };
    let (width, height) = {
        let timeline = project.timeline();
        (timeline.settings.width, timeline.settings.height)
    };
    let message = if video.height > video.width {
        gettext("This clip is portrait, the timeline is landscape")
    } else {
        gettext("This clip is landscape, the timeline is portrait")
    };
    let detail = fill(
        &gettext(
            "Fit shows all of the picture, small between black bars. Fill zooms \
             it to cover the {width}×{height} frame, cropping the rest; its zoom \
             and position can be changed in the inspector. Or the timeline can \
             change to the clip's {clip_width}×{clip_height}, fitting the other clips.",
        ),
        &[
            ("width", &width),
            ("height", &height),
            ("clip_width", &video.width),
            ("clip_height", &video.height),
        ],
    );
    let project = project.clone();
    crate::alert::ask(
        parent,
        &message,
        &detail,
        &[
            ("cancel", gettext("Cancel"), Look::Plain),
            ("fit", gettext("Fit"), Look::Plain),
            ("fill", gettext("Fill"), Look::Suggested),
            ("timeline", gettext("Change Timeline"), Look::Plain),
        ],
        move |answer| {
            let framing = match answer {
                "fit" => Framing::Keep,
                "fill" => Framing::Fill,
                _ => Framing::MatchTimeline,
            };
            let result = project.add_media(&media, streams.as_ref(), track, frame, insert, framing);
            done(result, Some(framing));
        },
    );
}

/// Asks which streams of `media` to add, with a checkbox for each (the
/// main video and the default audio checked); calls `then` with them
/// unless cancelled.
fn ask_streams(
    parent: &impl IsA<gtk::Widget>,
    media: &MediaInfo,
    then: impl FnOnce(Streams) + 'static,
) {
    use adw::prelude::*;
    let main = Streams::main(media);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let group_of = |title: String| {
        let group = adw::PreferencesGroup::builder().title(title).build();
        content.append(&group);
        group
    };
    // A row with a check button for stream `index`.
    let row = |group: &adw::PreferencesGroup,
               title: &str,
               detail: &str,
               on: bool,
               radio: Option<&gtk::CheckButton>| {
        let check = gtk::CheckButton::new();
        check.set_active(on);
        if let Some(first) = radio {
            check.set_group(Some(first));
        }
        check.set_valign(gtk::Align::Center);
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(detail)
            .activatable_widget(&check)
            .build();
        row.set_title_selectable(false);
        row.add_prefix(&check);
        group.add(&row);
        check
    };
    let stream_text = |index: usize| fill(&gettext("Stream {index}"), &[("index", &index)]);
    let mut videos: Vec<(usize, gtk::CheckButton)> = Vec::new();
    let all_video: Vec<&nox_media::VideoInfo> =
        media.video.iter().chain(&media.other_video).collect();
    if !all_video.is_empty() {
        let group = group_of(gettext("Video"));
        // One video stream at most: they'd cover each other.
        let mut first: Option<gtk::CheckButton> = None;
        for video in all_video {
            let title = format!("{} {}×{}", video.codec, video.width, video.height);
            let check = row(
                &group,
                &title,
                &stream_text(video.stream_index),
                main.video == Some(video.stream_index),
                first
                    .as_ref()
                    .filter(|_| media.video.is_some() && !media.other_video.is_empty()),
            );
            first.get_or_insert(check.clone());
            videos.push((video.stream_index, check));
        }
    }
    let mut audio: Vec<(usize, gtk::CheckButton)> = Vec::new();
    if !media.audio.is_empty() {
        let group = group_of(gettext("Audio"));
        for stream in &media.audio {
            let mut parts: Vec<String> = Vec::new();
            parts.extend(stream.tags.title.clone());
            parts.extend(stream.tags.language.clone());
            let title = match parts.is_empty() {
                true => stream.codec.clone(),
                false => format!("{} ({})", parts.join(", "), stream.codec),
            };
            let detail = fill(
                &gettext("{stream}, {channels} channels, {rate} Hz"),
                &[
                    ("stream", &stream_text(stream.stream_index)),
                    ("channels", &stream.channels),
                    ("rate", &stream.sample_rate),
                ],
            );
            let on = main.audio.contains(&stream.stream_index);
            audio.push((stream.stream_index, row(&group, &title, &detail, on, None)));
        }
    }
    let mut subtitles: Vec<(usize, gtk::CheckButton)> = Vec::new();
    if !media.subtitles.is_empty() {
        let group = group_of(gettext("Subtitles"));
        for stream in &media.subtitles {
            let mut detail = stream_text(stream.stream_index);
            if stream.tags.default {
                detail = fill(&gettext("{stream}, default"), &[("stream", &detail)]);
            }
            if stream.tags.forced {
                detail = fill(&gettext("{stream}, forced"), &[("stream", &detail)]);
            }
            subtitles.push((
                stream.stream_index,
                row(&group, &stream.label(), &detail, false, None),
            ));
        }
    }
    let scroller = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(420)
        .build();
    let name = media
        .path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let dialog = adw::AlertDialog::new(
        Some(&gettext("Which Streams?")),
        Some(&fill(
            &gettext(
                "“{file}” has several streams. The checked ones go on the timeline, each on a track of its own.",
            ),
            &[("file", &name)],
        )),
    );
    dialog.set_extra_child(Some(&scroller));
    dialog.add_response("cancel", &gettext("Cancel"));
    dialog.add_response("add", &gettext("Add"));
    dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("add"));
    dialog.set_close_response("cancel");
    let then = std::cell::Cell::new(Some(then));
    dialog.connect_response(None, move |_, response| {
        if response != "add" {
            return;
        }
        let checked = |rows: &[(usize, gtk::CheckButton)]| -> Vec<usize> {
            rows.iter()
                .filter(|(_, c)| c.is_active())
                .map(|(i, _)| *i)
                .collect()
        };
        let streams = Streams {
            video: checked(&videos).first().copied(),
            audio: checked(&audio),
            subtitles: checked(&subtitles),
        };
        if let Some(then) = then.take() {
            then(streams);
        }
    });
    dialog.present(Some(parent));
}
