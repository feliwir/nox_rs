//! The transitions Nox renders between two clips: what each is called, its
//! category and parameters. Every transition is GLSL in gl-transitions'
//! form (`shader_transitions`): Nox's own and the bundled gl-transitions in
//! `transitions/`, and the user's packages. The compositor draws one from
//! both clips' pictures. The project stores a transition's id and values
//! (`nox_project::Transition`).

use nox_project::{DISSOLVE, Transition};
use std::sync::OnceLock;

use crate::registry::{ParamDef, ParamKind, ParamValue, n_};
use crate::shader_transitions::{self, ShaderTransition};

/// Where the transitions list puts a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionCategory {
    Dissolve,
    Wipe,
    Motion,
    /// Effects that are none of the above.
    Stylize,
}

impl TransitionCategory {
    pub const ALL: [Self; 4] = [Self::Dissolve, Self::Wipe, Self::Motion, Self::Stylize];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dissolve => n_("Dissolves"),
            Self::Wipe => n_("Wipes"),
            Self::Motion => n_("Motion"),
            Self::Stylize => n_("Stylize"),
        }
    }

    /// A file's `Category` header (English, singular or plural).
    fn parse(name: &str) -> Option<Self> {
        let name = name.trim().to_lowercase();
        let name = name.trim_end_matches('s');
        Self::ALL
            .into_iter()
            .find(|c| c.label().to_lowercase().trim_end_matches('s') == name)
    }
}

/// A transition Nox renders.
#[derive(Debug)]
pub struct TransitionDef {
    /// The project's id (`Transition::id`).
    pub id: &'static str,
    pub label: &'static str,
    pub category: TransitionCategory,
    /// The shader's parameters, then the ease.
    pub params: &'static [ParamDef],
    pub shader: &'static ShaderTransition,
}

pub const DIP: &str = "nox.dip";
pub const EDGE_WIPE: &str = "nox.edge_wipe";
pub const SPLIT: &str = "nox.split";
pub const CLOCK_WIPE: &str = "nox.clock_wipe";
pub const IRIS: &str = "nox.iris";
pub const BLINDS: &str = "nox.blinds";
pub const CHECKERBOARD: &str = "nox.checkerboard";
pub const BLOCKS: &str = "nox.blocks";
pub const PUSH: &str = "nox.push";
pub const SLIDE: &str = "nox.slide";
pub const REVEAL: &str = "nox.reveal";
pub const ZOOM: &str = "nox.zoom";
pub const SPIN: &str = "nox.spin";

/// How progress runs through a transition, applied before the shader
/// gets it.
const EASE: ParamDef = ParamDef {
    name: "ease",
    label: n_("Ease"),
    kind: ParamKind::Choice {
        options: &[
            ("linear", n_("Linear")),
            ("in", n_("Ease In")),
            ("out", n_("Ease Out")),
            ("in_out", n_("Ease In and Out")),
        ],
        default: "linear",
    },
};

/// Nox's own, the bundled gl-transitions, then the user's packages, by
/// category. Loaded once.
pub fn transitions() -> &'static [&'static TransitionDef] {
    static ALL: OnceLock<Vec<&'static TransitionDef>> = OnceLock::new();
    ALL.get_or_init(|| {
        let def = |id: String, parsed: ShaderTransition| -> &'static TransitionDef {
            let parsed: &'static ShaderTransition = Box::leak(Box::new(parsed));
            let mut params = parsed.params.clone();
            params.push(EASE);
            let label = if parsed.name.is_empty() {
                id.rsplit('.').next().unwrap_or_default().to_owned()
            } else {
                parsed.name.clone()
            };
            Box::leak(Box::new(TransitionDef {
                id: Box::leak(id.into_boxed_str()),
                label: Box::leak(label.into_boxed_str()),
                category: parsed
                    .category
                    .as_deref()
                    .and_then(TransitionCategory::parse)
                    .unwrap_or(TransitionCategory::Stylize),
                params: Box::leak(params.into_boxed_slice()),
                shader: parsed,
            }))
        };
        let mut all = Vec::new();
        for (id, source) in shader_transitions::BUILT_IN {
            match ShaderTransition::parse(source) {
                Ok(parsed) => all.push(def((*id).to_owned(), parsed)),
                Err(err) => tracing::error!("transition {id}: {err}"),
            }
        }
        let dir = shader_transitions::packages_dir();
        let (packages, problems) = shader_transitions::load_packages(&dir);
        for (stem, parsed) in packages {
            all.push(def(format!("user.{stem}"), parsed));
        }
        let _ = shader_transitions::PROBLEMS.set(problems);
        // Each category together, in order (packages among Nox's own).
        all.sort_by_key(|t| {
            TransitionCategory::ALL
                .iter()
                .position(|&c| c == t.category)
        });
        all
    })
}

/// The transition with id `id`.
pub fn find_transition(id: &str) -> Option<&'static TransitionDef> {
    transitions().iter().copied().find(|t| t.id == id)
}

/// `transition`'s definition: by its id, or a dissolve (other tools'
/// transitions, unknown ids, missing packages).
pub fn transition_def(transition: &Transition) -> &'static TransitionDef {
    find_transition(transition.id())
        .or_else(|| find_transition(DISSOLVE))
        .expect("the dissolve")
}

/// `progress` (0–1) through `transition`, eased as it says.
pub fn eased(transition: &Transition, progress: f64) -> f64 {
    let def = transition_def(transition);
    let values = crate::registry::param_values(transition, def.params, 0.0);
    let ease = def
        .params
        .iter()
        .zip(values)
        .find_map(|(p, v)| match (p.name, v) {
            ("ease", ParamValue::Choice(c)) => Some(c),
            _ => None,
        });
    let p = progress.clamp(0.0, 1.0);
    match ease {
        Some("in") => p * p,
        Some("out") => 1.0 - (1.0 - p) * (1.0 - p),
        Some("in_out") => p * p * (3.0 - 2.0 * p),
        _ => p,
    }
}

/// A transition `def` as added: its English label as the name.
pub fn new_transition(def: &TransitionDef) -> Transition {
    let zero = nox_project::Time::zero(nox_project::Rational::new(1, 24));
    Transition::new(def.id, def.label, zero, zero)
}

/// Makes `transition` transition `def`, with default parameters.
pub fn set_transition(transition: &mut Transition, def: &TransitionDef) {
    transition.set_id(def.id, def.label);
}

#[cfg(test)]
mod tests {
    use super::*;
    use nox_project::{Parameters, Value};

    #[test]
    fn nox_transitions_keep_their_ids_names_and_parameters() {
        let dissolve = find_transition(DISSOLVE).unwrap();
        assert_eq!(
            (dissolve.label, dissolve.category),
            ("Cross Dissolve", TransitionCategory::Dissolve)
        );
        // Dissolves first: other tools' transitions fall back to it.
        assert_eq!(transitions()[0].id, DISSOLVE);
        let names = |id: &str| -> Vec<&str> {
            find_transition(id)
                .unwrap()
                .params
                .iter()
                .map(|p| p.name)
                .collect()
        };
        assert_eq!(names(DISSOLVE), ["blend", "ease"]);
        assert_eq!(names(DIP), ["color", "ease"]);
        assert_eq!(
            names(IRIS),
            [
                "shape",
                "center_x",
                "center_y",
                "softness",
                "border",
                "border_color",
                "reverse",
                "ease"
            ]
        );
        assert_eq!(names(SPIN), ["turns", "clockwise", "ease"]);
        for (id, category) in [
            (EDGE_WIPE, TransitionCategory::Wipe),
            (SPLIT, TransitionCategory::Wipe),
            (CLOCK_WIPE, TransitionCategory::Wipe),
            (BLINDS, TransitionCategory::Wipe),
            (CHECKERBOARD, TransitionCategory::Wipe),
            (BLOCKS, TransitionCategory::Wipe),
            (PUSH, TransitionCategory::Motion),
            (SLIDE, TransitionCategory::Motion),
            (REVEAL, TransitionCategory::Motion),
            (ZOOM, TransitionCategory::Motion),
            ("gl.cross_zoom", TransitionCategory::Stylize),
        ] {
            assert_eq!(find_transition(id).unwrap().category, category, "{id}");
        }
        // Choices from `@choice`, percentages from `@range(…, %)`.
        let iris = find_transition(IRIS).unwrap();
        assert!(matches!(
            iris.params[0].kind,
            ParamKind::Choice { default: "circle", options } if options.len() == 5
        ));
        assert!(matches!(
            iris.params[1].kind,
            ParamKind::Number {
                unit: crate::Unit::Percent,
                default: 50.0,
                ..
            }
        ));
    }

    #[test]
    fn eases_and_falls_back_to_a_dissolve() {
        let mut t = new_transition(find_transition(PUSH).unwrap());
        assert_eq!(eased(&t, 0.5), 0.5);
        t.set_param("ease", Some(Value::from("in")));
        assert_eq!(eased(&t, 0.5), 0.25);
        let mut other = new_transition(find_transition(DISSOLVE).unwrap());
        other.set_id("other.page_curl", "Curl");
        assert_eq!(transition_def(&other).id, DISSOLVE);
    }
}
