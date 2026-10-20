//! An event as a nox-text document: its runs as the base style and spans,
//! placed and wrapped in the picture as libass would (alignment, margins,
//! `\pos`), sizes in percent of the picture's height.

use nox_text::{Align, Document, Span};

use crate::script::{Event, Script};
use crate::tags::{self, State};

/// An ASS font size is the font's cell height (ascent and descent, as
/// Windows' GDI measures it), not its em: for Arial, the usual subtitle
/// font, the em is about 0.9 of it.
pub const FONT_SIZE_TO_EM: f64 = 0.9;

/// An event as Nox shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Caption {
    pub document: Document,
    /// Seconds it fades in and out (`\fad`).
    pub fade_in: f64,
    pub fade_out: f64,
    /// Its layer: higher ones show above.
    pub layer: i32,
}

impl Script {
    /// `event` as a caption, `None` if it shows no text (only drawings
    /// or spaces).
    pub fn caption(&self, event: &Event) -> Option<Caption> {
        let style = self.style(&event.style);
        let line = tags::parse(&event.text, &style, self);
        let text: String = line.runs.iter().map(|(t, _)| t.as_str()).collect();
        let base = &line.runs.iter().find(|(t, _)| !t.trim().is_empty())?.1;
        let (w, h) = self.play_res;
        // Script pixels in percent of the picture's height.
        let percent = |px: f64| (px / h * 100.0) as f32;
        let em = |state: &State| state.size * state.scale_y / 100.0 * FONT_SIZE_TO_EM;
        let font_style = nox_text::Style {
            family: base.font.clone(),
            size: percent(em(base)),
            weight: base.weight,
            italic: base.italic,
            color: base.primary,
            outline: percent(base.outline),
            outline_color: base.outline_color,
            shadow: percent(base.shadow),
            shadow_color: base.back,
        };
        let mut spans = Vec::new();
        let mut at = 0;
        for (run, state) in &line.runs {
            let span = Span {
                start: at,
                end: at + run.len(),
                family: (state.font != base.font).then(|| state.font.clone()),
                size: (em(state) != em(base)).then(|| percent(em(state))),
                weight: (state.weight != base.weight).then_some(state.weight),
                italic: (state.italic != base.italic).then_some(state.italic),
                color: (state.primary != base.primary).then_some(state.primary),
            };
            at = span.end;
            if span
                != (Span {
                    start: span.start,
                    end: span.end,
                    ..Span::default()
                })
            {
                spans.push(span);
            }
        }

        let alignment = line.alignment.unwrap_or(style.alignment).clamp(1, 9);
        let (column, row) = ((alignment - 1) % 3, (alignment - 1) / 3);
        let margin = |event: f64, style: f64| if event != 0.0 { event } else { style };
        let ml = margin(event.margin_l, style.margin_l);
        let mr = margin(event.margin_r, style.margin_r);
        let mv = margin(event.margin_v, style.margin_v);
        let position = match line.pos {
            Some((x, y)) => [x / w, y / h],
            None => [
                match column {
                    0 => ml / w,
                    1 => (ml + w - mr) / 2.0 / w,
                    _ => (w - mr) / w,
                },
                match row {
                    0 => (h - mv) / h,
                    1 => 0.5,
                    _ => mv / h,
                },
            ],
        };
        let document = Document {
            text,
            style: font_style,
            spans,
            align: [Align::Start, Align::Center, Align::End][usize::from(column)],
            // Lines are a cell apart.
            line_spacing: (1.0 / FONT_SIZE_TO_EM) as f32,
            tracking: if em(base) > 0.0 {
                (base.spacing / em(base)) as f32
            } else {
                0.0
            },
            width: if line.wrap_style == 2 {
                0.0
            } else {
                ((w - ml - mr) / w).max(0.05) as f32
            },
            position: position.map(|p| p as f32),
            anchor: [f32::from(column) * 0.5, 1.0 - f32::from(row) * 0.5],
        };
        let (fade_in, fade_out) = line.fade.unwrap_or_default();
        Some(Caption {
            document,
            fade_in,
            fade_out,
            layer: event.layer,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script() -> Script {
        Script::parse(
            "[Script Info]\nPlayResX: 1920\nPlayResY: 1080\n\
             [V4+ Styles]\n\
             Format: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, Italic, Outline, Shadow, Alignment, MarginL, MarginR, MarginV\n\
             Style: Default,Arial,60,&H00FFFFFF,&H00000000,&H80000000,0,0,2,1,2,40,80,54\n",
        )
    }

    fn event(text: &str) -> Event {
        Event {
            style: "Default".into(),
            text: text.into(),
            ..Event::default()
        }
    }

    #[test]
    fn styled_as_the_script_says() {
        let script = script();
        let caption = script
            .caption(&event("Hello {\\b1\\c&H00FFFF&}world"))
            .unwrap();
        let doc = &caption.document;
        assert_eq!(doc.text, "Hello world");
        assert_eq!(doc.style.family, "Arial");
        // 60 of 1080, its em 0.9 of that: 5 %.
        assert!((doc.style.size - 5.0).abs() < 1e-4);
        assert!((doc.style.outline - 2.0 / 10.8).abs() < 1e-4);
        assert_eq!(doc.style.shadow_color[3], 1.0 - 128.0 / 255.0);
        assert_eq!(doc.spans.len(), 1);
        let span = &doc.spans[0];
        assert_eq!((span.start, span.end), (6, 11));
        assert_eq!(span.weight, Some(700.0));
        assert_eq!(span.color, Some([1.0, 1.0, 0.0, 1.0]));
        assert_eq!(span.size, None);
    }

    #[test]
    fn placed_as_libass_does() {
        let script = script();
        // Bottom center between the margins, 54 above the bottom.
        let doc = script.caption(&event("a")).unwrap().document;
        assert_eq!(doc.anchor, [0.5, 1.0]);
        assert_eq!(doc.align, Align::Center);
        assert!((doc.position[0] - (40.0 + 1920.0 - 80.0) / 2.0 / 1920.0).abs() < 1e-6);
        assert!((doc.position[1] - 0.95).abs() < 1e-6);
        assert!((doc.width - 1800.0 / 1920.0).abs() < 1e-6);
        // Top left, with the event's own margins.
        let top_left = Event {
            margin_l: 100.0,
            margin_v: 108.0,
            ..event("{\\an7}a")
        };
        let doc = script.caption(&top_left).unwrap().document;
        assert_eq!((doc.anchor, doc.align), ([0.0, 0.0], Align::Start));
        assert!((doc.position[0] - 100.0 / 1920.0).abs() < 1e-6);
        assert!((doc.position[1] - 0.1).abs() < 1e-6);
        // Positioned: its anchor (middle right) at the point.
        let doc = script
            .caption(&event("{\\an6\\pos(960,540)}a"))
            .unwrap()
            .document;
        assert_eq!((doc.anchor, doc.align), ([1.0, 0.5], Align::End));
        assert_eq!(doc.position, [0.5, 0.5]);
        // Fades; nothing to show.
        let caption = script.caption(&event("{\\fad(200,400)}a")).unwrap();
        assert_eq!((caption.fade_in, caption.fade_out), (0.2, 0.4));
        assert!(script.caption(&event("{\\p1}m 0 0 l 1 1")).is_none());
        assert!(script.caption(&event("  ")).is_none());
    }
}
