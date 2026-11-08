//! Appendix G layout: a math list to flat geometry.
//!
//! One recursive walk in the current style, emitting positioned glyphs,
//! rules, and literal boxes relative to the expression's baseline origin.
//! Coordinates follow the host convention: x grows right, y grows down,
//! the baseline at y zero, so a superscript sits at negative y. `ascent`
//! and `descent` are positive extents above and below the baseline.
//!
//! `size` is the em size of text style in pixels; display and text render
//! at it, script and scriptscript at the font's percentage scale-downs.

use crate::font::MathFont;
use crate::mlist::{Atom, AtomClass, ColAlign, Field, MathList, Noad, TableGaps};
use std::ops::Range;

pub use crate::mlist::MathStyle;

/// A glyph at its position, in the math font at `size` pixels per em.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGlyph {
    pub glyph: crate::font::GlyphId,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    /// The character the glyph renders, when one exists: base glyphs and
    /// size variants keep their character, assembly pieces have none.
    pub ch: Option<char>,
    /// Byte range of the TeX source this glyph renders.
    pub source: Range<usize>,
    /// The color a `\color` or `\textcolor` gave it; `None` is the host's
    /// ink.
    pub color: Option<[u8; 3]>,
}

/// A filled rectangle: fraction bars, radical strokes.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The color a `\color` or `\textcolor` gave it; `None` is the host's
    /// ink.
    pub color: Option<[u8; 3]>,
}

/// A box reserved for literal TeX the host renders in its own face.
#[derive(Debug, Clone, PartialEq)]
pub struct Literal {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub width: f32,
    pub source: Range<usize>,
}

/// The laid-out expression.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MathLayout {
    pub glyphs: Vec<PositionedGlyph>,
    pub rules: Vec<Rule>,
    pub literals: Vec<Literal>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
}

/// Parses and lays out a TeX math string.
pub fn layout(tex: &str, style: MathStyle, size: f32, font: &dyn MathFont) -> MathLayout {
    let list = crate::parse::parse(tex);
    Ctx { font, base: size }.hlist(&list, style, false)
}

/// TeX's inter-atom spacing matrix, rows the left class, columns the right,
/// in the class order of [`AtomClass`]. Magnitudes are eighteenths of an em:
/// 3 thin, 4 medium, 5 thick. Negative entries are the TeXbook's
/// parenthesized ones, omitted in script styles. Pairs demotion makes
/// impossible hold zero.
#[rustfmt::skip]
const SPACING: [[i8; 8]; 8] = [
    // Ord Op  Bin Rel Open Close Punct Inner
    [0,  3, -4, -5,  0,  0,  0, -3], // Ord
    [3,  3,  0, -5,  0,  0,  0, -3], // Op
    [-4, -4,  0,  0, -4,  0,  0, -4], // Bin
    [-5, -5,  0,  0, -5,  0,  0, -5], // Rel
    [0,  0,  0,  0,  0,  0,  0,  0], // Open
    [0,  3, -4, -5,  0,  0,  0, -3], // Close
    [-3, -3,  0, -3, -3, -3, -3, -3], // Punct
    [-3,  3, -4, -5, -3,  0, -3, -3], // Inner
];

/// The classic italic remapping: letters render from the Mathematical
/// Alphanumeric block, the way TeX sets variables. Latin h is the one hole
/// in the block, U+210E Planck constant. Uppercase Greek stays upright;
/// the variant Greek letters and the partial differential sit outside the
/// α..ω run and map to their own italic codepoints.
fn math_italic(c: char) -> char {
    let mapped = match c {
        'h' => 0x210E,
        'a'..='z' => 0x1D44E + (c as u32 - 'a' as u32),
        'A'..='Z' => 0x1D434 + (c as u32 - 'A' as u32),
        '\u{03B1}'..='\u{03C9}' => 0x1D6FC + (c as u32 - 0x03B1),
        '\u{03D1}' => 0x1D717,
        '\u{03D5}' => 0x1D719,
        '\u{03D6}' => 0x1D71B,
        '\u{03F0}' => 0x1D718,
        '\u{03F1}' => 0x1D71A,
        '\u{03F5}' => 0x1D716,
        '\u{2202}' => 0x1D715,
        _ => c as u32,
    };
    char::from_u32(mapped).unwrap_or(c)
}

/// Plain TeX's `\strutbox` (8.5 pt over and 3.5 pt under the baseline at
/// 10 pt), in ems of the part's style: the least height and depth of a
/// `\cfrac` part, so that nested continued fractions keep even rows.
const STRUT_HEIGHT_EM: f32 = 0.85;
const STRUT_DEPTH_EM: f32 = 0.35;

/// The heights of TeX's four sized delimiters, `\big` to `\Bigg`, in ems
/// of the text size: KaTeX's `sizeToMaxHeight`.
const SIZED_DELIMITER_HEIGHTS_EM: [f32; 4] = [1.2, 1.8, 2.4, 3.0];
/// TeX lets a delimiter fall short of the height it is asked for by a
/// tenth (`\delimiterfactor` 901) or by half an em of the text size
/// (`\delimitershortfall` 5pt at 10pt), whichever is the smaller gap.
const DELIMITER_FACTOR: f32 = 0.901;
const DELIMITER_SHORTFALL_EM: f32 = 0.5;
/// The gap between a formula and the brace of `\overbrace` or
/// `\underbrace`, in ems of the current style.
const BRACE_GAP_EM: f32 = 0.1;
/// A long arrow (`\xrightarrow`) reaches this far past its texts on each
/// side, and is at least this long, in ems of the current style.
const ARROW_PAD_EM: f32 = 0.4;
const ARROW_MIN_EM: f32 = 1.2;

struct Ctx<'a> {
    font: &'a dyn MathFont,
    /// Text-style em size in pixels.
    base: f32,
}

/// An atom or list laid out relative to its own baseline origin, plus the
/// italic correction of its last nucleus glyph for script attachment.
struct LaidBox {
    geom: MathLayout,
    italic: f32,
}

/// A glyph stretched horizontally: its pieces with their x offsets from
/// the left edge, the character when one glyph renders it, and the width.
struct Spanned {
    glyphs: Vec<(crate::font::GlyphId, f32)>,
    ch: Option<char>,
    width: f32,
}

/// A glyph stretched along the vertical axis: one ladder pick or the parts
/// of an assembly, in stack coordinates with the ink bottom at zero.
struct Stretched {
    glyphs: Vec<(crate::font::GlyphId, f32)>,
    /// The stretched character for a base or ladder pick; assembly pieces
    /// render no character of their own.
    ch: Option<char>,
    width: f32,
    height: f32,
}

impl Ctx<'_> {
    fn style_scale(&self, style: MathStyle) -> f32 {
        let c = self.font.constants();
        match style {
            MathStyle::Display | MathStyle::Text => 1.0,
            MathStyle::Script => c.script_percent_scale_down / 100.0,
            MathStyle::ScriptScript => c.script_script_percent_scale_down / 100.0,
        }
    }

    /// Em size at the style, pixels.
    fn em(&self, style: MathStyle) -> f32 {
        self.base * self.style_scale(style)
    }

    /// Design units to pixels at the style.
    fn unit(&self, style: MathStyle) -> f32 {
        self.em(style) / self.font.units_per_em()
    }

    fn script_style(style: MathStyle) -> MathStyle {
        match style {
            MathStyle::Display | MathStyle::Text => MathStyle::Script,
            MathStyle::Script | MathStyle::ScriptScript => MathStyle::ScriptScript,
        }
    }

    fn space(&self, left: AtomClass, right: AtomClass, style: MathStyle) -> f32 {
        let entry = SPACING[left as usize][right as usize];
        let script = matches!(style, MathStyle::Script | MathStyle::ScriptScript);
        if entry == 0 || (entry < 0 && script) {
            return 0.0;
        }
        f32::from(entry.abs()) / 18.0 * self.em(style)
    }

    fn hlist(&self, list: &MathList, style: MathStyle, cramped: bool) -> MathLayout {
        let mut out = MathLayout::default();
        let mut cursor = 0.0;
        let mut prev: Option<AtomClass> = None;
        let mut style = style;
        let mut cramped = cramped;
        // `\color`: the glyphs and rules laid after it in this list take the
        // color, from the next color noad or to the end, an inner color
        // (already set) winning.
        let mut tint: Option<([u8; 3], usize, usize)> = None;
        for noad in &list.0 {
            let atom = match noad {
                Noad::Atom(atom) => atom,
                // TeX's style noad: the rest of the list in the new style,
                // uncramped, the spacing before the next atom read in it.
                Noad::Style(switched) => {
                    style = *switched;
                    cramped = false;
                    continue;
                }
                Noad::Color(color) => {
                    if let Some((c, g, r)) = tint.take() {
                        paint(&mut out, g, r, c);
                    }
                    tint = Some((*color, out.glyphs.len(), out.rules.len()));
                    continue;
                }
            };
            // An explicit kern is TeX's kern item, not an atom: it moves
            // the pen and breaks the adjacency the spacing matrix reads.
            if let Field::Kern(ems) = atom.nucleus {
                cursor += ems * self.em(style);
                prev = None;
                if atom.sup.is_none() && atom.sub.is_none() {
                    continue;
                }
                // A script on a kern (`~^2`, `\,^2`) sits on an empty nucleus
                // after the gap, TeX's `\nobreakspace{}^2`.
            }
            if let Some(p) = prev {
                cursor += self.space(p, atom.class, style);
            }
            let laid = self.atom_box(atom, style, cramped);
            let width = laid.geom.width;
            merge(&mut out, laid.geom, cursor, 0.0);
            cursor += width;
            prev = Some(atom.class);
        }
        if let Some((c, g, r)) = tint {
            paint(&mut out, g, r, c);
        }
        out.width = cursor;
        out
    }

    fn nucleus_box(&self, atom: &Atom, style: MathStyle, cramped: bool) -> LaidBox {
        match &atom.nucleus {
            Field::Symbol(c) => {
                let mapped = math_italic(*c);
                let Some(glyph) = self.font.glyph(mapped) else {
                    // A codepoint the font cannot draw degrades to a literal
                    // box the host renders, same as an unknown command.
                    return self.literal_box(&c.to_string(), atom, style);
                };
                if atom.class == AtomClass::Op {
                    return self.operator_box(atom, glyph, mapped, style);
                }
                let u = self.unit(style);
                let bounds = self.font.bounds(glyph);
                let geom = MathLayout {
                    glyphs: vec![PositionedGlyph {
                        glyph,
                        x: 0.0,
                        y: 0.0,
                        size: self.em(style),
                        ch: Some(mapped),
                        source: atom.nucleus_span.clone(),
                        color: None,
                    }],
                    rules: Vec::new(),
                    literals: Vec::new(),
                    width: self.font.advance(glyph) * u,
                    ascent: (bounds.y_max * u).max(0.0),
                    descent: (-bounds.y_min * u).max(0.0),
                };
                LaidBox {
                    geom,
                    italic: self.font.italic_correction(glyph) * u,
                }
            }
            Field::List(inner) => LaidBox {
                geom: self.hlist(inner, style, cramped),
                italic: 0.0,
            },
            Field::Literal(text) => self.literal_box(text, atom, style),
            Field::Text(text) => self.text_box(text, atom, style),
            Field::Sized { ch, size } => self.sized_box(atom, *ch, *size, style),
            Field::Line { base, under } => self.line_box(base, *under, style, cramped),
            Field::Brace { base, under } => self.brace_box(atom, base, *under, style, cramped),
            Field::Arrow { ch, above, below } => {
                self.arrow_box(atom, *ch, above, below, style, cramped)
            }
            Field::Phantom {
                body,
                width,
                height,
            } => self.phantom_box(body, *width, *height, style, cramped),
            Field::Colored { color, body } => {
                // One atom keeps its own box, its italic correction with it,
                // so that a colored operator's side scripts fall as the
                // operator's would.
                let mut laid = match body.0.as_slice() {
                    [Noad::Atom(a)] if !matches!(a.nucleus, Field::Kern(_)) => {
                        self.atom_box(a, style, cramped)
                    }
                    _ => LaidBox {
                        geom: self.hlist(body, style, cramped),
                        italic: 0.0,
                    },
                };
                paint(&mut laid.geom, 0, 0, *color);
                laid
            }
            Field::Negated { base } => self.negated_box(atom, base, style, cramped),
            Field::Accent {
                accent,
                stretch,
                base,
            } => self.accent_box(atom, *accent, *stretch, base, style),
            // Kerns are consumed by `hlist` before boxes are asked for.
            Field::Kern(_) => LaidBox {
                geom: MathLayout::default(),
                italic: 0.0,
            },
            Field::Table {
                rows,
                align,
                gaps,
                small,
            } => self.table_box(rows, align, *gaps, *small, style),
            Field::Fraction {
                numerator,
                denominator,
                bar,
                style: forced,
                continued,
            } => self.fraction_box(
                numerator,
                denominator,
                *bar,
                *forced,
                *continued,
                style,
                cramped,
            ),
            Field::Radical { radicand, degree } => self.radical_box(atom, radicand, degree, style),
            Field::LeftRight { open, close, body } => {
                self.left_right_box(atom, *open, *close, body, style, cramped)
            }
            Field::Empty => LaidBox {
                geom: MathLayout::default(),
                italic: 0.0,
            },
        }
    }

    /// Pushes a glyph at a position and widens the box's vertical extents
    /// by its ink, the bookkeeping `merge` does for whole boxes.
    #[allow(clippy::too_many_arguments)]
    fn push_glyph(
        &self,
        geom: &mut MathLayout,
        glyph: crate::font::GlyphId,
        x: f32,
        y: f32,
        style: MathStyle,
        ch: Option<char>,
        source: &Range<usize>,
    ) {
        let u = self.unit(style);
        let b = self.font.bounds(glyph);
        geom.glyphs.push(PositionedGlyph {
            glyph,
            x,
            y,
            size: self.em(style),
            ch,
            source: source.clone(),
            color: None,
        });
        geom.ascent = geom.ascent.max(-(y - b.y_max * u));
        geom.descent = geom.descent.max(y - b.y_min * u);
    }

    /// A glyph stretched vertically to cover `target`: the base, then the
    /// font's size ladder, then a glyph assembly from extenders. Offsets
    /// are stack coordinates: ink bottom at zero, growing upward.
    fn stretch_vertical(&self, ch: char, target: f32, style: MathStyle) -> Option<Stretched> {
        let u = self.unit(style);
        let base = self.font.glyph(ch)?;
        let ink = |g: crate::font::GlyphId| {
            let b = self.font.bounds(g);
            ((b.y_max - b.y_min) * u, b.y_min * u)
        };
        let (mut h, mut y_min) = ink(base);
        let mut chosen = base;
        if h < target {
            for v in self.font.vertical_variants(base) {
                let (vh, vy) = ink(v.glyph);
                chosen = v.glyph;
                h = vh;
                y_min = vy;
                if vh >= target {
                    break;
                }
            }
        }
        let assembly = self.font.vertical_assembly(base);
        if h >= target || assembly.is_none() {
            return Some(Stretched {
                glyphs: vec![(chosen, y_min)],
                ch: Some(ch),
                width: self.font.advance(chosen) * u,
                height: h,
            });
        }
        let asm = assembly.expect("checked");
        let (parts, overlap) = self.assembly_parts(&asm, target, u);
        let mut glyphs = Vec::new();
        let mut width = 0.0f32;
        let mut cursor = 0.0;
        for p in &parts {
            let b = self.font.bounds(p.glyph);
            glyphs.push((p.glyph, -cursor + b.y_min * u));
            width = width.max(self.font.advance(p.glyph) * u);
            cursor += p.full_advance * u - overlap;
        }
        Some(Stretched {
            glyphs,
            ch: None,
            width,
            height: cursor + overlap,
        })
    }

    /// The pieces of an assembly that reach `target`, with the overlap at
    /// every joint: the fewest repeats of the extenders whose length at
    /// the font's minimum overlap reaches the target, then the pieces
    /// overlapping as far as their connectors allow, the same at every
    /// joint, so that the assembly lands on the target and not a whole
    /// extender beyond it (the MATH table's rule). Past 64 repeats the
    /// assembly stays short.
    fn assembly_parts<'a>(
        &self,
        asm: &'a crate::font::Assembly,
        target: f32,
        u: f32,
    ) -> (Vec<&'a crate::font::AssemblyPart>, f32) {
        let min_overlap = self.font.constants().min_connector_overlap * u;
        for reps in 1..=64usize {
            let mut parts: Vec<&crate::font::AssemblyPart> = Vec::new();
            for p in &asm.parts {
                if p.is_extender {
                    for _ in 0..reps {
                        parts.push(p);
                    }
                } else {
                    parts.push(p);
                }
            }
            let advances: f32 = parts.iter().map(|p| p.full_advance * u).sum();
            let joints = parts.len().saturating_sub(1) as f32;
            if advances - min_overlap * joints >= target || reps == 64 {
                let most = parts
                    .windows(2)
                    .map(|w| w[0].end_connector.min(w[1].start_connector) * u)
                    .fold(f32::INFINITY, f32::min)
                    .max(min_overlap);
                let overlap = if joints > 0.0 {
                    ((advances - target) / joints).clamp(min_overlap, most)
                } else {
                    0.0
                };
                return (parts, overlap);
            }
        }
        unreachable!("the loop returns at 64 repeats")
    }

    /// A glyph stretched horizontally to cover `target`, the twin of
    /// `stretch_vertical`: the base, the font's variants, then an assembly
    /// laid left to right. The offsets are x positions from the left edge,
    /// the pieces on the baseline.
    fn stretch_horizontal(&self, ch: char, target: f32, style: MathStyle) -> Option<Spanned> {
        let u = self.unit(style);
        let base = self.font.glyph(ch)?;
        let ink_w = |g: crate::font::GlyphId| {
            let b = self.font.bounds(g);
            (b.x_max - b.x_min) * u
        };
        let mut chosen = base;
        let mut w = ink_w(base);
        if w < target {
            for v in self.font.horizontal_variants(base) {
                chosen = v.glyph;
                w = ink_w(v.glyph);
                if w >= target {
                    break;
                }
            }
        }
        let assembly = self.font.horizontal_assembly(base);
        if w >= target || assembly.is_none() {
            return Some(Spanned {
                glyphs: vec![(chosen, 0.0)],
                ch: Some(ch),
                width: self.font.advance(chosen) * u,
            });
        }
        let asm = assembly.expect("checked");
        let (parts, overlap) = self.assembly_parts(&asm, target, u);
        let mut glyphs = Vec::new();
        let mut cursor = 0.0;
        for p in &parts {
            glyphs.push((p.glyph, cursor));
            cursor += p.full_advance * u - overlap;
        }
        Some(Spanned {
            glyphs,
            ch: None,
            width: cursor + overlap,
        })
    }

    /// The ink of a horizontally stretched glyph above and below the
    /// baseline, in pixels, from its pieces' bounds: (top, bottom), the
    /// top negative when above the baseline.
    fn spanned_extent(&self, spanned: &Spanned, u: f32) -> (f32, f32) {
        spanned
            .glyphs
            .iter()
            .fold((f32::MAX, f32::MIN), |(t, b), (g, _)| {
                let bb = self.font.bounds(*g);
                (t.min(-bb.y_max * u), b.max(-bb.y_min * u))
            })
    }

    /// `\overline` and `\underline`: a rule at the font's gap above or
    /// below the base, the font's extra space beyond it. TeX's rule 11
    /// sets the overlined base cramped; rule 10 leaves the underlined one.
    fn line_box(&self, base: &MathList, under: bool, style: MathStyle, cramped: bool) -> LaidBox {
        let c = self.font.constants();
        let u = self.unit(style);
        let mut geom = self.hlist(base, style, cramped || !under);
        let (gap, thickness, extra) = if under {
            (
                c.underbar_vertical_gap,
                c.underbar_rule_thickness,
                c.underbar_extra_descender,
            )
        } else {
            (
                c.overbar_vertical_gap,
                c.overbar_rule_thickness,
                c.overbar_extra_ascender,
            )
        };
        let (gap, thickness, extra) = (gap * u, thickness * u, extra * u);
        let y = if under {
            geom.descent + gap
        } else {
            -(geom.ascent + gap + thickness)
        };
        geom.rules.push(Rule {
            x: 0.0,
            y,
            width: geom.width,
            height: thickness,
            color: None,
        });
        if under {
            geom.descent += gap + thickness + extra;
        } else {
            geom.ascent += gap + thickness + extra;
        }
        LaidBox { geom, italic: 0.0 }
    }

    /// `\overbrace` and `\underbrace`: the brace stretched to the base's
    /// width, a gap away from it, centered under or over it. The atom's
    /// scripts come as limits through `atom_box`.
    fn brace_box(
        &self,
        atom: &Atom,
        base: &MathList,
        under: bool,
        style: MathStyle,
        cramped: bool,
    ) -> LaidBox {
        let base = self.hlist(base, style, cramped);
        let ch = if under { '\u{23DF}' } else { '\u{23DE}' };
        let Some(brace) = self.stretch_horizontal(ch, base.width, style) else {
            return LaidBox {
                geom: base,
                italic: 0.0,
            };
        };
        let u = self.unit(style);
        let gap = BRACE_GAP_EM * self.em(style);
        let (top, bottom) = self.spanned_extent(&brace, u);
        let y = if under {
            base.descent + gap - top
        } else {
            -(base.ascent + gap) - bottom
        };
        let width = base.width.max(brace.width);
        let mut geom = MathLayout::default();
        let base_w = base.width;
        merge(&mut geom, base, (width - base_w) / 2.0, 0.0);
        let x0 = (width - brace.width) / 2.0;
        for (g, dx) in &brace.glyphs {
            self.push_glyph(
                &mut geom,
                *g,
                x0 + dx,
                y,
                style,
                brace.ch,
                &atom.nucleus_span,
            );
        }
        geom.width = width;
        LaidBox { geom, italic: 0.0 }
    }

    /// `\xrightarrow` and its kin: the texts set in script style, the arrow
    /// stretched past the wider one by `ARROW_PAD_EM` on each side and at
    /// least `ARROW_MIN_EM` long, the texts placed as an operator's limits.
    fn arrow_box(
        &self,
        atom: &Atom,
        ch: char,
        above: &MathList,
        below: &MathList,
        style: MathStyle,
        cramped: bool,
    ) -> LaidBox {
        let sub_style = Self::script_style(style);
        let up = (!above.0.is_empty()).then(|| self.hlist(above, sub_style, cramped));
        let low = (!below.0.is_empty()).then(|| self.hlist(below, sub_style, true));
        let em = self.em(style);
        let texts = up
            .as_ref()
            .map_or(0.0, |b| b.width)
            .max(low.as_ref().map_or(0.0, |b| b.width));
        let target = (texts + 2.0 * ARROW_PAD_EM * em).max(ARROW_MIN_EM * em);
        let Some(arrow) = self.stretch_horizontal(ch, target, style) else {
            return self.literal_box(&ch.to_string(), atom, style);
        };
        let mut geom = MathLayout::default();
        for (g, dx) in &arrow.glyphs {
            self.push_glyph(&mut geom, *g, *dx, 0.0, style, arrow.ch, &atom.nucleus_span);
        }
        geom.width = arrow.width;
        self.stack_limits(LaidBox { geom, italic: 0.0 }, up, low, style)
    }

    /// `\not` over a symbol with no negated form of its own: the combining
    /// long solidus, its ink centered on the base's, in the base's width.
    /// With no base (a `\not` before a closer or at the end), the slash
    /// alone: its ink from the left edge and the box as wide as the ink,
    /// so that it stands clear of what follows, where TeX's zero-width
    /// slash would overprint it.
    fn negated_box(
        &self,
        atom: &Atom,
        base: &MathList,
        style: MathStyle,
        cramped: bool,
    ) -> LaidBox {
        let mut geom = self.hlist(base, style, cramped);
        let Some(slash) = self.font.glyph('\u{0338}') else {
            return LaidBox { geom, italic: 0.0 };
        };
        let u = self.unit(style);
        if base.0.is_empty() {
            let b = self.font.bounds(slash);
            self.push_glyph(
                &mut geom,
                slash,
                -b.x_min * u,
                0.0,
                style,
                Some('\u{0338}'),
                &atom.nucleus_span,
            );
            geom.width = (b.x_max - b.x_min) * u;
            return LaidBox { geom, italic: 0.0 };
        }
        let (left, right) = geom.glyphs.iter().fold((f32::MAX, f32::MIN), |(l, r), g| {
            let b = self.font.bounds(g.glyph);
            let gu = g.size / self.font.units_per_em();
            (l.min(g.x + b.x_min * gu), r.max(g.x + b.x_max * gu))
        });
        let center = if left <= right {
            (left + right) / 2.0
        } else {
            geom.width / 2.0
        };
        let b = self.font.bounds(slash);
        let slash_center = (b.x_min + b.x_max) / 2.0 * u;
        let width = geom.width;
        self.push_glyph(
            &mut geom,
            slash,
            center - slash_center,
            0.0,
            style,
            Some('\u{0338}'),
            &atom.nucleus_span,
        );
        geom.width = width;
        LaidBox { geom, italic: 0.0 }
    }

    /// `\phantom` and its kin: the body's size with none of its ink.
    fn phantom_box(
        &self,
        body: &MathList,
        width: bool,
        height: bool,
        style: MathStyle,
        cramped: bool,
    ) -> LaidBox {
        let mut geom = self.hlist(body, style, cramped);
        geom.glyphs.clear();
        geom.rules.clear();
        geom.literals.clear();
        if !width {
            geom.width = 0.0;
        }
        if !height {
            geom.ascent = 0.0;
            geom.descent = 0.0;
        }
        LaidBox { geom, italic: 0.0 }
    }

    /// Upright text: `\text{...}` and operator names, each character its
    /// own glyph with no italic remap, spaces taking the font's space
    /// advance. A character the font cannot draw degrades the whole run
    /// to a literal.
    fn text_box(&self, text: &str, atom: &Atom, style: MathStyle) -> LaidBox {
        let u = self.unit(style);
        let mut geom = MathLayout::default();
        let mut x = 0.0;
        let mut italic = 0.0;
        for c in text.chars() {
            if c == ' ' {
                x += match self.font.glyph(' ') {
                    Some(g) => self.font.advance(g) * u,
                    None => 0.25 * self.em(style),
                };
                continue;
            }
            let Some(glyph) = self.font.glyph(c) else {
                return self.literal_box(text, atom, style);
            };
            self.push_glyph(&mut geom, glyph, x, 0.0, style, Some(c), &atom.nucleus_span);
            x += self.font.advance(glyph) * u;
            italic = self.font.italic_correction(glyph) * u;
        }
        geom.width = x;
        LaidBox { geom, italic }
    }

    /// Rule 12 with the MATH table's attachment points: the base sets
    /// cramped, the accent rides at `AccentBaseHeight` raised by the
    /// base's excess over it, and wide forms climb the horizontal ladder
    /// until they cover the base. Attachment abscissae align the accent
    /// with a single-glyph base; a longer base centers it.
    fn accent_box(
        &self,
        atom: &Atom,
        accent: char,
        stretch: bool,
        base_list: &MathList,
        style: MathStyle,
    ) -> LaidBox {
        let c = self.font.constants();
        let u = self.unit(style);
        let base = self.hlist(base_list, style, true);
        let Some(mut glyph) = self.font.glyph(accent) else {
            return self.literal_box(&accent.to_string(), atom, style);
        };
        let ink_w = |g: crate::font::GlyphId| {
            let b = self.font.bounds(g);
            (b.x_max - b.x_min) * u
        };
        if stretch && ink_w(glyph) < base.width {
            for v in self.font.horizontal_variants(glyph) {
                glyph = v.glyph;
                if ink_w(v.glyph) >= base.width {
                    break;
                }
            }
        }
        let base_attach = match base.glyphs.as_slice() {
            [g] => {
                let gu = g.size / self.font.units_per_em();
                g.x + self
                    .font
                    .top_accent(g.glyph)
                    .unwrap_or(self.font.advance(g.glyph) / 2.0)
                    * gu
            }
            _ => base.width / 2.0,
        };
        let acc_attach = self
            .font
            .top_accent(glyph)
            .map(|v| v * u)
            .unwrap_or_else(|| {
                let b = self.font.bounds(glyph);
                (b.x_min + b.x_max) / 2.0 * u
            });
        let rise = base.ascent - base.ascent.min(c.accent_base_height * u);
        let width = base.width;
        let mut geom = base;
        self.push_glyph(
            &mut geom,
            glyph,
            base_attach - acc_attach,
            -rise,
            style,
            Some(accent),
            &atom.nucleus_span,
        );
        geom.width = width;
        LaidBox { geom, italic: 0.0 }
    }

    /// An environment's grid, TeX's \halign inside \vcenter: columns at
    /// their widest cell with the gap rule between them, rows strutted
    /// to a minimum pitch, the block centered on the math axis. Cells
    /// set in text style, or script style for `smallmatrix`.
    fn table_box(
        &self,
        rows: &[Vec<MathList>],
        align: &[ColAlign],
        gaps: TableGaps,
        small: bool,
        style: MathStyle,
    ) -> LaidBox {
        let c = self.font.constants();
        let cell_style = if small {
            Self::script_style(style)
        } else {
            match style {
                MathStyle::Display => MathStyle::Text,
                s => s,
            }
        };
        let em = self.em(cell_style);
        let laid: Vec<Vec<MathLayout>> = rows
            .iter()
            .map(|row| {
                row.iter()
                    .enumerate()
                    .map(|(j, cell)| {
                        // amsmath's even-column `{}`: an empty atom ahead
                        // of the cell restores the relation's own spacing
                        // at the alignment point.
                        if matches!(gaps, TableGaps::Pairs) && j % 2 == 1 {
                            let mut list = MathList(Vec::with_capacity(cell.0.len() + 1));
                            list.0.push(Noad::Atom(Atom {
                                class: AtomClass::Ord,
                                nucleus: Field::Empty,
                                sup: None,
                                sub: None,
                                limits: crate::mlist::Limits::default(),
                                span: 0..0,
                                nucleus_span: 0..0,
                            }));
                            list.0.extend(cell.0.iter().cloned());
                            self.hlist(&list, cell_style, false)
                        } else {
                            self.hlist(cell, cell_style, false)
                        }
                    })
                    .collect()
            })
            .collect();
        if laid.is_empty() {
            return LaidBox {
                geom: MathLayout::default(),
                italic: 0.0,
            };
        }
        let cols = laid.iter().map(|row| row.len()).max().unwrap_or(0);
        let mut widths = vec![0.0f32; cols];
        for row in &laid {
            for (j, cell) in row.iter().enumerate() {
                widths[j] = widths[j].max(cell.width);
            }
        }
        // Struts hold short rows at a readable pitch; tall cells push
        // apart with a lineskip of air.
        let ascents: Vec<f32> = laid
            .iter()
            .map(|row| row.iter().map(|c| c.ascent).fold(0.7 * em, f32::max))
            .collect();
        let descents: Vec<f32> = laid
            .iter()
            .map(|row| row.iter().map(|c| c.descent).fold(0.3 * em, f32::max))
            .collect();
        let mut baselines = vec![0.0f32; laid.len()];
        for i in 1..laid.len() {
            let pitch = (descents[i - 1] + ascents[i] + 0.1 * em).max(1.2 * em);
            baselines[i] = baselines[i - 1] + pitch;
        }
        let gap_before = |j: usize| -> f32 {
            if j == 0 {
                return 0.0;
            }
            match gaps {
                TableGaps::Em(g) => g * em,
                // Inside an (r, l) pair the relation touches its side.
                TableGaps::Pairs => {
                    if j % 2 == 1 {
                        0.0
                    } else {
                        em
                    }
                }
            }
        };
        let mut col_x = vec![0.0f32; cols];
        let mut x = 0.0;
        for j in 0..cols {
            x += gap_before(j);
            col_x[j] = x;
            x += widths[j];
        }
        // \vcenter: the block's midpoint lands on the axis.
        let top = -ascents[0];
        let bottom = baselines[laid.len() - 1] + descents[laid.len() - 1];
        let height = bottom - top;
        let axis = c.axis_height * self.unit(style);
        let dy = -axis - (top + height / 2.0);
        let mut geom = MathLayout::default();
        for (i, row) in laid.into_iter().enumerate() {
            for (j, cell) in row.into_iter().enumerate() {
                let dx = match align[j % align.len()] {
                    ColAlign::Left => 0.0,
                    ColAlign::Center => (widths[j] - cell.width) / 2.0,
                    ColAlign::Right => widths[j] - cell.width,
                };
                merge(&mut geom, cell, col_x[j] + dx, baselines[i] + dy);
            }
        }
        geom.width = x;
        geom.ascent = axis + height / 2.0;
        geom.descent = height / 2.0 - axis;
        LaidBox { geom, italic: 0.0 }
    }

    /// Appendix G's radical: the argument cramped in its own style, the
    /// surd stretched to cover it plus the gap and the bar, the optional
    /// degree raised before the surd at scriptscript size.
    fn radical_box(
        &self,
        atom: &Atom,
        radicand: &MathList,
        degree: &Option<MathList>,
        style: MathStyle,
    ) -> LaidBox {
        let c = self.font.constants();
        let u = self.unit(style);
        let inner = self.hlist(radicand, style, true);
        let gap = if style == MathStyle::Display {
            c.radical_display_style_vertical_gap * u
        } else {
            c.radical_vertical_gap * u
        };
        let t = c.radical_rule_thickness * u;
        let target = inner.ascent + inner.descent + gap + t;
        let Some(surd) = self.stretch_vertical('\u{221A}', target, style) else {
            return self.literal_box("\\sqrt", atom, style);
        };
        // Rule 11: the bar joins the surd's top; a surd taller than the
        // target splits its excess, half widening the clearance above the
        // radicand, the rest hanging below.
        let clearance = gap + (surd.height - target).max(0.0) / 2.0;
        let bar_y = -(inner.ascent + clearance + t);
        let surd_bottom = bar_y + surd.height;

        let mut geom = MathLayout::default();
        let mut x = 0.0;
        if let Some(deg_list) = degree {
            let deg = self.hlist(deg_list, MathStyle::ScriptScript, false);
            // Two visual floors over the font's constants, the corner
            // every renderer tunes by hand: the raise floors at TeX's
            // classic 60 percent so the degree clears left-tick surd
            // designs, and a quarter of the kern's tuck stays untucked so
            // the degree keeps air from the diagonal.
            let percent = c.radical_degree_bottom_raise_percent.max(60.0);
            let raise = percent / 100.0 * surd.height;
            let deg_w = deg.width;
            let deg_y = surd_bottom - raise;
            x += c.radical_kern_before_degree * u;
            merge(&mut geom, deg, x, deg_y);
            x += deg_w + 0.75 * c.radical_kern_after_degree * u;
            x = x.max(0.0);
        }
        for (g, y_off) in &surd.glyphs {
            self.push_glyph(
                &mut geom,
                *g,
                x,
                y_off + surd_bottom,
                style,
                surd.ch,
                &atom.nucleus_span,
            );
        }
        let inner_x = x + surd.width;
        // The bar reaches back over the surd's flat terminal, so a
        // subpixel phase difference between glyph and rect rasterization
        // lands inside shared ink instead of showing as a notch.
        let overlap = (1.25 * t).min(0.15 * surd.width);
        geom.rules.push(Rule {
            x: inner_x - overlap,
            y: bar_y,
            width: inner.width + overlap,
            height: t,
            color: None,
        });
        let inner_w = inner.width;
        merge(&mut geom, inner, inner_x, 0.0);
        geom.ascent = geom.ascent.max(-bar_y + c.radical_extra_ascender * u);
        geom.width = inner_x + inner_w;
        LaidBox { geom, italic: 0.0 }
    }

    /// Appendix G's operator symbol: the display style swaps in a variant
    /// tall enough for `DisplayOperatorMinHeight`, and every style centers
    /// the glyph on the math axis.
    fn operator_box(
        &self,
        atom: &Atom,
        base: crate::font::GlyphId,
        ch: char,
        style: MathStyle,
    ) -> LaidBox {
        let c = self.font.constants();
        let u = self.unit(style);
        let mut glyph = base;
        if style == MathStyle::Display {
            let min_h = c.display_operator_min_height * u;
            let ink = |g: crate::font::GlyphId| {
                let b = self.font.bounds(g);
                (b.y_max - b.y_min) * u
            };
            if ink(glyph) < min_h {
                for v in self.font.vertical_variants(base) {
                    glyph = v.glyph;
                    if ink(v.glyph) >= min_h {
                        break;
                    }
                }
            }
        }
        let b = self.font.bounds(glyph);
        let y = (b.y_max + b.y_min) / 2.0 * u - c.axis_height * u;
        let mut geom = MathLayout::default();
        self.push_glyph(
            &mut geom,
            glyph,
            0.0,
            y,
            style,
            Some(ch),
            &atom.nucleus_span,
        );
        geom.width = self.font.advance(glyph) * u;
        LaidBox {
            geom,
            italic: self.font.italic_correction(glyph) * u,
        }
    }

    /// Whether an operator's `Limits::Default` resolves to stacked limits
    /// in display style: TeX's convention, everything except integrals.
    /// The character a nucleus is: a symbol, or one symbol atom inside a
    /// color, which keeps the character's script rule as KaTeX reads it.
    fn char_of(nucleus: &Field) -> Option<char> {
        match nucleus {
            Field::Symbol(c) => Some(*c),
            Field::Colored { body, .. } => match body.0.as_slice() {
                [Noad::Atom(a)] => Self::char_of(&a.nucleus),
                _ => None,
            },
            _ => None,
        }
    }

    fn op_defaults_to_limits(nucleus: &Field) -> bool {
        match nucleus {
            Field::Symbol(c) => !matches!(*c, '\u{222B}'..='\u{2233}' | '\u{2A0B}'..='\u{2A1C}'),
            // A colored operator is a box, as in TeX and KaTeX: its scripts
            // stay at the side.
            Field::Colored { .. } => false,
            _ => true,
        }
    }

    /// Rule 13a: limits centered above and below the operator, the gap and
    /// baseline-rise constants both honored, italic correction splitting
    /// the horizontal centers apart.
    fn limits_box(
        &self,
        atom: &Atom,
        nucleus: LaidBox,
        style: MathStyle,
        cramped: bool,
    ) -> LaidBox {
        let sub_style = Self::script_style(style);
        let upper = atom.sup.as_ref().map(|l| self.hlist(l, sub_style, cramped));
        let lower = atom.sub.as_ref().map(|l| self.hlist(l, sub_style, true));
        // The italic correction splits the limits of an operator alone
        // (rule 13a); amsmath's stacks set their base in a list, which
        // has none.
        let italic = if atom.class == AtomClass::Op {
            nucleus.italic
        } else {
            0.0
        };
        let nucleus = LaidBox {
            geom: nucleus.geom,
            italic,
        };
        self.stack_limits(nucleus, upper, lower, style)
    }

    /// The limits placement: the upper and lower boxes centered over and
    /// under the nucleus, at the font's gaps, the italic correction
    /// shifting them apart as TeX's rule 13a does.
    fn stack_limits(
        &self,
        nucleus: LaidBox,
        upper: Option<MathLayout>,
        lower: Option<MathLayout>,
        style: MathStyle,
    ) -> LaidBox {
        let c = self.font.constants();
        let u = self.unit(style);
        let op = nucleus.geom;
        let ic = nucleus.italic;
        let width = op
            .width
            .max(upper.as_ref().map(|b| b.width).unwrap_or(0.0))
            .max(lower.as_ref().map(|b| b.width).unwrap_or(0.0));
        let mut geom = MathLayout::default();
        let op_ascent = op.ascent;
        let op_descent = op.descent;
        let op_w = op.width;
        merge(&mut geom, op, (width - op_w) / 2.0, 0.0);
        if let Some(up) = upper {
            let rise =
                (c.upper_limit_baseline_rise_min * u).max(c.upper_limit_gap_min * u + up.descent);
            let x = (width - up.width) / 2.0 + ic / 2.0;
            merge(&mut geom, up, x, -(op_ascent + rise));
        }
        if let Some(low) = lower {
            let drop =
                (c.lower_limit_baseline_drop_min * u).max(c.lower_limit_gap_min * u + low.ascent);
            let x = (width - low.width) / 2.0 - ic / 2.0;
            merge(&mut geom, low, x, op_descent + drop);
        }
        geom.width = width;
        LaidBox { geom, italic: 0.0 }
    }

    /// `\big(` and its kin: the delimiter stretched to its size's height
    /// and centered on the axis, as `\left` centers a delimiter. The
    /// height is the text size's whatever the style, TeX's `\big` being
    /// defined on the text font, and the font's variant just under it is
    /// taken as TeX takes it (`DELIMITER_FACTOR`). A dot is the invisible
    /// box of the height, with no width.
    fn sized_box(&self, atom: &Atom, ch: Option<char>, size: u8, style: MathStyle) -> LaidBox {
        let axis = self.font.constants().axis_height * self.unit(style);
        let index = usize::from(size.clamp(1, 4) - 1);
        let text_em = self.em(MathStyle::Text);
        let height = SIZED_DELIMITER_HEIGHTS_EM[index] * text_em;
        let mut geom = MathLayout::default();
        let Some(ch) = ch else {
            geom.ascent = axis + height / 2.0;
            geom.descent = height / 2.0 - axis;
            return LaidBox { geom, italic: 0.0 };
        };
        let target = (height * DELIMITER_FACTOR).max(height - DELIMITER_SHORTFALL_EM * text_em);
        let width =
            self.place_delimiter(&mut geom, ch, target, axis, 0.0, style, &atom.nucleus_span);
        geom.width = width;
        LaidBox { geom, italic: 0.0 }
    }

    /// Rule 19: the body lays out first, then each delimiter stretches to
    /// cover its extent around the axis, honoring the delimited-formula
    /// minimum, and centers on the axis.
    fn left_right_box(
        &self,
        atom: &Atom,
        open: Option<char>,
        close: Option<char>,
        body_list: &MathList,
        style: MathStyle,
        cramped: bool,
    ) -> LaidBox {
        let c = self.font.constants();
        let u = self.unit(style);
        let body = self.hlist(body_list, style, cramped);
        let axis = c.axis_height * u;
        let half = (body.ascent - axis).max(body.descent + axis).max(0.0);
        let target = (2.0 * half).max(c.delimited_sub_formula_min_height * u);
        let mut geom = MathLayout::default();
        let mut x = 0.0;
        if let Some(ch) = open {
            x += self.place_delimiter(&mut geom, ch, target, axis, x, style, &atom.nucleus_span);
        }
        let body_w = body.width;
        merge(&mut geom, body, x, 0.0);
        x += body_w;
        if let Some(ch) = close {
            let from = geom.glyphs.len();
            x += self.place_delimiter(&mut geom, ch, target, axis, x, style, &atom.nucleus_span);
            // A color set in the body holds to the group's end, as TeX's
            // does: the closing delimiter takes it.
            let trailing = body_list.0.iter().rev().find_map(|n| match n {
                Noad::Color(c) => Some(*c),
                _ => None,
            });
            if let Some(color) = trailing {
                let rules = geom.rules.len();
                paint(&mut geom, from, rules, color);
            }
        }
        geom.width = x;
        LaidBox { geom, italic: 0.0 }
    }

    /// One stretched delimiter centered on the axis at `x`; answers its
    /// advance. A character the font cannot draw reserves a literal box.
    #[allow(clippy::too_many_arguments)]
    fn place_delimiter(
        &self,
        geom: &mut MathLayout,
        ch: char,
        target: f32,
        axis: f32,
        x: f32,
        style: MathStyle,
        span: &Range<usize>,
    ) -> f32 {
        let Some(stretched) = self.stretch_vertical(ch, target, style) else {
            let em = self.em(style);
            let text = ch.to_string();
            let width = self.font.measure_literal(&text, em);
            geom.literals.push(Literal {
                text,
                x,
                y: 0.0,
                size: em,
                width,
                source: span.clone(),
            });
            geom.ascent = geom.ascent.max(em * 0.8);
            geom.descent = geom.descent.max(em * 0.2);
            return width;
        };
        let bottom = -axis + stretched.height / 2.0;
        for (g, y_off) in &stretched.glyphs {
            self.push_glyph(geom, *g, x, y_off + bottom, style, stretched.ch, span);
        }
        stretched.width
    }

    /// The style a fraction's parts take, one size down from the whole.
    fn fraction_sub_style(style: MathStyle) -> MathStyle {
        match style {
            MathStyle::Display => MathStyle::Text,
            MathStyle::Text => MathStyle::Script,
            MathStyle::Script | MathStyle::ScriptScript => MathStyle::ScriptScript,
        }
    }

    /// Appendix G's fraction: parts one style down, denominator cramped,
    /// the bar centered on the axis with the minimum gaps enforced. The
    /// barless form is TeX's stack, `\binom`'s interior, on the stack
    /// constants.
    #[allow(clippy::too_many_arguments)]
    fn fraction_box(
        &self,
        num_list: &MathList,
        den_list: &MathList,
        bar: bool,
        forced: Option<MathStyle>,
        continued: bool,
        style: MathStyle,
        cramped: bool,
    ) -> LaidBox {
        // A forced style (\tfrac, \dfrac, \cfrac) replaces the style
        // around the fraction, scripts included, and is never cramped:
        // TeX's {\displaystyle ...} sets the style whole.
        let (style, cramped) = match forced {
            Some(forced) => (forced, false),
            None => (style, cramped),
        };
        let c = self.font.constants();
        let u = self.unit(style);
        let display = style == MathStyle::Display;
        let sub_style = Self::fraction_sub_style(style);
        let mut num = self.hlist(num_list, sub_style, cramped);
        let mut den = self.hlist(den_list, sub_style, true);
        if continued {
            let em = self.em(sub_style);
            for part in [&mut num, &mut den] {
                part.ascent = part.ascent.max(STRUT_HEIGHT_EM * em);
                part.descent = part.descent.max(STRUT_DEPTH_EM * em);
            }
        }
        let width = num.width.max(den.width);
        let mut geom = MathLayout::default();

        let (mut up, mut down) = if display {
            (
                c.fraction_numerator_display_style_shift_up * u,
                c.fraction_denominator_display_style_shift_down * u,
            )
        } else {
            (
                c.fraction_numerator_shift_up * u,
                c.fraction_denominator_shift_down * u,
            )
        };
        if bar {
            let axis = c.axis_height * u;
            let t = c.fraction_rule_thickness * u;
            let (gap_num, gap_den) = if display {
                (
                    c.fraction_num_display_style_gap_min * u,
                    c.fraction_denom_display_style_gap_min * u,
                )
            } else {
                (
                    c.fraction_numerator_gap_min * u,
                    c.fraction_denominator_gap_min * u,
                )
            };
            up = up.max(axis + t / 2.0 + gap_num + num.descent);
            down = down.max(den.ascent - axis + t / 2.0 + gap_den);
            geom.rules.push(Rule {
                x: 0.0,
                y: -(axis + t / 2.0),
                width,
                height: t,
                color: None,
            });
        } else {
            let (stack_up, stack_down, gap_min) = if display {
                (
                    c.stack_top_display_style_shift_up * u,
                    c.stack_bottom_display_style_shift_down * u,
                    c.stack_display_style_gap_min * u,
                )
            } else {
                (
                    c.stack_top_shift_up * u,
                    c.stack_bottom_shift_down * u,
                    c.stack_gap_min * u,
                )
            };
            up = stack_up;
            down = stack_down;
            let gap = (up - num.descent) - (den.ascent - down);
            if gap < gap_min {
                let half = (gap_min - gap) / 2.0;
                up += half;
                down += half;
            }
        }

        let (num_w, den_w) = (num.width, den.width);
        merge(&mut geom, num, (width - num_w) / 2.0, -up);
        merge(&mut geom, den, (width - den_w) / 2.0, down);
        geom.width = width;
        LaidBox { geom, italic: 0.0 }
    }

    fn literal_box(&self, text: &str, atom: &Atom, style: MathStyle) -> LaidBox {
        let em = self.em(style);
        let width = self.font.measure_literal(text, em);
        let geom = MathLayout {
            glyphs: Vec::new(),
            rules: Vec::new(),
            literals: vec![Literal {
                text: text.to_string(),
                x: 0.0,
                y: 0.0,
                size: em,
                width,
                source: atom.nucleus_span.clone(),
            }],
            width,
            ascent: em * 0.8,
            descent: em * 0.2,
        };
        LaidBox { geom, italic: 0.0 }
    }

    /// Appendix G's script attachment: shifts from the constants with the
    /// collision clauses, italic correction on the superscript side, the
    /// script space closing the atom.
    fn atom_box(&self, atom: &Atom, style: MathStyle, cramped: bool) -> LaidBox {
        let nucleus = self.nucleus_box(atom, style, cramped);
        if atom.sup.is_none() && atom.sub.is_none() {
            return nucleus;
        }
        // Explicit limits stack whatever the class: TeX's `\mathop{...}\limits`
        // behind `\stackrel`, `\overset` and the braces.
        let stacked = match atom.limits {
            crate::mlist::Limits::Limits => true,
            crate::mlist::Limits::NoLimits => false,
            crate::mlist::Limits::Default => {
                atom.class == AtomClass::Op
                    && style == MathStyle::Display
                    && Self::op_defaults_to_limits(&atom.nucleus)
            }
        };
        if stacked {
            return self.limits_box(atom, nucleus, style, cramped);
        }

        let c = self.font.constants();
        let u = self.unit(style);
        let sub_style = Self::script_style(style);
        let mut geom = nucleus.geom;
        let nucleus_width = geom.width;

        let sup = atom
            .sup
            .as_ref()
            .map(|list| self.hlist(list, sub_style, cramped));
        let sub = atom
            .sub
            .as_ref()
            .map(|list| self.hlist(list, sub_style, true));

        // Shifts are positive: sup up, sub down. Rule 18a: a character
        // nucleus takes the style constants alone; a box nucleus carries
        // its scripts with its own extent less the baseline drops.
        let is_char = Self::char_of(&atom.nucleus).is_some();
        let mut sup_shift = 0.0f32;
        if let Some(s) = &sup {
            let plain = if cramped {
                c.superscript_shift_up_cramped
            } else {
                c.superscript_shift_up
            } * u;
            sup_shift = plain.max(s.descent + c.superscript_bottom_min * u);
            if !is_char {
                sup_shift = sup_shift.max(geom.ascent - c.superscript_baseline_drop_max * u);
            }
        }
        let mut sub_shift = 0.0f32;
        if let Some(s) = &sub {
            sub_shift = (c.subscript_shift_down * u).max(s.ascent - c.subscript_top_max * u);
            if !is_char {
                sub_shift = sub_shift.max(geom.descent + c.subscript_baseline_drop_min * u);
            }
        }
        if let (Some(sup_box), Some(sub_box)) = (&sup, &sub) {
            let gap = (sup_shift - sup_box.descent) - (sub_box.ascent - sub_shift);
            let deficit = c.sub_superscript_gap_min * u - gap;
            if deficit > 0.0 {
                sub_shift += deficit;
            }
        }

        let mut end = nucleus_width;
        if let Some(s) = sup {
            let x = nucleus_width + nucleus.italic;
            let w = s.width;
            merge(&mut geom, s, x, -sup_shift);
            end = end.max(x + w);
        }
        if let Some(s) = sub {
            let w = s.width;
            merge(&mut geom, s, nucleus_width, sub_shift);
            end = end.max(nucleus_width + w);
        }
        geom.width = end + c.space_after_script * u;
        LaidBox { geom, italic: 0.0 }
    }
}

/// Gives the color to the glyphs and rules from the given indices on that
/// have none yet: an inner color was set first and wins.
fn paint(geom: &mut MathLayout, glyphs_from: usize, rules_from: usize, color: [u8; 3]) {
    for g in geom.glyphs.iter_mut().skip(glyphs_from) {
        g.color.get_or_insert(color);
    }
    for r in geom.rules.iter_mut().skip(rules_from) {
        r.color.get_or_insert(color);
    }
}

/// Translates `src` by (dx, dy) into `dst`, extending the vertical extents.
/// Widths stay the caller's business.
fn merge(dst: &mut MathLayout, src: MathLayout, dx: f32, dy: f32) {
    for mut g in src.glyphs {
        g.x += dx;
        g.y += dy;
        dst.glyphs.push(g);
    }
    for mut r in src.rules {
        r.x += dx;
        r.y += dy;
        dst.rules.push(r);
    }
    for mut l in src.literals {
        l.x += dx;
        l.y += dy;
        dst.literals.push(l);
    }
    dst.ascent = dst.ascent.max(src.ascent - dy);
    dst.descent = dst.descent.max(src.descent + dy);
}

#[cfg(all(test, feature = "ttf"))]
mod tests {
    use super::*;
    use crate::font::TtfMathFont;

    const STIX: &[u8] = include_bytes!("../fixtures/STIXTwoMath-Regular.otf");
    const SIZE: f32 = 20.0;
    const EPS: f32 = 0.01;

    fn font() -> TtfMathFont<'static> {
        TtfMathFont::from_bytes(STIX).unwrap()
    }

    fn lay(tex: &str) -> MathLayout {
        layout(tex, MathStyle::Text, SIZE, &font())
    }

    fn scale() -> f32 {
        use crate::MathFont as _;
        SIZE / font().units_per_em()
    }

    fn glyph_of(c: char) -> crate::font::GlyphId {
        use crate::MathFont as _;
        font().glyph(c).unwrap()
    }

    fn adv(c: char) -> f32 {
        use crate::MathFont as _;
        font().advance(glyph_of(c)) * scale()
    }

    #[test]
    fn letters_map_to_math_italic_and_digits_stay_upright() {
        let l = lay("x2");
        assert_eq!(l.glyphs.len(), 2);
        assert_eq!(l.glyphs[0].glyph, glyph_of('\u{1D465}'));
        assert_eq!(l.glyphs[1].glyph, glyph_of('2'));
        assert_eq!(l.glyphs[0].y, 0.0);
        assert_eq!(l.glyphs[0].size, SIZE);
        assert!(l.ascent > 0.0 && l.width > 0.0);
    }

    #[test]
    fn planck_h_and_greek_take_their_italic_codepoints() {
        let l = lay("h");
        assert_eq!(l.glyphs[0].glyph, glyph_of('\u{210E}'));
        let l = lay("\\alpha\\Gamma");
        assert_eq!(l.glyphs[0].glyph, glyph_of('\u{1D6FC}'));
        assert_eq!(l.glyphs[1].glyph, glyph_of('\u{0393}'));
    }

    #[test]
    fn every_command_table_codepoint_has_a_stix_glyph() {
        use crate::MathFont as _;
        let f = font();
        for (name, ch, _) in crate::parse::VOCABULARY {
            let c = math_italic(*ch);
            assert!(f.glyph(c).is_some(), "\\{name} U+{:04X}", c as u32);
        }
        for (name, ch) in crate::parse::DELIMITERS {
            assert!(f.glyph(*ch).is_some(), "\\{name} U+{:04X}", *ch as u32);
        }
        for (name, ch, _) in crate::parse::ACCENTS {
            assert!(f.glyph(*ch).is_some(), "\\{name} U+{:04X}", *ch as u32);
        }
        // The bold italic alphabet of \boldsymbol, letter by letter.
        for c in ('a'..='z')
            .chain('A'..='Z')
            .chain('0'..='9')
            .chain('\u{03B1}'..='\u{03C9}')
            .chain('\u{0391}'..='\u{03A9}')
            .chain(['\u{2207}', '\u{2202}'])
        {
            let m = crate::parse::map_alphabet("boldsymbol", c);
            assert!(f.glyph(m).is_some(), "boldsymbol {c} U+{:04X}", m as u32);
        }
    }

    #[test]
    fn variant_greek_and_partial_italicize() {
        // The variant letters sit outside the α..ω run but TeX still
        // sets them italic, as it does the partial differential.
        for (tex, italic) in [
            ("\\phi", '\u{1D719}'),
            ("\\epsilon", '\u{1D716}'),
            ("\\vartheta", '\u{1D717}'),
            ("\\varkappa", '\u{1D718}'),
            ("\\varrho", '\u{1D71A}'),
            ("\\varpi", '\u{1D71B}'),
            ("\\partial", '\u{1D715}'),
        ] {
            let l = lay(tex);
            assert_eq!(l.glyphs[0].glyph, glyph_of(italic), "{tex}");
        }
    }

    #[test]
    fn spacing_matrix_places_medium_and_thick_spaces() {
        // Ord Bin Ord: medium spaces, 4/18 em.
        let l = lay("a+b");
        let med = 4.0 / 18.0 * SIZE;
        let after_a = adv('\u{1D44E}');
        assert!((l.glyphs[1].x - (after_a + med)).abs() < EPS);
        // Ord Rel Ord: thick spaces, 5/18 em.
        let l = lay("a=b");
        let thick = 5.0 / 18.0 * SIZE;
        assert!((l.glyphs[1].x - (after_a + thick)).abs() < EPS);
        // Ord Open: no space.
        let l = lay("f(a)");
        let after_f = adv('\u{1D453}');
        assert!((l.glyphs[1].x - after_f).abs() < EPS);
    }

    #[test]
    fn script_styles_drop_the_parenthesized_spaces() {
        let l = layout("a+b", MathStyle::Script, SIZE, &font());
        use crate::MathFont as _;
        let c = font().constants();
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let after_a = font().advance(glyph_of('\u{1D44E}')) * script_scale;
        assert!((l.glyphs[1].x - after_a).abs() < EPS);
        assert!((l.glyphs[0].size - SIZE * c.script_percent_scale_down / 100.0).abs() < EPS);
    }

    #[test]
    fn superscript_rises_by_the_constant_at_script_size() {
        use crate::MathFont as _;
        let c = font().constants();
        let l = lay("x^2");
        assert_eq!(l.glyphs.len(), 2);
        let sup = &l.glyphs[1];
        assert!((sup.size - SIZE * c.script_percent_scale_down / 100.0).abs() < EPS);
        // The digit 2 has no descent to speak of, so the plain shift wins.
        let expected_y = -c.superscript_shift_up * scale();
        assert!(
            (sup.y - expected_y).abs() < EPS,
            "sup y {} vs {}",
            sup.y,
            expected_y
        );
        assert!(l.ascent > SIZE * 0.5);
    }

    #[test]
    fn deep_superscript_honors_the_bottom_minimum() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        // g descends; its bottom must clear SuperscriptBottomMin.
        let l = lay("x^g");
        let sup = &l.glyphs[1];
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let g_descent = -f.bounds(glyph_of('\u{1D454}')).y_min * script_scale;
        let plain = c.superscript_shift_up * scale();
        let clause = g_descent + c.superscript_bottom_min * scale();
        let expected_y = -plain.max(clause);
        assert!(
            (sup.y - expected_y).abs() < EPS,
            "sup y {} vs {}",
            sup.y,
            expected_y
        );
    }

    #[test]
    fn subscript_drops_by_the_constant() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("a_i");
        let sub = &l.glyphs[1];
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let i_ascent = f.bounds(glyph_of('\u{1D456}')).y_max * script_scale;
        let plain = c.subscript_shift_down * scale();
        let clause = i_ascent - c.subscript_top_max * scale();
        let expected_y = plain.max(clause);
        assert!(
            (sub.y - expected_y).abs() < EPS,
            "sub y {} vs {}",
            sub.y,
            expected_y
        );
        assert!(l.descent > 0.0);
    }

    #[test]
    fn stacked_scripts_keep_their_gap() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("x_i^2");
        let sup = l.glyphs.iter().find(|g| g.y < 0.0).expect("sup");
        let sub = l.glyphs.iter().find(|g| g.y > 0.0).expect("sub");
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let sup_bottom = sup.y - f.bounds(sup.glyph).y_min * script_scale;
        let sub_top = sub.y - f.bounds(sub.glyph).y_max * script_scale;
        let gap = sub_top - sup_bottom;
        assert!(
            gap + EPS >= c.sub_superscript_gap_min * scale(),
            "gap {} under minimum",
            gap
        );
    }

    #[test]
    fn italic_correction_pushes_the_superscript_right() {
        let sup_x = lay("f^2").glyphs[1].x;
        let sub_x = lay("f_2").glyphs[1].x;
        assert!(sup_x > sub_x + 0.1, "sup {} vs sub {}", sup_x, sub_x);
    }

    #[test]
    fn space_after_script_separates_the_next_atom() {
        use crate::MathFont as _;
        let c = font().constants();
        let l = lay("x^2y");
        let y = l
            .glyphs
            .iter()
            .find(|g| g.glyph == glyph_of('\u{1D466}'))
            .unwrap();
        let sup = &l.glyphs[1];
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let sup_end = sup.x + font().advance(sup.glyph) * script_scale;
        assert!(y.x + EPS >= sup_end + c.space_after_script * scale());
    }

    #[test]
    fn literal_fallback_reserves_its_measured_box() {
        use crate::MathFont as _;
        let f = font();
        let l = lay("\\foobar x");
        assert_eq!(l.literals.len(), 1);
        let lit = &l.literals[0];
        assert_eq!(lit.text, "\\foobar");
        assert_eq!(lit.x, 0.0);
        assert!((lit.width - f.measure_literal("\\foobar", SIZE)).abs() < EPS);
        assert_eq!(lit.source, 0..7);
        let x = l.glyphs.first().expect("x renders");
        assert!(x.x >= lit.width);
    }

    #[test]
    fn groups_flatten_onto_the_shared_baseline() {
        let l = lay("{ab}c");
        assert_eq!(l.glyphs.len(), 3);
        assert!(l.glyphs.windows(2).all(|w| w[0].x < w[1].x));
        assert!(l.glyphs.iter().all(|g| g.y == 0.0));
    }

    #[test]
    fn source_stamps_point_into_the_tex() {
        let l = lay("x^2");
        assert_eq!(l.glyphs[0].source, 0..1);
        assert_eq!(l.glyphs[1].source, 2..3);
    }

    #[test]
    fn fraction_bar_centers_on_the_axis_at_rule_thickness() {
        use crate::MathFont as _;
        let c = font().constants();
        let l = lay("\\frac{1}{2}");
        assert_eq!(l.rules.len(), 1);
        let bar = &l.rules[0];
        let t = c.fraction_rule_thickness * scale();
        let axis = c.axis_height * scale();
        assert!((bar.height - t).abs() < EPS);
        assert!(
            (bar.y - (-(axis + t / 2.0))).abs() < EPS,
            "bar top {} vs axis {}",
            bar.y,
            axis
        );
        assert!((bar.width - l.width).abs() < 1.0, "bar spans the fraction");
        // Text style sets numerator and denominator at script size.
        let sub_size = SIZE * c.script_percent_scale_down / 100.0;
        assert!(l.glyphs.iter().all(|g| (g.size - sub_size).abs() < EPS));
        let one = &l.glyphs[0];
        assert!(one.y < bar.y, "numerator above the bar");
        assert!(l.glyphs[1].y > 0.0, "denominator below the baseline");
    }

    #[test]
    fn display_style_raises_the_numerator_higher_at_full_size() {
        use crate::MathFont as _;
        let c = font().constants();
        let text = lay("\\frac{1}{2}");
        let disp = layout("\\frac{1}{2}", MathStyle::Display, SIZE, &font());
        assert!(
            (disp.glyphs[0].size - SIZE).abs() < EPS,
            "display numerator at text size"
        );
        assert!(
            disp.glyphs[0].y < text.glyphs[0].y - 1.0,
            "display shift-up beats text: {} vs {}",
            disp.glyphs[0].y,
            text.glyphs[0].y
        );
        assert!(disp.ascent > text.ascent);
        let _ = c;
    }

    #[test]
    fn deep_numerator_honors_the_bar_gap() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("\\frac{g}{2}");
        let axis = c.axis_height * scale();
        let t = c.fraction_rule_thickness * scale();
        let gap = c.fraction_numerator_gap_min * scale();
        let g = &l.glyphs[0];
        let script_scale = scale() * c.script_percent_scale_down / 100.0;
        let g_bottom = g.y - f.bounds(g.glyph).y_min * script_scale;
        assert!(
            g_bottom <= -(axis + t / 2.0 + gap) + EPS,
            "numerator ink clears the bar gap, bottom {}",
            g_bottom
        );
    }

    #[test]
    fn fractions_space_as_inner_atoms() {
        let l = lay("a\\frac{1}{2}");
        let thin = 3.0 / 18.0 * SIZE;
        let a_end = adv('\u{1D44E}');
        let frac_first = l
            .glyphs
            .iter()
            .skip(1)
            .map(|g| g.x)
            .fold(f32::MAX, f32::min);
        assert!(
            frac_first + EPS >= a_end + thin,
            "Ord-Inner thin space, start {} vs {}",
            frac_first,
            a_end + thin
        );
        let _ = font();
    }

    #[test]
    fn radical_draws_surd_and_overbar_over_the_argument() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("\\sqrt{x}");
        assert_eq!(l.rules.len(), 1, "the overbar");
        let bar = &l.rules[0];
        let t = c.radical_rule_thickness * scale();
        assert!((bar.height - t).abs() < EPS);
        assert!(bar.y < -SIZE * 0.3, "bar above the radicand");
        let x_glyph = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('\u{1D465}').unwrap())
            .expect("radicand renders");
        let surd = l
            .glyphs
            .iter()
            .find(|g| g.glyph != x_glyph.glyph)
            .expect("surd renders");
        assert!(surd.x < x_glyph.x, "surd before the radicand");
        assert!(bar.x + bar.width > x_glyph.x, "bar covers the radicand");
        assert!(l.ascent >= -bar.y + c.radical_extra_ascender * scale() - EPS);
        // Rule 11: the bar joins the surd's ink top exactly; a surd taller
        // than needed hangs below, never pokes above the bar.
        let surd_top = surd.y - f.bounds(surd.glyph).y_max * scale();
        assert!(
            (surd_top - bar.y).abs() < EPS,
            "bar top {} meets surd top {}",
            bar.y,
            surd_top
        );
    }

    #[test]
    fn taller_arguments_take_taller_surds() {
        use crate::MathFont as _;
        let f = font();
        let x_id = f.glyph('\u{1D465}').unwrap();
        let small = lay("\\sqrt{x}");
        let tall = layout("\\sqrt{\\frac{1}{2}}", MathStyle::Display, SIZE, &font());
        let small_surd = small.glyphs.iter().find(|g| g.glyph != x_id).unwrap();
        let tall_surds: Vec<_> = tall
            .glyphs
            .iter()
            .filter(|g| {
                let ink = f.bounds(g.glyph);
                (ink.y_max - ink.y_min) * scale() > SIZE * 0.8
            })
            .collect();
        assert!(
            tall_surds.iter().any(|g| g.glyph != small_surd.glyph) || tall_surds.len() > 1,
            "a taller variant or an assembly serves the tall argument"
        );
        assert!(tall.ascent + tall.descent > small.ascent + small.descent + 4.0);
    }

    #[test]
    fn degree_raises_small_before_the_surd() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("\\sqrt[3]{x}");
        let three = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('3').unwrap())
            .expect("degree renders");
        let ss = SIZE * c.script_script_percent_scale_down / 100.0;
        assert!((three.size - ss).abs() < EPS, "degree at scriptscript size");
        assert!(three.y < 0.0, "degree raised above the baseline");
        let x_glyph = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('\u{1D465}').unwrap())
            .unwrap();
        assert!(three.x < x_glyph.x, "degree before the radicand");
        // The raise floors at the classic 60 percent even when the font
        // asks lower, clearing left-tick surd designs.
        let surd = l
            .glyphs
            .iter()
            .find(|g| {
                let b = f.bounds(g.glyph);
                (b.y_max - b.y_min) * scale() > SIZE * 0.8
            })
            .expect("surd");
        let sb = f.bounds(surd.glyph);
        let ink_h = (sb.y_max - sb.y_min) * scale();
        let surd_bottom = surd.y - sb.y_min * scale();
        let raise = c.radical_degree_bottom_raise_percent.max(60.0) / 100.0;
        assert!(
            (three.y - (surd_bottom - raise * ink_h)).abs() < 0.5,
            "degree bottom at the floored raise, y {} vs {}",
            three.y,
            surd_bottom - raise * ink_h
        );
        // A quarter of the tuck stays untucked so the degree keeps air
        // from the diagonal.
        let deg_w =
            f.advance(f.glyph('3').unwrap()) * scale() * c.script_script_percent_scale_down / 100.0;
        let expected_surd_x = (c.radical_kern_before_degree * scale()
            + deg_w
            + 0.75 * c.radical_kern_after_degree * scale())
        .max(0.0);
        assert!(
            (surd.x - expected_surd_x).abs() < 0.5,
            "surd tucks by three quarters of the kern, x {} vs {}",
            surd.x,
            expected_surd_x
        );
    }

    #[test]
    fn radical_bar_overlaps_the_surd_terminal() {
        let l = lay("\\sqrt{x}");
        let bar = &l.rules[0];
        let surd_end = l.glyphs[0].x + {
            use crate::MathFont as _;
            font().advance(l.glyphs[0].glyph) * scale()
        };
        assert!(
            bar.x < surd_end - 1.0,
            "the bar starts inside the surd's terminal, {} vs {}",
            bar.x,
            surd_end
        );
        assert!(
            bar.x + bar.width > surd_end,
            "and still covers the radicand"
        );
    }

    #[test]
    fn display_style_enlarges_operators_onto_the_axis() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let text = lay("\\sum");
        let disp = layout("\\sum", MathStyle::Display, SIZE, &font());
        assert_ne!(
            text.glyphs[0].glyph, disp.glyphs[0].glyph,
            "display takes a larger variant"
        );
        let g = &disp.glyphs[0];
        let b = f.bounds(g.glyph);
        let ink_h = (b.y_max - b.y_min) * scale();
        let tb = f.bounds(text.glyphs[0].glyph);
        let text_h = (tb.y_max - tb.y_min) * scale();
        // The ladder answers the largest it has; STIX tops out below the
        // constant for the summation sign, so taller-than-text is the
        // honest contract.
        assert!(
            ink_h > text_h * 1.2,
            "display ink {} vs text {}",
            ink_h,
            text_h
        );
        let center = g.y - (b.y_max + b.y_min) / 2.0 * scale();
        assert!(
            (center - (-c.axis_height * scale())).abs() < 0.5,
            "centered on the axis, center {}",
            center
        );
    }

    #[test]
    fn display_limits_stack_above_and_below() {
        use crate::MathFont as _;
        let f = font();
        let disp = layout("\\sum_{i}^{n}", MathStyle::Display, SIZE, &font());
        let op = disp
            .glyphs
            .iter()
            .max_by(|a, b| a.size.partial_cmp(&b.size).unwrap())
            .expect("operator");
        let ob = f.bounds(op.glyph);
        let op_top = op.y - ob.y_max * scale();
        let op_bottom = op.y - ob.y_min * scale();
        let op_center = op.x + f.advance(op.glyph) * scale() / 2.0;
        let upper = disp
            .glyphs
            .iter()
            .find(|g| g.y < op_top)
            .expect("upper limit");
        let lower = disp
            .glyphs
            .iter()
            .find(|g| g.y > op_bottom)
            .expect("lower limit");
        for lim in [upper, lower] {
            let lw = f.advance(lim.glyph) * scale() * lim.size / SIZE;
            let center = lim.x + lw / 2.0;
            assert!(
                (center - op_center).abs() < SIZE * 0.4,
                "limit roughly centered, {} vs {}",
                center,
                op_center
            );
        }
    }

    #[test]
    fn text_style_and_nolimits_keep_side_scripts() {
        use crate::MathFont as _;
        let f = font();
        let side = |l: &MathLayout| {
            let op = &l.glyphs[0];
            let end = op.x + f.advance(op.glyph) * scale();
            l.glyphs.iter().skip(1).all(|g| g.x >= end - 1.0)
        };
        assert!(side(&lay("\\sum_i^n")), "text style scripts sit beside");
        assert!(
            side(&layout(
                "\\sum\\nolimits_i^n",
                MathStyle::Display,
                SIZE,
                &font()
            )),
            "nolimits forces the side form in display"
        );
        assert!(
            side(&layout("\\int_0^1", MathStyle::Display, SIZE, &font())),
            "integrals default to side scripts"
        );
        let stacked = layout("\\int\\limits_0^1", MathStyle::Display, SIZE, &font());
        assert!(
            !side(&stacked),
            "explicit limits override the integral default"
        );
    }

    #[test]
    fn left_right_stretches_to_cover_the_body() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let base_paren = f.glyph('(').unwrap();
        let l = layout(
            "\\left(\\frac{1}{\\frac{2}{3}}\\right)",
            MathStyle::Display,
            SIZE,
            &font(),
        );
        let open: Vec<_> = l
            .glyphs
            .iter()
            .filter(|g| g.x < l.glyphs.iter().map(|h| h.x).fold(f32::MAX, f32::min) + 0.5)
            .collect();
        assert!(
            open.iter().all(|g| g.glyph != base_paren),
            "a ladder variant or assembly serves, never the base paren"
        );
        let ink_top = open
            .iter()
            .map(|g| g.y - f.bounds(g.glyph).y_max * scale())
            .fold(f32::MAX, f32::min);
        let ink_bottom = open
            .iter()
            .map(|g| g.y - f.bounds(g.glyph).y_min * scale())
            .fold(0.0, f32::max);
        let axis = c.axis_height * scale();
        let need = (l.ascent - axis).max(l.descent + axis);
        assert!(
            ink_bottom - ink_top + 1.0 >= need,
            "delimiter ink {} covers the body's {}",
            ink_bottom - ink_top,
            need
        );
        let center = (ink_top + ink_bottom) / 2.0;
        assert!(
            (center - (-axis)).abs() < 1.5,
            "delimiter centers on the axis, center {}",
            center
        );
    }

    /// The glyphs as positions and sizes, without their source stamps,
    /// so that two spellings of one shape compare equal.
    fn shape(l: &MathLayout) -> Vec<(crate::font::GlyphId, i32, i32, i32)> {
        l.glyphs
            .iter()
            .map(|g| {
                (
                    g.glyph,
                    (g.x * 100.0).round() as i32,
                    (g.y * 100.0).round() as i32,
                    (g.size * 100.0).round() as i32,
                )
            })
            .collect()
    }

    #[test]
    fn tfrac_and_dfrac_take_their_own_style_whatever_the_style_around() {
        let f = font();
        let tfrac_display = layout("\\tfrac{1}{2}", MathStyle::Display, SIZE, &f);
        let frac_text = lay("\\frac{1}{2}");
        assert_eq!(shape(&tfrac_display), shape(&frac_text));
        assert_eq!(tfrac_display.rules, frac_text.rules);
        let dfrac_text = lay("\\dfrac{1}{2}");
        let frac_display = layout("\\frac{1}{2}", MathStyle::Display, SIZE, &f);
        assert_eq!(shape(&dfrac_text), shape(&frac_display));
        assert!((dfrac_text.ascent - frac_display.ascent).abs() < EPS);
        // The style holds inside a script too: a \dfrac in a superscript
        // is a display fraction at the text size, TeX's {\displaystyle}.
        let in_script = lay("x^{\\dfrac{1}{2}}");
        let plain_script = lay("x^{\\frac{1}{2}}");
        assert!(in_script.ascent > plain_script.ascent + 1.0);
    }

    #[test]
    fn cfrac_parts_stand_on_struts() {
        let f = font();
        let plain = layout("\\dfrac{1}{2}", MathStyle::Display, SIZE, &f);
        let cont = layout("\\cfrac{1}{2}", MathStyle::Display, SIZE, &f);
        assert!(
            cont.glyphs[0].y < plain.glyphs[0].y - 1.0,
            "the numerator rises on the strut's depth: {} vs {}",
            cont.glyphs[0].y,
            plain.glyphs[0].y
        );
        assert!(
            cont.glyphs[1].y > plain.glyphs[1].y + 1.0,
            "the denominator drops under the strut's height: {} vs {}",
            cont.glyphs[1].y,
            plain.glyphs[1].y
        );
        // Two continued fractions of different content keep one row height.
        let a = layout("\\cfrac{1}{2}", MathStyle::Display, SIZE, &f);
        let b = layout("\\cfrac{x}{y}", MathStyle::Display, SIZE, &f);
        assert!((a.glyphs[0].y - b.glyphs[0].y).abs() < EPS);
        assert!((a.glyphs[1].y - b.glyphs[1].y).abs() < EPS);
    }

    #[test]
    fn pmod_renders_mod_upright_in_parentheses_after_a_gap() {
        let l = lay("a \\pmod{n}");
        let chars: String = l.glyphs.iter().filter_map(|g| g.ch).collect();
        assert_eq!(chars, "\u{1D44E}(mod\u{1D45B})");
        let gap = l.glyphs[1].x - (l.glyphs[0].x + adv('\u{1D44E}'));
        assert!(
            (gap - 8.0 / 18.0 * SIZE).abs() < EPS,
            "8 mu before the parenthesis, got {gap}"
        );
        let l = lay("a \\mod n");
        let chars: String = l.glyphs.iter().filter_map(|g| g.ch).collect();
        assert_eq!(chars, "\u{1D44E}mod\u{1D45B}");
    }

    #[test]
    fn texvc_function_names_render_upright_with_operator_spacing() {
        let l = lay("\\sgn x");
        let chars: String = l.glyphs.iter().filter_map(|g| g.ch).collect();
        assert_eq!(chars, "sgn\u{1D465}");
        let plain = lay("\\sin x");
        // The same spacing as an operator name the font already knew.
        let gap = |l: &MathLayout| l.glyphs[3].x - l.glyphs[2].x;
        assert!((gap(&l) - gap(&plain)).abs() < EPS);
    }

    #[test]
    fn a_kern_s_scripts_land_after_the_gap() {
        // `~^2` is TeX's `\nobreakspace{}^2`: the script sits on an empty
        // nucleus after the gap, for the tie and the spacing commands.
        let l = lay("a~^2 b");
        let chars: String = l.glyphs.iter().filter_map(|g| g.ch).collect();
        assert_eq!(chars, "\u{1D44E}2\u{1D44F}");
        let two = l.glyphs.iter().find(|g| g.ch == Some('2')).unwrap();
        assert!(two.y < -1.0, "a superscript, got y {}", two.y);
        assert!(
            two.x >= adv('\u{1D44E}') + 0.25 * SIZE - EPS,
            "after the tie's gap, got x {}",
            two.x
        );
        let l = lay("\\,^2");
        assert_eq!(l.glyphs.len(), 1);
    }

    #[test]
    fn a_forced_style_is_not_cramped() {
        // A \dfrac in a denominator is TeX's {\displaystyle ...}, which is
        // never cramped: its superscript rises as at the top level.
        let f = font();
        let top = layout("\\dfrac{x^2}{2}", MathStyle::Display, SIZE, &f);
        let inside = layout("\\frac{1}{\\dfrac{x^2}{2}}", MathStyle::Display, SIZE, &f);
        let rise = |l: &MathLayout| {
            let x = l.glyphs.iter().find(|g| g.ch == Some('\u{1D465}')).unwrap();
            let two = l.glyphs.iter().find(|g| g.ch == Some('2')).unwrap();
            x.y - two.y
        };
        assert!(
            (rise(&top) - rise(&inside)).abs() < EPS,
            "{} at the top level, {} in the denominator",
            rise(&top),
            rise(&inside)
        );
    }

    #[test]
    fn a_quadruple_integral_keeps_its_limits_at_the_side() {
        let f = font();
        let l = layout("\\iiiint_V f", MathStyle::Display, SIZE, &f);
        let v = l
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{1D449}'))
            .expect("V");
        let plain = layout("\\iiint_V f", MathStyle::Display, SIZE, &f);
        let pv = plain
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{1D449}'))
            .expect("V");
        assert!(
            (v.y - pv.y).abs() < EPS,
            "the V at the side as for \\iiint: {} against {}",
            v.y,
            pv.y
        );
    }

    #[test]
    fn sized_delimiters_grow_with_their_size_and_sit_on_the_axis() {
        use crate::MathFont as _;
        let axis = font().constants().axis_height * scale();
        let mut last = 0.0;
        // TeX's rule: the delimiter may fall short of its size by a tenth
        // (\delimiterfactor 901), and the font's next step is taken above
        // it, so each size lands within a ladder step of its height.
        for (word, em) in [("big", 1.2), ("Big", 1.8), ("bigg", 2.4), ("Bigg", 3.0)] {
            let l = lay(&format!("\\{word}("));
            let height = l.ascent + l.descent;
            assert!(
                height >= 0.9 * em * SIZE - 0.5 && height < (em + 0.25) * SIZE,
                "\\{word}( is {height} tall for {em} em"
            );
            assert!(height > last, "\\{word}( taller than the size before it");
            last = height;
            // Centered on the axis: as much ink above it as below.
            let over = l.ascent - axis;
            let under = l.descent + axis;
            assert!(
                (over - under).abs() < 1.0,
                "\\{word}( off the axis: {over} over, {under} under"
            );
        }
        let plain = lay("(");
        let big = lay("\\big(");
        assert!(plain.ascent + plain.descent < big.ascent + big.descent);
        // The same glyph whatever the class of the form.
        assert_eq!(shape(&lay("\\bigl(")), shape(&big));
        assert_eq!(shape(&lay("\\bigm(")), shape(&big));
        // A body between a pair keeps its place: the pair adds no spacing
        // that \left and \right would not.
        let pair = lay("\\bigl( x \\bigr)");
        let x = pair
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{1D465}'))
            .expect("x");
        assert!(x.x > 0.0 && x.x < pair.width);
        assert!(pair.ascent + pair.descent >= 0.9 * 1.2 * SIZE - 0.5);
        // A bar has no size variants, only an assembly: its pieces overlap
        // as far as their connectors allow, so the four sizes grow as the
        // parentheses do.
        let mut last = 0.0;
        for (word, em) in [("big", 1.2), ("Big", 1.8), ("bigg", 2.4), ("Bigg", 3.0)] {
            let l = lay(&format!("\\{word}|"));
            let height = l.ascent + l.descent;
            assert!(
                height >= 0.9 * em * SIZE - 0.5 && height <= em * SIZE + 0.5,
                "\\{word}| is {height} tall for {em} em"
            );
            assert!(height > last, "\\{word}| taller than the size before it");
            last = height;
        }
    }

    #[test]
    fn a_sized_delimiter_keeps_the_text_height_in_a_script() {
        // TeX's \big sizes are the text size's whatever the style: in
        // script style the parenthesis is as tall as at the top level, an
        // assembly when the font's variants at that size fall short.
        let f = font();
        let height = |style| {
            let l = layout("\\Bigg(", style, SIZE, &f);
            l.ascent + l.descent
        };
        let top = height(MathStyle::Text);
        let script = height(MathStyle::Script);
        assert!(
            (top - script).abs() < 0.25 * SIZE,
            "{top} at the top level, {script} in script style"
        );
    }

    #[test]
    fn a_sized_dot_is_an_empty_box_of_its_height() {
        // `\big.` is TeX's invisible box of the size, centered on the axis:
        // nothing drawn, no width, and a script on it rises with it.
        let l = lay("\\big.");
        assert!(l.glyphs.is_empty() && l.literals.is_empty());
        assert!(l.width.abs() < EPS);
        assert!(
            (l.ascent + l.descent - 1.2 * SIZE).abs() < 0.5,
            "{} tall",
            l.ascent + l.descent
        );
        let two = |l: &MathLayout| l.glyphs.iter().find(|g| g.ch == Some('2')).expect("2").y;
        assert!(two(&lay("\\bigr.^2")) < two(&lay(")^2")) - 1.0);
    }

    #[test]
    fn a_left_right_bar_fits_its_body() {
        // An assembly lands on its target: `\left| x \right|` is the
        // delimited formula's minimum, no longer a whole extender above.
        use crate::MathFont as _;
        let min = font().constants().delimited_sub_formula_min_height * scale();
        let l = lay("\\left| x \\right|");
        let height = l.ascent + l.descent;
        assert!(
            height >= min - 0.5 && height <= min + 0.5,
            "{height} tall against the minimum {min}"
        );
    }

    #[test]
    fn infix_fractions_lay_out_as_their_commands() {
        assert_eq!(shape(&lay("{a \\over b}")), shape(&lay("\\frac{a}{b}")));
        assert_eq!(shape(&lay("{n \\choose k}")), shape(&lay("\\binom{n}{k}")));
        assert!(lay("{a \\atop b}").rules.is_empty(), "no bar under \\atop");
        // A style switch before \over stays in the numerator, as in TeX:
        // the numerator in display style at the text size, the denominator
        // one size down, where \dfrac would set both at the text size.
        let l = lay("{\\displaystyle a \\over b}");
        let a = l
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{1D44E}'))
            .expect("a");
        let b = l
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{1D44F}'))
            .expect("b");
        assert!((a.size - SIZE).abs() < EPS, "a at {}", a.size);
        assert!(b.size < SIZE - 1.0, "b at {}", b.size);
    }

    #[test]
    fn a_style_switch_lays_the_rest_out_in_its_style() {
        let f = font();
        // \displaystyle inside a text formula: the sum as in display style.
        let switched = lay("{\\displaystyle \\sum}");
        let display = layout("\\sum", MathStyle::Display, SIZE, &f);
        assert_eq!(shape(&switched), shape(&display));
        // \scriptstyle: the letter at the script size, as in a superscript.
        let small = lay("{\\scriptstyle x}");
        let x = small
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{1D465}'))
            .expect("x");
        let sup = lay("a^{x}");
        let sx = sup
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{1D465}'))
            .expect("x");
        assert!(
            (x.size - sx.size).abs() < EPS,
            "{} against the script's {}",
            x.size,
            sx.size
        );
        // The spacing after the switch is read in the new style: no space
        // around a relation in script style, and the medium spaces of a
        // binary operator kept across a switch to display style.
        let s = f.constants().script_percent_scale_down / 100.0;
        let l = lay("a \\scriptstyle = b");
        let expected = adv('\u{1D44E}') + s * (adv('=') + adv('\u{1D44F}'));
        assert!(
            (l.width - expected).abs() < 0.5,
            "{} against {expected} with no space around the relation",
            l.width
        );
        assert!((lay("a \\displaystyle + b").width - lay("a + b").width).abs() < EPS);
    }

    #[test]
    fn a_font_switch_draws_its_alphabet() {
        // {\rm d}x: an upright d, then the italic x.
        let l = lay("{\\rm d}x");
        let d = l.glyphs.iter().find(|g| g.ch == Some('d')).expect("d");
        assert_eq!(d.glyph, glyph_of('d'));
        assert!(l.glyphs.iter().any(|g| g.ch == Some('\u{1D465}')));
        // {\bf x}: the bold x.
        let l = lay("{\\bf x}");
        assert!(l.glyphs.iter().any(|g| g.ch == Some('\u{1D431}')));
    }

    /// The ink of a glyph in the layout: its left and right edges, its
    /// top and bottom, in the layout's pixels.
    fn ink(f: &TtfMathFont<'_>, g: &PositionedGlyph) -> (f32, f32, f32, f32) {
        use crate::MathFont as _;
        let b = f.bounds(g.glyph);
        let u = g.size / f.units_per_em();
        (
            g.x + b.x_min * u,
            g.x + b.x_max * u,
            g.y - b.y_max * u,
            g.y - b.y_min * u,
        )
    }

    #[test]
    fn an_overline_rules_above_its_base_and_an_underline_below() {
        let plain = lay("x");
        let l = lay("\\overline{x}");
        assert_eq!(l.rules.len(), 1);
        let r = &l.rules[0];
        assert!(
            r.y + r.height <= -plain.ascent + EPS,
            "the rule above the x: {} against {}",
            r.y + r.height,
            -plain.ascent
        );
        assert!(l.ascent > plain.ascent + r.height);
        assert!((r.width - plain.width).abs() < EPS);
        let l = lay("\\underline{x}");
        let r = &l.rules[0];
        assert!(r.y >= plain.descent - EPS, "the rule below the x: {}", r.y);
        assert!(l.descent > plain.descent + r.height);
    }

    #[test]
    fn a_brace_spans_its_base_and_takes_the_script_beyond_it() {
        let f = font();
        let base = lay("a+b+c");
        let l = lay("\\underbrace{a+b+c}_{n}");
        let is_brace = |g: &&PositionedGlyph| g.ch.is_none_or(|c| c == '\u{23DF}');
        let brace: Vec<&PositionedGlyph> = l.glyphs.iter().filter(is_brace).collect();
        assert!(!brace.is_empty(), "a brace glyph");
        let left = brace.iter().map(|g| ink(&f, g).0).fold(f32::MAX, f32::min);
        let right = brace.iter().map(|g| ink(&f, g).1).fold(f32::MIN, f32::max);
        assert!(
            right - left >= base.width - 1.0,
            "the brace spans {} of {}",
            right - left,
            base.width
        );
        let bottom = brace.iter().map(|g| ink(&f, g).3).fold(f32::MIN, f32::max);
        let n = glyph_by_ch(&l, '\u{1D45B}');
        assert!(ink(&f, n).2 >= bottom - EPS, "the n below the brace");
        assert!(n.size < SIZE);
        let l = lay("\\overbrace{a+b+c}^{n}");
        let brace: Vec<&PositionedGlyph> = l
            .glyphs
            .iter()
            .filter(|g| g.ch.is_none_or(|c| c == '\u{23DE}'))
            .collect();
        let top = brace.iter().map(|g| ink(&f, g).2).fold(f32::MAX, f32::min);
        let n = glyph_by_ch(&l, '\u{1D45B}');
        assert!(ink(&f, n).3 <= top + EPS, "the n above the brace");
    }

    #[test]
    fn a_stack_sets_its_text_as_a_limit() {
        let f = font();
        let l = lay("\\stackrel{a}{\\to}");
        let a = glyph_by_ch(&l, '\u{1D44E}');
        let arrow = glyph_by_ch(&l, '\u{2192}');
        assert!(
            ink(&f, a).3 <= ink(&f, arrow).2 + EPS,
            "the a above the arrow"
        );
        assert!(a.size < SIZE, "at the script size");
        let center = |g: &PositionedGlyph| (ink(&f, g).0 + ink(&f, g).1) / 2.0;
        assert!(
            (center(a) - center(arrow)).abs() < 1.0,
            "centered: {} against {}",
            center(a),
            center(arrow)
        );
        assert_eq!(
            shape(&lay("\\overset{a}{=}")),
            shape(&lay("\\stackrel{a}{=}"))
        );
        let l = lay("\\underset{a}{=}");
        let a = glyph_by_ch(&l, '\u{1D44E}');
        let eq = glyph_by_ch(&l, '=');
        assert!(
            ink(&f, a).2 >= ink(&f, eq).3 - EPS,
            "the a below the relation"
        );
    }

    #[test]
    fn a_long_arrow_spans_its_text() {
        let f = font();
        let l = lay("\\xrightarrow{abc}");
        let text: Vec<&PositionedGlyph> = l
            .glyphs
            .iter()
            .filter(|g| g.ch.is_some_and(|c| c != '\u{2192}'))
            .collect();
        assert_eq!(text.len(), 3);
        let arrow: Vec<&PositionedGlyph> = l
            .glyphs
            .iter()
            .filter(|g| g.ch.is_none_or(|c| c == '\u{2192}'))
            .collect();
        assert!(!arrow.is_empty());
        let text_left = text.iter().map(|g| ink(&f, g).0).fold(f32::MAX, f32::min);
        let text_right = text.iter().map(|g| ink(&f, g).1).fold(f32::MIN, f32::max);
        let arrow_left = arrow.iter().map(|g| ink(&f, g).0).fold(f32::MAX, f32::min);
        let arrow_right = arrow.iter().map(|g| ink(&f, g).1).fold(f32::MIN, f32::max);
        assert!(
            arrow_left <= text_left + EPS && arrow_right >= text_right - EPS,
            "the arrow under the whole text"
        );
        let arrow_top = arrow.iter().map(|g| ink(&f, g).2).fold(f32::MAX, f32::min);
        assert!(
            text.iter().all(|g| ink(&f, g).3 <= arrow_top + EPS),
            "the text above the arrow"
        );
        assert!(l.width >= 1.2 * SIZE - EPS);
        let l = lay("\\xleftarrow[a]{b}");
        assert!(glyph_by_ch(&l, '\u{1D44E}').y > 0.0 && glyph_by_ch(&l, '\u{1D44F}').y < 0.0);
    }

    #[test]
    fn phantoms_have_the_size_and_no_ink() {
        let int = lay("\\int");
        let v = lay("\\vphantom{\\int}");
        assert!(v.glyphs.is_empty() && v.rules.is_empty() && v.literals.is_empty());
        assert!(v.width.abs() < EPS);
        assert!((v.ascent - int.ascent).abs() < EPS && (v.descent - int.descent).abs() < EPS);
        let x = lay("x");
        let h = lay("\\hphantom{x}");
        assert!(h.glyphs.is_empty());
        assert!((h.width - x.width).abs() < EPS && h.ascent.abs() < EPS && h.descent.abs() < EPS);
        let p = lay("\\phantom{x}");
        assert!(p.glyphs.is_empty());
        assert!((p.width - x.width).abs() < EPS && (p.ascent - x.ascent).abs() < EPS);
    }

    #[test]
    fn a_color_tints_the_glyphs_after_it_in_its_group() {
        let l = lay("a {\\color{red} b} c");
        assert_eq!(glyph_by_ch(&l, '\u{1D44E}').color, None);
        assert_eq!(glyph_by_ch(&l, '\u{1D44F}').color, Some([255, 0, 0]));
        assert_eq!(glyph_by_ch(&l, '\u{1D450}').color, None);
        let l = lay("\\textcolor{blue}{x}");
        assert_eq!(glyph_by_ch(&l, '\u{1D465}').color, Some([0, 0, 255]));
        // An inner color wins, and a rule takes the color too.
        let l = lay("{\\color{red} a \\textcolor{blue}{b} \\frac{c}{d}}");
        assert_eq!(glyph_by_ch(&l, '\u{1D44E}').color, Some([255, 0, 0]));
        assert_eq!(glyph_by_ch(&l, '\u{1D44F}').color, Some([0, 0, 255]));
        assert_eq!(l.rules[0].color, Some([255, 0, 0]));
        // A second color replaces the first; the group's end ends it.
        let l = lay("{\\color{red} a \\color{blue} b} c");
        assert_eq!(glyph_by_ch(&l, '\u{1D44E}').color, Some([255, 0, 0]));
        assert_eq!(glyph_by_ch(&l, '\u{1D44F}').color, Some([0, 0, 255]));
        assert_eq!(glyph_by_ch(&l, '\u{1D450}').color, None);
    }

    #[test]
    fn a_slash_of_not_is_centered_over_its_symbol() {
        let f = font();
        for tex in ["\\not\\alpha", "\\not\\longrightarrow", "\\not|"] {
            let l = lay(tex);
            let slash = l
                .glyphs
                .iter()
                .find(|g| g.ch == Some('\u{0338}'))
                .expect("the slash");
            let base: Vec<&PositionedGlyph> = l
                .glyphs
                .iter()
                .filter(|g| g.ch != Some('\u{0338}'))
                .collect();
            let (sl, sr, _, _) = ink(&f, slash);
            let bl = base.iter().map(|g| ink(&f, g).0).fold(f32::MAX, f32::min);
            let br = base.iter().map(|g| ink(&f, g).1).fold(f32::MIN, f32::max);
            assert!(
                ((sl + sr) / 2.0 - (bl + br) / 2.0).abs() < 1.0,
                "{tex}: the slash at {} over the symbol at {}",
                (sl + sr) / 2.0,
                (bl + br) / 2.0
            );
            assert!(
                (l.width - lay(&tex[4..]).width).abs() < EPS,
                "{tex}: the symbol's width"
            );
        }
        // A relation: thick spaces around it, as around TeX's \not.
        assert!(lay("a \\not\\alpha b").width > lay("a \\alpha b").width + 2.0 * 5.0);
    }

    #[test]
    fn a_lone_slash_of_not_takes_its_ink_width() {
        let f = font();
        let l = lay("\\not");
        assert_eq!(l.glyphs.len(), 1);
        let slash = &l.glyphs[0];
        assert_eq!(slash.ch, Some('\u{0338}'));
        let (sl, sr, _, _) = ink(&f, slash);
        assert!(
            sl.abs() < EPS,
            "the ink starts at the box's left edge, not at {sl}"
        );
        assert!(
            (l.width - (sr - sl)).abs() < EPS,
            "the box as wide as the ink: {} against {}",
            l.width,
            sr - sl
        );
        assert!(l.ascent > 0.0 && l.descent > 0.0);
        // A relation: a thick space before it, as before TeX's \not.
        let gap = lay("a \\not").width - lay("a").width - (sr - sl);
        assert!(
            (4.0..8.0).contains(&gap),
            "the space before the slash: {gap}"
        );
        // Before a closer, the slash stands clear of it.
        let l = lay("\\left( a \\not\\right)");
        let slash = glyph_by_ch(&l, '\u{0338}');
        let close = glyph_by_ch(&l, ')');
        assert!(
            ink(&f, slash).1 <= ink(&f, close).0 + EPS,
            "the slash before the parenthesis"
        );
    }

    #[test]
    fn a_stack_over_an_operator_takes_the_display_size_and_side_scripts() {
        let f = font();
        let stacked = layout("\\overset{a}{\\sum}", MathStyle::Display, SIZE, &f);
        let plain = layout("\\sum", MathStyle::Display, SIZE, &f);
        let sum_in = |l: &MathLayout| glyph_by_ch(l, '\u{2211}').glyph;
        assert_eq!(sum_in(&stacked), sum_in(&plain));
        // A script after the stack sits at the side: the box grows in
        // width, and the 2 stands after the a.
        let one = lay("\\overset{a}{b}");
        let two = lay("\\overset{a}{b}^2");
        assert!(
            two.width > one.width + 0.3 * SIZE,
            "{} against {}",
            two.width,
            one.width
        );
        let digit = glyph_by_ch(&two, '2');
        let a = glyph_by_ch(&two, '\u{1D44E}');
        assert!(digit.x > a.x + 0.3 * SIZE, "the 2 after the stack");
    }

    #[test]
    fn textcolor_keeps_an_operator_s_limits_at_the_side() {
        // A colored operator is a box, as in TeX and KaTeX: its scripts
        // stay at the side in display style, with the operator's own
        // italic correction.
        let f = font();
        let colored = layout("\\textcolor{red}{\\int}_0^1", MathStyle::Display, SIZE, &f);
        let plain = layout("\\int_0^1", MathStyle::Display, SIZE, &f);
        assert_eq!(shape(&colored), shape(&plain));
        let colored = layout("\\textcolor{red}{\\sum}_0^1", MathStyle::Display, SIZE, &f);
        let plain = layout("\\sum\\nolimits_0^1", MathStyle::Display, SIZE, &f);
        assert_eq!(shape(&colored), shape(&plain));
    }

    #[test]
    fn a_color_before_over_or_right_reaches_the_bar_and_the_delimiter() {
        // TeX's color is a state of the group: `\over` splits the group
        // after it, and `\right` closes it.
        let l = lay("{\\color{red} A \\over B}");
        assert_eq!(glyph_by_ch(&l, '\u{1D434}').color, Some([255, 0, 0]));
        assert_eq!(glyph_by_ch(&l, '\u{1D435}').color, Some([255, 0, 0]));
        assert_eq!(l.rules[0].color, Some([255, 0, 0]));
        let l = lay("\\left( \\color{red} A \\right)");
        assert_eq!(glyph_by_ch(&l, '(').color, None);
        assert_eq!(glyph_by_ch(&l, '\u{1D434}').color, Some([255, 0, 0]));
        assert_eq!(glyph_by_ch(&l, ')').color, Some([255, 0, 0]));
    }

    #[test]
    fn binom_stacks_inside_parens_without_a_bar() {
        use crate::MathFont as _;
        let f = font();
        let l = lay("\\binom{n}{k}");
        assert!(l.rules.is_empty(), "no fraction bar");
        let n = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('\u{1D45B}').unwrap())
            .expect("n renders");
        let k = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('\u{1D458}').unwrap())
            .expect("k renders");
        assert!(n.y < 0.0 && k.y > 0.0, "stacked around the baseline");
        let leftmost = l.glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min);
        assert!(n.x > leftmost, "a delimiter sits before the stack");
        assert!(l.width > f.advance(f.glyph('\u{1D45B}').unwrap()) * scale() * 2.0);
    }

    #[test]
    fn scripts_ride_the_delimited_group() {
        use crate::MathFont as _;
        let f = font();
        let l = lay("\\left(x\\right)^2");
        let two = l
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('2').unwrap())
            .expect("the script renders");
        let rightmost_other = l
            .glyphs
            .iter()
            .filter(|g| g.glyph != two.glyph)
            .map(|g| g.x)
            .fold(0.0, f32::max);
        assert!(two.x > rightmost_other, "the script follows the closer");
        assert!(two.y < 0.0, "raised as a superscript");
    }

    #[test]
    fn scripts_on_boxes_rise_and_drop_with_the_box() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let tall = "\\left(\\frac{1}{1+\\frac{1}{x}}\\right)";
        let plain = lay(tall);
        let scripted = lay(&format!("{tall}^2"));
        let two = scripted
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('2').unwrap())
            .expect("superscript renders");
        // Rule 18a: on a box nucleus the shift is the box height less the
        // baseline drop, which dwarfs the character constant here.
        let expected = plain.ascent - c.superscript_baseline_drop_max * scale();
        assert!(
            (two.y - (-expected)).abs() < 0.6,
            "sup baseline {} vs box rule {}",
            two.y,
            -expected
        );
        let subbed = lay(&format!("{tall}_2"));
        let two = subbed
            .glyphs
            .iter()
            .find(|g| g.glyph == f.glyph('2').unwrap())
            .expect("subscript renders");
        let expected = plain.descent + c.subscript_baseline_drop_min * scale();
        assert!(
            (two.y - expected).abs() < 0.6,
            "sub baseline {} vs box rule {}",
            two.y,
            expected
        );
    }

    #[test]
    fn empty_and_hostile_inputs_yield_finite_geometry() {
        for tex in ["", "^", "{", "}}", "x^", "\\", "%"] {
            let l = lay(tex);
            assert!(l.width.is_finite() && l.ascent.is_finite() && l.descent.is_finite());
        }
        assert_eq!(lay("").width, 0.0);
    }

    #[test]
    fn glyphs_carry_the_characters_they_render() {
        let l = lay("x^2");
        let chars: Vec<Option<char>> = l.glyphs.iter().map(|g| g.ch).collect();
        assert_eq!(chars, vec![Some('\u{1D465}'), Some('2')]);
    }

    /// The leftmost column of glyphs, which is the opening delimiter.
    fn open_column(l: &MathLayout) -> Vec<&PositionedGlyph> {
        let min_x = l.glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min);
        l.glyphs.iter().filter(|g| g.x < min_x + 0.5).collect()
    }

    #[test]
    fn a_variant_delimiter_keeps_its_base_character() {
        let l = layout(
            "\\left(\\frac{1}{\\frac{2}{3}}\\right)",
            MathStyle::Display,
            SIZE,
            &font(),
        );
        let open = open_column(&l);
        assert_eq!(open.len(), 1, "the two-deep nest is served by the ladder");
        assert_eq!(open[0].ch, Some('('));
    }

    #[test]
    fn assembly_pieces_carry_no_character() {
        let l = layout(
            "\\left(\\frac{1}{\\frac{2}{\\frac{3}{\\frac{4}{5}}}}\\right)",
            MathStyle::Display,
            SIZE,
            &font(),
        );
        let open = open_column(&l);
        assert!(open.len() > 1, "the deep nest forces an assembly");
        assert!(open.iter().all(|g| g.ch.is_none()));
    }

    #[test]
    fn explicit_kerns_move_the_pen_exactly() {
        let plain = lay("ab");
        let thin = lay("a\\,b");
        let dx = thin.glyphs[1].x - plain.glyphs[1].x;
        assert!((dx - 3.0 / 18.0 * SIZE).abs() < EPS, "thin space, got {dx}");
        let quad = lay("a\\quad b");
        let dx = quad.glyphs[1].x - plain.glyphs[1].x;
        assert!((dx - SIZE).abs() < EPS, "quad is one em, got {dx}");
        let neg = lay("a\\!b");
        let dx = neg.glyphs[1].x - plain.glyphs[1].x;
        assert!(
            (dx + 3.0 / 18.0 * SIZE).abs() < EPS,
            "negative thin, got {dx}"
        );
    }

    #[test]
    fn text_renders_upright_with_its_space_kept() {
        let l = lay("\\text{if }");
        let chars: Vec<Option<char>> = l.glyphs.iter().map(|g| g.ch).collect();
        assert_eq!(
            chars,
            vec![Some('i'), Some('f')],
            "upright letters, no italic remap"
        );
        let bare = lay("\\text{if}");
        assert!(
            l.width > bare.width + 1.0,
            "the trailing space keeps its advance: {} vs {}",
            l.width,
            bare.width
        );
    }

    #[test]
    fn operator_names_set_upright_with_op_spacing() {
        use crate::MathFont as _;
        let l = lay("\\sin x");
        assert_eq!(l.glyphs[0].ch, Some('s'));
        assert_eq!(l.glyphs[1].ch, Some('i'));
        assert_eq!(l.glyphs[2].ch, Some('n'));
        let x = l
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{1D465}'))
            .expect("the argument renders italic");
        let n = &l.glyphs[2];
        let n_end = n.x + font().advance(glyph_of('n')) * scale();
        let thin = 3.0 / 18.0 * SIZE;
        assert!(
            (x.x - (n_end + thin)).abs() < EPS,
            "Op spacing before the argument, gap {}",
            x.x - n_end
        );
    }

    #[test]
    fn accents_place_at_the_accent_height() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("\\hat x");
        let hat = l
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{0302}'))
            .expect("the accent glyph renders");
        let base_h = f.bounds(glyph_of('\u{1D465}')).y_max * scale();
        let expected = -(base_h - base_h.min(c.accent_base_height * scale()));
        assert!(
            (hat.y - expected).abs() < EPS,
            "hat y {} vs {}",
            hat.y,
            expected
        );
        // A tall base lifts its accent by the excess over the height.
        let tall = lay("\\hat A");
        let hat_tall = tall
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{0302}'))
            .expect("accent");
        assert!(hat_tall.y < hat.y - 1.0, "a capital lifts its accent");
    }

    #[test]
    fn wide_accents_stretch_over_their_base() {
        use crate::MathFont as _;
        let f = font();
        let hat_of = |l: &MathLayout| {
            l.glyphs
                .iter()
                .find(|g| g.ch == Some('\u{0302}'))
                .cloned()
                .expect("accent glyph")
        };
        let narrow = hat_of(&lay("\\widehat{a}"));
        let wide = hat_of(&lay("\\widehat{abc}"));
        let ink = |g: &PositionedGlyph| {
            let b = f.bounds(g.glyph);
            (b.x_max - b.x_min) * scale()
        };
        assert!(
            ink(&wide) > ink(&narrow) + 1.0,
            "a wider base takes a wider hat: {} vs {}",
            ink(&wide),
            ink(&narrow)
        );
        assert_ne!(wide.glyph, narrow.glyph, "a ladder variant serves");
        let l = lay("\\vec v");
        assert!(
            l.glyphs.iter().any(|g| g.ch == Some('\u{20D7}')),
            "the vector arrow renders"
        );
        let l = lay("\\bar y");
        let bar = l
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{0304}'))
            .expect("the macron renders");
        let y_top = f.bounds(glyph_of('\u{1D466}')).y_max * scale();
        let bar_bottom = bar.y - f.bounds(bar.glyph).y_min * scale();
        assert!(
            bar_bottom <= -y_top + 1.0,
            "the bar sits above the base ink: bottom {} vs top {}",
            bar_bottom,
            -y_top
        );
    }

    fn glyph_by_ch(l: &MathLayout, ch: char) -> &PositionedGlyph {
        l.glyphs
            .iter()
            .find(|g| g.ch == Some(ch))
            .unwrap_or_else(|| panic!("no glyph for {ch:?}"))
    }

    #[test]
    fn a_matrix_measures_columns_and_struts_rows() {
        use crate::MathFont as _;
        let f = font();
        let c = f.constants();
        let l = lay("\\begin{matrix} 1 & 22 \\\\ 33 & 4 \\end{matrix}");
        let d = f.advance(glyph_of('1')) * scale();
        let one = glyph_by_ch(&l, '1');
        let four = glyph_by_ch(&l, '4');
        // Columns take their widest cell, cells center inside them, and
        // one em separates the columns.
        assert!((one.x - d / 2.0).abs() < EPS, "1 centers in its column");
        let col1_x = 2.0 * d + SIZE;
        assert!(
            (four.x - (col1_x + d / 2.0)).abs() < EPS,
            "4 centers in the second column, x {}",
            four.x
        );
        // Digit ink is shorter than the struts, so the pitch is exactly
        // the strutted baseline distance.
        assert!(
            (four.y - one.y - 1.2 * SIZE).abs() < EPS,
            "strutted pitch, got {}",
            four.y - one.y
        );
        // The whole block centers on the math axis.
        let axis = c.axis_height * scale();
        assert!(
            (l.ascent - l.descent - 2.0 * axis).abs() < EPS,
            "axis centering: ascent {} descent {}",
            l.ascent,
            l.descent
        );
    }

    #[test]
    fn pmatrix_fences_cover_a_ten_row_assembly() {
        let rows = ["1"; 10].join(" \\\\ ");
        let l = lay(&format!("\\begin{{pmatrix}} {rows} \\end{{pmatrix}}"));
        let open = open_column(&l);
        assert!(open.len() > 1, "ten rows force an assembly");
        use crate::MathFont as _;
        let f = font();
        let ink_top = open
            .iter()
            .map(|g| g.y - f.bounds(g.glyph).y_max * scale())
            .fold(f32::MAX, f32::min);
        let ink_bottom = open
            .iter()
            .map(|g| g.y - f.bounds(g.glyph).y_min * scale())
            .fold(f32::MIN, f32::max);
        assert!(
            ink_bottom - ink_top >= 9.0 * 1.2 * SIZE,
            "the fence covers the ten strutted rows, got {}",
            ink_bottom - ink_top
        );
    }

    #[test]
    fn aligned_lands_relations_at_one_x() {
        use crate::MathFont as _;
        let l = lay("\\begin{aligned} x &= y \\\\ xx &= y+1 \\end{aligned}");
        let eqs: Vec<&PositionedGlyph> = l.glyphs.iter().filter(|g| g.ch == Some('=')).collect();
        assert_eq!(eqs.len(), 2);
        assert!(
            (eqs[0].x - eqs[1].x).abs() < EPS,
            "both relations at one x: {} vs {}",
            eqs[0].x,
            eqs[1].x
        );
        // amsmath's even-column empty atom: the relation keeps its thick
        // space against the right-aligned left-hand side.
        let x_row0 = l
            .glyphs
            .iter()
            .find(|g| g.ch == Some('\u{1D465}') && (g.y - eqs[0].y).abs() < 0.1)
            .expect("the first row's x");
        let right = x_row0.x + font().advance(glyph_of('\u{1D465}')) * scale();
        let thick = 5.0 / 18.0 * SIZE;
        assert!(
            (eqs[0].x - right - thick).abs() < EPS,
            "thick space at the alignment point, got {}",
            eqs[0].x - right
        );
    }

    #[test]
    fn cases_left_aligns_behind_its_brace() {
        let l = lay("\\begin{cases} x & a \\\\ 0 & b \\end{cases}");
        let x = glyph_by_ch(&l, '\u{1D465}');
        let zero = glyph_by_ch(&l, '0');
        assert!(
            (x.x - zero.x).abs() < EPS,
            "the value column left-aligns: {} vs {}",
            x.x,
            zero.x
        );
        let brace_x = l.glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min);
        assert!(brace_x < x.x, "the brace sits left of the cells");
    }

    #[test]
    fn array_honors_its_column_spec() {
        use crate::MathFont as _;
        let f = font();
        let d = f.advance(glyph_of('1')) * scale();
        let l = lay("\\begin{array}{rl} 1 & 2 \\\\ 333 & 444 \\end{array}");
        let one = glyph_by_ch(&l, '1');
        let threes: Vec<&PositionedGlyph> = l.glyphs.iter().filter(|g| g.ch == Some('3')).collect();
        let last_three = threes
            .iter()
            .max_by(|a, b| a.x.total_cmp(&b.x))
            .expect("333 renders");
        assert!(
            (one.x - last_three.x).abs() < EPS,
            "the right column aligns its right edges: {} vs {}",
            one.x,
            last_three.x
        );
        let two = glyph_by_ch(&l, '2');
        let fours: Vec<&PositionedGlyph> = l.glyphs.iter().filter(|g| g.ch == Some('4')).collect();
        let first_four = fours
            .iter()
            .min_by(|a, b| a.x.total_cmp(&b.x))
            .expect("444 renders");
        assert!(
            (two.x - first_four.x).abs() < EPS,
            "the left column aligns its left edges: {} vs {}",
            two.x,
            first_four.x
        );
        let _ = d;
    }

    #[test]
    fn smallmatrix_takes_script_size() {
        use crate::MathFont as _;
        let l = lay("\\begin{smallmatrix} 1 & 2 \\end{smallmatrix}");
        let c = font().constants();
        let expect = SIZE * c.script_percent_scale_down / 100.0;
        assert!(
            l.glyphs.iter().all(|g| (g.size - expect).abs() < EPS),
            "cells set at script size"
        );
    }

    /// A deterministic xorshift step, the fuzz harness's only source of
    /// variation.
    fn xorshift(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    /// One mutation of a source string: truncate, delete, insert a
    /// syntax character, duplicate a run, or swap two characters, always
    /// on character boundaries.
    fn mutate(src: &str, state: &mut u64) -> String {
        const SPECIALS: &[char] = &['\\', '{', '}', '^', '_', '&', '#', '[', ']', '%', '\''];
        let mut out: Vec<char> = src.chars().collect();
        if out.is_empty() {
            return String::new();
        }
        let i = (xorshift(state) as usize) % out.len();
        match xorshift(state) % 5 {
            0 => out.truncate(i),
            1 => {
                out.remove(i);
            }
            2 => {
                let c = SPECIALS[(xorshift(state) as usize) % SPECIALS.len()];
                out.insert(i, c);
            }
            3 => {
                let j = (xorshift(state) as usize) % out.len();
                let (a, b) = (i.min(j), i.max(j));
                let run: Vec<char> = out[a..b].to_vec();
                out.splice(a..a, run);
            }
            _ => {
                let j = (xorshift(state) as usize) % out.len();
                out.swap(i, j);
            }
        }
        out.into_iter().collect()
    }

    #[test]
    fn fuzzed_real_world_corpus_never_panics() {
        let corpus: &[&str] = &[
            "E = mc^2",
            "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}",
            "\\int_{-\\infty}^{\\infty} e^{-x^2}\\,dx = \\sqrt{\\pi}",
            "e^{i\\pi} + 1 = 0",
            "\\sum_{n=1}^{\\infty} \\frac{1}{n^2} = \\frac{\\pi^2}{6}",
            "\\nabla \\times \\vec{B} = \\mu_0 \\vec{J} + \\mu_0 \\varepsilon_0 \
             \\frac{\\partial \\vec{E}}{\\partial t}",
            "i\\hbar\\frac{\\partial}{\\partial t}\\Psi = \\hat{H}\\Psi",
            "\\begin{pmatrix} \\cos\\theta & -\\sin\\theta \\\\ \
             \\sin\\theta & \\cos\\theta \\end{pmatrix}",
            "|x| = \\begin{cases} x & x \\geq 0 \\\\ -x & x < 0 \\end{cases}",
            "\\begin{aligned} (a+b)^2 &= a^2 + 2ab + b^2 \\\\ &\\geq 4ab \\end{aligned}",
            "\\lim_{x \\to 0} \\frac{\\sin x}{x} = 1",
            "\\binom{n}{k} = \\frac{n!}{k!(n-k)!}",
            "\\hat{f}(\\xi) = \\int_{-\\infty}^{\\infty} f(x)\\, e^{-2\\pi i x \\xi}\\,dx",
            "\\{x \\in \\mathbb{R} : x^2 < 2\\} \\subseteq \\mathbb{Q}^c",
            "\\newcommand{\\avg}[1]{\\left\\langle #1 \\right\\rangle} \
             \\avg{x^2} - \\avg{x}^2",
            "\\sqrt{2} = 1 + \\frac{1}{2 + \\frac{1}{2 + \\frac{1}{2 + \\cdots}}}",
            "\\oint_{\\partial \\Sigma} \\vec{F} \\cdot d\\vec{r} = \
             \\iint_{\\Sigma} \\nabla \\times \\vec{F} \\cdot d\\vec{S}",
            "\\operatorname{Var}(X) = \\mathbb{E}[X^2] - \\mathbb{E}[X]^2",
            "\\frac{\\mathrm{d}}{\\mathrm{d}x} \\ln x = \\frac{1}{x}",
            "\\lVert x + y \\rVert \\leq \\lVert x \\rVert + \\lVert y \\rVert",
            "\\forall \\epsilon > 0\\, \\exists \\delta > 0 : |x - a| < \\delta \
             \\implies |f(x) - f(a)| < \\epsilon",
            "P(A \\mid B) = \\frac{P(B \\mid A)\\,P(A)}{P(B)} \\quad \\text{(Bayes)}",
            "\\overrightarrow{AB} + \\overrightarrow{BC} = \\overrightarrow{AC}",
            "\\bigcup_{i=1}^{n} A_i \\supseteq \\bigcap_{i=1}^{n} A_i",
            "\\sqrt[3]{x^3 + y^3} \\neq x + y",
            "\\newcommand{\\norm}[2][2]{\\lVert #2 \\rVert_{#1}} \
             \\operatorname*{argmin}_{w} \\norm[1]{Xw - y}",
            "A \\subsetneq B \\iff A \\subseteq B \\land A \\neq B",
            "x^{y^{z^w}} + {a_b}_{c_d}",
        ];
        let f = font();
        let mut state: u64 = 0x243F_6A88_85A3_08D3;
        for src in corpus {
            for round in 0..49 {
                let tex = if round == 0 {
                    (*src).to_string()
                } else {
                    mutate(src, &mut state)
                };
                for style in [MathStyle::Display, MathStyle::Text] {
                    let l = layout(&tex, style, SIZE, &f);
                    assert!(l.width.is_finite(), "{tex}");
                    assert!(l.ascent.is_finite() && l.descent.is_finite(), "{tex}");
                }
            }
        }
    }
}
