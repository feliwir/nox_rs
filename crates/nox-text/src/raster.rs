//! Rasterizing a layout with Vello CPU: each letter's outlines, moved and
//! colored as its `Look` says, into a picture just large enough for them.

use kurbo::{Affine, Rect, Shape, Vec2};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::{Pixmap, RasterizerSettings, RenderContext, RenderMode, Resources};

use crate::document::Color;
use crate::layout::{Letter, TextLayout};

/// How a letter is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Applied to its outlines (layout box pixels) before placing them.
    pub transform: Affine,
    /// Times its color's alpha (0: hidden).
    pub opacity: f32,
    /// Instead of its style's color.
    pub color: Option<Color>,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            transform: Affine::IDENTITY,
            opacity: 1.0,
            color: None,
        }
    }
}

/// Rasterized text: straight-alpha sRGB RGBA8, `width` × `height`, whose
/// top left is at (`x`, `y`) in the picture it was laid out for.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The largest side of a picture (Vello's limit, and far beyond any
/// output).
const MAX_SIDE: f64 = 16384.0;

/// Rasterizes `layout`, its box moved by `offset` (e.g.
/// `TextLayout::centered`), each letter as `look` says; `None` if nothing
/// shows.
pub fn render(
    layout: &TextLayout,
    offset: Vec2,
    look: impl Fn(&Letter) -> Look,
) -> Option<Picture> {
    let place = Affine::translate(offset);
    let drawn: Vec<(&Letter, Look, Affine)> = layout
        .letters
        .iter()
        .map(|letter| {
            let look = look(letter);
            (letter, look, place * look.transform)
        })
        .filter(|(letter, look, _)| look.opacity > 0.0 && !letter.glyphs.is_empty())
        .collect();
    let bounds = drawn
        .iter()
        .flat_map(|(letter, _, transform)| {
            letter
                .glyphs
                .iter()
                .map(move |g| transform.transform_rect_bbox(g.path.bounding_box()))
        })
        .reduce(|a, b| a.union(b))?;
    // A pixel more on each side for anti-aliased edges.
    let bounds = Rect::new(
        bounds.x0.floor() - 1.0,
        bounds.y0.floor() - 1.0,
        bounds.x1.ceil() + 1.0,
        bounds.y1.ceil() + 1.0,
    );
    let (width, height) = (bounds.width().min(MAX_SIDE), bounds.height().min(MAX_SIDE));
    if width < 1.0 || height < 1.0 {
        return None;
    }
    let (width, height) = (width as u16, height as u16);
    let mut context = RenderContext::new(width, height);
    let origin = Affine::translate((-bounds.x0, -bounds.y0));
    for (letter, look, transform) in &drawn {
        context.set_transform(origin * *transform);
        for glyph in &letter.glyphs {
            let [r, g, b, a] = look.color.unwrap_or(glyph.color);
            context.set_paint(AlphaColor::<Srgb>::new([
                r,
                g,
                b,
                a * look.opacity.clamp(0.0, 1.0),
            ]));
            context.fill_path(&glyph.path);
        }
    }
    let mut pixmap = Pixmap::new(width, height);
    let mut resources = Resources::new();
    context.flush();
    context.render_with(
        &mut pixmap,
        &mut resources,
        RasterizerSettings {
            render_mode: RenderMode::OptimizeQuality,
            ..RasterizerSettings::default()
        },
    );
    // Premultiplied to straight alpha.
    let mut rgba = pixmap.data_as_u8_slice().to_vec();
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let a = u32::from(pixel[3]);
        if a > 0 && a < 255 {
            for c in &mut pixel[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Some(Picture {
        x: bounds.x0 as i32,
        y: bounds.y0 as i32,
        width: u32::from(width),
        height: u32::from(height),
        rgba,
    })
}
