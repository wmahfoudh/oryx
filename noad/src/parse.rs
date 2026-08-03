//! The parser: tokens to a math list.
//!
//! Recursive descent with no error path that aborts. Unknown commands
//! become literal atoms, stray closers and alignment markers degrade to
//! literals, and an unclosed group closes at the end of input. Hostile
//! input costs a fallback, never a panic.

use crate::mlist::{Atom, AtomClass, Field, MathList, Noad};

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
    };
    parser.list(true)
}

fn vocabulary_lookup(name: &str) -> Option<(char, AtomClass)> {
    VOCABULARY
        .binary_search_by(|row| row.0.cmp(name))
        .ok()
        .map(|i| (VOCABULARY[i].1, VOCABULARY[i].2))
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

struct Parser {
    tokens: Vec<crate::token::Token>,
    pos: usize,
}

impl Parser {
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
            K::Char(c) => Some(Atom {
                class: classify_char(c),
                nucleus: Field::Symbol(c),
                sup: None,
                sub: None,
                span: tok.span.clone(),
                nucleus_span: tok.span,
            }),
            K::Command(name) => Some(match vocabulary_lookup(&name) {
                Some((ch, class)) => Atom {
                    class,
                    nucleus: Field::Symbol(ch),
                    sup: None,
                    sub: None,
                    span: tok.span.clone(),
                    nucleus_span: tok.span,
                },
                None => literal_atom(format!("\\{name}"), tok.span),
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
                    span: tok.span.start..end,
                    nucleus_span: tok.span.start..end,
                })
            }
            _ => None,
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
                K::Prime => {
                    self.pos += 1;
                    let prime = Atom {
                        class: AtomClass::Ord,
                        nucleus: Field::Symbol('\u{2032}'),
                        sup: None,
                        sub: None,
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
}
