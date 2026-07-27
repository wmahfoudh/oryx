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

impl Rgba {
    /// Whether a background of this colour reads as light, by sRGB luma.
    pub fn is_light(self) -> bool {
        0.2126 * f32::from(self.r) + 0.7152 * f32::from(self.g) + 0.0722 * f32::from(self.b) > 127.5
    }
}

/// Sort rank for a theme list: light themes first, dark after; a theme
/// whose preview failed to load ranks dark.
pub fn dark_rank(preview: &Option<(Rgba, Rgba)>) -> bool {
    !preview.as_ref().is_some_and(|(bg, _)| bg.is_light())
}

/// Guarantees at least one theme file exists: when no scanned directory
/// holds any, the target directory is created and the compiled palette
/// is written there as `dracula.toml`, so the browser always has a row
/// and the reader a complete file to duplicate from.
pub fn seed(scan_dirs: &[PathBuf], target: &Path) -> std::io::Result<bool> {
    if scan_dirs.iter().any(|dir| !scan(dir).is_empty()) {
        return Ok(false);
    }
    std::fs::create_dir_all(target)?;
    save(&target.join("dracula.toml"), &Theme::default_dark())?;
    Ok(true)
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
    /// Backgrounds behind find matches; translucent so they tint body
    /// text, code, and table surfaces alike. The current match reads
    /// stronger than the rest.
    pub search_match_bg: Rgba,
    pub search_current_bg: Rgba,
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
pub fn parse_hex(s: &str) -> Option<Rgba> {
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

const fn ca(r: u8, g: u8, b: u8, a: u8) -> Rgba {
    Rgba { r, g, b, a }
}

impl Theme {
    /// The dracula palette, mirror of `themes/dracula.toml`
    /// (draculatheme.com, MIT): cold blue-gray ground, pink and purple
    /// leads, cyan links. Compiled-in fallback for every role, and the
    /// file the seeder writes when the collection is empty.
    pub fn default_dark() -> Theme {
        Theme {
            surface: Surface {
                background: c(0x28, 0x2A, 0x36),
                foreground: c(0xF8, 0xF8, 0xF2),
            },
            headings: Headings {
                h1: c(0xFF, 0x79, 0xC6),
                h2: c(0xBD, 0x93, 0xF9),
                h3: c(0x8B, 0xE9, 0xFD),
                h4: c(0x50, 0xFA, 0x7B),
                h5: c(0xFF, 0xB8, 0x6C),
                h6: c(0x62, 0x72, 0xA4),
            },
            text: Text {
                body: c(0xF8, 0xF8, 0xF2),
                bold: c(0xFF, 0xB8, 0x6C),
                italic: c(0xF1, 0xFA, 0x8C),
                strike: c(0x62, 0x72, 0xA4),
                inline_code: c(0x50, 0xFA, 0x7B),
                inline_code_bg: c(0x34, 0x37, 0x46),
                link: c(0x8B, 0xE9, 0xFD),
                math: c(0xBD, 0x93, 0xF9),
            },
            blocks: Blocks {
                code_bg: c(0x21, 0x22, 0x2C),
                code_border: c(0x44, 0x47, 0x5A),
                quote_bg: c(0x2E, 0x30, 0x40),
                quote_bar: c(0xBD, 0x93, 0xF9),
                table_border: c(0x44, 0x47, 0x5A),
                table_header_bg: c(0x34, 0x37, 0x46),
                table_row_alt_bg: c(0x2C, 0x2E, 0x3A),
                rule: c(0x44, 0x47, 0x5A),
                frontmatter_bg: c(0x21, 0x22, 0x2C),
                frontmatter_fg: c(0x62, 0x72, 0xA4),
            },
            syntax: Syntax {
                keyword: c(0xFF, 0x79, 0xC6),
                string: c(0xF1, 0xFA, 0x8C),
                number: c(0xBD, 0x93, 0xF9),
                function: c(0x50, 0xFA, 0x7B),
                type_: c(0x8B, 0xE9, 0xFD),
                comment: c(0x62, 0x72, 0xA4),
                operator: c(0xFF, 0x79, 0xC6),
                variable: c(0xF8, 0xF8, 0xF2),
                punctuation: c(0xF8, 0xF8, 0xF2),
            },
            alerts: Alerts {
                note: c(0x8B, 0xE9, 0xFD),
                tip: c(0x50, 0xFA, 0x7B),
                important: c(0xBD, 0x93, 0xF9),
                warning: c(0xFF, 0xB8, 0x6C),
                caution: c(0xFF, 0x55, 0x55),
            },
            ui: Ui {
                sidebar_bg: c(0x21, 0x22, 0x2C),
                sidebar_fg: c(0xF8, 0xF8, 0xF2),
                sidebar_dir: c(0xBD, 0x93, 0xF9),
                scrollbar: c(0x44, 0x47, 0x5A),
                scrollbar_hover: c(0x62, 0x72, 0xA4),
                selection_bg: c(0x44, 0x47, 0x5A),
                overlay_bg: c(0x34, 0x37, 0x46),
                overlay_fg: c(0xF8, 0xF8, 0xF2),
                overlay_highlight: c(0x44, 0x47, 0x5A),
                search_match_bg: ca(0xF1, 0xFA, 0x8C, 0x38),
                search_current_bg: ca(0xFF, 0xB8, 0x6C, 0x8C),
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
        impl $Raw {
            fn missing(&self, group: &str, out: &mut Vec<String>) {
                $( if self.$field.is_none() {
                    out.push(format!("{group}.{}", stringify!($field)));
                } )*
            }
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
    search_match_bg,
    search_current_bg,
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
                selection_bg, overlay_bg, overlay_fg, overlay_highlight,
                search_match_bg, search_current_bg
            })
        },
    }
}

macro_rules! role_table {
    ($(($group:ident, $key:ident, $name:literal)),* $(,)?) => {
        /// Every color role as (group, key) in file order; drives `save`
        /// and the theme editor.
        pub const ROLES: &[(&str, &str)] = &[$((stringify!($group), $name)),*];

        pub fn role(theme: &Theme, index: usize) -> Rgba {
            let mut i = 0usize;
            $( if index == i { return theme.$group.$key; } i += 1; )*
            let _ = i;
            panic!("role index {index} out of range")
        }

        pub fn role_mut(theme: &mut Theme, index: usize) -> &mut Rgba {
            let mut i = 0usize;
            $( if index == i { return &mut theme.$group.$key; } i += 1; )*
            let _ = i;
            panic!("role index {index} out of range")
        }
    };
}

role_table!(
    (surface, background, "background"),
    (surface, foreground, "foreground"),
    (headings, h1, "h1"),
    (headings, h2, "h2"),
    (headings, h3, "h3"),
    (headings, h4, "h4"),
    (headings, h5, "h5"),
    (headings, h6, "h6"),
    (text, body, "body"),
    (text, bold, "bold"),
    (text, italic, "italic"),
    (text, strike, "strike"),
    (text, inline_code, "inline_code"),
    (text, inline_code_bg, "inline_code_bg"),
    (text, link, "link"),
    (text, math, "math"),
    (blocks, code_bg, "code_bg"),
    (blocks, code_border, "code_border"),
    (blocks, quote_bg, "quote_bg"),
    (blocks, quote_bar, "quote_bar"),
    (blocks, table_border, "table_border"),
    (blocks, table_header_bg, "table_header_bg"),
    (blocks, table_row_alt_bg, "table_row_alt_bg"),
    (blocks, rule, "rule"),
    (blocks, frontmatter_bg, "frontmatter_bg"),
    (blocks, frontmatter_fg, "frontmatter_fg"),
    (syntax, keyword, "keyword"),
    (syntax, string, "string"),
    (syntax, number, "number"),
    (syntax, function, "function"),
    (syntax, type_, "type"),
    (syntax, comment, "comment"),
    (syntax, operator, "operator"),
    (syntax, variable, "variable"),
    (syntax, punctuation, "punctuation"),
    (alerts, note, "note"),
    (alerts, tip, "tip"),
    (alerts, important, "important"),
    (alerts, warning, "warning"),
    (alerts, caution, "caution"),
    (ui, sidebar_bg, "sidebar_bg"),
    (ui, sidebar_fg, "sidebar_fg"),
    (ui, sidebar_dir, "sidebar_dir"),
    (ui, scrollbar, "scrollbar"),
    (ui, scrollbar_hover, "scrollbar_hover"),
    (ui, selection_bg, "selection_bg"),
    (ui, overlay_bg, "overlay_bg"),
    (ui, overlay_fg, "overlay_fg"),
    (ui, overlay_highlight, "overlay_highlight"),
    (ui, search_match_bg, "search_match_bg"),
    (ui, search_current_bg, "search_current_bg"),
);

/// `#RRGGBB`, or `#RRGGBBAA` when the color is translucent.
pub fn hex_string(c: Rgba) -> String {
    if c.a == 255 {
        format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
    }
}

/// Writes a theme file with every role explicit, groups in schema order,
/// one key per line.
pub fn save(path: &Path, theme: &Theme) -> std::io::Result<()> {
    let mut out = String::new();
    let mut group = "";
    for (index, (g, key)) in ROLES.iter().enumerate() {
        if *g != group {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("[{g}]\n"));
            group = g;
        }
        out.push_str(&format!("{key} = \"{}\"\n", hex_string(role(theme, index))));
    }
    std::fs::write(path, out)
}

/// The shipped collection, which the editor never overwrites.
const BUNDLED: &[&str] = &[
    "ayu-light",
    "ayu-mirage",
    "be-vendible",
    "catppuccin-latte",
    "catppuccin-mocha",
    "dracula",
    "ember",
    "everforest-dark",
    "everforest-light",
    "flexoki-dark",
    "flexoki-light",
    "github-light",
    "gruvbox-dark",
    "gruvbox-light",
    "horizon",
    "inkstone",
    "kanagawa",
    "meadow",
    "night-owl",
    "nord",
    "one-dark",
    "oryx-dark",
    "oryx-light",
    "oryx-night",
    "oryx-sand",
    "rose-pine",
    "rose-pine-dawn",
    "slate",
    "solarized-dark",
    "solarized-light",
    "tokyo-night",
];

/// Whether a theme name belongs to the shipped collection.
pub fn is_bundled(name: &str) -> bool {
    BUNDLED.contains(&name)
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

/// Role keys a theme file leaves to fallback, empty when the file is
/// complete. None when the file does not read or parse.
pub fn missing_keys(path: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let raw = toml::from_str::<Raw>(&text).ok()?;
    let mut out = Vec::new();
    raw.surface.missing("surface", &mut out);
    raw.headings.missing("headings", &mut out);
    raw.text.missing("text", &mut out);
    raw.blocks.missing("blocks", &mut out);
    raw.syntax.missing("syntax", &mut out);
    raw.alerts.missing("alerts", &mut out);
    raw.ui.missing("ui", &mut out);
    Some(out)
}

/// Loads a theme by name from the first directory that has its file.
pub fn find(dirs: &[PathBuf], name: &str) -> Option<Theme> {
    dirs.iter()
        .map(|dir| dir.join(format!("{name}.toml")))
        .find(|path| path.is_file())
        .and_then(|path| load_file(&path))
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
    fn the_compiled_fallback_mirrors_the_dracula_file() {
        let theme = load_file(Path::new("themes/dracula.toml")).expect("the bundled file parses");
        assert_eq!(theme, Theme::default_dark(), "file and hardcode agree");
    }

    #[test]
    fn seeding_writes_dracula_when_no_theme_exists() {
        let base = std::env::temp_dir().join(format!("oryx-seed-{}", std::process::id()));
        let empty = base.join("empty");
        let user = base.join("user/themes");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(
            seed(std::slice::from_ref(&empty), &user).unwrap(),
            "an empty world seeds"
        );
        let written = load_file(&user.join("dracula.toml")).expect("the seeded file parses");
        assert_eq!(written, Theme::default_dark());
        assert!(
            !seed(&[empty, user.clone()], &user).unwrap(),
            "a second start leaves it alone"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn seeding_leaves_a_populated_collection_alone() {
        let base = std::env::temp_dir().join(format!("oryx-seeded-{}", std::process::id()));
        let full = base.join("full");
        let user = base.join("user/themes");
        std::fs::create_dir_all(&full).unwrap();
        std::fs::write(full.join("mine.toml"), "").unwrap();
        assert!(!seed(&[full], &user).unwrap());
        assert!(
            !user.exists(),
            "nothing is created behind a full collection"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn light_backgrounds_read_as_light() {
        assert!(hex("#fdf6e3").is_light(), "solarized paper");
        assert!(hex("#ffffff").is_light());
        assert!(!hex("#282a36").is_light(), "dracula night");
        assert!(!hex("#000000").is_light());
        assert!(
            !hex("#2d353b").is_light(),
            "everforest dark sits near the middle"
        );
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

    #[test]
    fn save_round_trips_through_load() {
        let dir = std::env::temp_dir().join(format!("oryx-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("saved.toml");
        let mut theme = Theme::default_dark();
        theme.ui.selection_bg = Rgba {
            r: 1,
            g: 2,
            b: 3,
            a: 128,
        };
        save(&path, &theme).unwrap();
        let loaded = load_file(&path).unwrap();
        let complete = missing_keys(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(loaded, theme);
        assert!(complete.is_empty(), "saved file left gaps: {complete:?}");
    }

    #[test]
    fn parse_hex_rejects_garbage() {
        assert!(parse_hex("").is_none());
        assert!(parse_hex("zz").is_none());
        assert!(parse_hex("#12345").is_none());
        assert!(parse_hex("#GGHHII").is_none());
        assert_eq!(
            parse_hex("#A1B2C3"),
            Some(Rgba {
                r: 0xA1,
                g: 0xB2,
                b: 0xC3,
                a: 255
            })
        );
    }

    #[test]
    fn bundled_names_are_known() {
        assert!(is_bundled("be-vendible"));
        assert!(is_bundled("dracula"));
        assert!(is_bundled("oryx-light"));
        assert!(!is_bundled("my-own-theme"));
        assert!(!is_bundled("dracula-copy"));
    }

    #[test]
    fn missing_keys_lists_unset_roles() {
        let path = temp_theme("gaps", "[surface]\nbackground = \"#000000\"\n");
        let missing = missing_keys(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(missing.contains(&"surface.foreground".to_string()));
        assert!(missing.contains(&"syntax.type_".to_string()));
        assert!(!missing.contains(&"surface.background".to_string()));
    }

    #[test]
    fn shipped_collection_is_complete() {
        let themes = Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
        let entries = scan(&themes);
        assert!(
            entries.len() >= 30,
            "expected the full collection, found {}",
            entries.len()
        );
        for entry in entries {
            let missing = missing_keys(&entry.path)
                .unwrap_or_else(|| panic!("{} does not parse", entry.name));
            assert!(missing.is_empty(), "{}: missing {missing:?}", entry.name);
        }
    }

    #[test]
    fn find_resolves_theme_by_name() {
        let path = temp_theme("findme", "[surface]\nbackground = \"#123456\"\n");
        let dir = path.parent().unwrap().to_path_buf();
        let theme = find(std::slice::from_ref(&dir), "findme").unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(theme.surface.background, hex("#123456"));
        assert!(find(&[dir], "absent").is_none());
    }
}
