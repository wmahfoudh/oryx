//! Embedded fonts and the cosmic-text font system.
//!
//! DejaVu Sans and Courier Prime ship inside the binary; system fonts are
//! loaded only as glyph fallback and as choices in the settings dialog.

use cosmic_text::{FontSystem, SwashCache};

pub const BODY_FAMILY: &str = "DejaVu Sans";
pub const CODE_FAMILY: &str = "Courier Prime";

pub struct FontStore {
    pub font_system: FontSystem,
    /// Glyph raster cache shared by every paint pass.
    pub swash: SwashCache,
}

static EMBEDDED: &[&[u8]] = &[
    include_bytes!("../../assets/fonts/DejaVuSans.ttf"),
    include_bytes!("../../assets/fonts/DejaVuSans-Bold.ttf"),
    include_bytes!("../../assets/fonts/DejaVuSans-Oblique.ttf"),
    include_bytes!("../../assets/fonts/DejaVuSans-BoldOblique.ttf"),
    include_bytes!("../../assets/fonts/CourierPrime_Regular.ttf"),
    include_bytes!("../../assets/fonts/CourierPrime_Bold.ttf"),
    include_bytes!("../../assets/fonts/CourierPrime_Italic.ttf"),
    include_bytes!("../../assets/fonts/CourierPrime_BoldItalic.ttf"),
];

impl FontStore {
    pub fn new() -> FontStore {
        let mut font_system = FontSystem::new();
        for bytes in EMBEDDED {
            font_system.db_mut().load_font_data(bytes.to_vec());
        }
        FontStore {
            font_system,
            swash: SwashCache::new(),
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
            .filter(|name| !bundled.contains(name))
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
}
