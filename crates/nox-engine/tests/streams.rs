//! Files with several streams through Nox and back: a test5-like MKV (as
//! the Matroska test suite's test5: video, a second audio stream titled
//! "Commentary", subtitles in several languages, one the default) plus an
//! ASS and a PGS stream. Each stream goes on a track of its own, tagged
//! as in the file, and comes out as a stream of its own, with its tags
//! and its subtitles' events.

mod common;

use common::{FPS, f, fixture};
use nox_engine::{EncodeQuality, ExportSettings, VideoCodec, export};
use nox_media::{Subtitles, probe, read_subtitles};
use nox_project::{Clip, Composable, MediaReference, TimeRange, Timeline, TrackKind};
use std::path::{Path, PathBuf};

const SRT_ENG: &str = "1\n00:00:00,500 --> 00:00:01,250\nHello <i>there</i>\n\n\
                       2\n00:00:02,000 --> 00:00:03,500\nTwo\nlines\n";
const SRT_GER: &str = "1\n00:00:00,500 --> 00:00:01,250\nHallo\n";
const ASS: &str = "[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Sign,Arial,30,&H0000FFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,8,10,10,10,1\n\n\
[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:01.00,0:00:02.50,Sign,,0,0,0,,{\\pos(320,40)}A sign\n";

/// The source: 4 s of 320×180 video; 440 Hz audio, then 880 Hz titled
/// Commentary; SRT eng (default), SRT ger, ASS "Signs" (forced), PGS.
fn source() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let write = |name: &str, text: &str| {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let (eng, ger, ass) = (
        write("streams-eng.srt", SRT_ENG),
        write("streams-ger.srt", SRT_GER),
        write("streams.ass", ASS),
    );
    let sup = dir.join("streams.sup");
    common::media::write_pgs(&sup, (320, 180), &[(1.0, 2.0, (100, 140, 40, 10))]);
    let path = |p: &PathBuf| p.to_str().unwrap().to_owned();
    fixture(
        "streams.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=25:duration=4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000:duration=4",
            "-i",
            &path(&eng),
            "-i",
            &path(&ger),
            "-i",
            &path(&ass),
            "-i",
            &path(&sup),
            "-map",
            "0",
            "-map",
            "1",
            "-map",
            "2",
            "-map",
            "3",
            "-map",
            "4",
            "-map",
            "5",
            "-map",
            "6",
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
            "-c:s:0",
            "srt",
            "-c:s:1",
            "srt",
            "-c:s:2",
            "ass",
            "-c:s:3",
            "copy",
            "-metadata:s:a:1",
            "language=eng",
            "-metadata:s:a:1",
            "title=Commentary",
            "-disposition:a:0",
            "default",
            "-disposition:a:1",
            "0",
            "-metadata:s:s:0",
            "language=eng",
            "-metadata:s:s:1",
            "language=ger",
            "-metadata:s:s:2",
            "title=Signs",
            "-disposition:s:0",
            "default",
            "-disposition:s:1",
            "0",
            "-disposition:s:2",
            "forced",
            "-disposition:s:3",
            "0",
        ],
    )
}

/// Every stream of `path` on a track of its own, from frame 0 for all of
/// it, tagged as in the file (as choosing all of them in the stream
/// checklist does).
fn all_streams(path: &Path) -> Timeline {
    let info = probe(path).unwrap();
    let video = info.video.as_ref().unwrap();
    // At the file's rate, as its first clip sets a timeline's.
    let fps = video.frame_rate.unwrap_or(FPS);
    let mut timeline = Timeline::new("t", fps);
    (timeline.settings.width, timeline.settings.height) = (video.width, video.height);
    let duration = info.duration.unwrap();
    let frames = (duration.as_seconds() * fps.as_f64()).ceil() as i64;
    let f = |n| nox_project::Time::from_frames(n, fps);
    let range = TimeRange::new(f(0), f(frames));
    let clip = |stream: usize| {
        Composable::Clip(Clip::new(
            "streams",
            MediaReference::from_path(path, Some(range)).with_stream(stream),
            range,
        ))
    };
    let video = info.video.as_ref().unwrap().stream_index;
    timeline.place(0, clip(video), f(0)).unwrap();
    for (i, audio) in info.audio.iter().enumerate() {
        let track = match i {
            0 => 1,
            _ => timeline.add_track(TrackKind::Audio),
        };
        timeline
            .place(track, clip(audio.stream_index), f(0))
            .unwrap();
        timeline
            .track_mut(track)
            .unwrap()
            .set_stream_tags(&audio.tags);
    }
    for subtitles in &info.subtitles {
        let Composable::Clip(clip) = clip(subtitles.stream_index) else {
            unreachable!()
        };
        timeline
            .add_subtitle_track(&subtitles.label(), &subtitles.tags, vec![(f(0), clip)])
            .unwrap();
    }
    timeline
}

fn settings(name: &str) -> ExportSettings {
    ExportSettings {
        path: Path::new(env!("CARGO_TARGET_TMPDIR")).join(name),
        codec: VideoCodec::H264,
        container: None,
        quality: EncodeQuality::Low,
        hardware: false,
        decode: nox_media::HwDecode::Software,
        encoder: None,
        range: None,
        audio: true,
        audio_per_track: true,
        audio_codec: None,
        subtitles: true,
        encoding: nox_media::OutputEncoding::Sdr,
        peak_nits: 1000.0,
    }
}

/// A stream's text events: times (ms) and their text as captions show it.
fn text_events(path: &Path, stream: usize) -> Vec<(i64, i64, String)> {
    let Subtitles::Text { header, events } = read_subtitles(path, stream).unwrap() else {
        panic!("text subtitles expected in stream {stream}");
    };
    let script = nox_ass::Script::parse(&header);
    events
        .iter()
        .map(|e| {
            let event = nox_ass::Event::from_ffmpeg(&e.ass, e.start, e.end).unwrap();
            let caption = script.caption(&event).unwrap();
            (
                (e.start * 1000.0).round() as i64,
                (e.end * 1000.0).round() as i64,
                format!("{:?}", caption.document),
            )
        })
        .collect()
}

#[test]
fn every_stream_round_trips_through_mkv() {
    let Some(source) = source() else {
        return;
    };
    let timeline = all_streams(&source);
    let summary = export(&timeline, &settings("streams-out.mkv"), |_, _| true).unwrap();
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);

    let (before, after) = (probe(&source).unwrap(), probe(&summary.path).unwrap());
    assert_eq!(after.audio.len(), 2);
    for (a, b) in before.audio.iter().zip(&after.audio) {
        assert_eq!(a.tags, b.tags);
    }
    assert_eq!(after.subtitles.len(), 4);
    for (a, b) in before.subtitles.iter().zip(&after.subtitles) {
        assert_eq!(
            (&a.codec, &a.tags),
            (&b.codec, &b.tags),
            "stream {}",
            a.stream_index
        );
    }
    // Text: the same events, styled and placed the same.
    for (a, b) in before.subtitles.iter().zip(&after.subtitles).take(3) {
        assert_eq!(
            text_events(&source, a.stream_index),
            text_events(&summary.path, b.stream_index)
        );
    }
    // PGS: the same picture at the same time, gone when it ends.
    let (a, b) = (&before.subtitles[3], &after.subtitles[3]);
    let events = |path: &Path, stream| match read_subtitles(path, stream).unwrap() {
        Subtitles::Bitmap { events } => events,
        _ => panic!("bitmaps expected"),
    };
    let (ea, eb) = (
        events(&source, a.stream_index),
        events(&summary.path, b.stream_index),
    );
    assert_eq!(eb.len(), 1);
    assert!(
        (ea[0].0 - eb[0].0).abs() < 0.002 && (ea[0].1 - eb[0].1).abs() < 0.002,
        "{ea:?} {eb:?}"
    );
    let picture = |path: &Path, stream, start| {
        nox_media::BitmapReader::open(path, stream)
            .unwrap()
            .picture(start)
            .unwrap()
            .unwrap()
    };
    assert_eq!(
        picture(&source, a.stream_index, ea[0].0),
        picture(&summary.path, b.stream_index, eb[0].0)
    );

    // The commentary is its own stream: 880 Hz where the first has 440.
    let crossings = |stream: usize| {
        let mut decoder =
            nox_media::AudioDecoder::open_stream(&summary.path, Some(stream), 48_000).unwrap();
        let mut samples = vec![0f32; 48_000 * 2];
        decoder.read(24_000, &mut samples).unwrap();
        samples
            .chunks(2)
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| (w[0][0] < 0.0) != (w[1][0] < 0.0))
            .count()
    };
    let (main, commentary) = (
        crossings(after.audio[0].stream_index),
        crossings(after.audio[1].stream_index),
    );
    assert!((870..=890).contains(&main), "{main}");
    assert!((1750..=1770).contains(&commentary), "{commentary}");
}

/// Edited: the clips moved 1 s later and cut to the file's 0.6–3 s, and
/// exported mixed: one audio stream; subtitle events shifted, those
/// outside left out, those across a cut cut.
#[test]
fn subtitles_follow_the_edits() {
    let Some(source) = source() else {
        return;
    };
    let mut timeline = Timeline::new("t", FPS);
    (timeline.settings.width, timeline.settings.height) = (320, 180);
    let range = TimeRange::new(f(0), f(100));
    let used = TimeRange::new(f(15), f(60));
    let reference = |stream| MediaReference::from_path(&source, Some(range)).with_stream(stream);
    timeline
        .place(
            0,
            Composable::Clip(Clip::new("v", reference(0), used)),
            f(25),
        )
        .unwrap();
    timeline
        .place(
            1,
            Composable::Clip(Clip::new("a", reference(1), used)),
            f(25),
        )
        .unwrap();
    let tags = nox_project::StreamTags {
        language: Some("eng".into()),
        ..Default::default()
    };
    timeline
        .add_subtitle_track(
            "S",
            &tags,
            vec![(f(25), Clip::new("s", reference(3), used))],
        )
        .unwrap();
    let settings = ExportSettings {
        audio_per_track: false,
        ..settings("streams-edited.mkv")
    };
    let summary = export(&timeline, &settings, |_, _| true).unwrap();
    let after = probe(&summary.path).unwrap();
    assert_eq!(after.audio.len(), 1);
    assert_eq!(after.subtitles.len(), 1);
    assert_eq!(after.subtitles[0].tags.language.as_deref(), Some("eng"));
    let times: Vec<(i64, i64)> = text_events(&summary.path, after.subtitles[0].stream_index)
        .into_iter()
        .map(|(start, end, _)| (start, end))
        .collect();
    // 0.5–1.25 s starts before the cut at 0.6 s: 1.0 + 0–0.65 s; 2–3.5 s
    // ends after it at 3 s: 1.0 + 1.4–2.4 s.
    assert_eq!(times, [(1000, 1650), (2400, 3400)]);
}

/// MP4 takes text as `mov_text`, not bitmaps: those are left out, and the
/// summary says so.
#[test]
fn mp4_converts_text_and_leaves_out_bitmaps() {
    let Some(source) = source() else {
        return;
    };
    let timeline = all_streams(&source);
    let summary = export(&timeline, &settings("streams-out.mp4"), |_, _| true).unwrap();
    assert_eq!(summary.skipped.len(), 1, "{:?}", summary.skipped);
    let after = probe(&summary.path).unwrap();
    assert_eq!(after.subtitles.len(), 3);
    assert!(after.subtitles.iter().all(|s| s.codec == "mov_text"));
    assert_eq!(after.subtitles[1].tags.language.as_deref(), Some("ger"));
    let texts: Vec<String> = text_events(&summary.path, after.subtitles[0].stream_index)
        .into_iter()
        .map(|(start, end, _)| format!("{start}-{end}"))
        .collect();
    assert_eq!(texts, ["500-1250", "2000-3500"]);
}

/// The Matroska test suite's test5.mkv (H.264, two AAC streams, eight
/// SRT streams in seven languages), from `NOX_TEST5`: every stream
/// through Nox into an MKV, with the same tags and subtitle events.
/// `NOX_TEST5=…/test5.mkv cargo test -p nox-engine --test streams -- --ignored`
#[test]
#[ignore = "needs NOX_TEST5, the Matroska test suite's test5.mkv"]
fn matroska_test5_round_trips() {
    let source = PathBuf::from(std::env::var_os("NOX_TEST5").expect("NOX_TEST5"));
    let timeline = all_streams(&source);
    let settings = ExportSettings {
        quality: EncodeQuality::Low,
        ..settings("test5-out.mkv")
    };
    let summary = export(&timeline, &settings, |_, _| true).unwrap();
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);
    let (before, after) = (probe(&source).unwrap(), probe(&summary.path).unwrap());
    assert_eq!(after.audio.len(), before.audio.len());
    for (a, b) in before.audio.iter().zip(&after.audio) {
        assert_eq!(a.tags, b.tags);
    }
    assert_eq!(after.subtitles.len(), before.subtitles.len());
    for (a, b) in before.subtitles.iter().zip(&after.subtitles) {
        assert_eq!((&a.codec, &a.tags), (&b.codec, &b.tags));
        let (ea, eb) = (
            text_events(&source, a.stream_index),
            text_events(&summary.path, b.stream_index),
        );
        for (x, y) in ea.iter().zip(&eb) {
            assert_eq!(x, y, "stream {}", a.stream_index);
        }
        assert_eq!(ea.len(), eb.len(), "stream {}", a.stream_index);
    }
}
