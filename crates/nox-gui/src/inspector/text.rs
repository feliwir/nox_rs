//! The inspector's Text section, for text clips: the text, its font
//! family, size, weight, italic, color, border, shadow and alignment.
//! Each change is one undo step; typing in a row is one.

use gtk::{gdk, gio, glib, prelude::*, subclass::prelude::*};
use nox_project::Composable;
use nox_text::{Align, Document};

use super::Inspector;
use crate::i18n::gettext;
use crate::ui_util::string_list;

/// The section's controls.
pub struct TextControls {
    text: gtk::TextView,
    family: gtk::DropDown,
    families: gtk::StringList,
    size: gtk::SpinButton,
    weight: gtk::SpinButton,
    italic: gtk::CheckButton,
    color: gtk::ColorDialogButton,
    outline: gtk::SpinButton,
    outline_color: gtk::ColorDialogButton,
    shadow: gtk::SpinButton,
    shadow_color: gtk::ColorDialogButton,
    align: gtk::DropDown,
}

/// A size in percent of the picture's height, 0 for none (borders,
/// shadows).
fn thickness() -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(0.0, 10.0, 0.05);
    spin.set_digits(2);
    spin.set_hexpand(true);
    spin.set_tooltip_text(Some(&gettext("In percent of the picture’s height")));
    spin
}

fn color_button(title: &str) -> gtk::ColorDialogButton {
    gtk::ColorDialogButton::new(Some(
        gtk::ColorDialog::builder()
            .with_alpha(true)
            .title(title)
            .build(),
    ))
}

/// Shows `color` on `button` (unless it does already).
fn show_color(button: &gtk::ColorDialogButton, [r, g, b, a]: nox_text::Color) {
    let rgba = gdk::RGBA::new(r, g, b, a);
    if button.rgba() != rgba {
        button.set_rgba(&rgba);
    }
}

/// The alignments in the drop-down's order.
const ALIGNS: [Align; 4] = [Align::Start, Align::Center, Align::End, Align::Justify];

impl Inspector {
    /// Builds the Text section's controls.
    pub(super) fn setup_text(&self) {
        let imp = self.imp();
        let text = gtk::TextView::new();
        text.set_wrap_mode(gtk::WrapMode::WordChar);
        text.set_top_margin(6);
        text.set_bottom_margin(6);
        text.set_left_margin(6);
        text.set_right_margin(6);
        text.update_property(&[gtk::accessible::Property::Label(&gettext("Text"))]);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_child(Some(&text));
        scroller.set_min_content_height(80);
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.add_css_class("card");

        // The bundled family at once; the system's when they're found.
        let families = gtk::StringList::new(&[nox_text::DEFAULT_FAMILY]);
        let family = gtk::DropDown::new(
            Some(families.clone()),
            Some(gtk::PropertyExpression::new(
                gtk::StringObject::static_type(),
                gtk::Expression::NONE,
                "string",
            )),
        );
        family.set_enable_search(true);
        family.set_hexpand(true);
        let list = families.clone();
        glib::spawn_future_local(async move {
            if let Ok(names) = gio::spawn_blocking(|| nox_text::Fonts::new().families()).await {
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                list.splice(0, list.n_items(), &names);
            }
        });

        let size = gtk::SpinButton::with_range(1.0, 100.0, 0.5);
        size.set_digits(1);
        size.set_tooltip_text(Some(&gettext("In percent of the picture’s height")));
        let weight = gtk::SpinButton::with_range(100.0, 900.0, 100.0);
        weight.set_tooltip_text(Some(&gettext("400 is regular, 700 bold")));
        let italic = gtk::CheckButton::with_label(&gettext("Italic"));
        let color = color_button(&gettext("Text Color"));
        let outline = thickness();
        let outline_color = color_button(&gettext("Border Color"));
        let shadow = thickness();
        let shadow_color = color_button(&gettext("Shadow Color"));
        let pair = |a: &gtk::Widget, b: &gtk::Widget| {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            row.append(a);
            row.append(b);
            row
        };
        let border_row = pair(outline.upcast_ref(), outline_color.upcast_ref());
        let shadow_row = pair(shadow.upcast_ref(), shadow_color.upcast_ref());
        let align = gtk::DropDown::new(
            Some(string_list([
                gettext("Start"),
                gettext("Center"),
                gettext("End"),
                gettext("Justify"),
            ])),
            gtk::Expression::NONE,
        );

        let grid = gtk::Grid::new();
        grid.set_row_spacing(6);
        grid.set_column_spacing(8);
        let rows: [(String, &gtk::Widget); 7] = [
            (gettext("Font"), family.upcast_ref()),
            (gettext("Size"), size.upcast_ref()),
            (gettext("Weight"), weight.upcast_ref()),
            (gettext("Color"), color.upcast_ref()),
            (gettext("Border"), border_row.upcast_ref()),
            (gettext("Shadow"), shadow_row.upcast_ref()),
            (gettext("Alignment"), align.upcast_ref()),
        ];
        for (row, (name, widget)) in rows.iter().enumerate() {
            let label = gtk::Label::new(Some(name));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            widget.update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
            grid.attach(&label, 0, row as i32, 1, 1);
            grid.attach(*widget, 1, row as i32, 1, 1);
        }
        grid.attach(&italic, 1, rows.len() as i32, 1, 1);
        imp.text_section.append(&scroller);
        imp.text_section.append(&grid);

        text.buffer().connect_changed(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |buffer| {
                let (start, end) = buffer.bounds();
                let text = buffer.text(&start, &end, false).to_string();
                inspector.edit_text(&gettext("Edit Text"), Some("text"), move |d| {
                    d.replace_text(&text)
                });
            }
        ));
        family.connect_selected_notify(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |dropdown| {
                let Some(name) = dropdown
                    .selected_item()
                    .and_downcast::<gtk::StringObject>()
                    .map(|s| s.string().to_string())
                else {
                    return;
                };
                inspector.edit_text(&gettext("Change Font"), None, move |d| {
                    d.style.family = name.clone()
                });
            }
        ));
        size.connect_value_changed(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |spin| {
                let value = spin.value() as f32;
                inspector.edit_text(&gettext("Change Text Size"), Some("size"), move |d| {
                    d.style.size = value
                });
            }
        ));
        weight.connect_value_changed(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |spin| {
                let value = spin.value() as f32;
                inspector.edit_text(&gettext("Change Font Weight"), Some("weight"), move |d| {
                    d.style.weight = value
                });
            }
        ));
        italic.connect_toggled(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |check| {
                let on = check.is_active();
                inspector.edit_text(&gettext("Change Italic"), None, move |d| {
                    d.style.italic = on
                });
            }
        ));
        color.connect_rgba_notify(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |button| {
                let rgba = button.rgba();
                let color = [rgba.red(), rgba.green(), rgba.blue(), rgba.alpha()];
                inspector.edit_text(&gettext("Change Text Color"), None, move |d| {
                    d.style.color = color
                });
            }
        ));
        outline.connect_value_changed(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |spin| {
                let value = spin.value() as f32;
                inspector.edit_text(&gettext("Change Text Border"), Some("outline"), move |d| {
                    d.style.outline = value
                });
            }
        ));
        outline_color.connect_rgba_notify(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |button| {
                let rgba = button.rgba();
                let color = [rgba.red(), rgba.green(), rgba.blue(), rgba.alpha()];
                inspector.edit_text(&gettext("Change Border Color"), None, move |d| {
                    d.style.outline_color = color
                });
            }
        ));
        shadow.connect_value_changed(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |spin| {
                let value = spin.value() as f32;
                inspector.edit_text(&gettext("Change Text Shadow"), Some("shadow"), move |d| {
                    d.style.shadow = value
                });
            }
        ));
        shadow_color.connect_rgba_notify(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |button| {
                let rgba = button.rgba();
                let color = [rgba.red(), rgba.green(), rgba.blue(), rgba.alpha()];
                inspector.edit_text(&gettext("Change Shadow Color"), None, move |d| {
                    d.style.shadow_color = color
                });
            }
        ));
        align.connect_selected_notify(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |dropdown| {
                let Some(&value) = ALIGNS.get(dropdown.selected() as usize) else {
                    return;
                };
                inspector.edit_text(&gettext("Change Alignment"), None, move |d| d.align = value);
            }
        ));
        let _ = imp.text_controls.set(TextControls {
            text,
            family,
            families,
            size,
            weight,
            italic,
            color,
            outline,
            outline_color,
            shadow,
            shadow_color,
            align,
        });
    }

    /// Changes the selected text clip's document (one undo step; changes
    /// with the same `merge` in a row make one).
    fn edit_text(&self, label: &str, merge: Option<&str>, change: impl Fn(&mut Document)) {
        let imp = self.imp();
        if imp.updating.get() {
            return;
        }
        let Some(timeline) = imp.timeline.get() else {
            return;
        };
        let Some(item) = timeline.selection() else {
            return;
        };
        let edit = |t: &mut nox_project::Timeline| {
            let parameters = t
                .track_mut(item.track)
                .and_then(|track| track.children.get_mut(item.index))
                .and_then(|child| match child {
                    Composable::Clip(clip) => clip.text_parameters_mut(),
                    _ => None,
                })
                .ok_or("not a text clip")?;
            let mut document = Document::from_parameters(parameters);
            change(&mut document);
            *parameters = document.to_parameters();
            Ok::<_, &str>(())
        };
        let project = timeline.project();
        let _ = match merge {
            Some(key) => {
                let key = format!("text:{}:{}:{key}", item.track, item.index);
                project.edit_merging(label, &key, edit)
            }
            None => project.edit(label, edit),
        };
    }

    /// Shows `document` in the controls (while `updating`).
    pub(super) fn show_text(&self, document: &Document) {
        let Some(controls) = self.imp().text_controls.get() else {
            return;
        };
        let buffer = controls.text.buffer();
        let (start, end) = buffer.bounds();
        // Unchanged text stays as it is (keeping the cursor).
        if buffer.text(&start, &end, false) != document.text {
            buffer.set_text(&document.text);
        }
        let family = &document.style.family;
        let position = (0..controls.families.n_items())
            .find(|&i| controls.families.string(i).is_some_and(|s| s == *family));
        let position = position.unwrap_or_else(|| {
            // A family not in the list (yet, or not on this system).
            controls.families.append(family);
            controls.families.n_items() - 1
        });
        controls.family.set_selected(position);
        controls.size.set_value(f64::from(document.style.size));
        controls.weight.set_value(f64::from(document.style.weight));
        controls.italic.set_active(document.style.italic);
        show_color(&controls.color, document.style.color);
        controls
            .outline
            .set_value(f64::from(document.style.outline));
        show_color(&controls.outline_color, document.style.outline_color);
        controls.shadow.set_value(f64::from(document.style.shadow));
        show_color(&controls.shadow_color, document.style.shadow_color);
        let align = ALIGNS
            .iter()
            .position(|&a| a == document.align)
            .unwrap_or(1);
        controls.align.set_selected(align as u32);
    }
}
