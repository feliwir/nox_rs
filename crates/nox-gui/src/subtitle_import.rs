//! Importing subtitles onto subtitle tracks, and converting a text
//! subtitle track into styled text clips.
//!
//! An imported stream (of the selected clip's file, or of a subtitle file
//! for it) becomes a subtitle clip timed as that clip is: the same trim,
//! the same speed, linked with it. Without a clip it shows from the
//! sequence's start.

use nox_media::Subtitles;
use nox_project::{
    Clip, Composable, EffectKind, MediaReference, Rational, Time, TimeRange, Timeline,
};
use std::path::Path;

/// Reads stream `stream` of `path` (a pass over the file: off the main
/// thread), so it's there for the timeline and the engine. Returns when
/// its last event ends (file seconds), `None` if it has none.
pub fn read(path: &Path, stream: usize) -> nox_media::Result<Option<f64>> {
    let subtitles = nox_media::subtitles(path, stream)?;
    Ok(match &*subtitles {
        Subtitles::Text { events, .. } => events.iter().map(|e| e.end).reduce(f64::max),
        Subtitles::Bitmap { events } => events.iter().map(|e| e.1).reduce(f64::max),
    })
}

/// A subtitle clip of stream `stream` of `path` timed as `like` (a clip
/// of the video they belong to): its media time, trim and speed.
pub fn clip_like(like: &Clip, path: &Path, stream: usize, name: &str) -> Clip {
    let available = like.available_range();
    let mut clip = Clip::new(
        name,
        MediaReference::from_path(path, available).with_stream(stream),
        like.trimmed_range()
            .unwrap_or_else(|| TimeRange::new(Time::zero(Rational::new(1, 1000)), like.duration())),
    );
    // Its time effects, so the events keep up with the picture.
    clip.item.effects = like
        .item
        .effects
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EffectKind::LinearTimeWarp { .. } | EffectKind::FreezeFrame
            )
        })
        .cloned()
        .collect();
    clip
}

/// A subtitle clip of stream `stream` of `path` showing it from the start
/// of the file until `end` (seconds), whole frames at `fps`.
pub fn clip_from_start(path: &Path, stream: usize, name: &str, end: f64, fps: Rational) -> Clip {
    let frames = (end * fps.as_f64()).ceil().max(1.0) as i64;
    Clip::new(
        name,
        MediaReference::from_path(path, None).with_stream(stream),
        TimeRange::new(Time::zero(fps.inverse()), Time::from_frames(frames, fps)),
    )
}

/// A clip name from a caption's text: its first line, shortened.
fn name_of(text: &str) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default()
        .trim();
    match line.char_indices().nth(40) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_owned(),
    }
}

/// A subtitle clip's place, file and stream: what converting it needs.
pub struct ToConvert {
    pub clip: Clip,
    pub place: TimeRange,
    pub path: std::path::PathBuf,
    pub stream: usize,
}

/// The subtitle clips of track `index` of `timeline` to convert.
pub fn to_convert(timeline: &Timeline, index: usize) -> Vec<ToConvert> {
    let Some(track) = timeline.track(index) else {
        return Vec::new();
    };
    track
        .child_ranges()
        .filter_map(|(place, child)| {
            let Composable::Clip(clip) = child else {
                return None;
            };
            Some(ToConvert {
                path: timeline.media_path(clip.media_reference()?)?,
                stream: clip.stream()?,
                clip: clip.clone(),
                place,
            })
        })
        .collect()
}

/// Text clips showing the text events of `clips` where they show on the
/// timeline, styled and placed as their scripts say, at `fps`: what
/// Convert to Text Clips puts on caption tracks. Reads the streams (off
/// the main thread). Bitmap streams have no text: `None` then.
pub fn text_clips(
    clips: &[ToConvert],
    fps: Rational,
) -> nox_media::Result<Option<Vec<(Time, Clip)>>> {
    let mut out = Vec::new();
    for convert in clips {
        let Subtitles::Text { header, events } =
            &*nox_media::subtitles(&convert.path, convert.stream)?
        else {
            return Ok(None);
        };
        let script = nox_ass::Script::parse(header);
        for event in events {
            let Some((start, end)) =
                convert
                    .clip
                    .place_file_span(convert.place, event.start, event.end)
            else {
                continue;
            };
            let caption = nox_ass::Event::from_ffmpeg(&event.ass, event.start, event.end)
                .and_then(|e| script.caption(&e));
            let Some(caption) = caption else {
                continue;
            };
            let first = (start * fps.as_f64()).round() as i64;
            let last = ((end * fps.as_f64()).round() as i64).max(first + 1);
            let mut clip = Clip::new(
                name_of(&caption.document.text),
                MediaReference::text(caption.document.to_parameters()),
                TimeRange::new(
                    Time::zero(fps.inverse()),
                    Time::from_frames(last - first, fps),
                ),
            );
            clip.item.set_fade_in(caption.fade_in);
            clip.item.set_fade_out(caption.fade_out);
            out.push((Time::from_frames(first, fps), clip));
        }
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FPS: Rational = Rational::new(25, 1);

    fn f(n: i64) -> Time {
        Time::from_frames(n, FPS)
    }

    #[test]
    fn timed_like_the_clip() {
        let mut video = Clip::new(
            "movie",
            MediaReference::from_path(
                Path::new("/m/movie.mkv"),
                Some(TimeRange::new(f(10), f(500))),
            ),
            TimeRange::new(f(60), f(100)),
        );
        video.item.effects.push(nox_project::Effect {
            name: "LinearTimeWarp".into(),
            metadata: nox_project::Object::new(),
            effect_name: "LinearTimeWarp".into(),
            enabled: true,
            kind: EffectKind::LinearTimeWarp { time_scalar: 2.0 },
            extra: nox_project::Object::new(),
        });
        let clip = clip_like(&video, Path::new("/m/movie.srt"), 0, "Subtitles");
        assert_eq!(clip.trimmed_range(), video.trimmed_range());
        assert_eq!(clip.available_range(), video.available_range());
        assert_eq!(clip.item.speed(), 2.0);
        assert_eq!(clip.stream(), Some(0));
        // The clip shows the file from second 2 (frame 60, after the
        // file's start at frame 10), at double speed: file second 4 is 1 s
        // into the clip, at 1 + 1 s on the timeline.
        let place = TimeRange::new(f(25), f(100));
        assert_eq!(clip.place_file_span(place, 4.0, 5.0), Some((2.0, 2.5)));
        // From the start: whole frames to the last event's end.
        let start = clip_from_start(Path::new("/m/a.srt"), 0, "S", 2.01, FPS);
        assert_eq!(start.duration(), f(51));
    }

    #[test]
    fn names() {
        assert_eq!(name_of("\n Hello\nworld"), "Hello");
        assert_eq!(name_of(&"x".repeat(50)), format!("{}…", "x".repeat(40)));
    }
}
