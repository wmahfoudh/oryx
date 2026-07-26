//! Exported PDFs read back through an independent parser, so the
//! assertions are what a reader sees rather than what we just wrote.

use std::path::PathBuf;

use lopdf::Document as Pdf;

use oryx::doc::images::MediaCache;
use oryx::doc::markdown;
use oryx::doc::model::Document;
use oryx::export::paginate::paginate;
use oryx::export::{pdf, ExportSettings, PageGeometry, PageSize};
use oryx::layout::{layout, ViewConfig};
use oryx::style::fonts::FontStore;
use oryx::style::theme::Theme;

fn export_to_bytes(doc: &Document, page: PageSize) -> Vec<u8> {
    export_with(doc, page, false)
}

fn export_with(doc: &Document, page: PageSize, page_numbers: bool) -> Vec<u8> {
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
    let settings = ExportSettings {
        body_size: cfg.body_size,
        code_size: cfg.code_size,
        page,
        page_numbers,
        ..ExportSettings::default()
    };
    let laid = layout(doc, &theme, &mut fonts, &mut media, &cfg, geometry.width);
    let pages = paginate(doc, &laid, &geometry);
    let job = pdf::Job {
        doc,
        layout: &laid,
        theme: &theme,
        geometry: &geometry,
        settings: &settings,
        title: "test",
    };
    pdf::build(&job, &pages, &mut fonts, &mut media)
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

fn annotation_uris(pdf: &Pdf) -> Vec<String> {
    let mut out = Vec::new();
    for (_, page_id) in pdf.get_pages() {
        let dict = pdf.get_object(page_id).unwrap().as_dict().unwrap();
        let Ok(annots) = dict.get(b"Annots").and_then(|a| a.as_array()) else {
            continue;
        };
        for entry in annots {
            let id = entry.as_reference().unwrap();
            let annot = pdf.get_object(id).unwrap().as_dict().unwrap();
            let Ok(action) = annot.get(b"A").and_then(|a| a.as_dict()) else {
                continue;
            };
            if let Ok(uri) = action.get(b"URI").and_then(|u| u.as_str()) {
                out.push(String::from_utf8_lossy(uri).to_string());
            }
        }
    }
    out
}

/// The page index an internal link points at, one-based, if there is one.
fn internal_destination_page(pdf: &Pdf) -> Option<usize> {
    let pages: Vec<_> = pdf.get_pages().into_iter().collect();
    for (_, page_id) in &pages {
        let dict = pdf.get_object(*page_id).unwrap().as_dict().unwrap();
        let Ok(annots) = dict.get(b"Annots").and_then(|a| a.as_array()) else {
            continue;
        };
        for entry in annots {
            let id = entry.as_reference().unwrap();
            let annot = pdf.get_object(id).unwrap().as_dict().unwrap();
            let Ok(action) = annot.get(b"A").and_then(|a| a.as_dict()) else {
                continue;
            };
            let Ok(dest) = action.get(b"D").and_then(|d| d.as_array()) else {
                continue;
            };
            let target = dest[0].as_reference().unwrap();
            return pages
                .iter()
                .position(|(_, id)| *id == target)
                .map(|at| at + 1);
        }
    }
    None
}

fn decode_title(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

fn outline_titles(pdf: &Pdf) -> Vec<String> {
    fn walk(pdf: &Pdf, id: lopdf::ObjectId, out: &mut Vec<String>) {
        let dict = pdf.get_object(id).unwrap().as_dict().unwrap();
        if let Ok(title) = dict.get(b"Title").and_then(|t| t.as_str()) {
            out.push(decode_title(title));
        }
        if let Ok(first) = dict.get(b"First").and_then(|f| f.as_reference()) {
            walk(pdf, first, out);
        }
        if let Ok(next) = dict.get(b"Next").and_then(|n| n.as_reference()) {
            walk(pdf, next, out);
        }
    }
    let catalog = pdf.catalog().unwrap();
    let Ok(root) = catalog.get(b"Outlines").and_then(|o| o.as_reference()) else {
        return Vec::new();
    };
    let dict = pdf.get_object(root).unwrap().as_dict().unwrap();
    let mut out = Vec::new();
    if let Ok(first) = dict.get(b"First").and_then(|f| f.as_reference()) {
        walk(pdf, first, &mut out);
    }
    out
}

fn image_xobjects(pdf: &Pdf) -> usize {
    pdf.objects
        .values()
        .filter(|object| {
            object.as_stream().is_ok_and(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .and_then(|s| s.as_name())
                    .is_ok_and(|name| name == b"Image")
            })
        })
        .count()
}

#[test]
fn an_external_link_becomes_a_uri_annotation() {
    let doc = markdown::parse("See [the site](https://oryx.example/docs).");
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    assert_eq!(
        annotation_uris(&pdf),
        vec!["https://oryx.example/docs".to_string()]
    );
}

#[test]
fn an_anchor_link_points_at_the_page_holding_its_heading() {
    let doc = markdown::parse(&format!(
        "[jump](#target)\n\n{}\n## Target\n\nThe section.\n",
        "Filler paragraph here.\n\n".repeat(140)
    ));
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    let page = internal_destination_page(&pdf).expect("an internal destination");
    assert!(page > 1, "the heading is not on the first page, got {page}");
}

#[test]
fn the_outline_carries_every_heading_in_order() {
    let doc = markdown::parse("# One\n\ntext\n\n## Two\n\ntext\n\n# Three\n\ntext\n");
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    assert_eq!(outline_titles(&pdf), vec!["One", "Two", "Three"]);
}

#[test]
fn page_numbers_appear_only_when_they_are_on() {
    let doc = markdown::parse(&"Filler paragraph here.\n\n".repeat(200));
    let with = Pdf::load_mem(&export_with(&doc, PageSize::A4, true)).unwrap();
    let without = Pdf::load_mem(&export_with(&doc, PageSize::A4, false)).unwrap();
    let numbered = with.extract_text(&[2]).unwrap();
    let plain = without.extract_text(&[2]).unwrap();
    assert!(numbered.contains('2'), "the second page is numbered");
    assert!(!plain.contains('2'), "no number when they are off");
}

#[test]
fn a_repeated_image_is_embedded_once() {
    let doc = markdown::parse(&"![logo](oryx-test.png)\n\n".repeat(40));
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    let count = image_xobjects(&pdf);
    assert!(count >= 1, "the image is embedded at all");
    assert!(count <= 2, "one source, one image and at most its mask");
}

fn image_sample_widths(pdf: &Pdf) -> Vec<i64> {
    pdf.objects
        .values()
        .filter_map(|object| {
            let stream = object.as_stream().ok()?;
            let subtype = stream.dict.get(b"Subtype").ok()?.as_name().ok()?;
            (subtype == b"Image").then(|| stream.dict.get(b"Width").ok()?.as_i64().ok())?
        })
        .collect()
}

#[test]
fn an_image_is_embedded_at_its_own_resolution() {
    // The fixture is 220 pixels square and lands in a box of about 110
    // points, so sampling at the placed size would halve it.
    let doc = markdown::parse("![logo](oryx-test.png)");
    let pdf = Pdf::load_mem(&export_to_bytes(&doc, PageSize::A4)).unwrap();
    let widths = image_sample_widths(&pdf);
    assert!(!widths.is_empty(), "the image is embedded");
    for width in widths {
        assert_eq!(width, 220, "the source's own pixels, not the placed size");
    }
}
