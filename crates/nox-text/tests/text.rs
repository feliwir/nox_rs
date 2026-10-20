//! Text from document to picture: shaping (ligatures, right to left,
//! clusters), styles per span, fonts (TrueType and CFF outlines, variable
//! weights, missing families) and rasterization. Fonts are the bundled one
//! and the test fonts in `tests/fonts`, never the system's.

use nox_text::kurbo::{Affine, Shape, Vec2};
use nox_text::{Document, Fonts, Look, Span, Style, layout, render};

const FRAME: (u32, u32) = (1920, 1080);

fn fonts() -> Fonts {
    let mut fonts = Fonts::bundled_only();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
    for file in ["NotoSansArabic-Variable.ttf", "SourceSans3-Regular.otf"] {
        fonts.add(std::fs::read(dir.join(file)).unwrap());
    }
    fonts
}

fn document(text: &str, family: &str) -> Document {
    Document {
        style: Style {
            family: family.to_owned(),
            ..Style::default()
        },
        ..Document::new(text)
    }
}

#[test]
fn letters_lines_and_words() {
    let mut fonts = fonts();
    let laid = layout(&mut fonts, &Document::new("Hello world"), FRAME);
    assert_eq!(laid.letters.len(), 11);
    assert_eq!(laid.lines, 1);
    assert_eq!(laid.words(), 2);
    let w = &laid.letters[6];
    assert_eq!((&w.range, w.word, w.space), (&(6..7), 1, false));
    assert!(laid.letters[5].space && laid.letters[5].glyphs.is_empty());
    assert!(laid.letters[0].glyphs.len() == 1);
    // In order along the line; 8 % of 1080 is the em size.
    assert!(
        laid.letters
            .windows(2)
            .all(|p| p[0].origin.x < p[1].origin.x)
    );
    // The box is the wrapping width (90 % of the picture), the line
    // centered in it.
    assert_eq!(laid.width, 1728.0);
    let first = laid.letters[0].origin.x;
    let last = laid.letters[10].origin.x + laid.letters[10].advance;
    assert!(
        last - first > 300.0 && last - first < 800.0,
        "{first} {last}"
    );
    assert!(((first + last) / 2.0 - 864.0).abs() < 1.0, "{first} {last}");
    // Centered in the picture.
    let offset = laid.centered(FRAME);
    assert!((offset.x * 2.0 + laid.width - 1920.0).abs() < 1e-6);
    // A line break and wrapping make lines.
    let two = layout(&mut fonts, &Document::new("one\ntwo"), FRAME);
    assert_eq!((two.lines, two.letters[4].line), (2, 1));
    let narrow = Document {
        width: 0.1,
        ..Document::new("many words to wrap here")
    };
    assert!(layout(&mut fonts, &narrow, FRAME).lines >= 4);
}

#[test]
fn clusters_ligatures_and_right_to_left() {
    let mut fonts = fonts();
    // An accent combined with its letter is one letter.
    let accented = layout(&mut fonts, &Document::new("e\u{301}a"), FRAME);
    assert_eq!(accented.letters.len(), 2);
    assert_eq!(accented.letters[0].range, 0..3);
    // A ligature's glyph is the first letter's, the later letters have
    // none and keep their place: Noto Sans joins f and i, Source Sans 3
    // (CFF outlines) two fs.
    for (text, family) in [("fi", "Noto Sans"), ("ff", "Source Sans 3")] {
        let joined = layout(&mut fonts, &document(text, family), FRAME);
        let glyphs: Vec<usize> = joined.letters.iter().map(|l| l.glyphs.len()).collect();
        assert_eq!(glyphs, [1, 0], "{text} in {family}");
        assert!(joined.letters[1].advance > 0.0);
        assert!(!joined.letters[0].glyphs[0].path.elements().is_empty());
    }
    // Arabic runs right to left: the first letter is on the right.
    let arabic = layout(&mut fonts, &document("سلام", "Noto Sans Arabic"), FRAME);
    assert_eq!(arabic.letters.len(), 4);
    let first = &arabic.letters[0];
    let last = &arabic.letters[3];
    assert!(first.origin.x > last.origin.x, "{first:?} {last:?}");
    assert!(arabic.letters.iter().all(|l| !l.glyphs.is_empty()));
}

#[test]
fn spans_style_ranges() {
    let mut fonts = fonts();
    let red = [1.0, 0.0, 0.0, 1.0];
    let doc = Document {
        spans: vec![Span {
            start: 6,
            end: 11,
            color: Some(red),
            size: Some(16.0),
            ..Span::default()
        }],
        ..Document::new("Hello world")
    };
    let laid = layout(&mut fonts, &doc, FRAME);
    assert_eq!(laid.letters[0].glyphs[0].color, [1.0; 4]);
    assert_eq!(laid.letters[6].glyphs[0].color, red);
    // Twice the size: the w twice as wide as in plain text.
    let plain = layout(&mut fonts, &Document::new("Hello world"), FRAME);
    let ratio = laid.letters[6].advance / plain.letters[6].advance;
    assert!((ratio - 2.0).abs() < 0.01, "{ratio}");
}

#[test]
fn weights_and_missing_families() {
    let mut fonts = fonts();
    let ink = |fonts: &mut Fonts, weight: f32| {
        let doc = Document {
            style: Style {
                weight,
                ..Style::default()
            },
            ..Document::new("O")
        };
        layout(fonts, &doc, FRAME).letters[0].glyphs[0]
            .path
            .area()
            .abs()
    };
    // The variable font's bold has more ink than its regular.
    let (regular, bold) = (ink(&mut fonts, 400.0), ink(&mut fonts, 700.0));
    assert!(bold > regular * 1.2, "{regular} {bold}");
    // A missing family is drawn in the bundled one.
    assert!(!fonts.has("No Such Family"));
    let missing = layout(&mut fonts, &document("Hi", "No Such Family"), FRAME);
    assert!(missing.letters.iter().all(|l| !l.glyphs.is_empty()));
    assert!(fonts.families().iter().any(|f| f == "Noto Sans"));
}

#[test]
fn rasterizes_letters_as_they_look() {
    let mut fonts = fonts();
    let laid = layout(&mut fonts, &Document::new("O"), FRAME);
    let offset = laid.centered(FRAME);
    let picture = render(&laid, offset, |_| Look::default()).unwrap();
    // The coverage adds up to the outline's area.
    let coverage: f64 = picture
        .rgba
        .chunks(4)
        .map(|p| f64::from(p[3]) / 255.0)
        .sum();
    let area = laid.letters[0].glyphs[0].path.area().abs();
    assert!((coverage / area - 1.0).abs() < 0.01, "{coverage} {area}");
    // White, straight alpha: partly covered edges are still white.
    assert!(
        picture
            .rgba
            .chunks(4)
            .filter(|p| p[3] > 16)
            .all(|p| p[0] > 240 && p[1] > 240)
    );
    // Where it is in the picture.
    let ink = laid.letters[0].ink() + offset;
    assert!((f64::from(picture.x) - ink.x0).abs() <= 2.0);
    assert!((f64::from(picture.y) - ink.y0).abs() <= 2.0);

    // Per letter: hidden, recolored, moved.
    let laid = layout(&mut fonts, &Document::new("ab"), FRAME);
    let only_first = render(&laid, Vec2::ZERO, |l| Look {
        opacity: if l.range.start == 0 { 1.0 } else { 0.0 },
        ..Look::default()
    })
    .unwrap();
    let both = render(&laid, Vec2::ZERO, |_| Look::default()).unwrap();
    assert!(only_first.width < both.width);
    let green = render(&laid, Vec2::ZERO, |_| Look {
        color: Some([0.0, 1.0, 0.0, 1.0]),
        ..Look::default()
    })
    .unwrap();
    assert!(
        green
            .rgba
            .chunks(4)
            .filter(|p| p[3] > 16)
            .all(|p| p[0] < 16 && p[1] > 240)
    );
    let moved = render(&laid, Vec2::ZERO, |_| Look {
        transform: Affine::translate((0.0, 100.0)),
        ..Look::default()
    })
    .unwrap();
    assert_eq!(moved.y, both.y + 100);
    // Nothing shown, no picture.
    assert!(
        render(&laid, Vec2::ZERO, |_| Look {
            opacity: 0.0,
            ..Look::default()
        })
        .is_none()
    );
}

#[test]
fn borders_shadows_and_placement() {
    let mut fonts = fonts();
    let plain = Document::new("I");
    let laid = layout(&mut fonts, &plain, FRAME);
    let bare = render(&laid, Vec2::ZERO, |_| Look::default()).unwrap();
    // A black border of 0.5 % of 1080 (5.4 px) around white letters:
    // wider by about twice that, black at the edges, white inside.
    let bordered = Document {
        style: Style {
            outline: 0.5,
            ..Style::default()
        },
        ..plain.clone()
    };
    let laid = layout(&mut fonts, &bordered, FRAME);
    let picture = render(&laid, Vec2::ZERO, |_| Look::default()).unwrap();
    let grown = f64::from(picture.width) - f64::from(bare.width);
    assert!((grown - 10.8).abs() <= 2.0, "{grown}");
    // Across the middle: black border, white letter, black border.
    let row = (picture.height / 2) as usize * picture.width as usize * 4;
    let shown: Vec<&[u8]> = picture.rgba[row..row + picture.width as usize * 4]
        .chunks(4)
        .filter(|p| p[3] > 250)
        .collect();
    let (first, last) = (shown[0], shown[shown.len() - 1]);
    assert!(first[0] < 40 && last[0] < 40, "{first:?} {last:?}");
    assert!(shown.iter().any(|p| p[0] > 240));
    // A half-transparent shadow, 1 % of 1080 down and right: the picture
    // grows by it, and the shadow is even where border and letters
    // overlap.
    let shadowed = Document {
        style: Style {
            shadow: 1.0,
            shadow_color: [0.0, 0.0, 0.0, 0.5],
            ..bordered.style.clone()
        },
        ..plain.clone()
    };
    let laid = layout(&mut fonts, &shadowed, FRAME);
    let with_shadow = render(&laid, Vec2::ZERO, |_| Look::default()).unwrap();
    assert_eq!(
        (
            with_shadow.width - picture.width,
            with_shadow.height - picture.height
        ),
        (11, 11)
    );
    // Below the letter's border only the shadow shows.
    let below = with_shadow.width as usize * 4 * (with_shadow.height as usize - 4)
        + (with_shadow.width as usize / 2 + 5) * 4;
    let alpha = with_shadow.rgba[below + 3];
    assert!((120..=135).contains(&alpha), "{alpha}");

    // Placed with the middle of the box's bottom edge at 90 % down.
    let caption = Document {
        position: [0.5, 0.9],
        anchor: [0.5, 1.0],
        ..plain
    };
    let laid = layout(&mut fonts, &caption, FRAME);
    let offset = laid.placed(FRAME);
    assert!((offset.y + laid.height - 972.0).abs() < 1e-3);
    assert!((offset.x * 2.0 + laid.width - 1920.0).abs() < 1e-6);
    // The default is centered.
    let laid = layout(&mut fonts, &Document::new("I"), FRAME);
    assert_eq!(laid.placed(FRAME), laid.centered(FRAME));
}
