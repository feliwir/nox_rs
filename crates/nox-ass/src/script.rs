//! A script's sections: its info (play size, wrap style), styles and
//! events, read leniently as players do (unknown sections, fields and
//! broken lines are skipped).

use nox_text::Color;

/// How a part of the script looks unless override tags say otherwise
/// (a `Style:` line of `[V4+ Styles]`, or SSA's `[V4 Styles]`).
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub name: String,
    pub font: String,
    /// The font's height in script pixels (`Caption`: its cell, not its
    /// em).
    pub size: f64,
    /// Straight-alpha sRGB, 0–1.
    pub primary: Color,
    pub secondary: Color,
    pub outline_color: Color,
    /// The shadow's color.
    pub back: Color,
    /// 100–900.
    pub weight: f32,
    pub italic: bool,
    pub underline: bool,
    pub strike_out: bool,
    /// Percent.
    pub scale_x: f64,
    pub scale_y: f64,
    /// Extra space between letters, script pixels.
    pub spacing: f64,
    /// Degrees.
    pub angle: f64,
    /// 1: border and shadow; 3: an opaque box.
    pub border_style: u8,
    /// Script pixels.
    pub outline: f64,
    pub shadow: f64,
    /// As on a numeric keypad: 1–3 bottom, 4–6 middle, 7–9 top; left,
    /// center, right.
    pub alignment: u8,
    pub margin_l: f64,
    pub margin_r: f64,
    pub margin_v: f64,
}

impl Default for Style {
    /// libass's default style.
    fn default() -> Self {
        Self {
            name: "Default".into(),
            font: "Arial".into(),
            size: 18.0,
            primary: [1.0, 1.0, 1.0, 1.0],
            secondary: [1.0, 0.0, 0.0, 1.0],
            outline_color: [0.0, 0.0, 0.0, 1.0],
            back: [0.0, 0.0, 0.0, 1.0],
            weight: 700.0,
            italic: false,
            underline: false,
            strike_out: false,
            scale_x: 100.0,
            scale_y: 100.0,
            spacing: 0.0,
            angle: 0.0,
            border_style: 1,
            outline: 2.0,
            shadow: 2.0,
            alignment: 2,
            margin_l: 20.0,
            margin_r: 20.0,
            margin_v: 20.0,
        }
    }
}

/// A subtitle line (`Dialogue:`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Event {
    pub layer: i32,
    /// When it shows, in seconds.
    pub start: f64,
    pub end: f64,
    pub style: String,
    /// The speaker.
    pub name: String,
    /// Instead of the style's, where not 0.
    pub margin_l: f64,
    pub margin_r: f64,
    pub margin_v: f64,
    pub effect: String,
    /// With its override tags (`{\i1}`) and escapes (`\N`).
    pub text: String,
}

/// A parsed script.
#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    /// The coordinate space of positions, margins and sizes.
    pub play_res: (f64, f64),
    /// 0: smart wrapping, top line wider; 1: at the end of a line; 2: no
    /// wrapping (`\n` breaks too); 3: smart, bottom line wider.
    pub wrap_style: u8,
    /// Whether borders and shadows scale with the picture (else they are
    /// in video pixels).
    pub scaled_border_and_shadow: bool,
    pub styles: Vec<Style>,
    pub events: Vec<Event>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            play_res: (384.0, 288.0),
            wrap_style: 0,
            scaled_border_and_shadow: true,
            styles: Vec::new(),
            events: Vec::new(),
        }
    }
}

#[derive(PartialEq)]
enum Section {
    Info,
    Styles,
    Events,
    Other,
}

/// The default `Format:` of `[Events]` (ASS).
const EVENT_FORMAT: &str =
    "Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text";
/// The default `Format:` of `[V4+ Styles]`.
const STYLE_FORMAT: &str = "Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, \
     OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, \
     Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding";

fn format_of(line: &str) -> Vec<String> {
    line.split(',')
        .map(|f| f.trim().to_ascii_lowercase())
        .collect()
}

impl Script {
    /// Reads a script (a `.ass`/`.ssa` file, or FFmpeg's subtitle header).
    pub fn parse(text: &str) -> Self {
        let mut script = Self::default();
        let mut section = Section::Other;
        let mut play_res = (None, None);
        let mut style_format = format_of(STYLE_FORMAT);
        let mut event_format = format_of(EVENT_FORMAT);
        // SSA (`[V4 Styles]`) aligns as 1–3 bottom, +4 top, +8 middle.
        let mut ssa = false;
        for line in text.lines() {
            let line = line.trim_start_matches('\u{feff}').trim();
            if line.starts_with('[') && line.ends_with(']') {
                let name = line[1..line.len() - 1].to_ascii_lowercase();
                section = match name.as_str() {
                    "script info" => Section::Info,
                    "v4+ styles" | "v4 styles+" => Section::Styles,
                    "v4 styles" => {
                        ssa = true;
                        Section::Styles
                    }
                    "events" => Section::Events,
                    _ => Section::Other,
                };
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim_start();
            match section {
                Section::Info => match key.trim().to_ascii_lowercase().as_str() {
                    "playresx" => play_res.0 = value.trim().parse::<f64>().ok(),
                    "playresy" => play_res.1 = value.trim().parse::<f64>().ok(),
                    "wrapstyle" => script.wrap_style = value.trim().parse().unwrap_or(0),
                    "scaledborderandshadow" => {
                        script.scaled_border_and_shadow = value.trim().eq_ignore_ascii_case("yes")
                    }
                    _ => {}
                },
                Section::Styles => match key {
                    "Format" => style_format = format_of(value),
                    "Style" => script.styles.push(parse_style(&style_format, value, ssa)),
                    _ => {}
                },
                Section::Events => match key {
                    "Format" => event_format = format_of(value),
                    "Dialogue" => {
                        if let Some(event) = parse_event(&event_format, value) {
                            script.events.push(event);
                        }
                    }
                    _ => {}
                },
                Section::Other => {}
            }
        }
        // As libass: a missing side follows the other at 4:3, and with
        // neither the play size is 384×288.
        script.play_res = match play_res {
            (Some(x), Some(y)) if x > 0.0 && y > 0.0 => (x, y),
            (Some(x), _) if x > 0.0 => (x, if x == 1280.0 { 1024.0 } else { x * 3.0 / 4.0 }),
            (_, Some(y)) if y > 0.0 => (if y == 1024.0 { 1280.0 } else { y * 4.0 / 3.0 }, y),
            _ => (384.0, 288.0),
        };
        script
    }

    /// The style named `name` (a leading `*` ignored, as VSFilter does),
    /// else `Default`, else the first, else libass's default.
    pub fn style(&self, name: &str) -> Style {
        let name = name.trim_start_matches('*');
        self.styles
            .iter()
            .find(|s| s.name == name)
            .or_else(|| {
                self.styles
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case("Default"))
            })
            .or_else(|| self.styles.first())
            .cloned()
            .unwrap_or_default()
    }
}

impl Event {
    /// An event as FFmpeg decodes it (`AVSubtitleRect::ass`): `ReadOrder,
    /// Layer, Style, Name, MarginL, MarginR, MarginV, Effect, Text`, its
    /// times given apart.
    pub fn from_ffmpeg(line: &str, start: f64, end: f64) -> Option<Self> {
        let format =
            format_of("ReadOrder, Layer, Style, Name, MarginL, MarginR, MarginV, Effect, Text");
        let mut event = parse_event(&format, line)?;
        event.start = start;
        event.end = end;
        Some(event)
    }
}

/// `fields` split as `format` names them; the last takes the rest (text
/// has commas).
fn fields<'a>(format: &'a [String], value: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> {
    format
        .iter()
        .map(String::as_str)
        .zip(value.splitn(format.len(), ','))
}

fn parse_event(format: &[String], value: &str) -> Option<Event> {
    if !format.iter().any(|f| f == "text") {
        return None;
    }
    let mut event = Event::default();
    let mut has_text = false;
    for (field, value) in fields(format, value) {
        let trimmed = value.trim();
        match field {
            "layer" => event.layer = trimmed.parse().unwrap_or(0),
            "start" => event.start = parse_time(trimmed)?,
            "end" => event.end = parse_time(trimmed)?,
            "style" => event.style = trimmed.to_owned(),
            "name" | "actor" => event.name = trimmed.to_owned(),
            "marginl" => event.margin_l = trimmed.parse().unwrap_or(0.0),
            "marginr" => event.margin_r = trimmed.parse().unwrap_or(0.0),
            "marginv" => event.margin_v = trimmed.parse().unwrap_or(0.0),
            "effect" => event.effect = trimmed.to_owned(),
            "text" => {
                event.text = value.to_owned();
                has_text = true;
            }
            _ => {}
        }
    }
    has_text.then_some(event)
}

fn parse_style(format: &[String], value: &str, ssa: bool) -> Style {
    let mut style = Style::default();
    let flag = |v: &str| v.parse::<i32>().is_ok_and(|v| v != 0);
    for (field, value) in fields(format, value) {
        let v = value.trim();
        let number = || v.parse::<f64>().ok();
        match field {
            "name" => style.name = v.trim_start_matches('*').to_owned(),
            "fontname" => style.font = v.trim_start_matches('@').to_owned(),
            "fontsize" => style.size = number().unwrap_or(style.size),
            "primarycolour" => style.primary = parse_color(v).unwrap_or(style.primary),
            "secondarycolour" => style.secondary = parse_color(v).unwrap_or(style.secondary),
            // SSA's `TertiaryColour` is the outline's.
            "outlinecolour" | "tertiarycolour" => {
                style.outline_color = parse_color(v).unwrap_or(style.outline_color)
            }
            "backcolour" => style.back = parse_color(v).unwrap_or(style.back),
            "bold" => style.weight = weight(v.parse().unwrap_or(0)),
            "italic" => style.italic = flag(v),
            "underline" => style.underline = flag(v),
            "strikeout" => style.strike_out = flag(v),
            "scalex" => style.scale_x = number().unwrap_or(100.0),
            "scaley" => style.scale_y = number().unwrap_or(100.0),
            "spacing" => style.spacing = number().unwrap_or(0.0),
            "angle" => style.angle = number().unwrap_or(0.0),
            "borderstyle" => style.border_style = v.parse().unwrap_or(1),
            "outline" => style.outline = number().unwrap_or(0.0).max(0.0),
            "shadow" => style.shadow = number().unwrap_or(0.0).max(0.0),
            "alignment" => {
                let a: u8 = v.parse().unwrap_or(2);
                style.alignment = if ssa { from_ssa_alignment(a) } else { a }
            }
            "marginl" => style.margin_l = number().unwrap_or(0.0),
            "marginr" => style.margin_r = number().unwrap_or(0.0),
            "marginv" => style.margin_v = number().unwrap_or(0.0),
            _ => {}
        }
    }
    if !(1..=9).contains(&style.alignment) {
        style.alignment = 2;
    }
    style
}

/// A weight from ASS's `Bold` (and `\b`): -1 or 1 bold, 0 regular, else
/// a weight.
pub(crate) fn weight(bold: i32) -> f32 {
    match bold {
        0 => 400.0,
        -1 | 1 => 700.0,
        w => (w as f32).clamp(100.0, 900.0),
    }
}

/// SSA's (and `\a`'s) alignment (1–3, +4 top, +8 middle) as numpad.
pub(crate) fn from_ssa_alignment(a: u8) -> u8 {
    let column = (a & 3).clamp(1, 3);
    match a & 12 {
        4 => column + 6,
        8 => column + 3,
        _ => column,
    }
}

/// A color as ASS writes it: `&HAABBGGRR` or `&HBBGGRR&` (alpha 00 is
/// opaque), or a decimal number (SSA).
pub fn parse_color(value: &str) -> Option<Color> {
    let v = value.trim().trim_end_matches('&');
    let raw = match v.strip_prefix("&H").or_else(|| v.strip_prefix("&h")) {
        Some(hex) => u32::from_str_radix(hex.trim_start_matches('&'), 16).ok()?,
        None => match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => v.parse::<i64>().ok()? as u32,
        },
    };
    let channel = |shift: u32| ((raw >> shift) & 0xff) as f32 / 255.0;
    Some([channel(0), channel(8), channel(16), 1.0 - channel(24)])
}

/// `h:mm:ss.cc` as seconds.
pub fn parse_time(value: &str) -> Option<f64> {
    let mut seconds = 0.0;
    for part in value.trim().split(':') {
        seconds = seconds * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    Some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "\u{feff}[Script Info]
; A comment
Title: Test
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 2
ScaledBorderAndShadow: no

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Source Sans 3,72,&H00FFFFFF,&H000000FF,&H80000000,&H00102030,-1,0,0,0,100,100,0,0,1,3,2,2,40,40,50,1
Style: Sign,Arial,40,&H0000FFFF,&H000000FF,&H00000000,&H00000000,0,-1,0,0,100,100,0,0,1,0,0,8,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Comment: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,Not shown
Dialogue: 0,0:00:01.50,0:00:04.25,Default,Ann,0,0,0,,Hello, {\\i1}world{\\i0}!
Dialogue: 1,1:02:03.04,1:02:05.00,*Sign,,0,0,80,,Sign text
";

    #[test]
    fn reads_scripts() {
        let script = Script::parse(SCRIPT);
        assert_eq!(script.play_res, (1920.0, 1080.0));
        assert_eq!(script.wrap_style, 2);
        assert!(!script.scaled_border_and_shadow);
        assert_eq!(script.styles.len(), 2);
        let default = &script.styles[0];
        assert_eq!(default.font, "Source Sans 3");
        assert_eq!(default.size, 72.0);
        assert_eq!(default.weight, 700.0);
        assert_eq!(default.outline_color, [0.0, 0.0, 0.0, 1.0 - 128.0 / 255.0]);
        assert_eq!(
            default.back,
            [
                0x30 as f32 / 255.0,
                0x20 as f32 / 255.0,
                0x10 as f32 / 255.0,
                1.0
            ]
        );
        assert_eq!(
            (default.outline, default.shadow, default.alignment),
            (3.0, 2.0, 2)
        );
        assert_eq!(script.styles[1].primary, [1.0, 1.0, 0.0, 1.0]);
        assert!(script.styles[1].italic);
        // Comments aren't events; text keeps its commas.
        assert_eq!(script.events.len(), 2);
        let hello = &script.events[0];
        assert_eq!((hello.start, hello.end), (1.5, 4.25));
        assert_eq!(hello.name, "Ann");
        assert_eq!(hello.text, "Hello, {\\i1}world{\\i0}!");
        let sign = &script.events[1];
        assert!((sign.start - 3723.04).abs() < 1e-9);
        assert_eq!((sign.layer, sign.margin_v), (1, 80.0));
        assert_eq!(script.style(&sign.style).name, "Sign");
        assert_eq!(script.style("Missing").name, "Default");
    }

    #[test]
    fn reads_ffmpeg_events_and_ssa() {
        let event = Event::from_ffmpeg("3,0,Default,,0,0,0,,Line, with comma", 1.0, 2.0).unwrap();
        assert_eq!(event.text, "Line, with comma");
        assert_eq!(
            (event.start, event.end, event.style.as_str()),
            (1.0, 2.0, "Default")
        );
        // SSA: top is +4, middle +8; colors in decimal; no play size: 4:3.
        let ssa = Script::parse(
            "[Script Info]\nPlayResY: 480\n[V4 Styles]\n\
             Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding\n\
             Style: Top,Arial,20,65535,255,0,0,0,0,1,2,0,6,10,10,10,0,0\n",
        );
        assert_eq!(ssa.play_res, (640.0, 480.0));
        assert_eq!(ssa.styles[0].alignment, 8);
        assert_eq!(ssa.styles[0].primary, [1.0, 1.0, 0.0, 1.0]);
        assert_eq!(Script::parse("").play_res, (384.0, 288.0));
    }

    #[test]
    fn colors_and_times() {
        assert_eq!(parse_color("&H0000FF&"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_color("&HFF00FF00"), Some([0.0, 1.0, 0.0, 0.0]));
        assert_eq!(parse_color("&hff0000"), Some([0.0, 0.0, 1.0, 1.0]));
        assert_eq!(parse_color("nonsense"), None);
        assert_eq!(parse_time("0:00:01.25"), Some(1.25));
        assert_eq!(parse_time("x"), None);
    }
}
