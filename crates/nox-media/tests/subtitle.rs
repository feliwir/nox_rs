//! Subtitle streams: text ones (SRT and ASS in an MKV made with the
//! `ffmpeg` CLI) read as ASS events, and bitmap ones (a PGS `.sup`
//! written here) as timed pictures.

mod common;

use common::fixture;
use nox_media::{BitmapReader, Subtitles, probe, read_subtitles};
use std::path::{Path, PathBuf};

const SRT: &str = "1
00:00:00,500 --> 00:00:01,750
Hello <i>world</i>

2
00:00:02,000 --> 00:00:03,000
Two
lines
";

const ASS: &str = "[Script Info]
PlayResX: 640
PlayResY: 360

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Sign,Arial,30,&H0000FFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,8,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.00,0:00:02.50,Sign,,0,0,0,,{\\pos(320,40)}A sign
";

fn with_text_streams() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let (srt, ass) = (dir.join("subs.srt"), dir.join("subs.ass"));
    std::fs::write(&srt, SRT).unwrap();
    std::fs::write(&ass, ASS).unwrap();
    fixture(
        "subtitled.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=25:duration=4",
            "-i",
            srt.to_str().unwrap(),
            "-i",
            ass.to_str().unwrap(),
            "-map",
            "0",
            "-map",
            "1",
            "-map",
            "2",
            "-c:v",
            "mpeg4",
            "-c:s:0",
            "srt",
            "-c:s:1",
            "ass",
            "-metadata:s:s:0",
            "language=eng",
            "-metadata:s:s:1",
            "title=Signs",
            "-disposition:s:1",
            "forced",
        ],
    )
}

#[test]
fn reads_text_streams_as_ass() {
    let Some(path) = with_text_streams() else {
        return;
    };
    let info = probe(&path).unwrap();
    assert_eq!(info.subtitles.len(), 2);
    let (srt, signs) = (&info.subtitles[0], &info.subtitles[1]);
    assert_eq!((srt.stream_index, srt.codec.as_str()), (1, "subrip"));
    assert_eq!(srt.tags.language.as_deref(), Some("eng"));
    assert!(!srt.bitmap && !srt.tags.forced);
    assert_eq!(srt.label(), "eng (SRT)");
    assert_eq!(signs.tags.title.as_deref(), Some("Signs"));
    assert!(signs.tags.forced);
    // Read once, then shared.
    let once = nox_media::subtitles(&path, 2).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &once,
        &nox_media::subtitles(&path, 2).unwrap()
    ));

    // SRT: FFmpeg's default script, its tags as ASS's.
    let Subtitles::Text { header, events } = read_subtitles(&path, 1).unwrap() else {
        panic!("text expected");
    };
    let script = nox_ass::Script::parse(&header);
    assert_eq!(events.len(), 2);
    assert!((events[0].start - 0.5).abs() < 1e-3 && (events[0].end - 1.75).abs() < 1e-3);
    let event =
        nox_ass::Event::from_ffmpeg(&events[0].ass, events[0].start, events[0].end).unwrap();
    let caption = script.caption(&event).unwrap();
    assert_eq!(caption.document.text, "Hello world");
    assert_eq!(caption.document.spans[0].italic, Some(true));
    let second = nox_ass::Event::from_ffmpeg(&events[1].ass, 0.0, 0.0).unwrap();
    assert_eq!(script.caption(&second).unwrap().document.text, "Two\nlines");

    // ASS: its own script, styles and tags kept.
    let Subtitles::Text { header, events } = read_subtitles(&path, 2).unwrap() else {
        panic!("text expected");
    };
    let script = nox_ass::Script::parse(&header);
    assert_eq!(script.play_res, (640.0, 360.0));
    assert_eq!(events.len(), 1);
    assert!((events[0].start - 1.0).abs() < 1e-3 && (events[0].end - 2.5).abs() < 1e-3);
    let event = nox_ass::Event::from_ffmpeg(&events[0].ass, 1.0, 2.5).unwrap();
    let doc = script.caption(&event).unwrap().document;
    assert_eq!(doc.text, "A sign");
    assert_eq!(doc.position, [0.5, 40.0 / 360.0]);
    assert_eq!(doc.style.color, [1.0, 1.0, 0.0, 1.0]);

    // Not a subtitle stream.
    assert!(read_subtitles(&path, 0).is_err());
}

#[test]
fn reads_bitmap_subtitles() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("subs.sup");
    common::write_pgs(
        &path,
        (720, 480),
        &[
            (1.0, 3.0, (100, 200, 40, 10)),
            (4.0, 5.5, (300, 400, 20, 5)),
        ],
    );

    let info = probe(&path).unwrap();
    assert_eq!(info.subtitles.len(), 1);
    assert!(info.subtitles[0].bitmap);
    assert_eq!(info.subtitles[0].label(), "(PGS)");
    let Subtitles::Bitmap { events } = read_subtitles(&path, 0).unwrap() else {
        panic!("bitmaps expected");
    };
    let rounded: Vec<(f64, f64)> = events
        .iter()
        .map(|&(s, e)| ((s * 100.0).round() / 100.0, (e * 100.0).round() / 100.0))
        .collect();
    assert_eq!(rounded, [(1.0, 3.0), (4.0, 5.5)]);

    let mut reader = BitmapReader::open(&path, 0).unwrap();
    // Out of order, as a timeline asks.
    for (start, (x, y, w, h)) in [
        (events[1].0, (300, 400, 20, 5)),
        (events[0].0, (100, 200, 40, 10)),
    ] {
        let picture = reader.picture(start).unwrap().unwrap();
        assert_eq!(picture.canvas, (720, 480));
        assert_eq!(
            (picture.x, picture.y, picture.width, picture.height),
            (x, y, w, h)
        );
        assert!(
            picture
                .rgba
                .chunks(4)
                .all(|p| p[0] > 240 && p[1] > 240 && p[3] == 255)
        );
    }
    // Nothing starts there.
    assert!(reader.picture(2.0).unwrap().is_none());
}
