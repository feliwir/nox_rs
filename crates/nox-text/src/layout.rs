//! A document laid out at a picture size: its letters (grapheme clusters,
//! in the text's order), each with its glyphs' outlines placed in the
//! layout box (pixels, y down), its line and word.

use kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use parley::{
    Alignment, AlignmentOptions, FontFamily, FontStyle, FontWeight, Layout, LineHeight,
    PositionedLayoutItem, StyleProperty,
};
use std::ops::Range;

use crate::document::{Align, Color, Document};
use crate::fonts::{DEFAULT_FAMILY, Fonts};

/// A glyph of a letter: its outline where it is in the layout box, and
/// its color.
#[derive(Debug, Clone)]
pub struct PlacedGlyph {
    pub path: BezPath,
    pub color: Color,
}

/// A letter (grapheme cluster) as laid out.
#[derive(Debug, Clone)]
pub struct Letter {
    /// Its bytes in the text.
    pub range: Range<usize>,
    /// Which line (0-based) and word (0-based, counting from the first
    /// letter that isn't a space) it is in.
    pub line: usize,
    pub word: usize,
    /// Whether it is white space (shows nothing).
    pub space: bool,
    /// Its glyphs; none for spaces and for the later letters of a
    /// ligature (whose glyph is the first letter's).
    pub glyphs: Vec<PlacedGlyph>,
    /// Where it starts on its baseline, and its advance.
    pub origin: Point,
    pub advance: f64,
    /// The box it takes in its line (advance by line height): the
    /// center letters are scaled and turned about.
    pub cell: Rect,
}

/// A laid out document.
#[derive(Debug, Clone, Default)]
pub struct TextLayout {
    /// In the text's order.
    pub letters: Vec<Letter>,
    /// The layout box (pixels).
    pub width: f64,
    pub height: f64,
    pub lines: usize,
}

impl TextLayout {
    /// The offset placing the layout box at the center of a picture of
    /// `frame` size.
    pub fn centered(&self, frame: (u32, u32)) -> Vec2 {
        Vec2::new(
            (f64::from(frame.0) - self.width) / 2.0,
            (f64::from(frame.1) - self.height) / 2.0,
        )
    }

    /// How many words it has.
    pub fn words(&self) -> usize {
        self.letters
            .iter()
            .filter(|l| !l.space)
            .map(|l| l.word + 1)
            .max()
            .unwrap_or(0)
    }
}

/// Lays `document` out for a picture of `frame` size (sizes are percent
/// of its height, wrapping a fraction of its width).
pub fn layout(fonts: &mut Fonts, document: &Document, frame: (u32, u32)) -> TextLayout {
    let px = |percent: f32| percent / 100.0 * frame.1 as f32;
    // Families that can't be found are drawn in the bundled one.
    let families: Vec<(String, String)> = document
        .families()
        .into_iter()
        .map(|family| {
            let used = if fonts.has(family) {
                family
            } else {
                DEFAULT_FAMILY
            };
            (family.to_owned(), used.to_owned())
        })
        .collect();
    let family = |name: &str| {
        families
            .iter()
            .find(|(asked, _)| asked == name)
            .map_or(DEFAULT_FAMILY.to_owned(), |(_, used)| used.clone())
    };
    let text = document.text.as_str();
    let style = &document.style;
    let base_family = family(&style.family);
    let size = px(style.size).max(1.0);
    let Fonts {
        font_cx,
        layout_cx,
        outlines,
        ..
    } = fonts;
    let mut builder = layout_cx.ranged_builder(font_cx, text, 1.0, true);
    builder.push_default(StyleProperty::FontFamily(FontFamily::named(&base_family)));
    builder.push_default(StyleProperty::FontSize(size));
    builder.push_default(StyleProperty::FontWeight(FontWeight::new(style.weight)));
    builder.push_default(StyleProperty::FontStyle(if style.italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    }));
    builder.push_default(StyleProperty::Brush(style.color));
    builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
        document.line_spacing,
    )));
    // Spaced letters don't join (as in CSS), so only when spaced.
    if document.tracking != 0.0 {
        builder.push_default(StyleProperty::LetterSpacing(document.tracking * size));
    }
    let span_families: Vec<Option<String>> = document
        .spans
        .iter()
        .map(|s| s.family.as_deref().map(family))
        .collect();
    for (span, span_family) in document.spans.iter().zip(&span_families) {
        let Some(range) = char_range(text, span.start, span.end) else {
            continue;
        };
        if let Some(family) = span_family {
            builder.push(
                StyleProperty::FontFamily(FontFamily::named(family)),
                range.clone(),
            );
        }
        if let Some(size) = span.size {
            builder.push(StyleProperty::FontSize(px(size).max(1.0)), range.clone());
        }
        if let Some(weight) = span.weight {
            builder.push(
                StyleProperty::FontWeight(FontWeight::new(weight)),
                range.clone(),
            );
        }
        if let Some(italic) = span.italic {
            let style = if italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            };
            builder.push(StyleProperty::FontStyle(style), range.clone());
        }
        if let Some(color) = span.color {
            builder.push(StyleProperty::Brush(color), range);
        }
    }
    let mut layout: Layout<Color> = builder.build(text);
    let wrap = (document.width > 0.0).then_some(document.width * frame.0 as f32);
    layout.break_all_lines(wrap);
    let alignment = match document.align {
        Align::Start => Alignment::Start,
        Align::Center => Alignment::Center,
        Align::End => Alignment::End,
        Align::Justify => Alignment::Justify,
    };
    layout.align(alignment, AlignmentOptions::default());

    let mut letters = Vec::new();
    for (line_index, line) in layout.lines().enumerate() {
        let metrics = line.metrics();
        let (top, bottom) = (
            f64::from(metrics.block_min_coord),
            f64::from(metrics.block_max_coord),
        );
        let mut seen_runs = Vec::new();
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            // A run's clusters, all at once from its first glyph run.
            if seen_runs.contains(&run.index()) {
                continue;
            }
            seen_runs.push(run.index());
            let font = run.font();
            let data = font.data.as_ref();
            let font_size = run.font_size();
            let coords: Vec<i16> = run
                .normalized_coords()
                .iter()
                .map(|c| c.to_bits())
                .collect();
            // Fake italic where the font has none.
            let skew = run.synthesis().skew().map_or(Affine::IDENTITY, |degrees| {
                Affine::skew(-f64::from(degrees).to_radians().tan(), 0.0)
            });
            let baseline = f64::from(glyph_run.baseline());
            let mut x = f64::from(glyph_run.offset());
            for cluster in run.visual_clusters() {
                let advance = f64::from(cluster.advance());
                let color = cluster.style().brush;
                let mut pen = x;
                let glyphs = cluster
                    .glyphs()
                    .map(|glyph| {
                        let outline = outlines.get(data, font.index, glyph.id, font_size, &coords);
                        let at = Affine::translate((
                            pen + f64::from(glyph.x),
                            baseline - f64::from(glyph.y),
                        ));
                        pen += f64::from(glyph.advance);
                        PlacedGlyph {
                            path: (at * skew) * outline.clone(),
                            color,
                        }
                    })
                    .filter(|g| !g.path.elements().is_empty())
                    .collect();
                letters.push(Letter {
                    range: cluster.text_range(),
                    line: line_index,
                    word: 0,
                    space: cluster.is_space_or_nbsp()
                        || text[cluster.text_range()].chars().all(char::is_whitespace),
                    glyphs,
                    origin: Point::new(x, baseline),
                    advance,
                    cell: Rect::new(x, top, x + advance, bottom),
                });
                x += advance;
            }
        }
    }
    // The text's order, and words counted along it.
    letters.sort_by_key(|l| l.range.start);
    let mut word = 0;
    let mut in_word = false;
    for letter in &mut letters {
        if letter.space {
            if in_word {
                word += 1;
            }
            in_word = false;
            letter.word = word;
        } else {
            in_word = true;
            letter.word = word;
        }
    }
    TextLayout {
        lines: layout.len(),
        // Lines are aligned within the wrapping width, if there is one.
        width: f64::from(wrap.unwrap_or(layout.width())),
        height: f64::from(layout.height()),
        letters,
    }
}

/// `start..end` within `text` on character boundaries, if not empty.
fn char_range(text: &str, start: usize, end: usize) -> Option<Range<usize>> {
    let end = end.min(text.len());
    let start = start.min(end);
    (start < end && text.is_char_boundary(start) && text.is_char_boundary(end))
        .then_some(start..end)
}

impl Letter {
    /// The box its glyphs cover (its cell's if it has none).
    pub fn ink(&self) -> Rect {
        self.glyphs
            .iter()
            .map(|g| g.path.bounding_box())
            .reduce(|a, b| a.union(b))
            .unwrap_or(self.cell)
    }
}
