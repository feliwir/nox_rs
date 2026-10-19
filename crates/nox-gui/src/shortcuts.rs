//! Keyboard Shortcuts (Ctrl+?, main menu): libadwaita 1.8's shortcuts
//! dialog, with the keys of main.rs, window.ui and the timeline. Built
//! with the `adw-1-8` feature only (Debian 13's libadwaita is 1.7).

use adw::prelude::*;

use crate::i18n::gettext;

/// Shows the shortcuts over `parent`.
pub fn show(parent: Option<&gtk::Window>) {
    let dialog = adw::ShortcutsDialog::new();
    let sections: [(String, Vec<(String, &str)>); 6] = [
        (
            gettext("Project"),
            vec![
                (gettext("New Project"), "<Control>n"),
                (gettext("Open Project"), "<Control>o"),
                (gettext("Save Project"), "<Control>s"),
                (gettext("Save Project As"), "<Control><Shift>s"),
                (gettext("Export"), "<Control>e"),
                (gettext("Project Settings"), "<Shift>9"),
                (gettext("Preferences"), "<Control>comma"),
                (gettext("Keyboard Shortcuts"), "<Control>question"),
                (gettext("Close Window"), "<Control>w"),
                (gettext("Quit"), "<Control>q"),
            ],
        ),
        (
            gettext("Pages"),
            vec![
                (gettext("Edit"), "<Shift>4"),
                (gettext("Effects"), "<Shift>5"),
                (gettext("Color"), "<Shift>6"),
            ],
        ),
        (
            gettext("Playback"),
            vec![
                (gettext("Play or Pause"), "space"),
                (gettext("Play"), "l"),
                (gettext("Pause"), "k"),
                (gettext("Previous or Next Frame"), "Left Right"),
                (gettext("Go to Start or End"), "Home End"),
                (gettext("Previous or Next Edit"), "Up Down"),
            ],
        ),
        (
            gettext("Marks and Markers"),
            vec![
                (gettext("Mark In"), "i"),
                (gettext("Mark Out"), "o"),
                (gettext("Clear In"), "<Shift>i"),
                (gettext("Clear Out"), "<Shift>o"),
                (gettext("Clear In and Out"), "<Shift>x"),
                (gettext("Add Marker"), "m"),
                (gettext("Previous or Next Marker"), "<Shift>Up <Shift>Down"),
            ],
        ),
        (
            gettext("Editing"),
            vec![
                (gettext("Undo"), "<Control>z"),
                (gettext("Redo"), "<Control><Shift>z"),
                (gettext("Insert Media at Playhead"), "F9"),
                (gettext("Overwrite at Playhead"), "F10"),
                (gettext("Split at Playhead"), "<Control>b"),
                (gettext("Add Dissolve"), "<Control>t"),
                (gettext("Add Text"), "<Control><Shift>t"),
                (gettext("Delete"), "Delete"),
                (gettext("Ripple Delete"), "<Shift>Delete"),
                (gettext("Snapping"), "n"),
            ],
        ),
        (
            gettext("Clips"),
            vec![
                (gettext("Select Clip at Playhead"), "<Shift>v"),
                (gettext("Select All Clips"), "<Control>a"),
                (
                    gettext("Move a Frame Earlier or Later"),
                    "<Control>Left <Control>Right",
                ),
                (
                    gettext("Move a Second Earlier or Later"),
                    "<Control><Shift>Left <Control><Shift>Right",
                ),
                (
                    gettext("Move to the Track Above or Below"),
                    "<Control>Up <Control>Down",
                ),
                (gettext("Trim Start to Playhead"), "braceleft"),
                (gettext("Trim End to Playhead"), "braceright"),
            ],
        ),
    ];
    for (title, items) in sections {
        let section = adw::ShortcutsSection::new(Some(&title));
        for (name, keys) in items {
            section.add(adw::ShortcutsItem::new(&name, keys));
        }
        dialog.add(section);
    }
    dialog.present(parent);
}
