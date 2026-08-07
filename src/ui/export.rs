//! The export overlay: what the reader watches while a PDF is written,
//! and what it says when the writing stops.

use winit::keyboard::{Key, NamedKey};

use crate::export::{ExportSettings, Orientation, PageSize, Progress};
use crate::paint::painter::Painter;
use crate::style::fonts::BODY_FAMILY;
use crate::style::theme::{Rgba, Theme};
use crate::ui::overlay::{inside, Action, Overlay, OverlayResult};

const MIN_W: f32 = 380.0;
const PANEL_H: f32 = 96.0;
const PAD: f32 = 24.0;
const RADIUS: f32 = 10.0;
const TITLE_SIZE: f32 = 17.0;
const LINE_SIZE: f32 = 15.0;

/// Trims a line to a width, keeping its end. A path's file name and an
/// error's message both live there, so the head is what gives way.
fn elide(painter: &mut Painter, text: &str, size: f32, weight: u16, max: f32) -> String {
    if painter.measure(text, BODY_FAMILY, size, weight) <= max {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    for cut in 1..chars.len() {
        let candidate: String = std::iter::once('\u{2026}')
            .chain(chars[cut..].iter().copied())
            .collect();
        if painter.measure(&candidate, BODY_FAMILY, size, weight) <= max {
            return candidate;
        }
    }
    String::from("\u{2026}")
}

/// Either a running export or the line it finished on.
pub enum ExportState {
    Running(Progress),
    Result(String),
}

pub struct ExportProgress {
    state: ExportState,
}

impl ExportProgress {
    pub fn new(progress: Progress) -> ExportProgress {
        ExportProgress {
            state: ExportState::Running(progress),
        }
    }

    /// The line the export ended on. The overlay stays up until a key
    /// dismisses it, because a failure has nowhere else to be reported
    /// and a fast export would otherwise finish with no sign that it ran.
    pub fn settled(line: String) -> ExportProgress {
        ExportProgress {
            state: ExportState::Result(line),
        }
    }

    fn lines(&self) -> (String, String) {
        match &self.state {
            ExportState::Running(progress) => {
                let detail = if progress.total > 0 {
                    format!(
                        "page {} of {}",
                        progress.done.min(progress.total),
                        progress.total
                    )
                } else if progress.done > 0 {
                    format!("{} pages written", progress.done)
                } else {
                    String::from("Escape cancels")
                };
                (progress.phase.label().to_string(), detail)
            }
            ExportState::Result(line) => (String::from("Export"), line.clone()),
        }
    }
}

impl Overlay for ExportProgress {
    fn draw(&mut self, painter: &mut Painter, theme: &Theme) {
        let (w, h) = (painter.width(), painter.height());
        let ui = &theme.ui;
        let (title, detail) = self.lines();
        // The panel takes the width its lines need, up to what the window
        // can hold; past that the line gives way rather than the panel.
        let widest = painter
            .measure(&title, BODY_FAMILY, TITLE_SIZE, 700)
            .max(painter.measure(&detail, BODY_FAMILY, LINE_SIZE, 400));
        let panel_w = (widest + 2.0 * PAD).clamp(MIN_W, (w - 40.0).max(MIN_W));
        let detail = elide(painter, &detail, LINE_SIZE, 400, panel_w - 2.0 * PAD);
        let px = ((w - panel_w) / 2.0).floor();
        let py = ((h - PANEL_H) / 2.0).floor();
        for (grow, alpha) in [(10.0, 14), (6.0, 22), (3.0, 34)] {
            painter.fill(
                px - grow,
                py - grow + 2.0,
                panel_w + 2.0 * grow,
                PANEL_H + 2.0 * grow,
                RADIUS + grow,
                Rgba {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: alpha,
                },
            );
        }
        painter.fill(px, py, panel_w, PANEL_H, RADIUS, ui.overlay_bg);

        let title_w = painter.measure(&title, BODY_FAMILY, TITLE_SIZE, 700);
        painter.text(
            px + (panel_w - title_w) / 2.0,
            py + 22.0,
            &title,
            BODY_FAMILY,
            TITLE_SIZE,
            700,
            ui.overlay_fg,
        );
        let detail_w = painter.measure(&detail, BODY_FAMILY, LINE_SIZE, 400);
        painter.text(
            px + (panel_w - detail_w) / 2.0,
            py + 54.0,
            &detail,
            BODY_FAMILY,
            LINE_SIZE,
            400,
            ui.overlay_fg,
        );
    }

    fn key(&mut self, key: &Key, _ctrl: bool, _shift: bool) -> OverlayResult {
        match &self.state {
            ExportState::Running(_) => match key {
                Key::Named(NamedKey::Escape) => OverlayResult::Close,
                _ => OverlayResult::Open,
            },
            ExportState::Result(_) => OverlayResult::Close,
        }
    }

    fn click(&mut self, _x: f32, _y: f32) -> OverlayResult {
        OverlayResult::Open
    }
}

/// One row of the export dialog: the settings, then the row that starts
/// the export. The justify row exists only when the document is a book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Theme,
    BodyFamily,
    BodySize,
    CodeFamily,
    CodeSize,
    Page,
    Orientation,
    PageNumbers,
    Justify,
    Export,
}

const ROWS: [Row; 9] = [
    Row::Theme,
    Row::BodyFamily,
    Row::BodySize,
    Row::CodeFamily,
    Row::CodeSize,
    Row::Page,
    Row::Orientation,
    Row::PageNumbers,
    Row::Export,
];

impl Row {
    fn label(self) -> &'static str {
        match self {
            Row::Theme => "theme",
            Row::BodyFamily => "body font",
            Row::BodySize => "body size",
            Row::CodeFamily => "code font",
            Row::CodeSize => "code size",
            Row::Page => "page",
            Row::Orientation => "orientation",
            Row::PageNumbers => "page numbers",
            Row::Justify => "justify (EPUB)",
            Row::Export => "Export",
        }
    }
}

const MIN_SIZE: f32 = 8.0;
const MAX_SIZE: f32 = 32.0;

/// An open list over a row: theme names or font families.
struct Pick {
    row: Row,
    selected: usize,
}

/// The export settings dialog. The values it holds are a working copy:
/// Escape drops them, and only the Export row hands them back.
pub struct ExportDialog {
    settings: ExportSettings,
    themes: Vec<(String, Option<(Rgba, Rgba)>)>,
    families: Vec<String>,
    /// The rows this dialog shows: the base table, with the justify row
    /// inserted before Export when the document is a book.
    rows: Vec<Row>,
    row: usize,
    pick: Option<Pick>,
    /// Panel rectangle from the last draw, for hit testing.
    geometry: (f32, f32, f32, f32),
    /// Where the panel would sit centred, which the drag offset is
    /// measured against.
    center: (f32, f32),
    moving: bool,
    grab: (f32, f32),
    offset: (f32, f32),
}

impl ExportDialog {
    pub fn new(
        settings: ExportSettings,
        families: Vec<String>,
        themes: Vec<(String, Option<(Rgba, Rgba)>)>,
        book: bool,
    ) -> ExportDialog {
        let mut rows: Vec<Row> = ROWS.to_vec();
        if book {
            rows.insert(rows.len() - 1, Row::Justify);
        }
        ExportDialog {
            settings,
            themes,
            families,
            rows,
            row: 0,
            pick: None,
            geometry: (0.0, 0.0, 0.0, 0.0),
            center: (0.0, 0.0),
            moving: false,
            grab: (0.0, 0.0),
            offset: (0.0, 0.0),
        }
    }

    pub fn settings(&self) -> &ExportSettings {
        &self.settings
    }

    pub fn select(&mut self, row: Row) {
        self.row = self.rows.iter().position(|r| *r == row).unwrap_or(0);
    }

    fn current(&self) -> Row {
        self.rows[self.row.min(self.rows.len() - 1)]
    }

    /// Left and Right: sizes step by one, the page size cycles.
    pub fn step(&mut self, delta: i32) {
        match self.current() {
            Row::BodySize => {
                self.settings.body_size = step_size(self.settings.body_size, delta as f32)
            }
            Row::CodeSize => {
                self.settings.code_size = step_size(self.settings.code_size, delta as f32)
            }
            Row::Page => self.settings.page = cycle_page(self.settings.page, delta),
            Row::Orientation => {
                self.settings.orientation = match self.settings.orientation {
                    Orientation::Portrait => Orientation::Landscape,
                    Orientation::Landscape => Orientation::Portrait,
                }
            }
            Row::PageNumbers => self.settings.page_numbers = !self.settings.page_numbers,
            Row::Justify => self.settings.justify = !self.settings.justify,
            _ => {}
        }
    }

    pub fn left(&mut self) {
        self.step(-1);
    }

    pub fn right(&mut self) {
        self.step(1);
    }

    pub fn toggle(&mut self) {
        match self.current() {
            Row::PageNumbers => self.settings.page_numbers = !self.settings.page_numbers,
            Row::Justify => self.settings.justify = !self.settings.justify,
            _ => {}
        }
    }

    fn open_pick(&mut self) {
        let row = self.current();
        let (options, current) = match row {
            Row::Theme => (
                self.themes.iter().map(|(name, _)| name.clone()).collect(),
                self.settings.theme.clone(),
            ),
            Row::BodyFamily => (self.families.clone(), self.settings.body_family.clone()),
            Row::CodeFamily => (self.families.clone(), self.settings.code_family.clone()),
            _ => return,
        };
        let options: Vec<String> = options;
        let selected = options.iter().position(|o| *o == current).unwrap_or(0);
        self.pick = Some(Pick { row, selected });
    }

    fn choose(&mut self) {
        let Some(pick) = self.pick.take() else {
            return;
        };
        match pick.row {
            Row::Theme => {
                if let Some((name, _)) = self.themes.get(pick.selected) {
                    self.settings.theme = name.clone();
                }
            }
            Row::BodyFamily => {
                if let Some(family) = self.families.get(pick.selected) {
                    self.settings.body_family = family.clone();
                }
            }
            Row::CodeFamily => {
                if let Some(family) = self.families.get(pick.selected) {
                    self.settings.code_family = family.clone();
                }
            }
            _ => {}
        }
    }

    fn options(&self, row: Row) -> usize {
        match row {
            Row::Theme => self.themes.len(),
            Row::BodyFamily | Row::CodeFamily => self.families.len(),
            _ => 0,
        }
    }

    /// The value column of a row, as it reads in the panel.
    fn value(&self, row: Row) -> String {
        match row {
            Row::Theme => self.settings.theme.clone(),
            Row::BodyFamily => self.settings.body_family.clone(),
            Row::BodySize => format!("{:.0}", self.settings.body_size),
            Row::CodeFamily => self.settings.code_family.clone(),
            Row::CodeSize => format!("{:.0}", self.settings.code_size),
            Row::Page => self.settings.page.label().to_string(),
            Row::Orientation => self.settings.orientation.label().to_string(),
            Row::PageNumbers => {
                if self.settings.page_numbers {
                    "on".to_string()
                } else {
                    "off".to_string()
                }
            }
            Row::Justify => {
                if self.settings.justify {
                    "on".to_string()
                } else {
                    "off".to_string()
                }
            }
            Row::Export => String::from("Enter"),
        }
    }
}

fn step_size(size: f32, delta: f32) -> f32 {
    (size + delta).clamp(MIN_SIZE, MAX_SIZE)
}

fn cycle_page(page: PageSize, delta: i32) -> PageSize {
    let order = [PageSize::A4, PageSize::Letter, PageSize::Legal];
    let at = order.iter().position(|p| *p == page).unwrap_or(0) as i32;
    let next = (at + delta).rem_euclid(order.len() as i32) as usize;
    order[next]
}

const DIALOG_W: f32 = 420.0;
const DIALOG_ROW_H: f32 = 34.0;
const DIALOG_HEADER_H: f32 = 44.0;
const DIALOG_PAD: f32 = 16.0;
const LIST_ROW_H: f32 = 26.0;
const LIST_MAX: usize = 12;

impl Overlay for ExportDialog {
    fn draw(&mut self, painter: &mut Painter, theme: &Theme) {
        let (w, h) = (painter.width(), painter.height());
        let ui = &theme.ui;
        let panel_h = DIALOG_HEADER_H + self.rows.len() as f32 * DIALOG_ROW_H + DIALOG_PAD;
        let center = (
            ((w - DIALOG_W) / 2.0).floor(),
            ((h - panel_h) / 2.0).floor(),
        );
        // Clamped so a dragged panel always keeps a grabbable edge on
        // screen, the same bounds the other panels use.
        let px = (center.0 + self.offset.0).clamp(60.0 - DIALOG_W, w - 60.0);
        let py = (center.1 + self.offset.1).clamp(-8.0, h - DIALOG_HEADER_H);
        self.center = center;
        self.geometry = (px, py, DIALOG_W, panel_h);
        for (grow, alpha) in [(10.0, 14), (6.0, 22), (3.0, 34)] {
            painter.fill(
                px - grow,
                py - grow + 2.0,
                DIALOG_W + 2.0 * grow,
                panel_h + 2.0 * grow,
                RADIUS + grow,
                Rgba {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: alpha,
                },
            );
        }
        painter.fill(px, py, DIALOG_W, panel_h, RADIUS, ui.overlay_bg);

        let title = "Export to PDF";
        let title_w = painter.measure(title, BODY_FAMILY, TITLE_SIZE, 700);
        painter.text(
            px + (DIALOG_W - title_w) / 2.0,
            py + 13.0,
            title,
            BODY_FAMILY,
            TITLE_SIZE,
            700,
            ui.overlay_fg,
        );

        for (index, row) in self.rows.iter().enumerate() {
            let ry = py + DIALOG_HEADER_H + index as f32 * DIALOG_ROW_H;
            if index == self.row {
                painter.fill(
                    px + DIALOG_PAD / 2.0,
                    ry,
                    DIALOG_W - DIALOG_PAD,
                    DIALOG_ROW_H,
                    4.0,
                    ui.overlay_highlight,
                );
            }
            painter.text(
                px + DIALOG_PAD,
                ry + 8.0,
                row.label(),
                BODY_FAMILY,
                LINE_SIZE,
                if *row == Row::Export { 700 } else { 400 },
                ui.overlay_fg,
            );
            let value = self.value(*row);
            let value_w = painter.measure(&value, BODY_FAMILY, LINE_SIZE, 400);
            painter.text(
                px + DIALOG_W - DIALOG_PAD - value_w,
                ry + 8.0,
                &value,
                BODY_FAMILY,
                LINE_SIZE,
                400,
                ui.overlay_fg,
            );
        }

        let Some(pick) = self.pick.as_ref() else {
            return;
        };
        let count = self.options(pick.row).min(LIST_MAX);
        if count == 0 {
            return;
        }
        // The list opens over the panel, scrolled to keep the selection
        // in view, so a long font list stays reachable.
        let list_h = count as f32 * LIST_ROW_H + DIALOG_PAD;
        let lx = px + DIALOG_PAD;
        let ly = py + DIALOG_HEADER_H;
        painter.fill(
            lx,
            ly,
            DIALOG_W - 2.0 * DIALOG_PAD,
            list_h,
            6.0,
            ui.overlay_bg,
        );
        painter.stroke(
            lx,
            ly,
            DIALOG_W - 2.0 * DIALOG_PAD,
            list_h,
            6.0,
            1.0,
            ui.overlay_highlight,
        );
        let first = pick.selected.saturating_sub(count.saturating_sub(1));
        for slot in 0..count {
            let at = first + slot;
            let ry = ly + DIALOG_PAD / 2.0 + slot as f32 * LIST_ROW_H;
            if at == pick.selected {
                painter.fill(
                    lx + 4.0,
                    ry,
                    DIALOG_W - 2.0 * DIALOG_PAD - 8.0,
                    LIST_ROW_H,
                    4.0,
                    ui.overlay_highlight,
                );
            }
            let (name, swatches) = match pick.row {
                Row::Theme => match self.themes.get(at) {
                    Some((name, preview)) => (name.clone(), *preview),
                    None => continue,
                },
                _ => match self.families.get(at) {
                    Some(family) => (family.clone(), None),
                    None => continue,
                },
            };
            let mut text_x = lx + 10.0;
            if let Some((background, heading)) = swatches {
                painter.fill(text_x, ry + 6.0, 14.0, 14.0, 3.0, background);
                painter.fill(text_x + 17.0, ry + 6.0, 14.0, 14.0, 3.0, heading);
                text_x += 40.0;
            }
            let family = match pick.row {
                Row::Theme => BODY_FAMILY,
                _ => name.as_str(),
            };
            painter.text(
                text_x,
                ry + 5.0,
                &name,
                family,
                LINE_SIZE,
                400,
                ui.overlay_fg,
            );
        }
    }

    fn key(&mut self, key: &Key, _ctrl: bool, _shift: bool) -> OverlayResult {
        if let Some(row) = self.pick.as_ref().map(|pick| pick.row) {
            let count = self.options(row);
            let pick = self.pick.as_mut().expect("checked");
            match key {
                Key::Named(NamedKey::ArrowUp) => pick.selected = pick.selected.saturating_sub(1),
                Key::Named(NamedKey::ArrowDown) => {
                    if pick.selected + 1 < count {
                        pick.selected += 1;
                    }
                }
                Key::Named(NamedKey::Enter) => self.choose(),
                Key::Named(NamedKey::Escape) => self.pick = None,
                _ => {}
            }
            return OverlayResult::Open;
        }
        match key {
            Key::Named(NamedKey::ArrowUp) => self.row = self.row.saturating_sub(1),
            Key::Named(NamedKey::ArrowDown) => self.row = (self.row + 1).min(self.rows.len() - 1),
            Key::Named(NamedKey::ArrowLeft) => self.left(),
            Key::Named(NamedKey::ArrowRight) => self.right(),
            Key::Named(NamedKey::Space) => self.toggle(),
            Key::Named(NamedKey::Enter) => match self.current() {
                Row::Export => {
                    return OverlayResult::Apply(Action::Export(Box::new(self.settings.clone())))
                }
                Row::PageNumbers => self.toggle(),
                Row::BodySize | Row::CodeSize | Row::Page | Row::Orientation => {}
                _ => self.open_pick(),
            },
            Key::Named(NamedKey::Escape) => return OverlayResult::Close,
            _ => {}
        }
        OverlayResult::Open
    }

    fn click(&mut self, x: f32, y: f32) -> OverlayResult {
        let (px, py, w, h) = self.geometry;
        if !inside((px, py, w, h), x, y) {
            return OverlayResult::Open;
        }
        if y < py + DIALOG_HEADER_H {
            self.moving = true;
            self.grab = (x - px, y - py);
            return OverlayResult::Open;
        }
        if self.pick.is_some() {
            return OverlayResult::Open;
        }
        let row = ((y - py - DIALOG_HEADER_H) / DIALOG_ROW_H).floor();
        if row >= 0.0 && (row as usize) < self.rows.len() {
            self.row = row as usize;
            if self.current() == Row::Export {
                return OverlayResult::Apply(Action::Export(Box::new(self.settings.clone())));
            }
        }
        OverlayResult::Open
    }

    fn drag(&mut self, x: f32, y: f32) -> OverlayResult {
        if self.moving {
            self.offset = (
                x - self.grab.0 - self.center.0,
                y - self.grab.1 - self.center.1,
            );
        }
        OverlayResult::Open
    }

    fn release(&mut self) {
        self.moving = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::resolve_theme;

    fn settings() -> ExportSettings {
        ExportSettings {
            body_size: 11.0,
            code_size: 9.0,
            page: PageSize::A4,
            page_numbers: true,
            ..ExportSettings::default()
        }
    }

    fn dialog() -> ExportDialog {
        ExportDialog::new(
            settings(),
            vec!["DejaVu Sans".to_string(), "Courier Prime".to_string()],
            vec![("oryx-light".to_string(), None), ("nord".to_string(), None)],
            false,
        )
    }

    fn book_dialog() -> ExportDialog {
        ExportDialog::new(
            settings(),
            vec!["DejaVu Sans".to_string(), "Courier Prime".to_string()],
            vec![("oryx-light".to_string(), None), ("nord".to_string(), None)],
            true,
        )
    }

    #[test]
    fn the_justify_row_appears_only_for_books() {
        assert!(!dialog().rows.contains(&Row::Justify));
        assert!(book_dialog().rows.contains(&Row::Justify));
    }

    #[test]
    fn the_justify_row_toggles_the_setting() {
        let mut d = book_dialog();
        d.select(Row::Justify);
        assert!(d.settings().justify, "on by default");
        d.toggle();
        assert!(!d.settings().justify);
        d.right();
        assert!(d.settings().justify, "left and right flip it too");
    }

    fn press(dialog: &mut ExportDialog, key: NamedKey) -> OverlayResult {
        dialog.key(&Key::Named(key), false, false)
    }

    #[test]
    fn a_running_export_cancels_on_escape_alone() {
        let mut p = ExportProgress::new(Progress {
            phase: crate::export::Phase::Highlight,
            done: 0,
            total: 0,
        });
        assert!(matches!(
            p.key(&Key::Character("a".into()), false, false),
            OverlayResult::Open
        ));
        assert!(matches!(
            p.key(&Key::Named(NamedKey::F1), false, false),
            OverlayResult::Open
        ));
        assert!(matches!(
            p.key(&Key::Named(NamedKey::ArrowDown), false, false),
            OverlayResult::Open
        ));
        assert!(matches!(
            p.key(&Key::Named(NamedKey::Escape), false, false),
            OverlayResult::Close
        ));
    }

    #[test]
    fn a_settled_result_dismisses_on_any_key() {
        let mut p = ExportProgress::settled(String::from("3 pages to out.pdf"));
        assert!(matches!(
            p.key(&Key::Character("a".into()), false, false),
            OverlayResult::Close
        ));
        let mut p = ExportProgress::settled(String::from("3 pages to out.pdf"));
        assert!(matches!(
            p.key(&Key::Named(NamedKey::Escape), false, false),
            OverlayResult::Close
        ));
    }

    #[test]
    fn arrows_step_the_sizes_within_range() {
        let mut d = dialog();
        d.select(Row::BodySize);
        d.left();
        assert_eq!(d.settings().body_size, 10.0);
        for _ in 0..40 {
            d.right();
        }
        assert_eq!(d.settings().body_size, MAX_SIZE, "clamped at the top");
        for _ in 0..40 {
            d.left();
        }
        assert_eq!(d.settings().body_size, MIN_SIZE, "clamped at the bottom");
    }

    #[test]
    fn the_page_size_cycles_both_ways() {
        let mut d = dialog();
        d.select(Row::Page);
        d.right();
        assert_eq!(d.settings().page, PageSize::Letter);
        d.right();
        assert_eq!(d.settings().page, PageSize::Legal);
        d.right();
        assert_eq!(d.settings().page, PageSize::A4, "wraps round");
        d.left();
        assert_eq!(d.settings().page, PageSize::Legal, "and the other way");
    }

    #[test]
    fn the_page_numbers_toggle_answers_the_arrows() {
        let mut d = dialog();
        d.select(Row::PageNumbers);
        let before = d.settings().page_numbers;
        press(&mut d, NamedKey::ArrowRight);
        assert_eq!(d.settings().page_numbers, !before, "right toggles");
        press(&mut d, NamedKey::ArrowLeft);
        assert_eq!(d.settings().page_numbers, before, "and left toggles back");
        press(&mut d, NamedKey::Space);
        assert_eq!(d.settings().page_numbers, !before, "space still works");
    }

    #[test]
    fn the_header_drags_the_panel_and_the_rows_do_not() {
        let mut d = dialog();
        d.geometry = (100.0, 100.0, DIALOG_W, 300.0);
        d.center = (100.0, 100.0);
        d.click(150.0, 110.0);
        d.drag(200.0, 160.0);
        assert_eq!(d.offset, (50.0, 50.0), "the header moves the panel");
        d.release();
        d.drag(400.0, 400.0);
        assert_eq!(d.offset, (50.0, 50.0), "and stops on release");
        d.click(150.0, 100.0 + DIALOG_HEADER_H + 5.0);
        d.drag(300.0, 300.0);
        assert_eq!(d.offset, (50.0, 50.0), "a row selects rather than drags");
        assert_eq!(d.row, 0);
    }

    #[test]
    fn a_list_row_opens_picks_and_applies() {
        let mut d = dialog();
        d.select(Row::Theme);
        press(&mut d, NamedKey::Enter);
        press(&mut d, NamedKey::ArrowDown);
        press(&mut d, NamedKey::Enter);
        assert_eq!(d.settings().theme, "nord");
        d.select(Row::BodyFamily);
        press(&mut d, NamedKey::Enter);
        press(&mut d, NamedKey::ArrowDown);
        press(&mut d, NamedKey::Enter);
        assert_eq!(d.settings().body_family, "Courier Prime");
    }

    #[test]
    fn escape_closes_a_list_before_the_dialog() {
        let mut d = dialog();
        d.select(Row::Theme);
        press(&mut d, NamedKey::Enter);
        assert!(
            matches!(press(&mut d, NamedKey::Escape), OverlayResult::Open),
            "the list closes and the dialog stays"
        );
        assert!(matches!(
            press(&mut d, NamedKey::Escape),
            OverlayResult::Close
        ));
    }

    #[test]
    fn the_export_row_hands_the_settings_back() {
        let mut d = dialog();
        d.select(Row::Page);
        d.right();
        d.select(Row::Export);
        match press(&mut d, NamedKey::Enter) {
            OverlayResult::Apply(Action::Export(settings)) => {
                assert_eq!(settings.page, PageSize::Letter, "the edit travels with it");
            }
            _ => panic!("the export row applies"),
        }
    }

    #[test]
    fn the_orientation_cycles_both_ways() {
        let mut d = dialog();
        d.select(Row::Orientation);
        d.right();
        assert_eq!(d.settings().orientation, Orientation::Landscape);
        d.right();
        assert_eq!(
            d.settings().orientation,
            Orientation::Portrait,
            "wraps round"
        );
        d.left();
        assert_eq!(
            d.settings().orientation,
            Orientation::Landscape,
            "and the other way"
        );
    }

    #[test]
    fn a_click_on_the_export_row_starts_the_export() {
        let mut d = dialog();
        d.geometry = (100.0, 100.0, DIALOG_W, 400.0);
        d.select(Row::Page);
        d.right();
        let y = 100.0 + DIALOG_HEADER_H + (ROWS.len() as f32 - 1.0) * DIALOG_ROW_H + 5.0;
        match d.click(150.0, y) {
            OverlayResult::Apply(Action::Export(settings)) => {
                assert_eq!(settings.page, PageSize::Letter, "the edit travels with it");
            }
            _ => panic!("a click on the export row applies"),
        }
    }

    #[test]
    fn a_theme_that_has_vanished_falls_back_to_the_active_one() {
        let active = Theme::default_dark();
        let (theme, fell_back) = resolve_theme(&[], "no-such-theme", &active);
        assert!(fell_back, "the reader is told");
        assert_eq!(theme.surface.background, active.surface.background);
    }
}
