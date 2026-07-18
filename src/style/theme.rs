//! Theme types, TOML loading with per-key fallback, and directory scanning.
//!
//! Every color role has a compiled-in default: a theme file states only what
//! differs. A malformed file is skipped with a warning, never a crash.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub surface: Surface,
    pub headings: Headings,
    pub text: Text,
    pub blocks: Blocks,
    pub syntax: Syntax,
    pub alerts: Alerts,
    pub ui: Ui,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    pub background: Rgba,
    pub foreground: Rgba,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headings {
    pub h1: Rgba,
    pub h2: Rgba,
    pub h3: Rgba,
    pub h4: Rgba,
    pub h5: Rgba,
    pub h6: Rgba,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub body: Rgba,
    pub bold: Rgba,
    pub italic: Rgba,
    pub strike: Rgba,
    pub inline_code: Rgba,
    pub inline_code_bg: Rgba,
    pub link: Rgba,
    pub math: Rgba,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocks {
    pub code_bg: Rgba,
    pub code_border: Rgba,
    pub quote_bg: Rgba,
    pub quote_bar: Rgba,
    pub table_border: Rgba,
    pub table_header_bg: Rgba,
    pub table_row_alt_bg: Rgba,
    pub rule: Rgba,
    pub frontmatter_bg: Rgba,
    pub frontmatter_fg: Rgba,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Syntax {
    pub keyword: Rgba,
    pub string: Rgba,
    pub number: Rgba,
    pub function: Rgba,
    pub type_: Rgba,
    pub comment: Rgba,
    pub operator: Rgba,
    pub variable: Rgba,
    pub punctuation: Rgba,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alerts {
    pub note: Rgba,
    pub tip: Rgba,
    pub important: Rgba,
    pub warning: Rgba,
    pub caution: Rgba,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ui {
    pub sidebar_bg: Rgba,
    pub sidebar_fg: Rgba,
    pub sidebar_dir: Rgba,
    pub scrollbar: Rgba,
    pub scrollbar_hover: Rgba,
    pub selection_bg: Rgba,
    pub overlay_bg: Rgba,
    pub overlay_fg: Rgba,
    pub overlay_highlight: Rgba,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ThemeEntry {
    pub name: String,
    pub path: PathBuf,
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D>(deserializer: D) -> Result<Rgba, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        parse_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid color {s:?}")))
    }
}

/// Accepts #RGB, #RRGGBB, and #RRGGBBAA.
fn parse_hex(s: &str) -> Option<Rgba> {
    let hex = s.strip_prefix('#')?;
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(match hex.len() {
        3 => {
            let (r, g, b) = ((v >> 8) as u8 & 0xF, (v >> 4) as u8 & 0xF, v as u8 & 0xF);
            Rgba {
                r: r * 17,
                g: g * 17,
                b: b * 17,
                a: 255,
            }
        }
        6 => Rgba {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
            a: 255,
        },
        8 => Rgba {
            r: (v >> 24) as u8,
            g: (v >> 16) as u8,
            b: (v >> 8) as u8,
            a: v as u8,
        },
        _ => return None,
    })
}

const fn c(r: u8, g: u8, b: u8) -> Rgba {
    Rgba { r, g, b, a: 255 }
}

impl Theme {
    /// The oryx-dark palette, mirror of `themes/oryx-dark.toml`: warm
    /// brown-black ground, headings distinguished by hue (red, gold,
    /// olive, sienna), teal for links. Compiled-in fallback for every role.
    pub fn default_dark() -> Theme {
        Theme {
            surface: Surface {
                background: c(0x24, 0x1E, 0x16),
                foreground: c(0xE8, 0xDF, 0xC8),
            },
            headings: Headings {
                h1: c(0xE2, 0x5A, 0x45),
                h2: c(0xD4, 0xA7, 0x3C),
                h3: c(0xA9, 0xB4, 0x4A),
                h4: c(0xC8, 0x7B, 0x45),
                h5: c(0xB3, 0xA8, 0x8E),
                h6: c(0x94, 0x8A, 0x76),
            },
            text: Text {
                body: c(0xE8, 0xDF, 0xC8),
                bold: c(0xF5, 0xED, 0xD8),
                italic: c(0xD9, 0xB8, 0xA6),
                strike: c(0x87, 0x7E, 0x71),
                inline_code: c(0xE0, 0xB0, 0x72),
                inline_code_bg: c(0x32, 0x2A, 0x1F),
                link: c(0x63, 0xB3, 0xA4),
                math: c(0xB7, 0x9F, 0xD1),
            },
            blocks: Blocks {
                code_bg: c(0x2C, 0x25, 0x1B),
                code_border: c(0x40, 0x37, 0x2A),
                quote_bg: c(0x29, 0x22, 0x18),
                quote_bar: c(0x6B, 0x5F, 0x4B),
                table_border: c(0x40, 0x37, 0x2A),
                table_header_bg: c(0x32, 0x2A, 0x1F),
                table_row_alt_bg: c(0x27, 0x20, 0x13),
                rule: c(0x4A, 0x41, 0x32),
                frontmatter_bg: c(0x29, 0x22, 0x18),
                frontmatter_fg: c(0x9C, 0x90, 0x78),
            },
            syntax: Syntax {
                keyword: c(0xE0, 0x68, 0x45),
                string: c(0xCE, 0x88, 0x71),
                number: c(0xC8, 0x9B, 0x5C),
                function: c(0xB7, 0xC2, 0x58),
                type_: c(0x63, 0xB3, 0xA4),
                comment: c(0x8D, 0x82, 0x71),
                operator: c(0xB3, 0xA8, 0x8E),
                variable: c(0xE8, 0xDF, 0xC8),
                punctuation: c(0xA3, 0x96, 0x82),
            },
            alerts: Alerts {
                note: c(0x6F, 0xA8, 0xD9),
                tip: c(0x8F, 0xBF, 0x6F),
                important: c(0xB7, 0x9F, 0xD1),
                warning: c(0xD4, 0xA7, 0x3C),
                caution: c(0xE2, 0x5A, 0x45),
            },
            ui: Ui {
                sidebar_bg: c(0x1D, 0x18, 0x11),
                sidebar_fg: c(0xC9, 0xBC, 0xA0),
                sidebar_dir: c(0xE0, 0x68, 0x45),
                scrollbar: c(0x40, 0x37, 0x2A),
                scrollbar_hover: c(0x59, 0x50, 0x3C),
                selection_bg: c(0x4A, 0x40, 0x28),
                overlay_bg: c(0x2C, 0x25, 0x1B),
                overlay_fg: c(0xE8, 0xDF, 0xC8),
                overlay_highlight: c(0x4A, 0x40, 0x28),
            },
        }
    }
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::default_dark()
    }
}

/// Raw file mirror: every key optional so partial themes merge over defaults.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Raw {
    surface: RawSurface,
    headings: RawHeadings,
    text: RawText,
    blocks: RawBlocks,
    syntax: RawSyntax,
    alerts: RawAlerts,
    ui: RawUi,
}

macro_rules! raw_group {
    ($Raw:ident { $($field:ident $(rename $name:literal)?),* $(,)? }) => {
        #[derive(Deserialize, Default)]
        #[serde(default, deny_unknown_fields)]
        struct $Raw {
            $( $(#[serde(rename = $name)])? $field: Option<Rgba>, )*
        }
    };
}

raw_group!(RawSurface {
    background,
    foreground
});
raw_group!(RawHeadings {
    h1,
    h2,
    h3,
    h4,
    h5,
    h6
});
raw_group!(RawText {
    body,
    bold,
    italic,
    strike,
    inline_code,
    inline_code_bg,
    link,
    math,
});
raw_group!(RawBlocks {
    code_bg,
    code_border,
    quote_bg,
    quote_bar,
    table_border,
    table_header_bg,
    table_row_alt_bg,
    rule,
    frontmatter_bg,
    frontmatter_fg,
});
raw_group!(RawSyntax {
    keyword,
    string,
    number,
    function,
    type_ rename "type",
    comment,
    operator,
    variable,
    punctuation,
});
raw_group!(RawAlerts {
    note,
    tip,
    important,
    warning,
    caution,
});
raw_group!(RawUi {
    sidebar_bg,
    sidebar_fg,
    sidebar_dir,
    scrollbar,
    scrollbar_hover,
    selection_bg,
    overlay_bg,
    overlay_fg,
    overlay_highlight,
});

macro_rules! merge {
    ($raw:expr, $default:expr, { $($field:ident),* $(,)? }) => {{
        let d = $default;
        let r = $raw;
        $( let $field = r.$field.unwrap_or(d.$field); )*
        MergeTarget { $($field,)* }
    }};
}

fn resolve(raw: Raw) -> Theme {
    let d = Theme::default_dark();
    Theme {
        surface: {
            type MergeTarget = Surface;
            merge!(raw.surface, d.surface, { background, foreground })
        },
        headings: {
            type MergeTarget = Headings;
            merge!(raw.headings, d.headings, { h1, h2, h3, h4, h5, h6 })
        },
        text: {
            type MergeTarget = Text;
            merge!(raw.text, d.text, {
                body, bold, italic, strike, inline_code, inline_code_bg, link, math
            })
        },
        blocks: {
            type MergeTarget = Blocks;
            merge!(raw.blocks, d.blocks, {
                code_bg, code_border, quote_bg, quote_bar, table_border,
                table_header_bg, table_row_alt_bg, rule, frontmatter_bg, frontmatter_fg
            })
        },
        syntax: {
            type MergeTarget = Syntax;
            merge!(raw.syntax, d.syntax, {
                keyword, string, number, function, type_, comment, operator,
                variable, punctuation
            })
        },
        alerts: {
            type MergeTarget = Alerts;
            merge!(raw.alerts, d.alerts, { note, tip, important, warning, caution })
        },
        ui: {
            type MergeTarget = Ui;
            merge!(raw.ui, d.ui, {
                sidebar_bg, sidebar_fg, sidebar_dir, scrollbar, scrollbar_hover,
                selection_bg, overlay_bg, overlay_fg, overlay_highlight
            })
        },
    }
}

pub fn load_file(path: &Path) -> Option<Theme> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("oryx: skipping theme {}: {e}", path.display());
            return None;
        }
    };
    match toml::from_str::<Raw>(&text) {
        Ok(raw) => Some(resolve(raw)),
        Err(e) => {
            eprintln!("oryx: skipping theme {}: {e}", path.display());
            None
        }
    }
}

pub fn scan(dir: &Path) -> Vec<ThemeEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut themes: Vec<ThemeEntry> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let is_toml = path.extension().is_some_and(|x| x == "toml");
            let name = path.file_stem()?.to_str()?.to_string();
            (is_toml && path.is_file()).then_some(ThemeEntry { name, path })
        })
        .collect();
    themes.sort_by(|a, b| a.name.cmp(&b.name));
    themes
}

pub fn preview(path: &Path) -> Option<(Rgba, Rgba)> {
    let theme = load_file(path)?;
    Some((theme.surface.background, theme.headings.h1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Rgba {
        let v = u32::from_str_radix(&s[1..], 16).unwrap();
        match s.len() {
            7 => Rgba {
                r: (v >> 16) as u8,
                g: (v >> 8) as u8,
                b: v as u8,
                a: 255,
            },
            9 => Rgba {
                r: (v >> 24) as u8,
                g: (v >> 16) as u8,
                b: (v >> 8) as u8,
                a: v as u8,
            },
            _ => panic!(),
        }
    }

    fn temp_theme(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oryx-theme-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.toml"));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn full_groups_parse_to_exact_colors() {
        let path = temp_theme(
            "exact",
            r##"
[surface]
background = "#101015"
foreground = "#e0e0e0"

[headings]
h1 = "#ff0000"

[text]
link = "#00ff00"

[syntax]
type = "#0000ff"

[alerts]
warning = "#ffaa00"

[ui]
selection_bg = "#33445566"
"##,
        );
        let t = load_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(t.surface.background, hex("#101015"));
        assert_eq!(t.surface.foreground, hex("#e0e0e0"));
        assert_eq!(t.headings.h1, hex("#ff0000"));
        assert_eq!(t.text.link, hex("#00ff00"));
        assert_eq!(t.syntax.type_, hex("#0000ff"));
        assert_eq!(t.alerts.warning, hex("#ffaa00"));
        assert_eq!(t.ui.selection_bg, hex("#33445566"));
    }

    #[test]
    fn missing_keys_fall_back_to_default_dark() {
        let path = temp_theme("partial", "[surface]\nbackground = \"#000000\"\n");
        let t = load_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let d = Theme::default_dark();
        assert_eq!(t.surface.background, hex("#000000"));
        assert_eq!(t.surface.foreground, d.surface.foreground);
        assert_eq!(t.headings, d.headings);
        assert_eq!(t.text, d.text);
        assert_eq!(t.blocks, d.blocks);
        assert_eq!(t.syntax, d.syntax);
        assert_eq!(t.alerts, d.alerts);
        assert_eq!(t.ui, d.ui);
    }

    #[test]
    fn short_hex_form_parses() {
        let path = temp_theme("short", "[surface]\nbackground = \"#fa0\"\n");
        let t = load_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            t.surface.background,
            Rgba {
                r: 0xFF,
                g: 0xAA,
                b: 0x00,
                a: 255
            }
        );
    }

    #[test]
    fn malformed_file_is_none() {
        let path = temp_theme("broken", "[surface\nbackground = not-a-color\n");
        assert!(load_file(&path).is_none());
        std::fs::remove_file(&path).unwrap();
        let bad_color = temp_theme("badcolor", "[surface]\nbackground = \"#zzz\"\n");
        assert!(load_file(&bad_color).is_none());
        std::fs::remove_file(&bad_color).unwrap();
    }

    #[test]
    fn scan_lists_only_toml_sorted() {
        let dir = std::env::temp_dir().join(format!("oryx-scan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("zeta.toml"), "").unwrap();
        std::fs::write(dir.join("alpha.toml"), "").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        let entries = scan(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "alpha");
        assert_eq!(entries[1].name, "zeta");
    }

    #[test]
    fn preview_returns_background_and_h1() {
        let path = temp_theme(
            "preview",
            "[surface]\nbackground = \"#111111\"\n\n[headings]\nh1 = \"#222222\"\n",
        );
        let (bg, h1) = preview(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(bg, hex("#111111"));
        assert_eq!(h1, hex("#222222"));
    }

    #[test]
    fn shipped_themes_load() {
        let themes = Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
        for name in ["oryx-dark", "oryx-light"] {
            let t = load_file(&themes.join(format!("{name}.toml")));
            assert!(t.is_some(), "{name} failed to load");
        }
        let entries = scan(&themes);
        assert!(entries.iter().any(|e| e.name == "oryx-dark"));
    }
}
