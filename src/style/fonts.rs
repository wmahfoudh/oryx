//! Embedded fonts and the cosmic-text font system.
//!
//! DejaVu Sans, Courier Prime and the designated script faces (Amiri
//! for Arabic, David Libre for Hebrew) ship inside the binary; system
//! fonts are loaded only as glyph fallback and as choices in the
//! settings dialog.

use std::collections::HashMap;

use cosmic_text::{FontSystem, SwashCache, Weight};

pub const BODY_FAMILY: &str = "DejaVu Sans";
pub const CODE_FAMILY: &str = "Courier Prime";
/// The equation face. Metrics-bound to the math engine, so no picker
/// offers it; `MATH_FONT` also feeds noad's OpenType MATH reader directly.
pub const MATH_FAMILY: &str = "STIX Two Math";
/// The designated script faces. Arabic and Hebrew runs route to them
/// regardless of the body family, because DejaVu covers both scripts
/// itself and fallback would never fire; see `script_segments`.
pub const ARABIC_FAMILY: &str = "Amiri";
pub const HEBREW_FAMILY: &str = "David Libre";

pub struct FontStore {
    pub font_system: FontSystem,
    /// Glyph raster cache shared by every paint pass.
    pub swash: SwashCache,
    /// The registered math face, the raster key for math glyph runs.
    pub math_face: cosmic_text::fontdb::ID,
    /// The weights each family has a face for, read once per family.
    family_weights: HashMap<String, Vec<u16>>,
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
    include_bytes!("../../assets/fonts/Amiri-Regular.ttf"),
    include_bytes!("../../assets/fonts/Amiri-Bold.ttf"),
    include_bytes!("../../assets/fonts/DavidLibre-Regular.ttf"),
    include_bytes!("../../assets/fonts/DavidLibre-Bold.ttf"),
];

/// The nearest of `available` (sorted) to `requested`, the requested
/// weight itself when present. A bold request (600 and up) prefers the
/// nearest heavier face, then the nearest lighter; any other request
/// prefers the nearest lighter, then the nearest heavier. None for an
/// empty list.
fn closest_weight(available: &[u16], requested: u16) -> Option<u16> {
    if available.binary_search(&requested).is_ok() {
        return Some(requested);
    }
    let heavier = available.iter().copied().find(|&w| w > requested);
    let lighter = available.iter().copied().rev().find(|&w| w < requested);
    if requested >= 600 {
        heavier.or(lighter)
    } else {
        lighter.or(heavier)
    }
}

/// Strong-script class of one character for face routing. Anything
/// non-alphabetic is neutral and follows its neighboring strong run.
#[derive(Clone, Copy, PartialEq)]
enum ScriptClass {
    Arabic,
    Hebrew,
    Other,
    Neutral,
}

fn script_class(c: char) -> ScriptClass {
    match c as u32 {
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => ScriptClass::Hebrew,
        0x0600..=0x06FF
        | 0x0750..=0x077F
        | 0x0870..=0x089F
        | 0x08A0..=0x08FF
        | 0xFB50..=0xFDFF
        | 0xFE70..=0xFEFF => ScriptClass::Arabic,
        _ if c.is_alphabetic() => ScriptClass::Other,
        _ => ScriptClass::Neutral,
    }
}

/// The bidi strength of one character: `Some(true)` for Arabic and
/// Hebrew, `Some(false)` for any other alphabetic, `None` for neutrals.
/// The first non-`None` answer over a text decides its base direction.
pub(crate) fn strong_rtl(c: char) -> Option<bool> {
    match script_class(c) {
        ScriptClass::Arabic | ScriptClass::Hebrew => Some(true),
        ScriptClass::Other => Some(false),
        ScriptClass::Neutral => None,
    }
}

fn family_for(class: ScriptClass) -> Option<&'static str> {
    match class {
        ScriptClass::Arabic => Some(ARABIC_FAMILY),
        ScriptClass::Hebrew => Some(HEBREW_FAMILY),
        _ => None,
    }
}

/// Splits text into byte ranges by strong script, each with the
/// designated family that renders it, `None` where the span family
/// keeps the run. Neutral characters (spaces, digits, punctuation)
/// follow the preceding strong run and leading neutrals join the
/// first, so a routed run carries its own spaces and mirrored
/// punctuation. Text without a character at U+0590 or above returns
/// whole on the fast path.
pub fn script_segments(text: &str) -> Vec<(std::ops::Range<usize>, Option<&'static str>)> {
    if !text.chars().any(|c| c >= '\u{0590}') {
        return vec![(0..text.len(), None)];
    }
    let mut out: Vec<(std::ops::Range<usize>, Option<&'static str>)> = Vec::new();
    let mut open: Option<ScriptClass> = None;
    let mut start = 0usize;
    for (i, c) in text.char_indices() {
        let class = script_class(c);
        if class == ScriptClass::Neutral {
            continue;
        }
        match open {
            None => open = Some(class),
            Some(current) if current == class => {}
            Some(current) => {
                out.push((start..i, family_for(current)));
                start = i;
                open = Some(class);
            }
        }
    }
    let last = open.map(family_for).unwrap_or(None);
    out.push((start..text.len(), last));
    out
}

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
            family_weights: HashMap::new(),
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
            family_weights: HashMap::new(),
        }
    }

    /// The weight to ask `family` for when `requested` is wanted: the
    /// requested weight when the family has a face at it, else the
    /// family's nearest (heavier first for a bold request, lighter first
    /// otherwise, as a browser does). The shaper's fallback keeps a family
    /// only for a face at the exact weight asked, so a bold heading in a
    /// family with one Regular face would otherwise leave the family for
    /// another font's Bold. A family with no face at all is answered as
    /// asked and the fallback chooses.
    pub fn weight_for(&mut self, family: &str, requested: Weight) -> Weight {
        if !self.family_weights.contains_key(family) {
            let mut weights: Vec<u16> = self
                .font_system
                .db()
                .faces()
                .filter(|face| face.families.iter().any(|(name, _)| name == family))
                .map(|face| face.weight.0)
                .collect();
            weights.sort_unstable();
            weights.dedup();
            self.family_weights.insert(family.to_string(), weights);
        }
        closest_weight(&self.family_weights[family], requested.0)
            .map(Weight)
            .unwrap_or(requested)
    }

    /// Every selectable family: the bundled faces first, defaults then
    /// the script faces, then system families sorted by name.
    pub fn families(&self) -> Vec<String> {
        let bundled = [
            CODE_FAMILY.to_string(),
            BODY_FAMILY.to_string(),
            ARABIC_FAMILY.to_string(),
            HEBREW_FAMILY.to_string(),
        ];
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
        assert!(families.contains(&ARABIC_FAMILY.to_string()));
        assert!(families.contains(&HEBREW_FAMILY.to_string()));
    }

    #[test]
    fn bundled_families_sort_before_system() {
        let families = FontStore::new().families();
        assert_eq!(families[0], CODE_FAMILY);
        assert_eq!(families[1], BODY_FAMILY);
        assert_eq!(families[2], ARABIC_FAMILY);
        assert_eq!(families[3], HEBREW_FAMILY);
    }

    /// Segments mapped back onto the text they cover, for readable
    /// assertions.
    fn segs(text: &str) -> Vec<(&str, Option<&'static str>)> {
        script_segments(text)
            .into_iter()
            .map(|(range, family)| (&text[range], family))
            .collect()
    }

    #[test]
    fn latin_text_routes_nowhere() {
        assert_eq!(segs("plain body text"), vec![("plain body text", None)]);
    }

    #[test]
    fn all_neutral_text_keeps_the_span_family() {
        assert_eq!(segs("12 + 34"), vec![("12 + 34", None)]);
    }

    #[test]
    fn arabic_words_and_their_spaces_stay_one_segment() {
        assert_eq!(
            segs("اعلم أن فن التاريخ"),
            vec![("اعلم أن فن التاريخ", Some(ARABIC_FAMILY))]
        );
    }

    #[test]
    fn hebrew_routes_to_its_face() {
        assert_eq!(segs("שלום עולם"), vec![("שלום עולם", Some(HEBREW_FAMILY))]);
    }

    #[test]
    fn scripts_split_and_neutrals_follow_the_preceding_run() {
        assert_eq!(
            segs("abc سلام def"),
            vec![("abc ", None), ("سلام ", Some(ARABIC_FAMILY)), ("def", None),]
        );
    }

    #[test]
    fn leading_neutrals_join_the_first_strong_run() {
        assert_eq!(segs("(سلام)"), vec![("(سلام)", Some(ARABIC_FAMILY))]);
        assert_eq!(segs("«שלום»"), vec![("«שלום»", Some(HEBREW_FAMILY))]);
    }

    #[test]
    fn digits_and_marks_follow_their_run() {
        assert_eq!(
            segs("سنة 808 هـ"),
            vec![("سنة 808 هـ", Some(ARABIC_FAMILY))]
        );
        assert_eq!(segs("וְאָהַבְתָּ"), vec![("וְאָהַבְתָּ", Some(HEBREW_FAMILY))]);
    }

    #[test]
    fn presentation_forms_route_with_their_scripts() {
        assert_eq!(segs("\u{FB50}"), vec![("\u{FB50}", Some(ARABIC_FAMILY))]);
        assert_eq!(segs("\u{FEFB}"), vec![("\u{FEFB}", Some(ARABIC_FAMILY))]);
        assert_eq!(segs("\u{FB1D}"), vec![("\u{FB1D}", Some(HEBREW_FAMILY))]);
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

    #[test]
    fn weight_for_keeps_a_weight_the_family_has() {
        let mut store = FontStore::new();
        assert_eq!(store.weight_for(BODY_FAMILY, Weight::BOLD), Weight::BOLD);
        assert_eq!(
            store.weight_for(BODY_FAMILY, Weight::NORMAL),
            Weight::NORMAL
        );
    }

    #[test]
    fn weight_for_answers_the_closest_weight_of_a_one_face_family() {
        let mut store = FontStore::new();
        assert_eq!(store.weight_for(MATH_FAMILY, Weight::BOLD), Weight::NORMAL);
        assert_eq!(
            store.weight_for(MATH_FAMILY, Weight::NORMAL),
            Weight::NORMAL
        );
    }

    #[test]
    fn weight_for_leaves_an_unknown_family_to_the_fallback() {
        let mut store = FontStore::new();
        assert_eq!(
            store.weight_for("No Such Family", Weight::BOLD),
            Weight::BOLD
        );
    }

    /// The family of the face the shaper picked for a word asked in
    /// `family` at `weight`.
    fn shaped_family(store: &mut FontStore, family: &str, weight: Weight) -> String {
        use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping};
        let mut buffer = Buffer::new(&mut store.font_system, Metrics::new(16.0, 20.0));
        buffer.set_size(&mut store.font_system, None, None);
        let attrs = Attrs::new().family(Family::Name(family)).weight(weight);
        buffer.set_text(
            &mut store.font_system,
            "bold",
            &attrs,
            Shaping::Advanced,
            None,
        );
        let glyph = buffer.layout_runs().next().unwrap().glyphs[0].clone();
        let face = store.font_system.db().face(glyph.font_id).unwrap();
        face.families[0].0.clone()
    }

    /// The shaper keeps a family only when a face has the exact weight
    /// asked, so a bold request on a one-face family leaves it; the
    /// weight `weight_for` answers keeps it. A shaper upgrade that
    /// changes this makes `weight_for` unnecessary.
    #[test]
    fn a_bold_request_on_a_one_face_family_leaves_it_unless_the_weight_is_resolved() {
        let mut store = FontStore::new();
        assert_ne!(
            shaped_family(&mut store, MATH_FAMILY, Weight::BOLD),
            MATH_FAMILY
        );
        let weight = store.weight_for(MATH_FAMILY, Weight::BOLD);
        assert_eq!(shaped_family(&mut store, MATH_FAMILY, weight), MATH_FAMILY);
    }
}
