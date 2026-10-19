//! Fonts for drawing text clips in the GUI (the clip strip's previews,
//! the viewer's transform box), shared by the main thread. System families
//! are found when a text first names them.

use std::cell::RefCell;

thread_local! {
    static FONTS: RefCell<nox_text::Fonts> = RefCell::new(nox_text::Fonts::new());
}

/// Runs `f` with the fonts.
pub fn with<R>(f: impl FnOnce(&mut nox_text::Fonts) -> R) -> R {
    FONTS.with_borrow_mut(f)
}

/// Where `document`'s text is in a picture of `frame` size, as the engine
/// draws it: `[x, y, width, height]` in pixels (`None` if it shows
/// nothing).
pub fn placed(document: &nox_text::Document, frame: (u32, u32)) -> Option<[f64; 4]> {
    with(|fonts| {
        let laid = nox_text::layout(fonts, document, frame);
        let ink = laid.ink()? + laid.centered(frame);
        Some([ink.x0, ink.y0, ink.width(), ink.height()])
    })
}
