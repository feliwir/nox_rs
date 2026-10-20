//! The streams an export writes besides its video: one audio stream (the
//! mix) or one per audio track, and a subtitle stream per subtitle track
//! that isn't burned in, its events taken from the clips' files through
//! the edits.

use nox_media::{BitmapCue, SubtitleContent, SubtitleOutput, Subtitles, TextCue};
use nox_project::{Composable, StreamTags, Time, TimeRange, Timeline, TrackKind};
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

/// The span of sequence frames `range`.
fn span(timeline: &Timeline, range: &Range<i64>) -> TimeRange {
    let fps = timeline.frame_rate();
    TimeRange::new(
        Time::from_frames(range.start, fps),
        Time::from_frames(range.end - range.start, fps),
    )
}

/// The audio tracks with something to hear in `range` (sequence frames),
/// with their tags, in stack order. Without a default among them the
/// first is.
pub(crate) fn audio_tracks(timeline: &Timeline, range: &Range<i64>) -> Vec<(usize, StreamTags)> {
    let mut heard: Vec<usize> = timeline
        .audio_segments(span(timeline, range))
        .iter()
        .map(|s| s.track)
        .collect();
    heard.sort_unstable();
    heard.dedup();
    let mut tracks: Vec<(usize, StreamTags)> = heard
        .into_iter()
        .filter_map(|i| Some((i, timeline.track(i)?.stream_tags())))
        .collect();
    if !tracks.iter().any(|(_, tags)| tags.default)
        && let Some((_, tags)) = tracks.first_mut()
    {
        tags.default = true;
    }
    tracks
}

/// An event of a subtitle clip, placed in the export.
enum Cue {
    Text(TextCue),
    Bitmap(BitmapCue),
}

/// The subtitle streams of `timeline`'s subtitle tracks over `range`
/// (sequence frames), in stack order: the enabled ones that aren't burned
/// in and have events there. Also what was left out, and why.
pub(crate) fn subtitle_outputs(
    timeline: &Timeline,
    range: &Range<i64>,
) -> (Vec<SubtitleOutput>, Vec<String>) {
    let span = span(timeline, range);
    let (from, to) = (span.start.as_seconds(), span.end().as_seconds());
    let mut outputs = Vec::new();
    let mut skipped = Vec::new();
    // Codec names by file and stream.
    let mut codecs: HashMap<(PathBuf, usize), Option<String>> = HashMap::new();
    for (_, track) in timeline.tracks() {
        if track.kind != TrackKind::Subtitle || !track.item.enabled || track.burned_in() {
            continue;
        }
        let name = track.item.name.clone();
        // The track's text header and format (of its first text clip).
        let mut text: Option<(String, Option<String>)> = None;
        let mut cues = Vec::new();
        for (place, child) in track.child_ranges() {
            let Composable::Clip(clip) = child else {
                continue;
            };
            if !clip.item.enabled || !place.overlaps(span) {
                continue;
            }
            let (Some(path), Some(stream)) = (
                clip.media_reference().and_then(|r| timeline.media_path(r)),
                clip.stream(),
            ) else {
                continue;
            };
            let subtitles = match nox_media::subtitles(&path, stream) {
                Ok(subtitles) => subtitles,
                Err(err) => {
                    skipped.push(format!("{name}: {}: {err}", path.display()));
                    continue;
                }
            };
            // An event at file times `start..end` as output seconds, cut
            // to the clip and the export.
            let placed = |start: f64, end: f64| -> Option<(f64, f64)> {
                let (a, b) = clip.place_file_span(place, start, end)?;
                let (a, b) = (a.max(from), b.min(to));
                (a < b).then_some((a - from, b - from))
            };
            match &*subtitles {
                Subtitles::Text { header, events } => {
                    let codec = codecs
                        .entry((path.clone(), stream))
                        .or_insert_with(|| codec_of(&path, stream))
                        .clone();
                    match &mut text {
                        None => text = Some((header.clone(), codec)),
                        // Another format than the first clip's: ASS keeps
                        // both.
                        Some((_, first)) if *first != codec => *first = Some("ass".into()),
                        Some(_) => {}
                    }
                    for event in events {
                        if let Some((start, end)) = placed(event.start, event.end) {
                            cues.push(Cue::Text(TextCue {
                                start,
                                end,
                                ass: event.ass.clone(),
                            }));
                        }
                    }
                }
                Subtitles::Bitmap { events } => {
                    for &(event_start, event_end) in events {
                        if let Some((start, end)) = placed(event_start, event_end) {
                            cues.push(Cue::Bitmap(BitmapCue {
                                start,
                                end,
                                path: path.clone(),
                                stream,
                                source: event_start,
                            }));
                        }
                    }
                }
            }
        }
        let bitmap = cues.iter().any(|c| matches!(c, Cue::Bitmap(_)));
        if bitmap && text.is_some() {
            skipped.push(format!(
                "{name}: text and bitmap subtitles on one track; the text is left out"
            ));
        }
        let content = if bitmap {
            let mut cues: Vec<BitmapCue> = cues
                .into_iter()
                .filter_map(|c| match c {
                    Cue::Bitmap(cue) => Some(cue),
                    Cue::Text(_) => None,
                })
                .collect();
            cues.sort_by(|a, b| a.start.total_cmp(&b.start));
            SubtitleContent::Bitmap { cues }
        } else if let Some((header, codec)) = text {
            let mut cues: Vec<TextCue> = cues
                .into_iter()
                .filter_map(|c| match c {
                    Cue::Text(cue) => Some(cue),
                    Cue::Bitmap(_) => None,
                })
                .collect();
            cues.sort_by(|a, b| a.start.total_cmp(&b.start));
            SubtitleContent::Text {
                codec: codec.unwrap_or_else(|| "ass".into()),
                header,
                cues,
            }
        } else {
            continue;
        };
        outputs.push(SubtitleOutput {
            tags: track.stream_tags(),
            content,
        });
    }
    (outputs, skipped)
}

/// FFmpeg's codec name of subtitle stream `stream` of `path`.
fn codec_of(path: &std::path::Path, stream: usize) -> Option<String> {
    let info = nox_media::probe(path).ok()?;
    info.subtitles
        .into_iter()
        .find(|s| s.stream_index == stream)
        .map(|s| s.codec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nox_project::{Clip, MediaReference, Rational};

    #[test]
    fn audio_tracks_tagged() {
        let fps = Rational::new(25, 1);
        let f = |n| Time::from_frames(n, fps);
        let mut timeline = Timeline::new("t", fps);
        let file = |stream| {
            MediaReference::from_path(
                std::path::Path::new("/m/a.mkv"),
                Some(TimeRange::new(f(0), f(100))),
            )
            .with_stream(stream)
        };
        let a1 = timeline
            .tracks()
            .find(|(_, t)| t.kind == TrackKind::Audio)
            .map(|(i, _)| i)
            .unwrap();
        timeline
            .place(
                a1,
                Composable::Clip(Clip::new("a", file(1), TimeRange::new(f(0), f(50)))),
                f(0),
            )
            .unwrap();
        let a2 = timeline.add_track(TrackKind::Audio);
        timeline
            .track_mut(a2)
            .unwrap()
            .set_stream_tags(&StreamTags {
                language: Some("eng".into()),
                title: Some("Commentary".into()),
                ..StreamTags::default()
            });
        timeline
            .place(
                a2,
                Composable::Clip(Clip::new("b", file(8), TimeRange::new(f(0), f(50)))),
                f(0),
            )
            .unwrap();
        let tracks = audio_tracks(&timeline, &(0..50));
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].0, a1);
        // No default set: the first is.
        assert!(tracks[0].1.default && !tracks[1].1.default);
        assert_eq!(tracks[1].1.title.as_deref(), Some("Commentary"));
        // Nothing heard there.
        assert!(audio_tracks(&timeline, &(60..70)).is_empty());
        // The segments name their streams.
        let segments = timeline.audio_segments(TimeRange::new(f(0), f(10)));
        let mut streams: Vec<_> = segments.iter().map(|s| s.stream).collect();
        streams.sort();
        assert_eq!(streams, [Some(1), Some(8)]);
    }
}
