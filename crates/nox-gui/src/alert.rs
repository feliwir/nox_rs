//! Messages and questions in libadwaita's alert dialogs (HIG: a short
//! heading, a body explaining it, and the answers as buttons).

use adw::prelude::*;
use gtk::gio;

use crate::i18n::gettext;

/// How an answer's button looks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Plain,
    /// The default answer (Enter).
    Suggested,
    /// An answer that loses something, such as Discard.
    Destructive,
}

/// Tells `heading` (explained by `body`) over `parent`'s window.
pub fn tell(parent: &impl IsA<gtk::Widget>, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_response("close", &gettext("Close"));
    dialog.set_default_response(Some("close"));
    dialog.present(Some(parent));
}

/// Asks `heading` (explained by `body`) over `parent`'s window with
/// `answers` (id, label, look); the first answer is the one Escape and
/// closing give. Calls `then` with the chosen answer's id, unless that's
/// the first.
pub fn ask(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    answers: &[(&str, String, Look)],
    then: impl FnOnce(&str) + 'static,
) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    for (id, label, look) in answers {
        dialog.add_response(id, label);
        match look {
            Look::Plain => {}
            Look::Suggested => {
                dialog.set_response_appearance(id, adw::ResponseAppearance::Suggested);
                dialog.set_default_response(Some(id));
            }
            Look::Destructive => {
                dialog.set_response_appearance(id, adw::ResponseAppearance::Destructive);
            }
        }
    }
    let cancel = answers.first().map(|(id, ..)| id.to_string());
    dialog.set_close_response(cancel.as_deref().unwrap_or("close"));
    dialog.choose(Some(parent), gio::Cancellable::NONE, move |answer| {
        if Some(answer.as_str()) != cancel.as_deref() {
            then(&answer);
        }
    });
}
