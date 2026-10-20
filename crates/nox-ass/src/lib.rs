//! ASS/SSA subtitles (Advanced SubStation Alpha, the format of anime
//! fansubs, MKV soft subtitles and FFmpeg's decoded text subtitles) for
//! Nox, in pure Rust.
//!
//! A script (`Script::parse`: its play size, styles and events, also
//! FFmpeg's decoded subtitle header with its event lines,
//! `Event::from_ffmpeg`) turns into captions (`Script::caption`): a
//! nox-text document per event, styled and placed as libass would show
//! it, which Nox imports as text clips.
//!
//! Shown as libass does: styles, the override tags for fonts, sizes,
//! weights, italics, colors and alpha, borders and shadows, alignment,
//! margins, `\pos`, `\move`'s start, fades, line breaks and wrap styles.
//! Not (yet): animated tags (`\t`, `\move`'s motion), rotation and
//! scaling of the line, clips, drawings (left out), karaoke, underline
//! and strike-through, opaque boxes (`BorderStyle=3`, drawn as a
//! border), blur, and moving lines that would overlap.

mod caption;
mod script;
mod tags;

pub use caption::{Caption, FONT_SIZE_TO_EM};
pub use script::{Event, Script, Style, parse_color, parse_time};
