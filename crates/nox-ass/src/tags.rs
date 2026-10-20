//! An event's text: override blocks (`{\b1\c&H0000FF&}`) and escapes
//! (`\N`, `\n`, `\h`) read into runs of plain text, each with the look
//! the tags before it give, and what holds for the whole line
//! (alignment, position, fade).

use nox_text::Color;

use crate::script::{Script, Style, from_ssa_alignment, parse_color, weight};

/// How a run of text looks.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct State {
    pub font: String,
    /// Script pixels, the font's cell height.
    pub size: f64,
    pub weight: f32,
    pub italic: bool,
    pub primary: Color,
    pub outline_color: Color,
    pub back: Color,
    pub outline: f64,
    pub shadow: f64,
    /// Percent.
    pub scale_y: f64,
    /// Script pixels.
    pub spacing: f64,
    /// `\p`: drawing commands, not text.
    pub drawing: bool,
}

impl State {
    pub fn of(style: &Style) -> Self {
        Self {
            font: style.font.clone(),
            size: style.size,
            weight: style.weight,
            italic: style.italic,
            primary: style.primary,
            outline_color: style.outline_color,
            back: style.back,
            outline: style.outline,
            shadow: style.shadow,
            scale_y: style.scale_y,
            spacing: style.spacing,
            drawing: false,
        }
    }
}

/// An event's text, read.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Line {
    /// Plain text (line breaks as `\n`) and how it looks.
    pub runs: Vec<(String, State)>,
    /// The first `\an` or `\a` (numpad).
    pub alignment: Option<u8>,
    /// The first `\pos`, or `\move`'s start, in script pixels.
    pub pos: Option<(f64, f64)>,
    /// `\fad`/`\fade`: seconds fading in and out.
    pub fade: Option<(f64, f64)>,
    /// The script's, or `\q`'s.
    pub wrap_style: u8,
}

/// Reads `text` of an event in `style`.
pub(crate) fn parse(text: &str, style: &Style, script: &Script) -> Line {
    let mut line = Line::default();
    let mut state = State::of(style);
    let mut wrap_style = script.wrap_style;
    let mut run = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(block) = rest.strip_prefix('{')
            && let Some(end) = block.find('}')
        {
            let before = state.clone();
            for tag in split_tags(&block[..end]) {
                apply(tag, &mut state, &mut line, &mut wrap_style, style, script);
            }
            if state != before && !run.is_empty() {
                line.runs.push((std::mem::take(&mut run), before));
            }
            rest = &block[end + 1..];
            continue;
        }
        let mut chars = rest.chars();
        let c = chars.next().unwrap_or_default();
        let (text, after) = match (c, chars.next()) {
            ('\\', Some('N')) => ("\n", &rest[2..]),
            ('\\', Some('n')) if wrap_style == 2 => ("\n", &rest[2..]),
            ('\\', Some('n')) => (" ", &rest[2..]),
            ('\\', Some('h')) => ("\u{a0}", &rest[2..]),
            _ => (&rest[..c.len_utf8()], &rest[c.len_utf8()..]),
        };
        if !state.drawing {
            run.push_str(text);
        }
        rest = after;
    }
    if !run.is_empty() {
        line.runs.push((run, state));
    }
    line.wrap_style = wrap_style;
    line
}

/// The tags of an override block: each from its `\` to the next one
/// outside parentheses (`\t(\fs20)` is one tag).
fn split_tags(block: &str) -> Vec<&str> {
    let mut tags = Vec::new();
    let mut depth = 0;
    let mut start = None;
    for (i, c) in block.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            '\\' if depth == 0 => {
                if let Some(s) = start {
                    tags.push(&block[s..i]);
                }
                start = Some(i + 1);
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        tags.push(&block[s..]);
    }
    tags
}

/// Tag names, longer before their prefixes (`fsp` before `fs`).
const NAMES: &[&str] = &[
    "xbord", "ybord", "xshad", "yshad", "alpha", "iclip", "frx", "fry", "frz", "fax", "fay",
    "fscx", "fscy", "fsp", "fade", "fad", "move", "bord", "shad", "blur", "clip", "pos", "org",
    "pbo", "1c", "2c", "3c", "4c", "1a", "2a", "3a", "4a", "an", "be", "fn", "fs", "fe", "fr",
    "kf", "ko", "a", "b", "c", "i", "k", "K", "p", "q", "r", "s", "t", "u",
];

/// The leading number of `arg`, if there is one.
fn number(arg: &str) -> Option<f64> {
    let arg = arg.trim();
    let end = arg
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && (c == '-' || c == '+'))))
        .map_or(arg.len(), |(i, _)| i);
    arg[..end].parse().ok()
}

/// The arguments of `(a, b, …)`.
fn arguments(arg: &str) -> Vec<f64> {
    arg.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .filter_map(number)
        .collect()
}

/// An alpha as ASS writes it (`&H80&`, 00 opaque) as 0–1 opacity.
fn alpha(arg: &str) -> Option<f32> {
    let hex = arg
        .trim()
        .trim_start_matches(['&', 'H', 'h'])
        .trim_end_matches('&');
    let value = u32::from_str_radix(hex, 16).ok()? & 0xff;
    Some(1.0 - value as f32 / 255.0)
}

/// `color`'s red, green and blue from `arg`, its alpha kept.
fn recolor(color: &mut Color, arg: &str) {
    if let Some([r, g, b, _]) = parse_color(arg) {
        *color = [r, g, b, color[3]];
    }
}

fn apply(
    tag: &str,
    state: &mut State,
    line: &mut Line,
    wrap_style: &mut u8,
    style: &Style,
    script: &Script,
) {
    let tag = tag.trim();
    let Some(name) = NAMES.iter().find(|n| tag.starts_with(**n)) else {
        return;
    };
    let arg = &tag[name.len()..];
    // A tag without a value goes back to the style's.
    let empty = arg.trim().is_empty();
    match *name {
        "b" => state.weight = number(arg).map_or(style.weight, |b| weight(b as i32)),
        "i" => state.italic = number(arg).map_or(style.italic, |i| i != 0.0),
        "fn" => {
            state.font = if empty {
                style.font.clone()
            } else {
                arg.trim().trim_start_matches('@').to_owned()
            }
        }
        "fs" => {
            state.size = match (arg.trim().chars().next(), number(arg)) {
                // `\fs+2`: relative (as VSFilter).
                (Some('+' | '-'), Some(by)) => (state.size + by).max(1.0),
                (_, Some(size)) if size > 0.0 => size,
                _ => style.size,
            }
        }
        "fscy" => state.scale_y = number(arg).unwrap_or(style.scale_y).max(0.0),
        "fsp" => state.spacing = number(arg).unwrap_or(style.spacing),
        "c" | "1c" => {
            if empty {
                state.primary = style.primary;
            } else {
                recolor(&mut state.primary, arg);
            }
        }
        "3c" => {
            if empty {
                state.outline_color = style.outline_color;
            } else {
                recolor(&mut state.outline_color, arg);
            }
        }
        "4c" => {
            if empty {
                state.back = style.back;
            } else {
                recolor(&mut state.back, arg);
            }
        }
        "alpha" => {
            for (color, of_style) in [
                (&mut state.primary, style.primary),
                (&mut state.outline_color, style.outline_color),
                (&mut state.back, style.back),
            ] {
                color[3] = alpha(arg).unwrap_or(of_style[3]);
            }
        }
        "1a" => state.primary[3] = alpha(arg).unwrap_or(style.primary[3]),
        "3a" => state.outline_color[3] = alpha(arg).unwrap_or(style.outline_color[3]),
        "4a" => state.back[3] = alpha(arg).unwrap_or(style.back[3]),
        "bord" => state.outline = number(arg).unwrap_or(style.outline).max(0.0),
        "shad" => state.shadow = number(arg).unwrap_or(style.shadow).max(0.0),
        "an" => {
            if line.alignment.is_none()
                && let Some(a) = number(arg).filter(|a| (1.0..=9.0).contains(a))
            {
                line.alignment = Some(a as u8);
            }
        }
        "a" => {
            if line.alignment.is_none()
                && let Some(a) = number(arg).filter(|a| (1.0..=11.0).contains(a))
            {
                line.alignment = Some(from_ssa_alignment(a as u8));
            }
        }
        "pos" | "move" => {
            if line.pos.is_none()
                && let [x, y, ..] = arguments(arg)[..]
            {
                line.pos = Some((x, y));
            }
        }
        "fad" => {
            if let [fade_in, fade_out] = arguments(arg)[..] {
                line.fade = Some((fade_in / 1000.0, fade_out / 1000.0));
            }
        }
        "fade" => {
            // `\fade(a1, a2, a3, t1, t2, t3, t4)`: as a fade in over t1–t2
            // and out over t3–t4.
            if let [_, _, _, t1, t2, t3, t4] = arguments(arg)[..] {
                line.fade = Some(((t2 - t1).max(0.0) / 1000.0, (t4 - t3).max(0.0) / 1000.0));
            }
        }
        "p" => state.drawing = number(arg).is_some_and(|p| p > 0.0),
        "q" => {
            if let Some(q) = number(arg).filter(|q| (0.0..=3.0).contains(q)) {
                *wrap_style = q as u8;
            }
        }
        "r" => {
            let drawing = state.drawing;
            *state = if empty {
                State::of(style)
            } else {
                State::of(&script.style(arg.trim()))
            };
            state.drawing = drawing;
        }
        // Not shown (yet): see the crate's documentation.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Line {
        parse(text, &Style::default(), &Script::default())
    }

    #[test]
    fn runs_and_escapes() {
        let line = read("Hello{\\i1}world{\\i0}\\Nnext\\nsoft\\hspace");
        let texts: Vec<&str> = line.runs.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(texts, ["Hello", "world", "\nnext soft\u{a0}space"]);
        assert!(line.runs[1].1.italic && !line.runs[2].1.italic);
        // Wrap style 2: `\n` breaks too; `\q` per line.
        let line = read("{\\q2}a\\nb");
        assert_eq!(line.runs[0].0, "a\nb");
        // Comments and unknown tags change nothing; blocks between
        // runs of the same look don't split them.
        let line = read("a{comment}b{\\frz30\\blur2}c");
        assert_eq!(line.runs.len(), 1);
        assert_eq!(line.runs[0].0, "abc");
    }

    #[test]
    fn tags() {
        let line = read(
            "{\\an8\\pos(100,200)\\fad(250,500)\\b1\\fnSource Sans 3\\fs40\\c&H0000FF&\\3c&HFF0000&\\alpha&H80&\\bord3\\shad0.5}x",
        );
        assert_eq!(line.alignment, Some(8));
        assert_eq!(line.pos, Some((100.0, 200.0)));
        assert_eq!(line.fade, Some((0.25, 0.5)));
        let state = &line.runs[0].1;
        assert_eq!(state.font, "Source Sans 3");
        assert_eq!((state.size, state.weight), (40.0, 700.0));
        let half = 1.0 - 128.0 / 255.0;
        assert_eq!(state.primary, [1.0, 0.0, 0.0, half]);
        assert_eq!(state.outline_color, [0.0, 0.0, 1.0, half]);
        assert_eq!((state.outline, state.shadow), (3.0, 0.5));
        // Only the first alignment and position count; `\move` starts at
        // its first point; a nested `\t` is one tag.
        let line = read("{\\move(1,2,3,4)\\an1\\an9\\t(0,500,\\fs80)}x");
        assert_eq!((line.alignment, line.pos), (Some(1), Some((1.0, 2.0))));
        assert_eq!(line.runs[0].1.size, Style::default().size);
        // Relative sizes, resets, legacy alignment.
        let line = read("{\\a6\\fs+2}a{\\r}b");
        assert_eq!(line.alignment, Some(8));
        assert_eq!(line.runs[0].1.size, 20.0);
        assert_eq!(line.runs[1].1, State::of(&Style::default()));
        // Drawings are left out.
        let line = read("{\\p1}m 0 0 l 10 0 10 10{\\p0}text");
        assert_eq!(line.runs.len(), 1);
        assert_eq!(line.runs[0].0, "text");
    }
}
