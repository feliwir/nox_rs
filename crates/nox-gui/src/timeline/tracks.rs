//! Track operations: enable, lock, solo, sync lock, rename and delete.

use super::*;

impl TimelineView {
    /// Mutes (audio) or hides (video) track `index`, or turns it back on:
    /// OTIO's `enabled`, which playback and export respect.
    pub fn set_track_enabled(&self, index: usize, enabled: bool) {
        let project = self.imp().project().clone();
        let Some(kind) = project.timeline().track(index).map(|t| t.kind.clone()) else {
            return;
        };
        project.edit_track(index, &enable_label(&kind, enabled), |track| {
            track.item.enabled = enabled;
        });
    }

    /// Locks or unlocks track `index`; locked tracks reject edits.
    pub fn set_track_locked(&self, index: usize, locked: bool) {
        let project = self.imp().project().clone();
        project.edit_track(index, &lock_label(locked), |track| {
            track.item.set_locked(locked);
        });
    }

    /// Applies a header toggle to track `index`.
    pub(super) fn set_track_toggle(&self, index: usize, toggle: Toggle, on: bool) {
        match toggle {
            Toggle::Off => self.set_track_enabled(index, !on),
            Toggle::Solo => self.set_track_solo(index, on),
            Toggle::Shown => self.set_track_shown(index, on),
            Toggle::Lock => self.set_track_locked(index, on),
            Toggle::Sync => self.set_track_sync_locked(index, on),
        }
    }

    /// Solos audio track `index`, or not: while any track is soloed, only
    /// soloed tracks are heard.
    pub fn set_track_solo(&self, index: usize, on: bool) {
        let project = self.imp().project().clone();
        let label = if on {
            crate::i18n::gettext("Solo Track")
        } else {
            crate::i18n::gettext("Unsolo Track")
        };
        project.edit_track(index, &label, |track| {
            track.item.set_solo(on);
        });
    }

    /// Makes track `index` follow ripple edits on other tracks, or not.
    pub fn set_track_sync_locked(&self, index: usize, on: bool) {
        let project = self.imp().project().clone();
        let label = if on {
            crate::i18n::gettext("Sync Lock Track")
        } else {
            crate::i18n::gettext("Remove Sync Lock")
        };
        project.edit_track(index, &label, |track| {
            track.item.set_sync_locked(on);
        });
    }

    /// Shows subtitle track `index` in the viewer (and no other: they'd
    /// overlap), or not.
    pub fn set_track_shown(&self, index: usize, on: bool) {
        let project = self.imp().project().clone();
        let label = if on {
            crate::i18n::gettext("Show Subtitles in Viewer")
        } else {
            crate::i18n::gettext("Hide Subtitles in Viewer")
        };
        let _ = project.edit(&label, |timeline| {
            let subtitles: Vec<usize> = timeline
                .tracks()
                .filter(|(_, t)| t.kind == TrackKind::Subtitle)
                .map(|(i, _)| i)
                .collect();
            for i in subtitles {
                if let Some(track) = timeline.track_mut(i) {
                    track.set_shown(on && i == index);
                }
            }
            Ok::<_, EditError>(())
        });
    }

    /// Draws subtitle track `index` into the picture (in the viewer and
    /// on export) instead of exporting it as a stream, or not.
    pub fn set_track_burned_in(&self, index: usize, on: bool) {
        let project = self.imp().project().clone();
        let label = if on {
            crate::i18n::gettext("Burn In Subtitles")
        } else {
            crate::i18n::gettext("Don't Burn In Subtitles")
        };
        project.edit_track(index, &label, |track| track.set_burned_in(on));
    }

    /// Asks for track `index`'s stream settings: the language, title and
    /// flags its stream is exported with.
    pub fn show_stream_settings(&self, index: usize) {
        use crate::i18n::gettext;
        use adw::prelude::*;
        let project = self.imp().project().clone();
        let Some((name, tags)) = project
            .timeline()
            .track(index)
            .map(|t| (t.item.name.clone(), t.stream_tags()))
        else {
            return;
        };
        let group = adw::PreferencesGroup::new();
        let language = adw::EntryRow::builder()
            .title(gettext("Language"))
            .text(tags.language.clone().unwrap_or_default())
            .build();
        language.set_tooltip_text(Some(&gettext("An ISO 639-2 code, such as eng or ger")));
        let title = adw::EntryRow::builder()
            .title(gettext("Title"))
            .text(tags.title.clone().unwrap_or_default())
            .build();
        let switch =
            |label: String, on: bool| adw::SwitchRow::builder().title(label).active(on).build();
        let default = switch(gettext("Default"), tags.default);
        let forced = switch(gettext("Forced"), tags.forced);
        let hearing = switch(gettext("For the Hearing Impaired"), tags.hearing_impaired);
        group.add(&language);
        group.add(&title);
        group.add(&default);
        group.add(&forced);
        group.add(&hearing);
        let dialog = adw::AlertDialog::new(
            Some(&gettext("Stream Settings")),
            Some(&crate::i18n::fill(
                &gettext("How the stream of “{track}” is tagged when exported."),
                &[("track", &name)],
            )),
        );
        dialog.set_extra_child(Some(&group));
        dialog.add_response("cancel", &gettext("Cancel"));
        dialog.add_response("apply", &gettext("Apply"));
        dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("apply"));
        dialog.set_close_response("cancel");
        dialog.connect_response(None, move |_, response| {
            if response != "apply" {
                return;
            }
            let text =
                |row: &adw::EntryRow| Some(row.text().trim().to_owned()).filter(|t| !t.is_empty());
            let tags = nox_project::StreamTags {
                language: text(&language),
                title: text(&title),
                default: default.is_active(),
                forced: forced.is_active(),
                hearing_impaired: hearing.is_active(),
            };
            project.edit_track(index, &gettext("Change Stream Settings"), |track| {
                track.set_stream_tags(&tags)
            });
        });
        dialog.present(self.root().as_ref());
    }

    pub fn rename_track(&self, index: usize, name: &str) {
        let project = self.imp().project().clone();
        project.edit_track(index, &crate::i18n::gettext("Rename Track"), |track| {
            track.item.name = name.to_owned();
        });
    }

    /// Deletes track `index` with its clips (not if it is locked).
    pub fn delete_track(&self, index: usize) {
        let project = self.imp().project().clone();
        if project
            .edit(&crate::i18n::gettext("Delete Track"), |t| {
                t.remove_track(index)
            })
            .is_ok()
        {
            // Selections are by track index, which has changed.
            self.set_selection(None);
        }
    }

    /// Asks for a new name for track `index`.
    pub fn show_rename_dialog(&self, index: usize) {
        let project = self.imp().project().clone();
        let Some(name) = project.timeline().track(index).map(|t| t.item.name.clone()) else {
            return;
        };
        let builder = rename_dialog();
        let dialog: gtk::Window = builder.object("dialog").expect("dialog");
        let entry: gtk::Entry = builder.object("name").expect("name");
        let cancel: gtk::Button = builder.object("cancel").expect("cancel");
        let rename: gtk::Button = builder.object("rename").expect("rename");
        dialog.set_transient_for(self.root().and_downcast_ref::<gtk::Window>());
        entry.set_text(&name);
        cancel.connect_clicked(glib::clone!(
            #[weak]
            dialog,
            move |_| dialog.close()
        ));
        rename.connect_clicked(glib::clone!(
            #[weak(rename_to = view)]
            self,
            #[weak]
            dialog,
            #[weak]
            entry,
            move |_| {
                view.rename_track(index, entry.text().trim());
                dialog.close();
            }
        ));
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::NamedAction::new("window.close")),
        ));
        dialog.add_controller(shortcuts);
        dialog.present();
    }
}

/// The rename dialog for tracks (`resources/ui/rename-track.ui`).
pub(crate) fn rename_dialog() -> gtk::Builder {
    let builder = gtk::Builder::new();
    builder.set_translation_domain(Some(crate::i18n::DOMAIN));
    builder
        .add_from_resource("/io/github/feliwir/Nox/ui/rename-track.ui")
        .expect("rename-track.ui is valid");
    builder
}
