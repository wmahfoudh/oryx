//! The parser: tokens to a math list.
//!
//! Recursive descent with no error path that aborts. Unknown commands
//! become literal atoms, stray closers and alignment markers degrade to
//! literals, and an unclosed group closes at the end of input. Hostile
//! input costs a fallback, never a panic.

use crate::mlist::{Atom, AtomClass, ColAlign, Field, Limits, MathList, Noad, TableGaps};

/// The symbol vocabulary: command name to codepoint and class, sorted by
/// name for binary search. Coverage grows by adding rows.
const VOCABULARY: &[(&str, char, AtomClass)] = &[
    ("Delta", '\u{0394}', AtomClass::Ord),
    ("Gamma", '\u{0393}', AtomClass::Ord),
    ("Lambda", '\u{039B}', AtomClass::Ord),
    ("Omega", '\u{03A9}', AtomClass::Ord),
    ("Phi", '\u{03A6}', AtomClass::Ord),
    ("Pi", '\u{03A0}', AtomClass::Ord),
    ("Psi", '\u{03A8}', AtomClass::Ord),
    ("Sigma", '\u{03A3}', AtomClass::Ord),
    ("Theta", '\u{0398}', AtomClass::Ord),
    ("Xi", '\u{039E}', AtomClass::Ord),
    ("alpha", '\u{03B1}', AtomClass::Ord),
    ("approx", '\u{2248}', AtomClass::Rel),
    ("beta", '\u{03B2}', AtomClass::Ord),
    ("cdot", '\u{22C5}', AtomClass::Bin),
    ("cdots", '\u{22EF}', AtomClass::Ord),
    ("chi", '\u{03C7}', AtomClass::Ord),
    ("delta", '\u{03B4}', AtomClass::Ord),
    ("div", '\u{00F7}', AtomClass::Bin),
    ("epsilon", '\u{03F5}', AtomClass::Ord),
    ("equiv", '\u{2261}', AtomClass::Rel),
    ("eta", '\u{03B7}', AtomClass::Ord),
    ("gamma", '\u{03B3}', AtomClass::Ord),
    ("geq", '\u{2265}', AtomClass::Rel),
    ("in", '\u{2208}', AtomClass::Rel),
    ("infty", '\u{221E}', AtomClass::Ord),
    ("int", '\u{222B}', AtomClass::Op),
    ("iota", '\u{03B9}', AtomClass::Ord),
    ("kappa", '\u{03BA}', AtomClass::Ord),
    ("lambda", '\u{03BB}', AtomClass::Ord),
    ("ldots", '\u{2026}', AtomClass::Ord),
    ("leftarrow", '\u{2190}', AtomClass::Rel),
    ("leq", '\u{2264}', AtomClass::Rel),
    ("mu", '\u{03BC}', AtomClass::Ord),
    ("nabla", '\u{2207}', AtomClass::Ord),
    ("neq", '\u{2260}', AtomClass::Rel),
    ("nu", '\u{03BD}', AtomClass::Ord),
    ("omega", '\u{03C9}', AtomClass::Ord),
    ("oplus", '\u{2295}', AtomClass::Bin),
    ("otimes", '\u{2297}', AtomClass::Bin),
    ("partial", '\u{2202}', AtomClass::Ord),
    ("phi", '\u{03D5}', AtomClass::Ord),
    ("pi", '\u{03C0}', AtomClass::Ord),
    ("pm", '\u{00B1}', AtomClass::Bin),
    ("prod", '\u{220F}', AtomClass::Op),
    ("psi", '\u{03C8}', AtomClass::Ord),
    ("rho", '\u{03C1}', AtomClass::Ord),
    ("rightarrow", '\u{2192}', AtomClass::Rel),
    ("sigma", '\u{03C3}', AtomClass::Ord),
    ("subset", '\u{2282}', AtomClass::Rel),
    ("subseteq", '\u{2286}', AtomClass::Rel),
    ("sum", '\u{2211}', AtomClass::Op),
    ("tau", '\u{03C4}', AtomClass::Ord),
    ("theta", '\u{03B8}', AtomClass::Ord),
    ("times", '\u{00D7}', AtomClass::Bin),
    ("to", '\u{2192}', AtomClass::Rel),
    ("upsilon", '\u{03C5}', AtomClass::Ord),
    ("varepsilon", '\u{03B5}', AtomClass::Ord),
    ("varphi", '\u{03C6}', AtomClass::Ord),
    ("xi", '\u{03BE}', AtomClass::Ord),
    ("zeta", '\u{03B6}', AtomClass::Ord),
    ("{", '{', AtomClass::Open),
    ("|", '\u{2016}', AtomClass::Ord),
    ("}", '}', AtomClass::Close),
];

/// Parses a TeX math string into a math list. Total: any input yields a
/// list, degraded where not understood.
pub fn parse(tex: &str) -> MathList {
    let mut parser = Parser {
        tokens: crate::token::tokenize(tex),
        pos: 0,
        src: tex,
    };
    parser.list(true)
}

/// Delimiter commands `\left` and `\right` accept, sorted for binary
/// search. Plain characters and `.` resolve without the table.
const DELIMITERS: &[(&str, char)] = &[
    ("Vert", '\u{2016}'),
    ("backslash", '\\'),
    ("langle", '\u{27E8}'),
    ("lbrace", '{'),
    ("lceil", '\u{2308}'),
    ("lfloor", '\u{230A}'),
    ("rangle", '\u{27E9}'),
    ("rbrace", '}'),
    ("rceil", '\u{2309}'),
    ("rfloor", '\u{230B}'),
    ("vert", '|'),
    ("{", '{'),
    ("|", '\u{2016}'),
    ("}", '}'),
];

fn vocabulary_lookup(name: &str) -> Option<(char, AtomClass)> {
    VOCABULARY
        .binary_search_by(|row| row.0.cmp(name))
        .ok()
        .map(|i| (VOCABULARY[i].1, VOCABULARY[i].2))
}

/// Accent commands: the combining character and whether wide forms may
/// stretch horizontally. Sorted for binary search.
const ACCENTS: &[(&str, char, bool)] = &[
    ("acute", '\u{0301}', false),
    ("bar", '\u{0304}', false),
    ("breve", '\u{0306}', false),
    ("check", '\u{030C}', false),
    ("ddot", '\u{0308}', false),
    ("dot", '\u{0307}', false),
    ("grave", '\u{0300}', false),
    ("hat", '\u{0302}', false),
    ("mathring", '\u{030A}', false),
    ("tilde", '\u{0303}', false),
    ("vec", '\u{20D7}', false),
    ("widehat", '\u{0302}', true),
    ("widetilde", '\u{0303}', true),
];

/// Operator names: upright Op atoms, flagged when TeX stacks their
/// limits in display style. Sorted for binary search.
const OP_NAMES: &[(&str, bool)] = &[
    ("Pr", true),
    ("arccos", false),
    ("arcsin", false),
    ("arctan", false),
    ("arg", false),
    ("cos", false),
    ("cosh", false),
    ("cot", false),
    ("coth", false),
    ("csc", false),
    ("deg", false),
    ("det", true),
    ("dim", false),
    ("exp", false),
    ("gcd", true),
    ("hom", false),
    ("inf", true),
    ("ker", false),
    ("lg", false),
    ("lim", true),
    ("liminf", true),
    ("limsup", true),
    ("ln", false),
    ("log", false),
    ("max", true),
    ("min", true),
    ("sec", false),
    ("sin", false),
    ("sinh", false),
    ("sup", true),
    ("tan", false),
    ("tanh", false),
];

/// The explicit spacing commands, in ems of the current style.
fn spacing_ems(name: &str) -> Option<f32> {
    Some(match name {
        "," => 3.0 / 18.0,
        ":" => 4.0 / 18.0,
        ";" => 5.0 / 18.0,
        "!" => -3.0 / 18.0,
        " " => 0.25,
        "quad" => 1.0,
        "qquad" => 2.0,
        _ => return None,
    })
}

/// One letter-style command's codepoint remap into the Mathematical
/// Alphanumeric block, Letterlike Symbols holes included. A character
/// outside the command's alphabet stays itself.
fn map_alphabet(name: &str, c: char) -> char {
    let a = c as u32;
    let mapped = match name {
        "mathbb" => match c {
            'C' => 0x2102,
            'H' => 0x210D,
            'N' => 0x2115,
            'P' => 0x2119,
            'Q' => 0x211A,
            'R' => 0x211D,
            'Z' => 0x2124,
            'A'..='Z' => 0x1D538 + (a - 'A' as u32),
            'a'..='z' => 0x1D552 + (a - 'a' as u32),
            '0'..='9' => 0x1D7D8 + (a - '0' as u32),
            _ => a,
        },
        "mathbf" => match c {
            'A'..='Z' => 0x1D400 + (a - 'A' as u32),
            'a'..='z' => 0x1D41A + (a - 'a' as u32),
            '0'..='9' => 0x1D7CE + (a - '0' as u32),
            '\u{0391}'..='\u{03A9}' => 0x1D6A8 + (a - 0x0391),
            '\u{03B1}'..='\u{03C9}' => 0x1D6C2 + (a - 0x03B1),
            _ => a,
        },
        "mathit" => match c {
            'h' => 0x210E,
            'A'..='Z' => 0x1D434 + (a - 'A' as u32),
            'a'..='z' => 0x1D44E + (a - 'a' as u32),
            _ => a,
        },
        "mathcal" => match c {
            'B' => 0x212C,
            'E' => 0x2130,
            'F' => 0x2131,
            'H' => 0x210B,
            'I' => 0x2110,
            'L' => 0x2112,
            'M' => 0x2133,
            'R' => 0x211B,
            'e' => 0x212F,
            'g' => 0x210A,
            'o' => 0x2134,
            'A'..='Z' => 0x1D49C + (a - 'A' as u32),
            'a'..='z' => 0x1D4B6 + (a - 'a' as u32),
            _ => a,
        },
        "mathfrak" => match c {
            'C' => 0x212D,
            'H' => 0x210C,
            'I' => 0x2111,
            'R' => 0x211C,
            'Z' => 0x2128,
            'A'..='Z' => 0x1D504 + (a - 'A' as u32),
            'a'..='z' => 0x1D51E + (a - 'a' as u32),
            _ => a,
        },
        "mathsf" => match c {
            'A'..='Z' => 0x1D5A0 + (a - 'A' as u32),
            'a'..='z' => 0x1D5BA + (a - 'a' as u32),
            '0'..='9' => 0x1D7E2 + (a - '0' as u32),
            _ => a,
        },
        "mathtt" => match c {
            'A'..='Z' => 0x1D670 + (a - 'A' as u32),
            'a'..='z' => 0x1D68A + (a - 'a' as u32),
            '0'..='9' => 0x1D7F6 + (a - '0' as u32),
            _ => a,
        },
        _ => a,
    };
    char::from_u32(mapped).unwrap_or(c)
}

/// Applies a letter-style remap through a list: symbols map, every
/// nested field recurses, literals and text stay themselves.
fn restyle(list: &mut MathList, name: &str) {
    for noad in &mut list.0 {
        let Noad::Atom(atom) = noad;
        restyle_field(&mut atom.nucleus, name);
        if let Some(s) = &mut atom.sup {
            restyle(s, name);
        }
        if let Some(s) = &mut atom.sub {
            restyle(s, name);
        }
    }
}

fn restyle_field(field: &mut Field, name: &str) {
    match field {
        Field::Symbol(c) => *c = map_alphabet(name, *c),
        Field::List(inner) => restyle(inner, name),
        Field::Fraction {
            numerator,
            denominator,
            ..
        } => {
            restyle(numerator, name);
            restyle(denominator, name);
        }
        Field::Radical { radicand, degree } => {
            restyle(radicand, name);
            if let Some(deg) = degree {
                restyle(deg, name);
            }
        }
        Field::LeftRight { body, .. } => restyle(body, name),
        Field::Accent { base, .. } => restyle(base, name),
        Field::Table { rows, .. } => {
            for row in rows {
                for cell in row {
                    restyle(cell, name);
                }
            }
        }
        Field::Text(_) | Field::Literal(_) | Field::Kern(_) | Field::Empty => {}
    }
}

fn classify_char(c: char) -> AtomClass {
    match c {
        '+' | '\u{2212}' | '-' | '*' => AtomClass::Bin,
        '=' | '<' | '>' | ':' => AtomClass::Rel,
        '(' | '[' => AtomClass::Open,
        ')' | ']' => AtomClass::Close,
        ',' | ';' => AtomClass::Punct,
        _ => AtomClass::Ord,
    }
}

fn literal_atom(text: impl Into<String>, span: std::ops::Range<usize>) -> Atom {
    Atom {
        class: AtomClass::Ord,
        nucleus: Field::Literal(text.into()),
        sup: None,
        sub: None,
        limits: Limits::default(),
        span: span.clone(),
        nucleus_span: span,
    }
}

/// The TeX demotion rule: a binary atom with no quantity on its left reads
/// as an ordinary symbol, so leading signs and doubled operators space as
/// signs. Applies per list; groups and scripts recurse through `list`.
fn demote_bins(items: &mut [Noad]) {
    let mut prev: Option<AtomClass> = None;
    for noad in items.iter_mut() {
        let Noad::Atom(atom) = noad;
        // Kerns are not atoms: demotion reads through them.
        if matches!(atom.nucleus, Field::Kern(_)) {
            continue;
        }
        if atom.class == AtomClass::Bin
            && !matches!(
                prev,
                Some(AtomClass::Ord) | Some(AtomClass::Close) | Some(AtomClass::Inner)
            )
        {
            atom.class = AtomClass::Ord;
        }
        prev = Some(atom.class);
    }
}

struct Parser<'a> {
    tokens: Vec<crate::token::Token>,
    pos: usize,
    src: &'a str,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&crate::token::Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<crate::token::Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// Parses noads until end of input, or until the matching group closer
    /// when `top` is false. A stray closer at top level degrades to a
    /// literal; a missing closer closes at the end.
    fn list(&mut self, top: bool) -> MathList {
        use crate::token::TokenKind as K;
        let mut items: Vec<Noad> = Vec::new();
        while let Some(tok) = self.peek() {
            let span = tok.span.clone();
            match &tok.kind {
                K::EndGroup => {
                    self.pos += 1;
                    if !top {
                        break;
                    }
                    items.push(Noad::Atom(literal_atom("}", span)));
                }
                K::Align => {
                    self.pos += 1;
                    items.push(Noad::Atom(literal_atom("&", span)));
                }
                K::Sup | K::Sub | K::Prime => {
                    // A script with nothing to hang on: TeX's implicit empty
                    // group becomes an empty-nucleus atom.
                    let mut atom = Atom {
                        class: AtomClass::Ord,
                        nucleus: Field::Empty,
                        sup: None,
                        sub: None,
                        limits: Limits::default(),
                        span: span.start..span.start,
                        nucleus_span: span.start..span.start,
                    };
                    self.scripts(&mut atom);
                    items.push(Noad::Atom(atom));
                }
                _ => {
                    if let Some(mut atom) = self.atom() {
                        self.scripts(&mut atom);
                        items.push(Noad::Atom(atom));
                    }
                }
            }
        }
        demote_bins(&mut items);
        MathList(items)
    }

    /// One scriptless atom from the stream: a character, a command, or a
    /// braced group. The caller has excluded every other token kind.
    fn atom(&mut self) -> Option<Atom> {
        use crate::token::TokenKind as K;
        let tok = self.next()?;
        match tok.kind {
            K::Char(c) => {
                // Math mode's hyphen is the minus sign.
                let c = if c == '-' { '\u{2212}' } else { c };
                Some(Atom {
                    class: classify_char(c),
                    nucleus: Field::Symbol(c),
                    sup: None,
                    sub: None,
                    limits: Limits::default(),
                    span: tok.span.clone(),
                    nucleus_span: tok.span,
                })
            }
            K::Command(name) => Some(match name.as_str() {
                "frac" => self.fraction(tok.span, true),
                "binom" => self.binom(tok.span),
                "sqrt" => self.radical(tok.span),
                "left" => self.left_right(tok.span),
                "right" => {
                    // A stray closer: its delimiter goes with it, the pair
                    // degrades to a literal.
                    let _ = self.delimiter();
                    literal_atom("\\right", tok.span)
                }
                "text" => self.text(tok.span),
                "begin" => self.environment(tok.span),
                "end" => {
                    // A stray closer: its name goes with it, the pair
                    // degrades to a literal.
                    let _ = self.env_name();
                    let end = self.consumed_end(tok.span.end);
                    let span = tok.span.start..end;
                    literal_atom(
                        self.src.get(span.clone()).unwrap_or("\\end").to_string(),
                        span,
                    )
                }
                "mathbb" | "mathbf" | "mathcal" | "mathfrak" | "mathit" | "mathsf" | "mathtt" => {
                    self.styled(tok.span, &name)
                }
                _ => {
                    if let Some(ems) = spacing_ems(&name) {
                        Atom {
                            class: AtomClass::Ord,
                            nucleus: Field::Kern(ems),
                            sup: None,
                            sub: None,
                            limits: Limits::default(),
                            span: tok.span.clone(),
                            nucleus_span: tok.span,
                        }
                    } else if let Ok(i) = ACCENTS.binary_search_by(|row| row.0.cmp(name.as_str())) {
                        let (_, accent, stretch) = ACCENTS[i];
                        self.accent(tok.span, accent, stretch)
                    } else if let Ok(i) = OP_NAMES.binary_search_by(|row| row.0.cmp(name.as_str()))
                    {
                        Atom {
                            class: AtomClass::Op,
                            nucleus: Field::Text(name.clone()),
                            sup: None,
                            sub: None,
                            limits: if OP_NAMES[i].1 {
                                Limits::Default
                            } else {
                                Limits::NoLimits
                            },
                            span: tok.span.clone(),
                            nucleus_span: tok.span,
                        }
                    } else {
                        match vocabulary_lookup(&name) {
                            Some((ch, class)) => Atom {
                                class,
                                nucleus: Field::Symbol(ch),
                                sup: None,
                                sub: None,
                                limits: Limits::default(),
                                span: tok.span.clone(),
                                nucleus_span: tok.span,
                            },
                            None => literal_atom(format!("\\{name}"), tok.span),
                        }
                    }
                }
            }),
            K::BeginGroup => {
                let inner = self.list(false);
                let end = self
                    .tokens
                    .get(self.pos.saturating_sub(1))
                    .map(|t| t.span.end)
                    .unwrap_or(tok.span.end);
                Some(Atom {
                    class: AtomClass::Ord,
                    nucleus: Field::List(inner),
                    sup: None,
                    sub: None,
                    limits: Limits::default(),
                    span: tok.span.start..end,
                    nucleus_span: tok.span.start..end,
                })
            }
            _ => None,
        }
    }

    /// A construct atom covering `start` through everything consumed since.
    fn construct(
        &mut self,
        start: std::ops::Range<usize>,
        nucleus: Field,
        class: AtomClass,
    ) -> Atom {
        let end = self.consumed_end(start.end);
        Atom {
            class,
            nucleus,
            sup: None,
            sub: None,
            limits: Limits::default(),
            span: start.start..end,
            nucleus_span: start.start..end,
        }
    }

    /// `\frac{num}{den}`; the argument reader accepts single tokens the
    /// way TeX does, so `\frac12` works.
    fn fraction(&mut self, start: std::ops::Range<usize>, bar: bool) -> Atom {
        let numerator = self.script_operand();
        let denominator = self.script_operand();
        self.construct(
            start,
            Field::Fraction {
                numerator,
                denominator,
                bar,
            },
            AtomClass::Inner,
        )
    }

    /// `\binom{n}{k}`: a barless stack inside stretched parentheses.
    fn binom(&mut self, start: std::ops::Range<usize>) -> Atom {
        let numerator = self.script_operand();
        let denominator = self.script_operand();
        let end = self.consumed_end(start.end);
        let span = start.start..end;
        let stack = Atom {
            class: AtomClass::Inner,
            nucleus: Field::Fraction {
                numerator,
                denominator,
                bar: false,
            },
            sup: None,
            sub: None,
            limits: Limits::default(),
            span: span.clone(),
            nucleus_span: span.clone(),
        };
        Atom {
            class: AtomClass::Inner,
            nucleus: Field::LeftRight {
                open: Some('('),
                close: Some(')'),
                body: MathList(vec![Noad::Atom(stack)]),
            },
            sup: None,
            sub: None,
            limits: Limits::default(),
            span: span.clone(),
            nucleus_span: span,
        }
    }

    /// A letter-style command: the operand parses normally, then its
    /// symbols remap into the command's alphabet. A single restyled atom
    /// keeps its own class; a longer operand wraps as a group.
    fn styled(&mut self, start: std::ops::Range<usize>, name: &str) -> Atom {
        let mut operand = self.script_operand();
        restyle(&mut operand, name);
        let end = self.consumed_end(start.end);
        let span = start.start..end;
        if operand.0.len() == 1 {
            let Noad::Atom(mut atom) = operand.0.pop().expect("one noad");
            atom.span = span.clone();
            atom.nucleus_span = span;
            atom
        } else {
            Atom {
                class: AtomClass::Ord,
                nucleus: Field::List(operand),
                sup: None,
                sub: None,
                limits: Limits::default(),
                span: span.clone(),
                nucleus_span: span,
            }
        }
    }

    /// An accent command over its operand.
    fn accent(&mut self, start: std::ops::Range<usize>, accent: char, stretch: bool) -> Atom {
        let base = self.script_operand();
        self.construct(
            start,
            Field::Accent {
                accent,
                stretch,
                base,
            },
            AtomClass::Ord,
        )
    }

    /// `\text{...}`: the braced source verbatim, spaces and nested braces
    /// included, which the tokenizer's spans recover from the source.
    /// Without a group the command degrades to a literal.
    fn text(&mut self, start: std::ops::Range<usize>) -> Atom {
        use crate::token::TokenKind as K;
        if !matches!(self.peek().map(|t| &t.kind), Some(K::BeginGroup)) {
            return literal_atom("\\text", start);
        }
        let open = self.next().expect("peeked");
        let content_start = open.span.end;
        let mut content_end = content_start;
        let mut depth = 1usize;
        while let Some(tok) = self.next() {
            match tok.kind {
                K::BeginGroup => depth += 1,
                K::EndGroup => {
                    depth -= 1;
                    if depth == 0 {
                        content_end = tok.span.start;
                        break;
                    }
                }
                _ => {}
            }
            content_end = tok.span.end;
        }
        let text = self.src.get(content_start..content_end).unwrap_or("");
        self.construct(start, Field::Text(text.to_string()), AtomClass::Ord)
    }

    /// `\begin{name} ... \end{name}`: cells split on `&`, rows on `\\`,
    /// each environment bringing its alignment, gap rule and fences. An
    /// unknown name skips to its `\end` and degrades to a literal; an
    /// unterminated body degrades whole.
    fn environment(&mut self, start: std::ops::Range<usize>) -> Atom {
        let Some(name) = self.env_name() else {
            let end = self.consumed_end(start.end);
            let span = start.start..end;
            return literal_atom(
                self.src.get(span.clone()).unwrap_or("\\begin").to_string(),
                span,
            );
        };
        type Fences = Option<(char, Option<char>)>;
        let known: Option<(Vec<ColAlign>, TableGaps, bool, Fences)> = match name.as_str() {
            "matrix" => Some((vec![ColAlign::Center], TableGaps::Em(1.0), false, None)),
            "smallmatrix" => Some((vec![ColAlign::Center], TableGaps::Em(0.5), true, None)),
            "pmatrix" => Some((
                vec![ColAlign::Center],
                TableGaps::Em(1.0),
                false,
                Some(('(', Some(')'))),
            )),
            "bmatrix" => Some((
                vec![ColAlign::Center],
                TableGaps::Em(1.0),
                false,
                Some(('[', Some(']'))),
            )),
            "Bmatrix" => Some((
                vec![ColAlign::Center],
                TableGaps::Em(1.0),
                false,
                Some(('{', Some('}'))),
            )),
            "vmatrix" => Some((
                vec![ColAlign::Center],
                TableGaps::Em(1.0),
                false,
                Some(('|', Some('|'))),
            )),
            "Vmatrix" => Some((
                vec![ColAlign::Center],
                TableGaps::Em(1.0),
                false,
                Some(('\u{2016}', Some('\u{2016}'))),
            )),
            "cases" => Some((
                vec![ColAlign::Left],
                TableGaps::Em(1.0),
                false,
                Some(('{', None)),
            )),
            "aligned" => Some((
                vec![ColAlign::Right, ColAlign::Left],
                TableGaps::Pairs,
                false,
                None,
            )),
            "array" => Some((self.array_spec(), TableGaps::Em(1.0), false, None)),
            _ => None,
        };
        let Some((align, gaps, small, fences)) = known else {
            self.skip_environment();
            let end = self.consumed_end(start.end);
            let span = start.start..end;
            return literal_atom(
                self.src.get(span.clone()).unwrap_or("\\begin").to_string(),
                span,
            );
        };
        let (rows, terminated) = self.table_cells();
        let end = self.consumed_end(start.end);
        let span = start.start..end;
        if !terminated {
            return literal_atom(
                self.src.get(span.clone()).unwrap_or("\\begin").to_string(),
                span,
            );
        }
        let table = Atom {
            class: AtomClass::Ord,
            nucleus: Field::Table {
                rows,
                align,
                gaps,
                small,
            },
            sup: None,
            sub: None,
            limits: Limits::default(),
            span: span.clone(),
            nucleus_span: span.clone(),
        };
        match fences {
            Some((open, close)) => Atom {
                class: AtomClass::Inner,
                nucleus: Field::LeftRight {
                    open: Some(open),
                    close,
                    body: MathList(vec![Noad::Atom(table)]),
                },
                sup: None,
                sub: None,
                limits: Limits::default(),
                span: span.clone(),
                nucleus_span: span,
            },
            None => table,
        }
    }

    /// The braced environment name after `\begin` or `\end`: letters
    /// and stars only, anything else answers none.
    fn env_name(&mut self) -> Option<String> {
        use crate::token::TokenKind as K;
        if !matches!(self.peek().map(|t| &t.kind), Some(K::BeginGroup)) {
            return None;
        }
        self.pos += 1;
        let mut name = String::new();
        while let Some(tok) = self.peek() {
            match &tok.kind {
                K::Char(c) => {
                    name.push(*c);
                    self.pos += 1;
                }
                K::EndGroup => {
                    self.pos += 1;
                    return Some(name);
                }
                _ => return None,
            }
        }
        None
    }

    /// `array`'s column specification: `r`, `c`, `l` collect, rules and
    /// separators pass quietly, a missing group means one centered
    /// column.
    fn array_spec(&mut self) -> Vec<ColAlign> {
        use crate::token::TokenKind as K;
        let mut align = Vec::new();
        if matches!(self.peek().map(|t| &t.kind), Some(K::BeginGroup)) {
            self.pos += 1;
            while let Some(tok) = self.peek() {
                match &tok.kind {
                    K::Char('l') => align.push(ColAlign::Left),
                    K::Char('c') => align.push(ColAlign::Center),
                    K::Char('r') => align.push(ColAlign::Right),
                    K::Char(_) => {}
                    K::EndGroup => {
                        self.pos += 1;
                        break;
                    }
                    _ => break,
                }
                self.pos += 1;
            }
        }
        if align.is_empty() {
            align.push(ColAlign::Center);
        }
        align
    }

    /// Consumes an unknown environment through its matching `\end`,
    /// nested environments counted; end of input stops the skip.
    fn skip_environment(&mut self) {
        use crate::token::TokenKind as K;
        let mut depth = 1usize;
        while let Some(tok) = self.next() {
            match &tok.kind {
                K::Command(c) if c == "begin" => depth += 1,
                K::Command(c) if c == "end" => {
                    let _ = self.env_name();
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// An environment body: atoms accumulate into cells, `&` closes a
    /// cell, `\\` a row, `\end` the table. End of input or the enclosing
    /// group's closer answers unterminated, the closer left in place.
    fn table_cells(&mut self) -> (Vec<Vec<MathList>>, bool) {
        use crate::token::TokenKind as K;
        let mut rows: Vec<Vec<MathList>> = Vec::new();
        let mut row: Vec<MathList> = Vec::new();
        let mut cell: Vec<Noad> = Vec::new();
        loop {
            let Some(tok) = self.peek() else {
                return (rows, false);
            };
            let span = tok.span.clone();
            match &tok.kind {
                K::Align => {
                    self.pos += 1;
                    demote_bins(&mut cell);
                    row.push(MathList(std::mem::take(&mut cell)));
                }
                K::Command(name) if name == "\\" => {
                    self.pos += 1;
                    demote_bins(&mut cell);
                    row.push(MathList(std::mem::take(&mut cell)));
                    rows.push(std::mem::take(&mut row));
                }
                K::Command(name) if name == "end" => {
                    self.pos += 1;
                    let _ = self.env_name();
                    demote_bins(&mut cell);
                    row.push(MathList(std::mem::take(&mut cell)));
                    // A trailing \\ leaves one empty row; TeX drops it.
                    let trailing_empty = row.len() == 1 && row[0].0.is_empty() && !rows.is_empty();
                    if !trailing_empty {
                        rows.push(std::mem::take(&mut row));
                    }
                    return (rows, true);
                }
                K::EndGroup => {
                    return (rows, false);
                }
                K::Sup | K::Sub | K::Prime => {
                    let mut atom = Atom {
                        class: AtomClass::Ord,
                        nucleus: Field::Empty,
                        sup: None,
                        sub: None,
                        limits: Limits::default(),
                        span: span.start..span.start,
                        nucleus_span: span.start..span.start,
                    };
                    self.scripts(&mut atom);
                    cell.push(Noad::Atom(atom));
                }
                _ => {
                    if let Some(mut atom) = self.atom() {
                        self.scripts(&mut atom);
                        cell.push(Noad::Atom(atom));
                    } else {
                        self.pos += 1;
                    }
                }
            }
        }
    }

    /// `\sqrt{x}` with the optional `[degree]`.
    fn radical(&mut self, start: std::ops::Range<usize>) -> Atom {
        use crate::token::TokenKind as K;
        let degree = if matches!(self.peek().map(|t| &t.kind), Some(K::Char('['))) {
            self.pos += 1;
            Some(self.list_until_char(']'))
        } else {
            None
        };
        let radicand = self.script_operand();
        self.construct(start, Field::Radical { radicand, degree }, AtomClass::Ord)
    }

    /// Elements up to a closing character, consumed; end of input closes.
    fn list_until_char(&mut self, closer: char) -> MathList {
        use crate::token::TokenKind as K;
        let mut items: Vec<Noad> = Vec::new();
        while let Some(tok) = self.peek() {
            if matches!(&tok.kind, K::Char(c) if *c == closer) {
                self.pos += 1;
                break;
            }
            if matches!(tok.kind, K::EndGroup) {
                break;
            }
            if let Some(mut atom) = self.atom() {
                self.scripts(&mut atom);
                items.push(Noad::Atom(atom));
            } else {
                self.pos += 1;
            }
        }
        demote_bins(&mut items);
        MathList(items)
    }

    /// `\left⟨delim⟩ ... \right⟨delim⟩`. A missing `\right` fails open at
    /// the end of input or at the enclosing group's closer, which stays
    /// for the group to consume.
    fn left_right(&mut self, start: std::ops::Range<usize>) -> Atom {
        use crate::token::TokenKind as K;
        let open = self.delimiter();
        let mut items: Vec<Noad> = Vec::new();
        let mut close = None;
        while let Some(tok) = self.peek() {
            match &tok.kind {
                K::Command(name) if name == "right" => {
                    self.pos += 1;
                    close = self.delimiter();
                    break;
                }
                K::EndGroup => break,
                _ => {
                    if let Some(mut atom) = self.atom() {
                        self.scripts(&mut atom);
                        items.push(Noad::Atom(atom));
                    } else {
                        self.pos += 1;
                    }
                }
            }
        }
        demote_bins(&mut items);
        self.construct(
            start,
            Field::LeftRight {
                open,
                close,
                body: MathList(items),
            },
            AtomClass::Inner,
        )
    }

    /// One delimiter token after `\left` or `\right`: a character, `.` for
    /// none, or a delimiter command. Anything else answers none and stays.
    fn delimiter(&mut self) -> Option<char> {
        use crate::token::TokenKind as K;
        let resolved = match self.peek().map(|t| &t.kind) {
            Some(K::Char('.')) => Some(None),
            Some(K::Char(c)) => Some(Some(*c)),
            Some(K::Command(name)) => DELIMITERS
                .binary_search_by(|row| row.0.cmp(name.as_str()))
                .ok()
                .map(|i| Some(DELIMITERS[i].1)),
            _ => None,
        };
        match resolved {
            Some(delim) => {
                self.pos += 1;
                delim
            }
            None => None,
        }
    }

    /// Attaches every following script marker to the atom. Repeated markers
    /// merge into the existing script list, TeX's double-script degraded
    /// quietly instead of erroring.
    fn scripts(&mut self, atom: &mut Atom) {
        use crate::token::TokenKind as K;
        while let Some(tok) = self.peek() {
            let span = tok.span.clone();
            match tok.kind {
                K::Command(ref name) if name == "limits" || name == "nolimits" => {
                    // TeX's postfix limit modifiers bind to an operator;
                    // on anything else they fall through as literals.
                    if atom.class != AtomClass::Op {
                        break;
                    }
                    atom.limits = if name == "limits" {
                        Limits::Limits
                    } else {
                        Limits::NoLimits
                    };
                    atom.span.end = atom.span.end.max(span.end);
                    self.pos += 1;
                }
                K::Prime => {
                    self.pos += 1;
                    let prime = Atom {
                        class: AtomClass::Ord,
                        nucleus: Field::Symbol('\u{2032}'),
                        sup: None,
                        sub: None,
                        limits: Limits::default(),
                        span: span.clone(),
                        nucleus_span: span.clone(),
                    };
                    atom.sup
                        .get_or_insert_with(MathList::default)
                        .0
                        .push(Noad::Atom(prime));
                    atom.span.end = atom.span.end.max(span.end);
                }
                K::Sup => {
                    self.pos += 1;
                    let operand = self.script_operand();
                    let target = atom.sup.get_or_insert_with(MathList::default);
                    target.0.extend(operand.0);
                    atom.span.end = self.consumed_end(atom.span.end);
                }
                K::Sub => {
                    self.pos += 1;
                    let operand = self.script_operand();
                    let target = atom.sub.get_or_insert_with(MathList::default);
                    target.0.extend(operand.0);
                    atom.span.end = self.consumed_end(atom.span.end);
                }
                _ => break,
            }
        }
    }

    fn consumed_end(&self, fallback: usize) -> usize {
        self.tokens
            .get(self.pos.saturating_sub(1))
            .map(|t| t.span.end)
            .unwrap_or(fallback)
    }

    /// One script operand: a single token or a braced group. A missing or
    /// impossible operand yields an empty list.
    fn script_operand(&mut self) -> MathList {
        use crate::token::TokenKind as K;
        let Some(tok) = self.peek() else {
            return MathList::default();
        };
        let span = tok.span.clone();
        match &tok.kind {
            K::BeginGroup => {
                self.pos += 1;
                self.list(false)
            }
            K::Char(_) | K::Command(_) => {
                let atom = self.atom().expect("peeked");
                MathList(vec![Noad::Atom(atom)])
            }
            K::Sup | K::Sub | K::Prime | K::Align | K::EndGroup => {
                // ^ with no legal operand: degrade the marker itself.
                let text = match tok.kind {
                    K::Sup => "^",
                    K::Sub => "_",
                    K::Prime => "'",
                    K::Align => "&",
                    _ => "}",
                };
                self.pos += 1;
                MathList(vec![Noad::Atom(literal_atom(text, span))])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atoms(tex: &str) -> Vec<Atom> {
        parse(tex).atoms().cloned().collect()
    }

    #[test]
    fn vocabulary_is_sorted_and_duplicate_free() {
        for pair in VOCABULARY.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
    }

    #[test]
    fn commands_resolve_symbol_and_class() {
        let a = atoms("\\alpha\\pm\\leq\\sum");
        assert_eq!(a.len(), 4);
        assert_eq!(a[0].nucleus, Field::Symbol('\u{03B1}'));
        assert_eq!(a[0].class, AtomClass::Ord);
        assert_eq!(a[1].nucleus, Field::Symbol('\u{00B1}'));
        assert_eq!(a[1].class, AtomClass::Bin);
        assert_eq!(a[2].class, AtomClass::Rel);
        assert_eq!(a[3].nucleus, Field::Symbol('\u{2211}'));
        assert_eq!(a[3].class, AtomClass::Op);
    }

    #[test]
    fn plain_characters_classify() {
        let a = atoms("x+2=y,");
        let classes: Vec<AtomClass> = a.iter().map(|a| a.class).collect();
        assert_eq!(
            classes,
            vec![
                AtomClass::Ord,
                AtomClass::Bin,
                AtomClass::Ord,
                AtomClass::Rel,
                AtomClass::Ord,
                AtomClass::Punct,
            ]
        );
    }

    #[test]
    fn scripts_attach_to_their_atom() {
        let a = atoms("x^2");
        assert_eq!(a.len(), 1);
        let sup = a[0].sup.as_ref().expect("sup");
        assert_eq!(sup.atoms().next().unwrap().nucleus, Field::Symbol('2'));
        assert!(a[0].sub.is_none());

        let a = atoms("a_i");
        assert!(a[0].sup.is_none());
        assert!(a[0].sub.is_some());

        let a = atoms("x_i^2");
        assert!(a[0].sup.is_some() && a[0].sub.is_some());
    }

    #[test]
    fn braced_scripts_and_group_nuclei() {
        let a = atoms("x^{ab}");
        let sup = a[0].sup.as_ref().unwrap();
        assert_eq!(sup.atoms().count(), 2);

        let a = atoms("{ab}c");
        assert_eq!(a.len(), 2);
        match &a[0].nucleus {
            Field::List(inner) => assert_eq!(inner.atoms().count(), 2),
            other => panic!("expected group nucleus, got {other:?}"),
        }
    }

    #[test]
    fn hyphen_reads_as_minus() {
        let a = atoms("a-b");
        assert_eq!(a[1].nucleus, Field::Symbol('\u{2212}'));
        assert_eq!(a[1].class, AtomClass::Bin);
    }

    #[test]
    fn binary_atoms_demote_where_no_operand_precedes() {
        // Leading, after another Bin, after Rel, after Open: Ord.
        let a = atoms("+x");
        assert_eq!(a[0].class, AtomClass::Ord);
        let a = atoms("a+-b");
        assert_eq!(a[2].class, AtomClass::Ord);
        let a = atoms("a=-b");
        assert_eq!(a[2].class, AtomClass::Ord);
        let a = atoms("(-b)");
        assert_eq!(a[1].class, AtomClass::Ord);
        // With a real left operand it stays binary.
        let a = atoms("a-b");
        assert_eq!(a[1].class, AtomClass::Bin);
    }

    #[test]
    fn unknown_commands_become_literals() {
        let a = atoms("\\foobar x");
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].nucleus, Field::Literal("\\foobar".into()));
        assert_eq!(a[0].class, AtomClass::Ord);
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
    }

    #[test]
    fn stray_closers_and_alignment_degrade_to_literals() {
        let a = atoms("}x");
        assert_eq!(a[0].nucleus, Field::Literal("}".into()));
        let a = atoms("a&b");
        assert_eq!(a[1].nucleus, Field::Literal("&".into()));
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn primes_become_superscripts() {
        let a = atoms("x'");
        let sup = a[0].sup.as_ref().expect("prime lands in sup");
        assert_eq!(
            sup.atoms().next().unwrap().nucleus,
            Field::Symbol('\u{2032}')
        );
        let a = atoms("x''");
        assert_eq!(a[0].sup.as_ref().unwrap().atoms().count(), 2);
    }

    #[test]
    fn spans_stamp_source_bytes() {
        let a = atoms("x^2+\\alpha");
        assert_eq!(a[0].span, 0..3);
        assert_eq!(a[1].span, 3..4);
        assert_eq!(a[2].span, 4..10);
    }

    #[test]
    fn hostile_input_never_panics() {
        for tex in [
            "", "^", "_", "^^", "{", "}", "{{{", "}}}", "x^", "x_", "\\", "x\\", "&", "\\\\",
            "a^{b", "π^é", "%", "x%",
        ] {
            let _ = parse(tex);
        }
    }

    #[test]
    fn unclosed_group_closes_at_end() {
        let a = atoms("{ab");
        assert_eq!(a.len(), 1);
        match &a[0].nucleus {
            Field::List(inner) => assert_eq!(inner.atoms().count(), 2),
            other => panic!("expected group, got {other:?}"),
        }
    }

    #[test]
    fn frac_takes_two_arguments() {
        let a = atoms("\\frac{a+b}{2}x");
        assert_eq!(a.len(), 2);
        let Field::Fraction {
            numerator,
            denominator,
            bar,
        } = &a[0].nucleus
        else {
            panic!("expected fraction, got {:?}", a[0].nucleus)
        };
        assert!(bar);
        assert_eq!(numerator.atoms().count(), 3);
        assert_eq!(denominator.atoms().count(), 1);
        assert_eq!(a[0].class, AtomClass::Inner);
        // Single-token arguments work without braces.
        let a = atoms("\\frac12");
        let Field::Fraction { numerator, .. } = &a[0].nucleus else {
            panic!()
        };
        assert_eq!(
            numerator.atoms().next().unwrap().nucleus,
            Field::Symbol('1')
        );
    }

    #[test]
    fn binom_is_a_barless_stack_in_parens() {
        let a = atoms("\\binom{n}{k}");
        let Field::LeftRight { open, close, body } = &a[0].nucleus else {
            panic!("expected delimited group, got {:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (Some('('), Some(')')));
        let inner = body.atoms().next().expect("stack inside");
        let Field::Fraction { bar, .. } = &inner.nucleus else {
            panic!("expected stack, got {:?}", inner.nucleus)
        };
        assert!(!bar);
    }

    #[test]
    fn sqrt_takes_optional_degree() {
        let a = atoms("\\sqrt{x+1}");
        let Field::Radical { radicand, degree } = &a[0].nucleus else {
            panic!("expected radical, got {:?}", a[0].nucleus)
        };
        assert_eq!(radicand.atoms().count(), 3);
        assert!(degree.is_none());
        let a = atoms("\\sqrt[3]{x}");
        let Field::Radical { degree, .. } = &a[0].nucleus else {
            panic!()
        };
        let deg = degree.as_ref().expect("degree parsed");
        assert_eq!(deg.atoms().next().unwrap().nucleus, Field::Symbol('3'));
    }

    #[test]
    fn left_right_wraps_its_body() {
        let a = atoms("\\left( \\frac{a}{b} \\right)^2");
        assert_eq!(a.len(), 1);
        let Field::LeftRight { open, close, body } = &a[0].nucleus else {
            panic!("expected delimited group, got {:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (Some('('), Some(')')));
        assert_eq!(body.atoms().count(), 1);
        assert_eq!(a[0].class, AtomClass::Inner);
        assert!(a[0].sup.is_some(), "the script rides the whole group");
        // The dot delimiter means none; command delimiters resolve.
        let a = atoms("\\left. x \\right\\}");
        let Field::LeftRight { open, close, .. } = &a[0].nucleus else {
            panic!()
        };
        assert_eq!((*open, *close), (None, Some('}')));
    }

    #[test]
    fn unmatched_left_fails_open() {
        let a = atoms("\\left( x");
        assert!(!a.is_empty());
        let flat = parse("\\left( x");
        assert!(flat.atoms().count() >= 1, "never panics, keeps content");
        let _ = parse("x \\right)");
        let _ = parse("\\left");
        let _ = parse("\\left(\\left[x");
    }

    #[test]
    fn limits_modifiers_bind_to_operators() {
        let a = atoms("\\sum\\limits x");
        assert_eq!(a[0].limits, Limits::Limits);
        assert_eq!(a.len(), 2);
        let a = atoms("\\int\\nolimits x");
        assert_eq!(a[0].limits, Limits::NoLimits);
        // On a non-operator the modifier is a quiet literal.
        let a = atoms("x\\limits");
        assert_eq!(a[1].nucleus, Field::Literal("\\limits".into()));
    }

    #[test]
    fn spacing_commands_become_kerns() {
        let a = atoms("a\\,b");
        assert_eq!(a.len(), 3);
        assert_eq!(a[1].nucleus, Field::Kern(3.0 / 18.0));
        let a = atoms("\\:");
        assert_eq!(a[0].nucleus, Field::Kern(4.0 / 18.0));
        let a = atoms("\\;");
        assert_eq!(a[0].nucleus, Field::Kern(5.0 / 18.0));
        let a = atoms("\\!");
        assert_eq!(a[0].nucleus, Field::Kern(-3.0 / 18.0));
        let a = atoms("\\quad");
        assert_eq!(a[0].nucleus, Field::Kern(1.0));
        let a = atoms("\\qquad");
        assert_eq!(a[0].nucleus, Field::Kern(2.0));
    }

    #[test]
    fn kerns_are_transparent_to_demotion() {
        // The kern hides nothing: + still has its left operand.
        let a = atoms("a\\,+b");
        assert_eq!(a[2].class, AtomClass::Bin);
        // A leading kern provides no operand.
        let a = atoms("\\,+b");
        assert_eq!(a[1].class, AtomClass::Ord);
    }

    #[test]
    fn alphabet_commands_remap_codepoints() {
        for (tex, mapped) in [
            ("\\mathbb{R}", '\u{211D}'),
            ("\\mathbb{A}", '\u{1D538}'),
            ("\\mathbf{A}", '\u{1D400}'),
            ("\\mathit{A}", '\u{1D434}'),
            ("\\mathcal{L}", '\u{2112}'),
            ("\\mathfrak{g}", '\u{1D524}'),
            ("\\mathsf{x}", '\u{1D5D1}'),
            ("\\mathtt{0}", '\u{1D7F6}'),
        ] {
            let a = atoms(tex);
            assert_eq!(a[0].nucleus, Field::Symbol(mapped), "{tex}");
        }
    }

    #[test]
    fn alphabet_commands_reach_nested_groups() {
        let a = atoms("\\mathbf{ab}");
        let Field::List(inner) = &a[0].nucleus else {
            panic!("expected group nucleus, got {:?}", a[0].nucleus)
        };
        let mapped: Vec<Field> = inner.atoms().map(|at| at.nucleus.clone()).collect();
        assert_eq!(
            mapped,
            vec![Field::Symbol('\u{1D41A}'), Field::Symbol('\u{1D41B}')]
        );
    }

    #[test]
    fn text_keeps_its_source_verbatim() {
        let a = atoms("\\text{if }x");
        assert_eq!(a[0].nucleus, Field::Text("if ".into()));
        assert_eq!(a[0].class, AtomClass::Ord);
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        // Nested braces stay inside.
        let a = atoms("\\text{a{b}c}");
        assert_eq!(a[0].nucleus, Field::Text("a{b}c".into()));
        // No group degrades quietly.
        let a = atoms("\\text x");
        assert_eq!(a[0].nucleus, Field::Literal("\\text".into()));
    }

    #[test]
    fn operator_names_are_upright_op_atoms() {
        let a = atoms("\\sin x");
        assert_eq!(a[0].nucleus, Field::Text("sin".into()));
        assert_eq!(a[0].class, AtomClass::Op);
        assert_eq!(a[0].limits, Limits::NoLimits);
        let a = atoms("\\lim_n x");
        assert_eq!(a[0].nucleus, Field::Text("lim".into()));
        assert_eq!(
            a[0].limits,
            Limits::Default,
            "lim stacks its limits in display"
        );
    }

    #[test]
    fn accents_parse_with_their_stretch_flag() {
        let a = atoms("\\hat x");
        let Field::Accent {
            accent,
            stretch,
            base,
        } = &a[0].nucleus
        else {
            panic!("expected accent, got {:?}", a[0].nucleus)
        };
        assert_eq!((*accent, *stretch), ('\u{0302}', false));
        assert_eq!(base.atoms().next().unwrap().nucleus, Field::Symbol('x'));
        let a = atoms("\\widehat{abc}");
        let Field::Accent { stretch, base, .. } = &a[0].nucleus else {
            panic!("expected accent, got {:?}", a[0].nucleus)
        };
        assert!(stretch);
        assert_eq!(base.atoms().count(), 3);
        let a = atoms("\\vec v");
        let Field::Accent { accent, .. } = &a[0].nucleus else {
            panic!("expected accent, got {:?}", a[0].nucleus)
        };
        assert_eq!(*accent, '\u{20D7}');
        let a = atoms("\\bar y");
        let Field::Accent { accent, .. } = &a[0].nucleus else {
            panic!("expected accent, got {:?}", a[0].nucleus)
        };
        assert_eq!(*accent, '\u{0304}');
    }

    #[test]
    fn environments_parse_to_tables() {
        let a = atoms("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}");
        assert_eq!(a.len(), 1);
        let Field::LeftRight { open, close, body } = &a[0].nucleus else {
            panic!("expected fenced table, got {:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (Some('('), Some(')')));
        let inner = body.atoms().next().expect("the table inside");
        let Field::Table {
            rows, align, small, ..
        } = &inner.nucleus
        else {
            panic!("expected table, got {:?}", inner.nucleus)
        };
        assert!(!small);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), 2);
        assert_eq!(
            rows[1][1].atoms().next().unwrap().nucleus,
            Field::Symbol('d')
        );
        assert_eq!(align, &vec![ColAlign::Center]);
        // A bare matrix takes no fences; smallmatrix flags its size.
        let a = atoms("\\begin{matrix} a \\end{matrix}");
        assert!(matches!(&a[0].nucleus, Field::Table { small: false, .. }));
        let a = atoms("\\begin{smallmatrix} a \\end{smallmatrix}");
        assert!(matches!(&a[0].nucleus, Field::Table { small: true, .. }));
    }

    #[test]
    fn the_matrix_family_picks_its_fences() {
        for (tex, open, close) in [
            ("\\begin{bmatrix} a \\end{bmatrix}", '[', ']'),
            ("\\begin{vmatrix} a \\end{vmatrix}", '|', '|'),
            ("\\begin{Vmatrix} a \\end{Vmatrix}", '\u{2016}', '\u{2016}'),
        ] {
            let a = atoms(tex);
            let Field::LeftRight {
                open: o, close: c, ..
            } = &a[0].nucleus
            else {
                panic!("expected fenced table for {tex}, got {:?}", a[0].nucleus)
            };
            assert_eq!((*o, *c), (Some(open), Some(close)), "{tex}");
        }
    }

    #[test]
    fn cases_aligned_and_array_set_their_columns() {
        let a = atoms("\\begin{cases} x & y \\\\ 0 & z \\end{cases}");
        let Field::LeftRight { open, close, body } = &a[0].nucleus else {
            panic!("expected braced table, got {:?}", a[0].nucleus)
        };
        assert_eq!((*open, *close), (Some('{'), None));
        let inner = body.atoms().next().unwrap();
        let Field::Table { align, .. } = &inner.nucleus else {
            panic!("expected table, got {:?}", inner.nucleus)
        };
        assert_eq!(align, &vec![ColAlign::Left]);

        let a = atoms("\\begin{aligned} x &= y \\\\ z &= w \\end{aligned}");
        let Field::Table { align, rows, .. } = &a[0].nucleus else {
            panic!("expected table, got {:?}", a[0].nucleus)
        };
        assert_eq!(align, &vec![ColAlign::Right, ColAlign::Left]);
        assert_eq!(rows.len(), 2);

        let a = atoms("\\begin{array}{rcl} a & b & c \\end{array}");
        let Field::Table { align, .. } = &a[0].nucleus else {
            panic!("expected table, got {:?}", a[0].nucleus)
        };
        assert_eq!(
            align,
            &vec![ColAlign::Right, ColAlign::Center, ColAlign::Left]
        );
    }

    #[test]
    fn broken_environments_degrade_to_literals() {
        let a = atoms("\\begin{pmatrix} a & b");
        assert!(
            matches!(&a[0].nucleus, Field::Literal(t) if t.contains("\\begin{pmatrix}")),
            "unterminated environment degrades whole, got {:?}",
            a[0].nucleus
        );
        let a = atoms("\\begin{foo} x \\end{foo} y");
        assert!(
            matches!(&a[0].nucleus, Field::Literal(t) if t.contains("foo")),
            "unknown environment degrades, got {:?}",
            a[0].nucleus
        );
        assert_eq!(a[1].nucleus, Field::Symbol('y'));
        let a = atoms("\\end{pmatrix} x");
        assert!(matches!(&a[0].nucleus, Field::Literal(_)));
        assert_eq!(a[1].nucleus, Field::Symbol('x'));
        let _ = parse("\\begin");
        let _ = parse("\\begin{");
        let _ = parse("\\begin{pmatrix");
        let _ = parse("{\\begin{matrix} a}");
    }
}
