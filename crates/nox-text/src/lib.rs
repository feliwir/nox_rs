//! Text for Nox: a document (text, a base style and styled spans), laid
//! out with Parley (shaping by HarfRust, bidi, line breaking), glyph
//! outlines from Skrifa, and rasterized with Vello CPU into a picture the
//! compositor takes as a layer. All in Rust.
//!
//! Every letter (grapheme cluster) stays addressable after layout
//! (`TextLayout::letters`): its outlines are kept as paths, so a letter can
//! be shown or hidden (letter by letter), colored, and moved, scaled,
//! skewed or bent before it is rasterized (`render`, given a `Look` per
//! letter).
//!
//! Sizes are in percent of the picture's height, so text looks the same at
//! every preview and export size; layout happens at the output's size, so
//! it's sharp at each.

mod document;
mod fonts;
mod layout;
mod outline;
mod raster;

pub use document::{Align, Color, Document, Span, Style};
pub use fonts::{DEFAULT_FAMILY, Fonts};
pub use kurbo;
pub use layout::{Letter, PlacedGlyph, TextLayout, layout};
pub use raster::{Look, Picture, render};
