//! The TeX math tokenizer.
//!
//! TeX's lexical rules cut down to math mode: control sequences, groups,
//! script markers, alignment, primes, and comments. Whitespace separates
//! tokens and is otherwise ignored, as TeX math ignores it. Every token
//! carries the byte range of its source, the origin of the source stamps
//! layout hands back.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// A control sequence, name without the backslash: `frac`, or a single
    /// non-letter for control symbols: `,`, `\`, `{`.
    Command(String),
    Char(char),
    BeginGroup,
    EndGroup,
    Sup,
    Sub,
    Align,
    Prime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Range<usize>,
}

/// Tokenizes a TeX math string. Total: every byte lands in a token, a
/// comment, or ignored whitespace; no input errors.
pub fn tokenize(tex: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let bytes = tex.as_bytes();
    let mut chars = tex.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        match c {
            '%' => {
                for (_, n) in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            '\\' => {
                let mut end = start + 1;
                let mut name = String::new();
                if let Some(&(_, first)) = chars.peek() {
                    if first.is_ascii_alphabetic() {
                        while let Some(&(i, n)) = chars.peek() {
                            if n.is_ascii_alphabetic() {
                                name.push(n);
                                end = i + n.len_utf8();
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    } else {
                        name.push(first);
                        end = start + 1 + first.len_utf8();
                        chars.next();
                    }
                }
                if name.is_empty() {
                    // A lone trailing backslash degrades to its character.
                    out.push(Token {
                        kind: TokenKind::Char('\\'),
                        span: start..start + 1,
                    });
                } else {
                    out.push(Token {
                        kind: TokenKind::Command(name),
                        span: start..end,
                    });
                }
            }
            '{' => out.push(Token {
                kind: TokenKind::BeginGroup,
                span: start..start + 1,
            }),
            '}' => out.push(Token {
                kind: TokenKind::EndGroup,
                span: start..start + 1,
            }),
            '^' => out.push(Token {
                kind: TokenKind::Sup,
                span: start..start + 1,
            }),
            '_' => out.push(Token {
                kind: TokenKind::Sub,
                span: start..start + 1,
            }),
            '&' => out.push(Token {
                kind: TokenKind::Align,
                span: start..start + 1,
            }),
            '\'' => out.push(Token {
                kind: TokenKind::Prime,
                span: start..start + 1,
            }),
            _ => out.push(Token {
                kind: TokenKind::Char(c),
                span: start..start + c.len_utf8(),
            }),
        }
    }
    debug_assert!(out.iter().all(|t| t.span.end <= bytes.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(tex: &str) -> Vec<TokenKind> {
        tokenize(tex).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn commands_take_letter_runs_and_single_symbols() {
        assert_eq!(
            kinds("\\frac\\alpha"),
            vec![
                TokenKind::Command("frac".into()),
                TokenKind::Command("alpha".into()),
            ]
        );
        assert_eq!(
            kinds("\\, \\\\ \\{"),
            vec![
                TokenKind::Command(",".into()),
                TokenKind::Command("\\".into()),
                TokenKind::Command("{".into()),
            ]
        );
    }

    #[test]
    fn specials_and_chars() {
        assert_eq!(
            kinds("{x^2_i}&'"),
            vec![
                TokenKind::BeginGroup,
                TokenKind::Char('x'),
                TokenKind::Sup,
                TokenKind::Char('2'),
                TokenKind::Sub,
                TokenKind::Char('i'),
                TokenKind::EndGroup,
                TokenKind::Align,
                TokenKind::Prime,
            ]
        );
    }

    #[test]
    fn whitespace_ignored_and_comments_run_to_line_end() {
        assert_eq!(
            kinds("x % rest ^ ignored\n+ y"),
            vec![
                TokenKind::Char('x'),
                TokenKind::Char('+'),
                TokenKind::Char('y'),
            ]
        );
    }

    #[test]
    fn spans_cover_source_bytes() {
        let toks = tokenize("\\alpha^2");
        assert_eq!(toks[0].span, 0..6);
        assert_eq!(toks[1].span, 6..7);
        assert_eq!(toks[2].span, 7..8);
    }

    #[test]
    fn multibyte_and_trailing_backslash_never_panic() {
        tokenize("π^é");
        tokenize("\\");
        tokenize("x\\");
        let toks = tokenize("π");
        assert_eq!(toks[0].span, 0..2);
    }
}
