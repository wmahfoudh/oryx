use std::path::PathBuf;

use oryx::doc::images::MediaCache;
use oryx::doc::markdown;
use oryx::layout::{layout, ViewConfig};
use oryx::style::fonts::FontStore;
use oryx::style::theme::Theme;

#[test]
#[ignore = "measurement only"]
fn table_column_widths() {
    let source = "| Shortcut | Action | Notes |\n|---|---|---|\n\
        | Ctrl+O | Open file | Native dialog, filtered to what Oryx reads |\n\
        | Ctrl+T | Theme browser | Previews and applies live |\n\
        | Ctrl+B | Folder sidebar | Keyboard navigable |\n\
        | Ctrl+F | Find in document | Smart case matching |\n\
        | F1 | Shortcuts help | Every binding, in one screen |\n";
    let doc = markdown::parse(source);
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("."));
    for width in [1305.0_f32, 900.0] {
        let l = layout(
            &doc,
            &Theme::default_dark(),
            &mut fonts,
            &mut media,
            &ViewConfig::default(),
            width,
        );
        // Column x positions from the header runs.
        let mut xs: Vec<i64> = l.runs.iter().map(|r| r.x as i64).collect();
        xs.sort_unstable();
        xs.dedup();
        let rows: std::collections::BTreeSet<i64> = l.runs.iter().map(|r| r.y as i64).collect();
        println!("viewport {width}: column x = {xs:?}");
        println!("   rows = {} (6 means nothing wrapped)", rows.len());
        for r in l.runs.iter().filter(|r| r.text.contains("Find in")) {
            println!("   'Find in...' run: {:?} x={} w={}", r.text, r.x, r.width);
        }
    }
}
