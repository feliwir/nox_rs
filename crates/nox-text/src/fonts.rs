//! The fonts text can use: Noto Sans, bundled so projects look the same
//! everywhere, and the system's fonts, found by fontdb (pure Rust: it
//! reads the font folders and fontconfig's configuration files). A family
//! is loaded into Parley's collection when a document first uses it.

use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use parley::{FontContext, LayoutContext};
use std::collections::HashSet;
use std::sync::Arc;

use crate::document::Color;
use crate::outline::Outlines;

/// The bundled family, used when none is given or one is missing.
pub const DEFAULT_FAMILY: &str = "Noto Sans";

const BUNDLED: [&[u8]; 2] = [
    include_bytes!("../fonts/NotoSans-Variable.ttf"),
    include_bytes!("../fonts/NotoSans-Italic-Variable.ttf"),
];

/// The fonts and Parley's caches. Not shared between threads: each
/// renderer has its own.
pub struct Fonts {
    pub(crate) font_cx: FontContext,
    pub(crate) layout_cx: LayoutContext<Color>,
    /// Glyph outlines drawn so far.
    pub(crate) outlines: Outlines,
    /// The system's fonts, scanned when first needed.
    system: Option<fontdb::Database>,
    /// Whether to look at the system's fonts at all (not in tests).
    use_system: bool,
    /// Families asked for (loaded, or found missing).
    tried: HashSet<String>,
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    /// The bundled font and the system's.
    pub fn new() -> Self {
        Self::with_system(true)
    }

    /// The bundled font only (and what `add` adds): the same everywhere.
    pub fn bundled_only() -> Self {
        Self::with_system(false)
    }

    fn with_system(use_system: bool) -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let mut default = Vec::new();
        for data in BUNDLED {
            for (family, _) in collection.register_fonts(Blob::new(Arc::new(data)), None) {
                if !default.contains(&family) {
                    default.push(family);
                }
            }
        }
        // The bundled font stands in for generic families and covers the
        // scripts other fonts don't.
        for generic in [
            GenericFamily::SansSerif,
            GenericFamily::SystemUi,
            GenericFamily::Serif,
        ] {
            collection.set_generic_families(generic, default.iter().copied());
        }
        let mut fonts = Self {
            font_cx: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            layout_cx: LayoutContext::new(),
            outlines: Outlines::default(),
            system: None,
            use_system,
            tried: HashSet::new(),
        };
        fonts.tried.insert(DEFAULT_FAMILY.to_owned());
        fonts
    }

    /// Adds the fonts in `data` (a font file, e.g. a project's or a
    /// test's), returning the families in it.
    pub fn add(&mut self, data: Vec<u8>) -> Vec<String> {
        let registered = self
            .font_cx
            .collection
            .register_fonts(Blob::new(Arc::new(data)), None);
        let mut names = Vec::new();
        for (family, _) in registered {
            if let Some(name) = self.font_cx.collection.family_name(family) {
                self.tried.insert(name.to_owned());
                if !names.iter().any(|n| n == name) {
                    names.push(name.to_owned());
                }
            }
        }
        names
    }

    /// Whether `family` can be used (loading it from the system if need
    /// be).
    pub fn has(&mut self, family: &str) -> bool {
        self.ensure(family);
        self.font_cx.collection.family_id(family).is_some()
    }

    /// Loads `family` from the system's fonts if it isn't loaded yet.
    pub(crate) fn ensure(&mut self, family: &str) {
        if !self.tried.insert(family.to_owned()) || !self.use_system {
            return;
        }
        let system = self.system();
        let mut files = Vec::new();
        for face in system.faces() {
            if face
                .families
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(family))
                && let fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) =
                    &face.source
                && !files.contains(path)
            {
                files.push(path.clone());
            }
        }
        for path in files {
            match std::fs::read(&path) {
                Ok(data) => {
                    self.font_cx
                        .collection
                        .register_fonts(Blob::new(Arc::new(data)), None);
                }
                Err(err) => tracing::warn!("cannot read font {}: {err}", path.display()),
            }
        }
        if self.font_cx.collection.family_id(family).is_none() {
            tracing::info!("font family {family} not found; using {DEFAULT_FAMILY}");
        }
    }

    /// Every family that can be used, sorted.
    pub fn families(&mut self) -> Vec<String> {
        let mut names: Vec<String> = self
            .font_cx
            .collection
            .family_names()
            .map(str::to_owned)
            .collect();
        if self.use_system {
            for face in self.system().faces() {
                if let Some((name, _)) = face.families.first() {
                    names.push(name.clone());
                }
            }
        }
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    }

    fn system(&mut self) -> &fontdb::Database {
        self.system.get_or_insert_with(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            db
        })
    }
}
