//! Prints the parser's tables as tab-separated rows, one per entry: the
//! kind, the name, the codepoints and a detail. The script that writes
//! the reference part of the math page of Oryx reads this print
//! (`internal/tools/math-page.py` in Oryx's repository).
//!
//!     cargo run -q -p noad --example tables

use noad::parse::{
    ACCENTS, COLORS, COMMANDS, DELIMITERS, ENVIRONMENTS, NEGATIONS, OP_NAMES, SPACING_WORDS,
    VOCABULARY,
};

fn code(c: char) -> String {
    format!("U+{:04X}", c as u32)
}

fn main() {
    println!("kind\tname\tcodepoints\tdetail");
    for &(name, c, class) in VOCABULARY {
        println!("symbol\t{name}\t{}\t{class:?}", code(c));
    }
    for &(name, c) in DELIMITERS {
        println!("delimiter\t{name}\t{}\t", code(c));
    }
    for &(name, c, wide) in ACCENTS {
        let kind = if wide { "wide" } else { "narrow" };
        println!("accent\t{name}\t{}\t{kind}", code(c));
    }
    for &(name, limits) in OP_NAMES {
        let kind = if limits { "limits" } else { "nolimits" };
        println!("operator\t{name}\t\t{kind}");
    }
    for &(name, ems) in SPACING_WORDS {
        println!("spacing\t{name}\t\t{ems}");
    }
    for &(name, family) in COMMANDS {
        println!("command\t{name}\t\t{family}");
    }
    for &name in ENVIRONMENTS {
        println!("environment\t{name}\t\t");
    }
    for &(name, [r, g, b]) in COLORS {
        println!("color\t{name}\t\t#{r:02x}{g:02x}{b:02x}");
    }
    for &(from, to) in NEGATIONS {
        println!("negation\t{}\t{}\t", code(from), code(to));
    }
}
