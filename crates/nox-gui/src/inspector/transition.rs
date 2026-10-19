//! The inspector's transition section: which transition it is (picked from
//! nox-fx's list, by category, with search) and its parameters, with the
//! controls effects use (`effect_controls`, without keyframes). Its length
//! is `Inspector::set_transition_length`.

use gtk::{glib, prelude::*, subclass::prelude::*};
use nox_fx::transitions::{TransitionDef, set_transition, transition_def};
use nox_project::{Composable, Parameters, Transition};
use std::rc::Rc;

use super::Inspector;
use crate::effect_controls::{
    EffectHost, Offer, param_controls, transition_list_popover, update_controls,
};
use crate::i18n::{fill, gettext};

/// The selected transition, as controls edit through it.
struct TransitionHost(glib::WeakRef<Inspector>);

impl EffectHost for TransitionHost {
    fn updating(&self) -> bool {
        self.0.upgrade().is_none_or(|i| i.imp().updating.get())
    }

    fn media_time(&self) -> f64 {
        0.0
    }

    fn edit_effect(
        &self,
        label: &str,
        merge_key: Option<&str>,
        _key: &str,
        change: &dyn Fn(&mut dyn Parameters),
    ) {
        if let Some(inspector) = self.0.upgrade() {
            inspector.edit_transition(label, merge_key, &|t| change(t));
        }
    }
}

impl Inspector {
    /// Builds the transitions list into its button.
    pub(super) fn setup_transitions(&self) {
        let popover = transition_list_popover(glib::clone!(
            #[weak(rename_to = inspector)]
            self,
            move |def| inspector.set_transition(def)
        ));
        self.imp().transition_type.set_popover(Some(&popover));
    }

    /// Makes the selected transition `def` (default parameters).
    fn set_transition(&self, def: &'static TransitionDef) {
        let label = fill(
            &gettext("Change Transition to {transition}"),
            &[("transition", &gettext(def.label))],
        );
        self.edit_transition(&label, None, &|t| set_transition(t, def));
    }

    /// Changes the selected transition (one undo step; edits with the same
    /// `merge_key` in a row merge, as a dragged slider's).
    fn edit_transition(
        &self,
        label: &str,
        merge_key: Option<&str>,
        change: &dyn Fn(&mut Transition),
    ) {
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
            let transition = t
                .track_mut(item.track)
                .and_then(|track| track.children.get_mut(item.index))
                .and_then(|child| match child {
                    Composable::Transition(transition) => Some(transition),
                    _ => None,
                })
                .ok_or("not a transition")?;
            change(transition);
            Ok::<_, &str>(())
        };
        let project = timeline.project();
        let _ = match merge_key {
            Some(key) => {
                let key = format!("transition:{}:{}:{key}", item.track, item.index);
                project.edit_merging(label, &key, edit)
            }
            None => project.edit(label, edit),
        };
    }

    /// Shows `transition` (while `updating`): its name, type and
    /// parameters.
    pub(super) fn show_transition(&self, transition: &Transition) {
        let imp = self.imp();
        let def = transition_def(transition);
        let title = gettext(def.label);
        imp.name_label.set_text(&title);
        imp.transition_type.set_label(&title);
        let mut controls = imp.transition_controls.borrow_mut();
        if controls.0 != Some(def.id) {
            let grid = imp.transition_params.get();
            while let Some(child) = grid.first_child() {
                grid.remove(&child);
            }
            let host: Rc<dyn EffectHost> = Rc::new(TransitionHost(self.downgrade()));
            *controls = (
                Some(def.id),
                param_controls(&host, &grid, "", &title, def.params, Offer::TRANSITION),
            );
        }
        update_controls(&controls.1, def.params, transition, 0.0);
    }
}
