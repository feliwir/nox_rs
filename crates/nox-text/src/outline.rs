//! Glyph outlines from Skrifa as `kurbo` paths (unhinted: text in video is
//! scaled and moved freely), cached per font, glyph, size and variation.

use kurbo::{BezPath, Point};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::types::F2Dot14;
use skrifa::{FontRef, GlyphId, MetadataProvider};
use std::collections::HashMap;

/// Outlines drawn so far, at the origin, y down.
#[derive(Default)]
pub(crate) struct Outlines {
    cache: HashMap<Key, BezPath>,
}

/// A glyph of a font (its data's address and index in the file) at a size
/// and a variation.
#[derive(Hash, PartialEq, Eq)]
struct Key {
    font: (usize, u32),
    glyph: u32,
    size: u32,
    coords: Vec<i16>,
}

/// A pen drawing into a path, flipping y (fonts are y up).
struct Pen(BezPath);

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(point(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(point(x, y));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to(point(cx0, cy0), point(x, y));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0
            .curve_to(point(cx0, cy0), point(cx1, cy1), point(x, y));
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}

fn point(x: f32, y: f32) -> Point {
    Point::new(f64::from(x), -f64::from(y))
}

impl Outlines {
    /// Glyph `glyph` of the font in `data` (`index` in a collection) at
    /// `size` pixels per em and variation `coords` (normalized, in
    /// 2.14 bits); empty if the font has no outline for it.
    pub fn get(
        &mut self,
        data: &[u8],
        index: u32,
        glyph: u32,
        size: f32,
        coords: &[i16],
    ) -> &BezPath {
        let key = Key {
            font: (data.as_ptr() as usize, index),
            glyph,
            size: size.to_bits(),
            coords: coords.to_vec(),
        };
        self.cache.entry(key).or_insert_with(|| {
            let mut pen = Pen(BezPath::new());
            let Ok(font) = FontRef::from_index(data, index) else {
                return pen.0;
            };
            let coords: Vec<F2Dot14> = coords.iter().map(|&c| F2Dot14::from_bits(c)).collect();
            if let Some(outline) = font.outline_glyphs().get(GlyphId::new(glyph)) {
                let settings = DrawSettings::unhinted(Size::new(size), LocationRef::new(&coords));
                if let Err(err) = outline.draw(settings, &mut pen) {
                    tracing::debug!("glyph {glyph}: {err}");
                }
            }
            pen.0
        })
    }
}
