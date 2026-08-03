//! Embedded fonts and the cosmic-text font system.
//!
//! DejaVu Sans and Courier Prime ship inside the binary; system fonts are
//! loaded only as glyph fallback and as choices in the settings dialog.

use cosmic_text::{FontSystem, SwashCache};

pub const BODY_FAMILY: &str = "DejaVu Sans";
pub const CODE_FAMILY: &str = "Courier Prime";
/// The equation face. Metrics-bound to the math engine, so no picker
/// offers it; `MATH_FONT` also feeds noad's OpenType MATH reader directly.
pub const MATH_FAMILY: &str = "STIX Two Math";

pub struct FontStore {
    pub font_system: FontSystem,
    /// Glyph raster cache shared by every paint pass.
    pub swash: SwashCache,
    /// The registered math face, the raster key for math glyph runs.
    pub math_face: cosmic_text::fontdb::ID,
}

/// The regular code face bytes; math literal fallbacks measure against
/// its fixed advance without a shaping pass.
pub(crate) static CODE_REGULAR: &[u8] =
    include_bytes!("../../assets/fonts/CourierPrime_Regular.ttf");

pub(crate) static EMBEDDED: &[&[u8]] = &[
    include_bytes!("../../assets/fonts/DejaVuSans.ttf"),
    include_bytes!("../../assets/fonts/DejaVuSans-Bold.ttf"),
    include_bytes!("../../assets/fonts/DejaVuSans-Oblique.ttf"),
    include_bytes!("../../assets/fonts/DejaVuSans-BoldOblique.ttf"),
    CODE_REGULAR,
    include_bytes!("../../assets/fonts/CourierPrime_Bold.ttf"),
    include_bytes!("../../assets/fonts/CourierPrime_Italic.ttf"),
    include_bytes!("../../assets/fonts/CourierPrime_BoldItalic.ttf"),
];

pub(crate) static MATH_FONT: &[u8] = include_bytes!("../../assets/fonts/STIXTwoMath-Regular.otf");

fn find_math_face(db: &cosmic_text::fontdb::Database) -> cosmic_text::fontdb::ID {
    db.faces()
        .find(|face| face.families.iter().any(|(name, _)| name == MATH_FAMILY))
        .map(|face| face.id)
        .expect("the embedded math face registers")
}

/// Template for pooled stores: the locale and the seeded database built
/// once, cloned per worker, so no worker pays its own system scan. The
/// face data rides shared handles; the clone copies metadata only.
pub struct FontSeed {
    locale: String,
    db: cosmic_text::fontdb::Database,
}

impl FontStore {
    pub fn new() -> FontStore {
        let mut font_system = FontSystem::new();
        for bytes in EMBEDDED {
            font_system.db_mut().load_font_data(bytes.to_vec());
        }
        font_system.db_mut().load_font_data(MATH_FONT.to_vec());
        let math_face = find_math_face(font_system.db());
        FontStore {
            font_system,
            swash: SwashCache::new(),
            math_face,
        }
    }

    /// The template a pool clones its workers' stores from.
    pub fn seed(&self) -> FontSeed {
        FontSeed {
            locale: self.font_system.locale().to_string(),
            db: self.font_system.db().clone(),
        }
    }

    /// A worker's store: the template's faces, its own shaping caches. The
    /// database clone keeps face ids, so the math face carries over.
    pub fn pooled(seed: &FontSeed) -> FontStore {
        let math_face = find_math_face(&seed.db);
        FontStore {
            font_system: FontSystem::new_with_locale_and_db(seed.locale.clone(), seed.db.clone()),
            swash: SwashCache::new(),
            math_face,
        }
    }

    /// Every selectable family: the bundled two first, then system families,
    /// each group sorted by name.
    pub fn families(&self) -> Vec<String> {
        let bundled = [CODE_FAMILY.to_string(), BODY_FAMILY.to_string()];
        let mut system: Vec<String> = self
            .font_system
            .db()
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .filter(|name| !bundled.contains(name) && name != MATH_FAMILY)
            .collect();
        system.sort();
        system.dedup();
        let mut all = bundled.to_vec();
        all.extend(system);
        all
    }
}

impl Default for FontStore {
    fn default() -> FontStore {
        FontStore::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_families_present() {
        let store = FontStore::new();
        let families = store.families();
        assert!(families.contains(&BODY_FAMILY.to_string()));
        assert!(families.contains(&CODE_FAMILY.to_string()));
    }

    #[test]
    fn bundled_families_sort_before_system() {
        let families = FontStore::new().families();
        assert_eq!(families[0], CODE_FAMILY);
        assert_eq!(families[1], BODY_FAMILY);
    }

    #[test]
    fn a_pooled_store_sees_the_template_faces() {
        let template = FontStore::new();
        let pooled = FontStore::pooled(&template.seed());
        assert_eq!(pooled.families(), template.families());
        assert_eq!(pooled.font_system.locale(), template.font_system.locale());
    }

    #[test]
    fn math_face_registers_but_no_picker_offers_it() {
        let store = FontStore::new();
        let face = store
            .font_system
            .db()
            .face(store.math_face)
            .expect("math face id resolves");
        assert!(face.families.iter().any(|(name, _)| name == MATH_FAMILY));
        assert!(!store.families().contains(&MATH_FAMILY.to_string()));
        let pooled = FontStore::pooled(&store.seed());
        assert_eq!(pooled.math_face, store.math_face);
    }
}
