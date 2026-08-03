//! The font boundary.
//!
//! Layout asks a font questions through [`MathFont`] and nothing else: no
//! file access, no rasterization, no text shaping. All returned lengths are
//! in font design units; the engine scales them by the requested size over
//! [`MathFont::units_per_em`]. The one exception is
//! [`MathFont::measure_literal`], which measures text the host will render
//! itself in its own fallback face, in pixels at the given size.

/// A glyph index in the math font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphId(pub u16);

/// A glyph's ink extents in design units.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Bounds {
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
}

/// One entry of a glyph's size ladder for stretching.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Variant {
    pub glyph: GlyphId,
    /// Advance along the stretch axis, design units.
    pub advance: f32,
}

/// One piece of a glyph assembly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AssemblyPart {
    pub glyph: GlyphId,
    pub start_connector: f32,
    pub end_connector: f32,
    pub full_advance: f32,
    pub is_extender: bool,
}

/// An extensible glyph construction: end pieces plus repeatable extenders.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembly {
    pub parts: Vec<AssemblyPart>,
    pub italic_correction: f32,
}

/// A corner of a glyph for cut-in kerning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}

/// The OpenType MATH constants layout consumes, in design units except the
/// two percentages. Fields follow the spec's names.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MathConstants {
    pub script_percent_scale_down: f32,
    pub script_script_percent_scale_down: f32,
    pub delimited_sub_formula_min_height: f32,
    pub display_operator_min_height: f32,
    pub axis_height: f32,
    pub accent_base_height: f32,
    pub flattened_accent_base_height: f32,
    pub subscript_shift_down: f32,
    pub subscript_top_max: f32,
    pub subscript_baseline_drop_min: f32,
    pub superscript_shift_up: f32,
    pub superscript_shift_up_cramped: f32,
    pub superscript_bottom_min: f32,
    pub superscript_baseline_drop_max: f32,
    pub sub_superscript_gap_min: f32,
    pub superscript_bottom_max_with_subscript: f32,
    pub space_after_script: f32,
    pub upper_limit_gap_min: f32,
    pub upper_limit_baseline_rise_min: f32,
    pub lower_limit_gap_min: f32,
    pub lower_limit_baseline_drop_min: f32,
    pub stack_top_shift_up: f32,
    pub stack_top_display_style_shift_up: f32,
    pub stack_bottom_shift_down: f32,
    pub stack_bottom_display_style_shift_down: f32,
    pub stack_gap_min: f32,
    pub stack_display_style_gap_min: f32,
    pub fraction_numerator_shift_up: f32,
    pub fraction_numerator_display_style_shift_up: f32,
    pub fraction_denominator_shift_down: f32,
    pub fraction_denominator_display_style_shift_down: f32,
    pub fraction_numerator_gap_min: f32,
    pub fraction_num_display_style_gap_min: f32,
    pub fraction_rule_thickness: f32,
    pub fraction_denominator_gap_min: f32,
    pub fraction_denom_display_style_gap_min: f32,
    pub radical_vertical_gap: f32,
    pub radical_display_style_vertical_gap: f32,
    pub radical_rule_thickness: f32,
    pub radical_extra_ascender: f32,
    pub radical_kern_before_degree: f32,
    pub radical_kern_after_degree: f32,
    pub radical_degree_bottom_raise_percent: f32,
    pub min_connector_overlap: f32,
}

/// What layout needs from a math font. Lengths in design units.
pub trait MathFont {
    fn units_per_em(&self) -> f32;
    fn glyph(&self, c: char) -> Option<GlyphId>;
    fn advance(&self, glyph: GlyphId) -> f32;
    fn bounds(&self, glyph: GlyphId) -> Bounds;
    fn italic_correction(&self, glyph: GlyphId) -> f32;
    /// The accent attachment abscissa, when the font records one.
    fn top_accent(&self, glyph: GlyphId) -> Option<f32>;
    fn constants(&self) -> MathConstants;
    /// The size ladder for vertical stretching, smallest first.
    fn vertical_variants(&self, glyph: GlyphId) -> Vec<Variant>;
    /// The size ladder for horizontal stretching, smallest first.
    fn horizontal_variants(&self, glyph: GlyphId) -> Vec<Variant>;
    fn vertical_assembly(&self, glyph: GlyphId) -> Option<Assembly>;
    fn horizontal_assembly(&self, glyph: GlyphId) -> Option<Assembly>;
    /// Cut-in kern at a corner for an attachment at `height` above the
    /// baseline (negative below), design units.
    fn kern(&self, glyph: GlyphId, corner: Corner, height: f32) -> f32;
    /// Width of literal fallback text the host renders itself, in pixels at
    /// `size` pixels per em.
    fn measure_literal(&self, text: &str, size: f32) -> f32;
}

#[cfg(feature = "ttf")]
pub use ttf::TtfMathFont;

#[cfg(feature = "ttf")]
mod ttf {
    use super::*;

    /// A [`MathFont`] over any face carrying an OpenType MATH table, parsed
    /// with ttf-parser from bytes the caller keeps alive.
    pub struct TtfMathFont<'a> {
        face: ttf_parser::Face<'a>,
    }

    impl<'a> TtfMathFont<'a> {
        /// Returns None when the bytes do not parse or carry no MATH table.
        pub fn from_bytes(data: &'a [u8]) -> Option<Self> {
            let face = ttf_parser::Face::parse(data, 0).ok()?;
            face.tables().math?;
            Some(Self { face })
        }
    }

    impl TtfMathFont<'_> {
        fn math(&self) -> ttf_parser::math::Table<'_> {
            self.face.tables().math.expect("checked at construction")
        }

        fn construction(
            &self,
            glyph: GlyphId,
            vertical: bool,
        ) -> Option<ttf_parser::math::GlyphConstruction<'_>> {
            let variants = self.math().variants?;
            let table = if vertical {
                variants.vertical_constructions
            } else {
                variants.horizontal_constructions
            };
            table.get(ttf_parser::GlyphId(glyph.0))
        }

        fn ladder(&self, glyph: GlyphId, vertical: bool) -> Vec<Variant> {
            let Some(c) = self.construction(glyph, vertical) else {
                return Vec::new();
            };
            c.variants
                .into_iter()
                .map(|r| Variant {
                    glyph: GlyphId(r.variant_glyph.0),
                    advance: f32::from(r.advance_measurement),
                })
                .collect()
        }

        fn assembly(&self, glyph: GlyphId, vertical: bool) -> Option<Assembly> {
            let asm = self.construction(glyph, vertical)?.assembly?;
            let parts = asm
                .parts
                .into_iter()
                .map(|p| AssemblyPart {
                    glyph: GlyphId(p.glyph_id.0),
                    start_connector: f32::from(p.start_connector_length),
                    end_connector: f32::from(p.end_connector_length),
                    full_advance: f32::from(p.full_advance),
                    is_extender: p.part_flags.extender(),
                })
                .collect();
            Some(Assembly {
                parts,
                italic_correction: f32::from(asm.italics_correction.value),
            })
        }
    }

    impl MathFont for TtfMathFont<'_> {
        fn units_per_em(&self) -> f32 {
            f32::from(self.face.units_per_em())
        }

        fn glyph(&self, c: char) -> Option<GlyphId> {
            self.face.glyph_index(c).map(|g| GlyphId(g.0))
        }

        fn advance(&self, glyph: GlyphId) -> f32 {
            self.face
                .glyph_hor_advance(ttf_parser::GlyphId(glyph.0))
                .map(f32::from)
                .unwrap_or(0.0)
        }

        fn bounds(&self, glyph: GlyphId) -> Bounds {
            match self.face.glyph_bounding_box(ttf_parser::GlyphId(glyph.0)) {
                Some(r) => Bounds {
                    x_min: f32::from(r.x_min),
                    y_min: f32::from(r.y_min),
                    x_max: f32::from(r.x_max),
                    y_max: f32::from(r.y_max),
                },
                None => Bounds::default(),
            }
        }

        fn italic_correction(&self, glyph: GlyphId) -> f32 {
            self.math()
                .glyph_info
                .and_then(|gi| gi.italic_corrections)
                .and_then(|ic| ic.get(ttf_parser::GlyphId(glyph.0)))
                .map(|v| f32::from(v.value))
                .unwrap_or(0.0)
        }

        fn top_accent(&self, glyph: GlyphId) -> Option<f32> {
            self.math()
                .glyph_info
                .and_then(|gi| gi.top_accent_attachments)
                .and_then(|ta| ta.get(ttf_parser::GlyphId(glyph.0)))
                .map(|v| f32::from(v.value))
        }

        fn constants(&self) -> MathConstants {
            let Some(c) = self.math().constants else {
                return MathConstants::default();
            };
            let v = |m: ttf_parser::math::MathValue| f32::from(m.value);
            MathConstants {
                script_percent_scale_down: f32::from(c.script_percent_scale_down()),
                script_script_percent_scale_down: f32::from(c.script_script_percent_scale_down()),
                delimited_sub_formula_min_height: f32::from(c.delimited_sub_formula_min_height()),
                display_operator_min_height: f32::from(c.display_operator_min_height()),
                axis_height: v(c.axis_height()),
                accent_base_height: v(c.accent_base_height()),
                flattened_accent_base_height: v(c.flattened_accent_base_height()),
                subscript_shift_down: v(c.subscript_shift_down()),
                subscript_top_max: v(c.subscript_top_max()),
                subscript_baseline_drop_min: v(c.subscript_baseline_drop_min()),
                superscript_shift_up: v(c.superscript_shift_up()),
                superscript_shift_up_cramped: v(c.superscript_shift_up_cramped()),
                superscript_bottom_min: v(c.superscript_bottom_min()),
                superscript_baseline_drop_max: v(c.superscript_baseline_drop_max()),
                sub_superscript_gap_min: v(c.sub_superscript_gap_min()),
                superscript_bottom_max_with_subscript: v(c.superscript_bottom_max_with_subscript()),
                space_after_script: v(c.space_after_script()),
                upper_limit_gap_min: v(c.upper_limit_gap_min()),
                upper_limit_baseline_rise_min: v(c.upper_limit_baseline_rise_min()),
                lower_limit_gap_min: v(c.lower_limit_gap_min()),
                lower_limit_baseline_drop_min: v(c.lower_limit_baseline_drop_min()),
                stack_top_shift_up: v(c.stack_top_shift_up()),
                stack_top_display_style_shift_up: v(c.stack_top_display_style_shift_up()),
                stack_bottom_shift_down: v(c.stack_bottom_shift_down()),
                stack_bottom_display_style_shift_down: v(c.stack_bottom_display_style_shift_down()),
                stack_gap_min: v(c.stack_gap_min()),
                stack_display_style_gap_min: v(c.stack_display_style_gap_min()),
                fraction_numerator_shift_up: v(c.fraction_numerator_shift_up()),
                fraction_numerator_display_style_shift_up: v(
                    c.fraction_numerator_display_style_shift_up()
                ),
                fraction_denominator_shift_down: v(c.fraction_denominator_shift_down()),
                fraction_denominator_display_style_shift_down: v(
                    c.fraction_denominator_display_style_shift_down()
                ),
                fraction_numerator_gap_min: v(c.fraction_numerator_gap_min()),
                fraction_num_display_style_gap_min: v(c.fraction_num_display_style_gap_min()),
                fraction_rule_thickness: v(c.fraction_rule_thickness()),
                fraction_denominator_gap_min: v(c.fraction_denominator_gap_min()),
                fraction_denom_display_style_gap_min: v(c.fraction_denom_display_style_gap_min()),
                radical_vertical_gap: v(c.radical_vertical_gap()),
                radical_display_style_vertical_gap: v(c.radical_display_style_vertical_gap()),
                radical_rule_thickness: v(c.radical_rule_thickness()),
                radical_extra_ascender: v(c.radical_extra_ascender()),
                radical_kern_before_degree: v(c.radical_kern_before_degree()),
                radical_kern_after_degree: v(c.radical_kern_after_degree()),
                radical_degree_bottom_raise_percent: f32::from(
                    c.radical_degree_bottom_raise_percent(),
                ),
                min_connector_overlap: self
                    .math()
                    .variants
                    .map(|va| f32::from(va.min_connector_overlap))
                    .unwrap_or(0.0),
            }
        }

        fn vertical_variants(&self, glyph: GlyphId) -> Vec<Variant> {
            self.ladder(glyph, true)
        }

        fn horizontal_variants(&self, glyph: GlyphId) -> Vec<Variant> {
            self.ladder(glyph, false)
        }

        fn vertical_assembly(&self, glyph: GlyphId) -> Option<Assembly> {
            self.assembly(glyph, true)
        }

        fn horizontal_assembly(&self, glyph: GlyphId) -> Option<Assembly> {
            self.assembly(glyph, false)
        }

        fn kern(&self, glyph: GlyphId, corner: Corner, height: f32) -> f32 {
            let Some(info) = self
                .math()
                .glyph_info
                .and_then(|gi| gi.kern_infos)
                .and_then(|ki| ki.get(ttf_parser::GlyphId(glyph.0)))
            else {
                return 0.0;
            };
            let table = match corner {
                Corner::TopRight => info.top_right,
                Corner::TopLeft => info.top_left,
                Corner::BottomRight => info.bottom_right,
                Corner::BottomLeft => info.bottom_left,
            };
            let Some(table) = table else { return 0.0 };
            // The spec keeps one more kern value than correction heights;
            // value i applies below height i, the last above every height.
            let count = table.count();
            let mut idx = 0;
            while idx < count {
                match table.height(idx) {
                    Some(h) if height > f32::from(h.value) => idx += 1,
                    _ => break,
                }
            }
            table.kern(idx).map(|v| f32::from(v.value)).unwrap_or(0.0)
        }

        fn measure_literal(&self, text: &str, size: f32) -> f32 {
            let upem = self.units_per_em();
            let units: f32 = text
                .chars()
                .map(|ch| {
                    self.glyph(ch)
                        .map(|g| self.advance(g))
                        .unwrap_or(upem * 0.5)
                })
                .sum();
            units * size / upem
        }
    }
}

#[cfg(all(test, feature = "ttf"))]
mod tests {
    use super::*;

    const STIX: &[u8] = include_bytes!("../fixtures/STIXTwoMath-Regular.otf");

    fn font() -> TtfMathFont<'static> {
        TtfMathFont::from_bytes(STIX).expect("fixture parses and has MATH")
    }

    #[test]
    fn fixture_parses_and_plain_bytes_do_not() {
        assert!(TtfMathFont::from_bytes(STIX).is_some());
        assert!(TtfMathFont::from_bytes(b"not a font").is_none());
    }

    #[test]
    fn units_per_em_is_1000() {
        assert_eq!(font().units_per_em(), 1000.0);
    }

    #[test]
    fn glyph_lookup_covers_ascii_symbols_and_math_alphanumerics() {
        let f = font();
        assert!(f.glyph('x').is_some());
        assert!(f.glyph('=').is_some());
        assert!(f.glyph('\u{2211}').is_some()); // n-ary summation
        assert!(f.glyph('\u{222B}').is_some()); // integral
        assert!(f.glyph('\u{1D465}').is_some()); // mathematical italic small x
        assert!(f.glyph('\u{1D53D}').is_some()); // double-struck capital F
        assert!(f.glyph('\u{10FFFD}').is_none());
    }

    #[test]
    fn advances_and_bounds_are_positive_for_ink_glyphs() {
        let f = font();
        let x = f.glyph('x').unwrap();
        assert!(f.advance(x) > 0.0);
        let b = f.bounds(x);
        assert!(b.x_max > b.x_min);
        assert!(b.y_max > b.y_min);
        assert!(b.y_max > 0.0, "x-height ink sits above the baseline");
    }

    #[test]
    fn constants_match_stix_exactly() {
        let c = font().constants();
        assert_eq!(c.script_percent_scale_down, 70.0);
        assert_eq!(c.script_script_percent_scale_down, 55.0);
        assert_eq!(c.axis_height, 258.0);
        assert_eq!(c.fraction_rule_thickness, 68.0);
        assert_eq!(c.radical_rule_thickness, 68.0);
        assert_eq!(c.superscript_shift_up, 360.0);
        assert_eq!(c.subscript_shift_down, 210.0);
        assert_eq!(c.sub_superscript_gap_min, 150.0);
        assert_eq!(c.display_operator_min_height, 1800.0);
        assert_eq!(c.min_connector_overlap, 100.0);
        assert!(c.superscript_shift_up_cramped < c.superscript_shift_up);
    }

    #[test]
    fn integral_carries_a_large_italic_correction() {
        let f = font();
        let int = f.glyph('\u{222B}').unwrap();
        assert!(f.italic_correction(int) > 100.0);
        let x = f.glyph('x').unwrap();
        assert!(f.italic_correction(x) < f.italic_correction(int));
    }

    #[test]
    fn top_accent_attachment_exists_for_letters() {
        let f = font();
        let x = f.glyph('\u{1D465}').unwrap();
        let a = f.top_accent(x).expect("math italic x records attachment");
        assert!(a > 0.0 && a < f.advance(x) + 200.0);
    }

    #[test]
    fn paren_stretches_through_ladder_then_assembly() {
        let f = font();
        let paren = f.glyph('(').unwrap();
        let ladder = f.vertical_variants(paren);
        assert!(ladder.len() >= 3, "STIX carries several paren sizes");
        for pair in ladder.windows(2) {
            assert!(
                pair[0].advance <= pair[1].advance,
                "ladder sorted ascending"
            );
        }
        let asm = f.vertical_assembly(paren).expect("paren assembles");
        assert!(asm.parts.len() >= 2);
        assert!(asm.parts.iter().any(|p| p.is_extender));
        assert!(asm.parts.iter().all(|p| p.full_advance > 0.0));
    }

    #[test]
    fn horizontal_stretch_exists_for_wide_accents() {
        let f = font();
        let tilde = f.glyph('\u{0303}').or_else(|| f.glyph('\u{02DC}'));
        let tilde = tilde.expect("combining or spacing tilde present");
        assert!(
            !f.horizontal_variants(tilde).is_empty() || f.horizontal_assembly(tilde).is_some(),
            "a wide-tilde stretch path exists"
        );
    }

    #[test]
    fn cut_in_kerns_answer_finite_values() {
        let f = font();
        // Superscript attachment high on the italic f, a strongly kerned shape.
        let letter = f.glyph('\u{1D453}').unwrap();
        let k = f.kern(letter, Corner::TopRight, 400.0);
        assert!(k.is_finite());
        let unkerned = f.glyph('=').unwrap();
        assert_eq!(f.kern(unkerned, Corner::TopRight, 400.0), 0.0);
    }

    #[test]
    fn literal_measure_grows_with_text() {
        let f = font();
        let short = f.measure_literal("\\a", 20.0);
        let long = f.measure_literal("\\alphabet", 20.0);
        assert!(short > 0.0);
        assert!(long > short);
    }
}
