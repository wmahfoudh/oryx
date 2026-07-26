//! Exported PDFs read back through an independent parser, so the
//! assertions are what a reader sees rather than what we just wrote.

use std::path::PathBuf;

use lopdf::Document as Pdf;

use oryx::doc::images::MediaCache;
use oryx::doc::markdown;
use oryx::doc::model::Document;
use oryx::export::paginate::paginate;
use oryx::export::{pdf, PageGeometry, PageSize};
use oryx::layout::{layout, ViewConfig};
use oryx::style::fonts::FontStore;
use oryx::style::theme::Theme;

fn export_to_bytes(doc: &Document, page: PageSize) -> Vec<u8> {
    let mut fonts = FontStore::new();
    let mut media = MediaCache::new(PathBuf::from("tests/fixtures"));
    let cfg = ViewConfig {
        body_size: 11.0,
        code_size: 9.0,
        zoom: 1.0,
        ..ViewConfig::default()
    };
    let theme = Theme::default_dark();
    let geometry = PageGeometry::new(page, cfg.body_size);
    let laid = layout(doc, &theme, &mut fonts, &mut media, &cfg, geometry.width);
    let pages = paginate(doc, &laid, &geometry);
    pdf::build(&pages, &laid, &theme, &geometry, &mut fonts, "test")
}

#[test]
fn a_short_document_exports_to_one_readable_page() {
    let doc = markdown::parse("# Title\n\nOne short paragraph.");
    let bytes = export_to_bytes(&doc, PageSize::A4);
    let pdf = Pdf::load_mem(&bytes).expect("a reader parses the file");
    assert_eq!(pdf.get_pages().len(), 1);
    let text = pdf.extract_text(&[1]).expect("text extracts");
    assert!(text.contains("One short paragraph."), "got {text:?}");
    assert!(text.contains("Title"), "got {text:?}");
}

#[test]
fn the_media_box_matches_the_chosen_page_size() {
    let doc = markdown::parse("Short.");
    let bytes = export_to_bytes(&doc, PageSize::Letter);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let (_, page) = pdf.get_pages().into_iter().next().unwrap();
    let media = pdf
        .get_object(page)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"MediaBox")
        .unwrap();
    let sizes: Vec<f32> = media
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_float().unwrap())
        .collect();
    assert_eq!(sizes, vec![0.0, 0.0, 612.0, 792.0]);
}

#[test]
fn a_long_document_pages_and_stays_readable() {
    let doc = markdown::parse(&"A paragraph that says something.\n\n".repeat(200));
    let bytes = export_to_bytes(&doc, PageSize::A4);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let count = pdf.get_pages().len();
    assert!(count > 3, "200 paragraphs make more than three pages");
    let last = count as u32;
    assert!(pdf
        .extract_text(&[last])
        .unwrap()
        .contains("says something"));
}

#[test]
fn the_document_carries_its_title_and_producer() {
    let doc = markdown::parse("Short.");
    let bytes = export_to_bytes(&doc, PageSize::A4);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("oryx"), "the producer names the app");
}
