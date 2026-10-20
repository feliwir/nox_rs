//! What a text is: its characters, a base style, and spans overriding the
//! style for ranges of it (a word in another color, font or size), and
//! how a text clip stores it (`to_parameters`).

use nox_project::{Object, Value};

/// Straight-alpha sRGB, 0–1.
pub type Color = [f32; 4];

/// How text looks where no span says otherwise.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// A font family name (`Fonts::families`).
    pub family: String,
    /// Percent of the picture's height (the font's em size).
    pub size: f32,
    /// 100 (thin) to 900 (black); 400 is regular, 700 bold.
    pub weight: f32,
    pub italic: bool,
    pub color: Color,
    /// A border drawn around the letters, its width in percent of the
    /// picture's height (0: none).
    pub outline: f32,
    pub outline_color: Color,
    /// A copy of the letters (and their border) drawn behind them, moved
    /// right and down by this much, in percent of the picture's height
    /// (0: none).
    pub shadow: f32,
    pub shadow_color: Color,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            family: crate::DEFAULT_FAMILY.to_owned(),
            size: 8.0,
            weight: 400.0,
            italic: false,
            color: [1.0; 4],
            outline: 0.0,
            outline_color: [0.0, 0.0, 0.0, 1.0],
            shadow: 0.0,
            shadow_color: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

/// A part of the text (`start..end`, byte offsets on character
/// boundaries) styled differently; `None` keeps the base style's value.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub family: Option<String>,
    pub size: Option<f32>,
    pub weight: Option<f32>,
    pub italic: Option<bool>,
    pub color: Option<Color>,
}

/// How lines line up with each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Left for left-to-right text, right for right-to-left.
    Start,
    #[default]
    Center,
    End,
    Justify,
}

/// A text and how it looks.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub text: String,
    pub style: Style,
    pub spans: Vec<Span>,
    pub align: Align,
    /// Line height as a multiple of the font size.
    pub line_spacing: f32,
    /// Extra space between letters, as a fraction of the font size.
    pub tracking: f32,
    /// Where lines wrap, as a fraction of the picture's width (0: only at
    /// line breaks in the text).
    pub width: f32,
    /// Where in the picture the text goes (fractions of its width and
    /// height, from the top left) ...
    pub position: [f32; 2],
    /// ... with which point of its layout box (fractions of the box: 0
    /// left or top, 1 right or bottom). Captions sit with the middle of
    /// their bottom edge a little above the picture's bottom.
    pub anchor: [f32; 2],
}

impl Default for Document {
    fn default() -> Self {
        Self {
            text: String::new(),
            style: Style::default(),
            spans: Vec::new(),
            align: Align::Center,
            line_spacing: 1.2,
            tracking: 0.0,
            width: 0.9,
            position: [0.5; 2],
            anchor: [0.5; 2],
        }
    }
}

impl Document {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            ..Self::default()
        }
    }

    /// Replaces the text with `text`, keeping spans on what they styled:
    /// after the change they move with it, inside it they end where it
    /// does (an edit is the part between the common start and end).
    pub fn replace_text(&mut self, text: &str) {
        let old = self.text.as_str();
        let prefix = old
            .char_indices()
            .zip(text.chars())
            .find(|((_, a), b)| a != b)
            .map_or(old.len().min(text.len()), |((i, _), _)| i);
        let suffix = old[prefix..]
            .chars()
            .rev()
            .zip(text[prefix..].chars().rev())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a.len_utf8())
            .sum::<usize>();
        let (old_end, new_end) = (old.len() - suffix, text.len() - suffix);
        let moved = |at: usize| {
            if at <= prefix {
                at
            } else if at >= old_end {
                at - old_end + new_end
            } else {
                new_end
            }
        };
        for span in &mut self.spans {
            span.start = moved(span.start);
            span.end = moved(span.end);
        }
        self.spans.retain(|s| s.start < s.end);
        self.text = text.to_owned();
    }

    /// The font families it uses.
    pub fn families(&self) -> Vec<&str> {
        let mut families = vec![self.style.family.as_str()];
        for span in &self.spans {
            if let Some(family) = &span.family
                && !families.contains(&family.as_str())
            {
                families.push(family);
            }
        }
        families
    }
}

/// A color as stored: `[r, g, b, a]`.
fn color_value(color: Color) -> Value {
    Value::Array(color.iter().map(|&c| Value::from(f64::from(c))).collect())
}

fn color_of(value: Option<&Value>) -> Option<Color> {
    let list = value?.as_array()?;
    let channel = |i: usize| list.get(i).and_then(Value::as_f64).map(|c| c as f32);
    Some([
        channel(0)?,
        channel(1)?,
        channel(2)?,
        channel(3).unwrap_or(1.0),
    ])
}

fn number(object: &Object, key: &str) -> Option<f32> {
    object.get(key)?.as_f64().map(|v| v as f32)
}

/// A point as stored: `[x, y]`.
fn point_value(point: [f32; 2]) -> Value {
    Value::Array(point.iter().map(|&c| Value::from(f64::from(c))).collect())
}

fn point_of(value: Option<&Value>) -> Option<[f32; 2]> {
    match value?.as_array()? {
        [x, y] => Some([x.as_f64()? as f32, y.as_f64()? as f32]),
        _ => None,
    }
}

impl Align {
    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
            Self::Justify => "justify",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        [Self::Start, Self::Center, Self::End, Self::Justify]
            .into_iter()
            .find(|a| a.as_str() == name)
    }
}

impl Document {
    /// The document a text clip's parameters describe (what isn't there
    /// takes the defaults).
    pub fn from_parameters(parameters: &Object) -> Self {
        let defaults = Self::default();
        let style = parameters
            .get("style")
            .and_then(Value::as_object)
            .map(|s| Style {
                family: s
                    .get("family")
                    .and_then(Value::as_str)
                    .map_or(defaults.style.family.clone(), str::to_owned),
                size: number(s, "size").unwrap_or(defaults.style.size),
                weight: number(s, "weight").unwrap_or(defaults.style.weight),
                italic: s.get("italic").and_then(Value::as_bool).unwrap_or(false),
                color: color_of(s.get("color")).unwrap_or(defaults.style.color),
                outline: number(s, "outline").unwrap_or(defaults.style.outline),
                outline_color: color_of(s.get("outline_color"))
                    .unwrap_or(defaults.style.outline_color),
                shadow: number(s, "shadow").unwrap_or(defaults.style.shadow),
                shadow_color: color_of(s.get("shadow_color"))
                    .unwrap_or(defaults.style.shadow_color),
            })
            .unwrap_or_else(|| defaults.style.clone());
        let spans = parameters
            .get("spans")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_object)
            .filter_map(|s| {
                Some(Span {
                    start: s.get("start")?.as_i64()?.max(0) as usize,
                    end: s.get("end")?.as_i64()?.max(0) as usize,
                    family: s.get("family").and_then(Value::as_str).map(str::to_owned),
                    size: number(s, "size"),
                    weight: number(s, "weight"),
                    italic: s.get("italic").and_then(Value::as_bool),
                    color: color_of(s.get("color")),
                })
            })
            .collect();
        Self {
            text: parameters
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            style,
            spans,
            align: parameters
                .get("align")
                .and_then(Value::as_str)
                .and_then(Align::parse)
                .unwrap_or(defaults.align),
            line_spacing: number(parameters, "line_spacing").unwrap_or(defaults.line_spacing),
            tracking: number(parameters, "tracking").unwrap_or(defaults.tracking),
            width: number(parameters, "width").unwrap_or(defaults.width),
            position: point_of(parameters.get("position")).unwrap_or(defaults.position),
            anchor: point_of(parameters.get("anchor")).unwrap_or(defaults.anchor),
        }
    }

    /// As a text clip's parameters.
    pub fn to_parameters(&self) -> Object {
        let mut style = Object::new();
        style.insert("family", self.style.family.as_str());
        style.insert("size", f64::from(self.style.size));
        style.insert("weight", f64::from(self.style.weight));
        style.insert("italic", self.style.italic);
        style.insert("color", color_value(self.style.color));
        // Borders and shadows only where there are some, so plain titles
        // store (and are cached) as before.
        if self.style.outline > 0.0 {
            style.insert("outline", f64::from(self.style.outline));
            style.insert("outline_color", color_value(self.style.outline_color));
        }
        if self.style.shadow > 0.0 {
            style.insert("shadow", f64::from(self.style.shadow));
            style.insert("shadow_color", color_value(self.style.shadow_color));
        }
        let spans = self
            .spans
            .iter()
            .map(|span| {
                let mut s = Object::new();
                s.insert("start", span.start as i64);
                s.insert("end", span.end as i64);
                if let Some(family) = &span.family {
                    s.insert("family", family.as_str());
                }
                if let Some(size) = span.size {
                    s.insert("size", f64::from(size));
                }
                if let Some(weight) = span.weight {
                    s.insert("weight", f64::from(weight));
                }
                if let Some(italic) = span.italic {
                    s.insert("italic", italic);
                }
                if let Some(color) = span.color {
                    s.insert("color", color_value(color));
                }
                Value::Object(s)
            })
            .collect();
        let mut parameters = Object::new();
        parameters.insert("text", self.text.as_str());
        parameters.insert("style", style);
        parameters.insert("spans", Value::Array(spans));
        parameters.insert("align", self.align.as_str());
        parameters.insert("line_spacing", f64::from(self.line_spacing));
        parameters.insert("tracking", f64::from(self.tracking));
        parameters.insert("width", f64::from(self.width));
        let defaults = Self::default();
        if self.position != defaults.position || self.anchor != defaults.anchor {
            parameters.insert("position", point_value(self.position));
            parameters.insert("anchor", point_value(self.anchor));
        }
        parameters
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_round_trip() {
        let document = Document {
            text: "Hello world".into(),
            style: Style {
                family: "Source Sans 3".into(),
                size: 12.5,
                weight: 700.0,
                italic: true,
                color: [1.0, 0.5, 0.25, 0.75],
                outline: 0.3,
                outline_color: [0.0, 0.0, 0.25, 1.0],
                shadow: 0.2,
                shadow_color: [0.0, 0.0, 0.0, 0.5],
            },
            spans: vec![Span {
                start: 6,
                end: 11,
                color: Some([0.0, 1.0, 0.0, 1.0]),
                size: Some(20.0),
                ..Span::default()
            }],
            align: Align::End,
            line_spacing: 1.5,
            tracking: 0.05,
            width: 0.5,
            position: [0.5, 0.95],
            anchor: [0.5, 1.0],
        };
        assert_eq!(
            Document::from_parameters(&document.to_parameters()),
            document
        );
        // Missing values take the defaults.
        let mut text_only = Object::new();
        text_only.insert("text", "Hi");
        assert_eq!(Document::from_parameters(&text_only), Document::new("Hi"));
    }

    #[test]
    fn spans_follow_edits() {
        let green = Some([0.0, 1.0, 0.0, 1.0]);
        let mut document = Document {
            spans: vec![Span {
                start: 6,
                end: 11,
                color: green,
                ..Span::default()
            }],
            ..Document::new("Hello world")
        };
        // Typed before the span: it moves.
        document.replace_text("Hello, world");
        assert_eq!((document.spans[0].start, document.spans[0].end), (7, 12));
        // Typed inside: it grows.
        document.replace_text("Hello, wonderful world");
        assert_eq!((document.spans[0].start, document.spans[0].end), (7, 22));
        // The styled text deleted: the span goes.
        document.replace_text("Hello, ");
        assert!(document.spans.is_empty());
        // Multi-byte characters stay whole.
        let mut accented = Document {
            spans: vec![Span {
                start: 3,
                end: 5,
                color: green,
                ..Span::default()
            }],
            ..Document::new("ab éé")
        };
        accented.replace_text("abc éé");
        assert_eq!((accented.spans[0].start, accented.spans[0].end), (4, 6));
    }
}
