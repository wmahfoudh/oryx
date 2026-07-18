//! Maps syntect parse scopes onto theme syntax roles at load time.

use std::ops::Range;
use std::sync::OnceLock;

use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxRole {
    Keyword,
    String,
    Number,
    Function,
    Type,
    Comment,
    Operator,
    Variable,
    Punctuation,
    Plain,
}

/// Per-line styled ranges for a code block. Lines with no recognized
/// language come back as single Plain ranges.
pub fn spans(lines: &[String], language: Option<&str>) -> Vec<Vec<(Range<usize>, SyntaxRole)>> {
    let set = syntax_set();
    let syntax = language
        .and_then(resolve_syntax)
        .unwrap_or_else(|| set.find_syntax_plain_text());
    let mut parse = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    lines
        .iter()
        .map(|line| {
            let text = format!("{line}\n");
            let ops = parse.parse_line(&text, set).unwrap_or_default();
            let mut ranges: Vec<(Range<usize>, SyntaxRole)> = Vec::new();
            let mut last = 0usize;
            let mut push = |from: usize, to: usize, stack: &ScopeStack| {
                let to = to.min(line.len());
                if from < to {
                    let role = role_for(stack);
                    match ranges.last_mut() {
                        Some((prev, r)) if *r == role && prev.end == from => prev.end = to,
                        _ => ranges.push((from..to, role)),
                    }
                }
            };
            for (index, op) in &ops {
                push(last, *index, &stack);
                last = (*index).max(last);
                let _ = stack.apply(op);
            }
            push(last, line.len(), &stack);
            ranges
        })
        .collect()
}

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Languages whose token has no grammar in the bundled set map to the
/// closest available grammar rather than plain text.
const ALIASES: &[(&str, &str)] = &[("csharp", "cs"), ("tsx", "js"), ("typescript", "js")];

fn resolve_syntax(token: &str) -> Option<&'static syntect::parsing::SyntaxReference> {
    let set = syntax_set();
    set.find_syntax_by_token(token).or_else(|| {
        ALIASES
            .iter()
            .find(|(from, _)| *from == token)
            .and_then(|(_, to)| set.find_syntax_by_token(to))
    })
}

/// The innermost scope with a known mapping wins.
fn role_for(stack: &ScopeStack) -> SyntaxRole {
    for scope in stack.as_slice().iter().rev() {
        let name = scope.build_string();
        let role = if name.starts_with("comment") {
            SyntaxRole::Comment
        } else if name.starts_with("string") {
            SyntaxRole::String
        } else if name.starts_with("constant.numeric") {
            SyntaxRole::Number
        } else if name.starts_with("constant") {
            SyntaxRole::Keyword
        } else if name.starts_with("keyword.operator") {
            SyntaxRole::Operator
        } else if name.starts_with("keyword") || name.starts_with("storage") {
            SyntaxRole::Keyword
        } else if name.starts_with("entity.name.function")
            || name.starts_with("support.function")
            || name.starts_with("variable.function")
        {
            SyntaxRole::Function
        } else if name.starts_with("entity.name") || name.starts_with("support") {
            SyntaxRole::Type
        } else if name.starts_with("punctuation") {
            SyntaxRole::Punctuation
        } else if name.starts_with("variable") {
            SyntaxRole::Variable
        } else {
            continue;
        };
        return role;
    }
    SyntaxRole::Plain
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn role_at(line: &[(Range<usize>, SyntaxRole)], pos: usize) -> SyntaxRole {
        line.iter()
            .find(|(r, _)| r.contains(&pos))
            .map(|(_, role)| *role)
            .unwrap_or(SyntaxRole::Plain)
    }

    #[test]
    fn rust_keyword_string_comment() {
        let src = lines(&["fn main() {", "    // a note", "    let s = \"hi\";", "}"]);
        let h = spans(&src, Some("rust"));
        assert_eq!(h.len(), 4);
        assert_eq!(role_at(&h[0], 0), SyntaxRole::Keyword, "fn");
        assert_eq!(role_at(&h[1], 6), SyntaxRole::Comment, "comment body");
        assert_eq!(role_at(&h[2], 13), SyntaxRole::String, "string literal");
    }

    #[test]
    fn python_keyword() {
        let src = lines(&["def greet(name):", "    return name"]);
        let h = spans(&src, Some("python"));
        assert_eq!(role_at(&h[0], 0), SyntaxRole::Keyword, "def");
        assert_eq!(role_at(&h[1], 4), SyntaxRole::Keyword, "return");
    }

    #[test]
    fn unknown_language_is_plain() {
        let src = lines(&["anything at all"]);
        let h = spans(&src, Some("nosuchlang"));
        assert_eq!(h.len(), 1);
        assert!(h[0].iter().all(|(_, role)| *role == SyntaxRole::Plain));
    }

    #[test]
    fn no_language_is_plain() {
        let src = lines(&["plain text"]);
        let h = spans(&src, None);
        assert!(h[0].iter().all(|(_, role)| *role == SyntaxRole::Plain));
    }

    #[test]
    fn empty_lines_produce_empty_ranges() {
        let src = lines(&["", "x"]);
        let h = spans(&src, Some("rust"));
        assert_eq!(h.len(), 2);
        assert!(h[0].is_empty());
    }
}
