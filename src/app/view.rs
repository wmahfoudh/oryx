//! The waiting view: where the view is, and what it still waits for
//! while the layout has not reached a place the reader asked for.

use oryx::doc::images::MediaCache;
use oryx::doc::model::{BlockKind, Document};
use oryx::edit::caret::{self, Caret, CaretBox};
use oryx::layout::{LayoutDoc, LayoutPass, ViewConfig};
use oryx::paint;
use oryx::paint::scroll;
use oryx::style::fonts::FontStore;
use oryx::style::theme::Theme;

use super::row_height;

/// What a rule of the waiting view reads of the page: the document,
/// its layout and the pass that still measures it, what shapes a row
/// the layout does not hold, and the state of the window the rules
/// depend on. `App::page` builds one around each rule. A test builds
/// one over a layout pass that it steps itself, with no window.
pub(super) struct Page<'a> {
    pub(super) doc: &'a Document,
    pub(super) lay: Option<&'a LayoutDoc>,
    pub(super) pass: Option<&'a LayoutPass>,
    pub(super) theme: &'a Theme,
    pub(super) fonts: &'a mut FontStore,
    pub(super) media: &'a mut MediaCache,
    pub(super) cfg: &'a ViewConfig,
    /// The width the layout was made for.
    pub(super) width: f32,
    /// The height of the view.
    pub(super) view_h: f32,
    /// The editor is open.
    pub(super) editing: bool,
    pub(super) caret: Option<Caret>,
    /// The parse worker still owes a document.
    pub(super) parse_pending: bool,
    /// An export runs.
    pub(super) exporting: bool,
    /// The search bar is open.
    pub(super) searching: bool,
    /// A gesture of the pointer that the view follows: a drag that is
    /// not the scrollbar's thumb, a selection, the mouse on a dialog.
    pub(super) gesture: bool,
}

impl Page<'_> {
    /// The height the pass has placed.
    pub(super) fn height(&self) -> f32 {
        self.lay.map(|l| l.height).unwrap_or(0.0)
    }

    pub(super) fn row_h(&self) -> f32 {
        row_height(self.doc, self.cfg)
    }

    /// Whether a pass exists with blocks still to place.
    pub(super) fn pass_active(&self) -> bool {
        self.pass.is_some_and(|p| !p.is_complete())
    }

    /// Whether the layout still owes places: the pass has blocks left, or
    /// the parse worker owes a document that will grow it.
    pub(super) fn pending(&self) -> bool {
        self.pass_active() || self.parse_pending
    }

    /// Whether the layout is there and owes nothing more: each place
    /// of the document can be read in it.
    pub(super) fn whole(&self) -> bool {
        self.lay.is_some() && !self.pending()
    }

    /// Whether the document is a code or text file of one block whose
    /// pass still measures it: the case the preview shows a place for
    /// (`View::wanted`).
    pub(super) fn measured(&self) -> bool {
        let doc = self.doc;
        self.pass_active()
            && !self.parse_pending
            && !self.exporting
            && (doc.code_file || doc.plain_file)
            && matches!(&doc.blocks[..], [block] if matches!(&block.kind, BlockKind::CodeBlock { lines, .. } if !lines.is_empty()))
    }

    /// The y of the row an offset stands on in the editor. Placed runs
    /// answer exactly, which covers a caret near the view. Beyond the
    /// layout window there are no runs to read, and a jump across a
    /// whole file is that case, so the block table answers by line
    /// index, which is how a source view is indexed. Inside a line
    /// longer than `LONG_LINE` that answer is the top of the place's
    /// piece. It is enough to judge whether a row is in view, and
    /// which edge it is beyond (`seat_caret_line`, `keep_view_with`,
    /// `edit_view`): once the pass has ended and the window has slid
    /// to the view, a piece that crosses the view is held whole, so a
    /// row in view answers exactly. The height of a row out of view is
    /// read with `row_y_in_piece`.
    pub(super) fn editor_row_y(&self, offset: usize) -> Option<f32> {
        row_y(self.lay?, self.doc, offset)
    }

    /// The y of the row an offset stands on in the editor, wherever
    /// the view is. As `editor_row_y`, and inside a line longer than
    /// `LONG_LINE` whose rows the layout does not hold, the row
    /// itself: the block table answers the top of the place's piece,
    /// and the row can be a hundred rows under it
    /// (`preview::row_in_piece`). The distance the history files for a
    /// caret out of view is counted from this row (`View::standing`),
    /// since `row_landing` brings it back to this row.
    pub(super) fn row_y_in_piece(&mut self, offset: usize) -> Option<f32> {
        let lay = self.lay?;
        let doc = self.doc;
        if let Some(y) = caret::row_top(lay, doc, offset) {
            return Some(y);
        }
        let block = doc.block_at_offset(offset)?;
        let top = scroll::table_row_top(lay, doc, block, offset)?;
        Some(top + self.piece_depth(offset))
    }

    /// How far the row holding `offset` is under the top of its piece
    /// of a line longer than `LONG_LINE`. Zero anywhere else.
    pub(super) fn piece_depth(&mut self, offset: usize) -> f32 {
        let Some(lay) = self.lay else {
            return 0.0;
        };
        paint::preview::row_in_piece(
            self.doc, lay, self.theme, self.fonts, self.media, self.cfg, self.width, offset,
        )
        .unwrap_or(0.0)
    }

    /// The scroll of a landing that stands the editor row holding
    /// `offset` `below` the top of the view (`row_y_in_piece`). The
    /// piece of a very long line is laid out to read the row only once
    /// the pass has placed the table's answer: until then the landing
    /// waits whatever the row is, and each frame of the wait would lay
    /// the piece out again.
    pub(super) fn row_landing(&mut self, offset: usize, below: f32) -> Option<f32> {
        let pending = self.pending();
        let view_h = self.view_h;
        let lay = self.lay?;
        let doc = self.doc;
        if let Some(y) = caret::row_top(lay, doc, offset) {
            return Some(caret::seated(y, below));
        }
        let block = doc.block_at_offset(offset)?;
        let top = scroll::table_row_top(lay, doc, block, offset)?;
        let waits = caret::seated(top, below);
        if pending && !scroll::reached(waits, lay.height, view_h) {
            return Some(waits);
        }
        // The same sum as the layout's own row, taken before the seat:
        // far down a big file a float is 2 px apart from the next.
        Some(caret::seated(top + self.piece_depth(offset), below))
    }

    /// The scroll that shows the source line at `offset` with its row
    /// `below` the top of the view. The row the page drew answers
    /// exactly. A row not drawn yet stands at its share of its block,
    /// and `settle_landing` puts the exact row there once the frame's
    /// slide draws it. A line of code is placed by the block table, and
    /// a line that draws no row, an image's, stands its block whole in
    /// the view, or from its top when taller. A blank line after a
    /// block taller than the view stands at the end of that block. In
    /// a code or text file, a place inside a line longer than
    /// `LONG_LINE` is a row of the editor, which the page draws the
    /// same: it lands as in the editor (`row_landing`).
    pub(super) fn line_landing(&mut self, offset: usize, below: f32) -> Option<f32> {
        let lay = self.lay?;
        if in_source_piece(lay, self.doc, offset) {
            return self.row_landing(offset, below);
        }
        line_landing(lay, self.doc, offset, below, self.view_h)
    }
}

/// The scroll of the page and the landings that wait for the layout.
/// Its rules are its methods: what an action of the reader asks of the
/// view, and what a frame brings back once the pass has placed it.
/// `App` keeps the event, the history, the document and the paint.
/// A frame calls the rules in a fixed order, written in `App::redraw`
/// and again in the frame of the bench of the tests below: a change
/// of one is a change of the other.
#[derive(Default)]
pub(super) struct View {
    /// A rule scrolled the page (`scroll_to`): `App` owes the hover
    /// under the pointer and a frame (`App::after_scroll`).
    pub(super) moved: bool,
    /// The scroll of the page: how far its top is above the top of the
    /// view, in pixels.
    pub(super) scroll_y: f32,
    /// What the running pass brings back of the view once it places it:
    /// the place taken before a new layout of the same text, or the
    /// scroll of a reload.
    pub(super) kept_view: Option<Kept>,
    /// Anchor target clicked before its heading was placed.
    pub(super) pending_anchor: Option<String>,
    /// A book source offset to land on once delivered and placed: a
    /// restored reading position or an internal link's target.
    pub(super) pending_offset: Option<Place>,
    /// The scroll the page was left at by a drop of the scrollbar's
    /// thumb whose place the measure has not reached (`drag_to`), while
    /// `pending_offset` holds that place. A view that has moved since
    /// is the reader's: the landing is released, as a held `Ctrl+End`
    /// is.
    pub(super) thumb_landing: Option<f32>,
    /// The scroll of the view when a seat of the editor was left
    /// waiting for the measure of the file (`seat_editor_on`), while
    /// `pending_row` holds that place. The preview shows the place
    /// meanwhile, over the view. A view that has moved since is the
    /// reader's: the seat is released, as a drop of the thumb is.
    pub(super) row_parked: Option<f32>,
    /// A line landing made this frame, whose exact row `settle_landing`
    /// finds once the slide has drawn the rows around it.
    pub(super) landing_settle: Option<Place>,
    /// A jump to the bottom of a document the pass had not finished
    /// placing, waiting for the whole document to seat it.
    pub(super) bottom_hold: scroll::BottomHold,
    /// A row to seat in the editor once the layout places it, the
    /// source view's counterpart to `pending_offset`.
    pub(super) pending_row: Option<Place>,
    /// A typing edit or caret motion moved content under a stale
    /// layout, or the caret moved to a line the layout does not hold;
    /// the next placed frame snaps the view to the caret.
    pub(super) caret_snap: bool,
    /// The seat made for the line of a caret that the layout did not
    /// hold, while its row is still owed (`caret_snap`).
    pub(super) caret_seat: Option<CaretSeat>,
    /// The steps of the history asked while the layout still owed
    /// places, a step back as minus one and a step forward as plus
    /// one. They are made once the layout owes nothing (`take_steps`).
    pub(super) steps_held: i32,
}

/// A place in the source the view shows once the layout reaches it.
/// `below` is how far under the top of the view its line stands: zero
/// for a jump, and for a crossing between the page and the editor the
/// height the line had on the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Place {
    pub(super) offset: usize,
    pub(super) below: f32,
    /// The offset names a line to show, a caret's or a jump's, rather
    /// than what stood at the top of the page: on a rendered page the
    /// line's own row stands at `below`, not its block's top (`line_landing`).
    /// Never set for a place the page itself recorded, which comes back
    /// pixel for pixel through its block.
    pub(super) line: bool,
}

impl Place {
    pub(super) fn top(offset: usize) -> Place {
        Place {
            offset,
            below: 0.0,
            line: false,
        }
    }

    pub(super) fn by_line(self, line: bool) -> Place {
        Place { line, ..self }
    }

    /// A jump to a line: the line stands in the middle of the view,
    /// with as much of the file above it as below. The scroll's clamp
    /// keeps a line near the file's start or end as close to the
    /// middle as the file allows.
    pub(super) fn centered(offset: usize, view_h: f32, row_h: f32) -> Place {
        Place {
            offset,
            below: ((view_h - row_h) / 2.0).max(0.0),
            line: false,
        }
    }

    /// A step of the history: the line comes back at the height it
    /// stood at, kept inside the view when the window shrank since.
    /// While editing the caret's row comes back whole; while reading, a
    /// line the top edge cut keeps its cut, so the view is the one left.
    /// In the editor, a caret the reader had scrolled away from (`away`)
    /// comes back at its true distance from the view, outside it again.
    /// On the page that distance measures nothing, since it counts rows
    /// of the editor. The page cannot bring that view back, so the step
    /// is a jump to the line of the caret, which stands in the middle. A
    /// caret row that an edge only cut shows whole at that edge.
    pub(super) fn returned(
        offset: usize,
        below: f32,
        away: bool,
        view_h: f32,
        row_h: f32,
        editing: bool,
    ) -> Place {
        if away && editing {
            return Place {
                offset,
                below,
                line: false,
            };
        }
        let outside = below + row_h <= 0.0 || below >= view_h;
        if away && outside {
            return Place::centered(offset, view_h, row_h);
        }
        let floor = if editing || away { 0.0 } else { f32::MIN };
        Place {
            offset,
            below: below.min((view_h - row_h).max(0.0)).max(floor),
            line: false,
        }
    }
}

/// The place a landing still waiting for the layout will show, which is
/// where the reader stands for the history. While a landing waits the
/// view has not moved, so a measure of the screen would file the place
/// left a second time, or the distance of a caret the view has not
/// followed yet. In the editor only a landing on the caret counts, since
/// the caret may have moved since. While reading, the landing of a page
/// is applied after the landing of a row, so it is the one that stays.
pub(super) fn waiting_place(
    row: Option<Place>,
    page: Option<Place>,
    caret: Option<usize>,
) -> Option<Place> {
    match caret {
        Some(caret) => row.filter(|place| place.offset == caret),
        None => page.or(row),
    }
}

/// Whether a new layout pass with no kept place keeps the scroll of the
/// view as a distance, to bring it back once the layout reaches that
/// height again. Not while a landing waits: the landing says where the
/// view goes, and on a large file the kept distance would be applied
/// after it. After a crossing between the page and the editor the kept
/// distance would even be the other document's.
pub(super) fn keeps_scroll(scroll_y: f32, landing_waits: bool) -> bool {
    scroll_y > 0.0 && !landing_waits
}

/// What a new layout pass keeps of the view, brought back once the
/// layout reaches it. A new size or a new width changes the height of
/// the page above the view, so a distance in pixels would show other
/// lines: the pass keeps a place in the text where it can.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Kept {
    /// The scroll as a distance from the top of the page, when no
    /// layout was there to read a place from: a reload.
    Distance(f32),
    /// In the editor, how far under the top of the view the caret's row
    /// stood. The caret is where the reader works, so its row keeps its
    /// height on the screen.
    Caret(f32),
    /// The block or the line at the top of the view.
    Top(scroll::TopPlace),
    /// The scroll, for the new pass an edit starts in a file that is
    /// still measured, when the edit is in view: the lines above an
    /// edit keep their heights, so the same distance shows the same
    /// lines, and the view does not move under the hand that types.
    /// It lives until the caret moves by a key or a click.
    Edit(f32),
}

impl Kept {
    /// What the view keeps. `caret_row` is the top of the caret's row
    /// while editing. A caret in view is kept at its height. A caret
    /// the reader scrolled away from, or whose row an edge cuts, is
    /// left where it is: the view is kept by its top, as on the page.
    /// None at the top of the file, where the view stays.
    pub(super) fn of(
        caret_row: Option<f32>,
        row_h: f32,
        scroll_y: f32,
        view_h: f32,
        top: impl FnOnce() -> Option<scroll::TopPlace>,
    ) -> Option<Kept> {
        let in_view = caret_row
            .map(|row_y| filed_height(row_y, row_h, scroll_y, view_h))
            .filter(|(_, away)| !away);
        match in_view {
            Some((below, _)) => Some(Kept::Caret(below)),
            None => top().map(Kept::Top),
        }
    }

    /// What an edit keeps of a view no kept place or landing answers
    /// for: the scroll, when the row of the edit stands whole in the
    /// view. `row` is the top of that row, read before the edit drops
    /// the layout. None for an edit out of view, whose caret the view
    /// goes to, and at the top of the file, where the view stays.
    pub(super) fn of_edit(
        row: Option<f32>,
        row_h: f32,
        scroll_y: f32,
        view_h: f32,
    ) -> Option<Kept> {
        let (_, away) = filed_height(row?, row_h, scroll_y, view_h);
        (!away && scroll_y > 0.0).then_some(Kept::Edit(scroll_y))
    }

    /// For the view an edit kept, whether the row `b` of the caret
    /// stands whole in it. None for any other kept view.
    pub(super) fn edit_holds(self, b: CaretBox, view_h: f32) -> Option<bool> {
        match self {
            Kept::Edit(scroll_y) => Some(caret_line_in_view(b.y, b.h, scroll_y, view_h)),
            _ => None,
        }
    }

    /// Whether the kept view shows the caret: its row at its height, or
    /// the view of an edit made in view.
    pub(super) fn shows_caret(self) -> bool {
        matches!(self, Kept::Caret(_) | Kept::Edit(_))
    }

    /// What stays of a kept view when an edit owes the view to its
    /// caret. A view that shows the caret agrees with the edit. A top
    /// or a distance is the view the edit leaves: in a big file the
    /// view goes to the caret first, and the kept place would take it
    /// back seconds later, when the measure reaches it.
    pub(super) fn under_snap(self, snap_owed: bool) -> Option<Kept> {
        match self {
            kept if kept.shows_caret() => Some(kept),
            _ if snap_owed => None,
            other => Some(other),
        }
    }

    /// A zoom step made while the kept view still waits for the layout.
    /// A distance grows with the text. A place holds, so several steps
    /// in a row return to the place of the first one.
    pub(super) fn zoomed(self, ratio: f32) -> Kept {
        match self {
            Kept::Distance(y) => Kept::Distance(y * ratio),
            Kept::Edit(y) => Kept::Edit(y * ratio),
            place => place,
        }
    }
}

/// The height a kept caret row comes back at: where it stood, moved up
/// when its rows grew taller and the bottom edge would cut it.
pub(super) fn kept_caret_height(below: f32, view_h: f32, row_h: f32) -> f32 {
    below.min((view_h - row_h).max(0.0)).max(0.0)
}

/// What a preview shows: the landing that waits, as `View::land`
/// will spend it.
pub(super) enum Wanted {
    /// A line of the editor, `pending_row`.
    Row(Place),
    /// An offset, `pending_offset`.
    Offset(Place),
    /// The line at the top of the view, kept across a new layout or
    /// read from the scroll.
    Top(scroll::TopPlace),
    /// The caret's row at its height in the window.
    Caret(usize, f32),
    /// The end of the file, a held `Ctrl+End`.
    Bottom,
}

/// The top of the editor row holding `offset`: the row the layout drew,
/// else the line's place in the block table.
pub(super) fn row_y(lay: &LayoutDoc, doc: &Document, offset: usize) -> Option<f32> {
    if let Some(y) = caret::row_top(lay, doc, offset) {
        return Some(y);
    }
    let block = doc.block_at_offset(offset)?;
    scroll::table_row_top(lay, doc, block, offset)
}

/// Whether `offset` is inside a piece of a line longer than
/// `LONG_LINE`, on a page that draws the rows of its editor: a code or
/// text file. A fenced block of a rendered page is placed in pieces
/// too, and its lines land as the other lines of the page do.
pub(super) fn in_source_piece(lay: &LayoutDoc, doc: &Document, offset: usize) -> bool {
    (doc.code_file || doc.plain_file)
        && doc.is_one_code_block()
        && scroll::offset_piece(lay, doc, offset).is_some()
}

/// The scroll that shows the source line at `offset` with its row
/// `below` the top of a view `view_h` tall (`Page::line_landing`).
pub(super) fn line_landing(
    lay: &LayoutDoc,
    doc: &Document,
    offset: usize,
    below: f32,
    view_h: f32,
) -> Option<f32> {
    // Inside a line longer than `LONG_LINE` the block table places the
    // piece, and the row can be a hundred rows under its top: the row
    // the layout holds answers, as in the editor.
    if in_source_piece(lay, doc, offset) {
        return row_y(lay, doc, offset).map(|y| caret::seated(y, below));
    }
    let row = caret::line_top(lay, doc, offset).or_else(|| scroll::line_estimate(lay, doc, offset));
    if let Some(y) = row {
        return Some(caret::seated(y, below));
    }
    if let Some(end) = caret::tall_block_end(lay, doc, offset, view_h) {
        return Some(caret::seated(end, below));
    }
    let below = scroll::fitted_below(lay, doc, offset, below, view_h);
    scroll::offset_top(lay, doc, offset).map(|y| caret::seated(y, below))
}

/// Whether the first row of a caret's line stands whole in the view.
pub(super) fn caret_line_in_view(row_y: f32, row_h: f32, scroll_y: f32, view_h: f32) -> bool {
    row_y >= scroll_y && row_y + row_h <= scroll_y + view_h
}

/// The height the line of a far caret is seated at: the top edge of the
/// view when the line is above it, else the bottom edge, as the view
/// follows a caret that walks out of it.
pub(super) fn caret_line_edge(row_y: f32, row_h: f32, scroll_y: f32, view_h: f32) -> f32 {
    if row_y < scroll_y {
        0.0
    } else {
        (view_h - row_h).max(0.0)
    }
}

/// Whether the next frame still owes the view to the caret after a
/// caret key showed it. `settled` says that the row of the caret
/// answered, or that its line is seated with nothing left to wait for.
/// What an edit owed stays owed: until the frame lays the rows out
/// again, the box of the caret is the first row of its line, so a key
/// read before that frame cannot bring the caret's own row into view.
pub(super) fn snap_still_owed(owed: bool, settled: bool) -> bool {
    owed || !settled
}

/// The view as the seat of a far caret's line left it. While the view
/// stays there, the caret's own row is still owed. A view that moved
/// since, by the wheel or the scrollbar, owes the caret nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CaretSeat {
    /// The offset of the caret when its line was seated.
    pub(super) offset: usize,
    /// The scroll that the seat left.
    pub(super) scroll: f32,
}

/// What the box of a caret (`Caret::geometry`) is worth to the code
/// that brings the view to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum CaretRow {
    /// A row the layout has placed.
    Placed(CaretBox),
    /// The block table's seat for a line the pass has not reached. On
    /// a page with no row yet, right after an edit of a big file
    /// started a new pass, the table counts one row for each line it
    /// has not placed, so the seat is short of the line's real place,
    /// and a snap to it stops at the end of the placed part.
    Guess,
    /// No box.
    Unknown,
}

/// Reads a caret's box against the height the pass has placed.
/// `pending` says that the pass still runs: once it has ended, the
/// table is whole and each box is a row.
pub(super) fn caret_row(found: Option<CaretBox>, placed: f32, pending: bool) -> CaretRow {
    match found {
        Some(b) if pending && b.y + b.h > placed => CaretRow::Guess,
        Some(b) => CaretRow::Placed(b),
        None => CaretRow::Unknown,
    }
}

/// What a frame does for a caret that is still owed its view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum CaretDue {
    /// The layout holds the row of the caret: the view snaps to it.
    Snap,
    /// No row to read and no seat made yet: the line is seated with the
    /// block table.
    Seat,
    /// The line is seated and the layout pass still runs: the row
    /// waits, and the view stays where it is.
    Wait,
    /// Nothing more is owed.
    Drop,
}

/// What the frame owes a caret at `offset`. `has_box` says that the
/// layout holds its row, `seat` is the seat made for its line, and
/// `pending` says that the layout pass still runs. A line is seated
/// once: while the pass of a file of lines runs, the layout cannot
/// take the rows of a far line, and a seat made again at each frame
/// would hold the view against the wheel.
pub(super) fn caret_due(
    has_box: bool,
    seat: Option<CaretSeat>,
    offset: usize,
    scroll_y: f32,
    pending: bool,
) -> CaretDue {
    match seat.filter(|seat| seat.offset == offset) {
        Some(seat) if seat.scroll != scroll_y => CaretDue::Drop,
        Some(_) if has_box => CaretDue::Snap,
        Some(_) if pending => CaretDue::Wait,
        Some(_) => CaretDue::Drop,
        None if has_box => CaretDue::Snap,
        None => CaretDue::Seat,
    }
}

/// The height the history files for the caret's row, and whether the
/// row stood outside the view or cut by one of its edges. A row in view
/// files the height a step keeps inside the view. A row the reader
/// scrolled away from files its true distance from the top of the view,
/// so the step brings back the view that was left, not the caret.
pub(super) fn filed_height(row_y: f32, row_h: f32, scroll_y: f32, view_h: f32) -> (f32, bool) {
    let below = row_y - scroll_y;
    let held = caret::held(row_y, row_h, scroll_y, view_h);
    (below, below != held)
}

/// What the history files for a caret whose landing still waits for
/// the layout: the height the landing will stand its row at, and whether
/// that height is outside the view or cut by one of its edges, the case
/// of a step back to a caret the reader had scrolled away from.
pub(super) fn waiting_height(place: Place, row_h: f32, view_h: f32) -> (f32, bool) {
    // The height is counted from the top of the view the landing will
    // show, so that view starts at zero.
    filed_height(place.below, row_h, 0.0, view_h)
}

/// What `View::wanted` answers: the place, its row, the file's line
/// count, the source bytes of the pieces around a place inside a line
/// longer than `LONG_LINE`, and the offset the preview is built around.
pub(super) type WantedPlace = (Wanted, usize, usize, Option<std::ops::Range<usize>>, usize);

impl View {
    /// Scrolls to `y`, clamped to the page. A scroll that moved is
    /// marked for `App` (`moved`).
    pub(super) fn scroll_to(&mut self, page: &Page, y: f32) {
        let clamped = scroll::clamp(y, page.height(), page.view_h);
        if clamped != self.scroll_y {
            self.scroll_y = clamped;
            self.moved = true;
        }
    }

    /// The wheel, a page key, a pan: the view moves by `delta` from
    /// where it is, and stays where the hand leaves it.
    pub(super) fn scroll_by(&mut self, page: &Page, delta: f32) {
        // A step the clamp leaves in place, the wheel turned up at the
        // top, is still the reader's hand: the scroll alone would not
        // say so to what waits.
        if delta != 0.0 {
            self.drop_waits();
        }
        self.scroll_to(page, self.scroll_y + delta);
    }

    /// A movement to a height of the page that the layout holds: a
    /// step of the search, a link to a heading, the scrollbar of a
    /// page that is not measured.
    pub(super) fn jump_to(&mut self, page: &Page, y: f32) {
        self.drop_waits();
        self.scroll_to(page, y);
    }

    /// The Top command: the start of the page.
    pub(super) fn go_top(&mut self, page: &Page) {
        self.jump_to(page, 0.0);
    }

    /// The Bottom command: the end of the page.
    pub(super) fn go_bottom(&mut self, page: &Page) {
        self.drop_waits();
        self.scroll_to(page, page.height());
        // The placed height is all Oryx knows, so on a document
        // still streaming this lands short of the file's end;
        // the intent is held and the completing pass spends it.
        self.bottom_hold.take(self.scroll_y, !page.pending());
    }

    /// Whether a landing waits for the layout: it says where the view
    /// goes.
    fn landing_waits(&self) -> bool {
        self.pending_row.is_some() || self.pending_offset.is_some() || self.pending_anchor.is_some()
    }

    /// Whether a kept view or a landing waits for the layout.
    pub(super) fn waits(&self) -> bool {
        self.kept_view.is_some() || self.landing_waits()
    }

    /// Puts the row holding the place's offset at its height in the
    /// editor: at the top the way an outline jump lands, in the middle
    /// for a jump to a line, or where a crossing found the line on the
    /// page, never the way a typed caret is kept in view. A placed row
    /// answers exactly; past the placed height the block table answers
    /// by line index, which is what a source view is indexed by, and
    /// inside a very long line the row is read in its piece
    /// (`Page::row_landing`).
    pub(super) fn seat_editor_on(&mut self, page: &mut Page, place: Place) {
        // A row the block table knows may still lie below the height the
        // pass has placed; scrolling now would stop short, so the target
        // is held until the document is tall enough to show it.
        let (height, vh) = (page.height(), page.view_h);
        let target = page.row_landing(place.offset, place.below);
        match target {
            Some(y) if scroll::reached(y, height, vh) || !page.pending() => {
                self.pending_row = None;
                self.scroll_to(page, y);
            }
            _ => {
                self.pending_row = Some(place);
                // In the editor, while the file is measured, the
                // preview shows the place over the view, which stays
                // where it is (`wanted`). A jump to the end
                // still held from the page is older than the seat: spent
                // at the measure's end, it would move the view, and the
                // seat would be released as if the reader had.
                self.row_parked = None;
                if page.editing && page.measured() {
                    self.bottom_hold.clear();
                    self.row_parked = Some(self.scroll_y);
                }
            }
        }
    }

    /// Releases the seat of the editor that waits for the measure of
    /// the file, once the view is no longer where the seat found it:
    /// the wheel, the scrollbar or a key has moved it since. The
    /// caret's view is no longer owed either, or the next frame would
    /// seat its line again.
    pub(super) fn release_parked_row(&mut self) {
        let Some(parked) = self.row_parked else {
            return;
        };
        if self.pending_row.is_some() && self.scroll_y != parked {
            self.drop_parked_row();
        }
        if self.pending_row.is_none() {
            self.row_parked = None;
        }
    }

    /// Drops a seat of the editor that waits for the measure of the
    /// file, with the caret's owed view. Nothing for any other seat.
    pub(super) fn drop_parked_row(&mut self) {
        if self.row_parked.take().is_some() && self.pending_row.take().is_some() {
            self.caret_snap = false;
            self.caret_seat = None;
        }
    }

    /// Shows again a seat of the editor that still waits after a new
    /// pass started, at an edit or a zoom step: the new pass clamped
    /// the view, so the seat is counted from where the view is now.
    /// Asked by the frame once the scroll is clamped.
    pub(super) fn show_waiting_row(&mut self, page: &Page) {
        if self.pending_row.is_some()
            && self.row_parked.is_none()
            && page.editing
            && page.measured()
        {
            self.row_parked = Some(self.scroll_y);
        }
        // The last rule of a frame: the mark of a wait stands only
        // beside the landing it waits for.
        debug_assert!(
            self.row_parked.is_none() || self.pending_row.is_some(),
            "a parked row with no row that waits"
        );
        debug_assert!(
            self.thumb_landing.is_none() || self.pending_offset.is_some(),
            "a landing of the thumb with no offset that waits"
        );
        debug_assert!(
            self.waiting() <= 1,
            "more than one request waits: kept {:?}, row {:?}, offset {:?}, anchor {:?}",
            self.kept_view,
            self.pending_row,
            self.pending_offset,
            self.pending_anchor
        );
    }

    /// Brings the view to the caret after it moved. A row the layout
    /// holds answers exactly. A caret on a line the layout does not
    /// hold has no row to read, the case of Ctrl+End in a long file:
    /// its line is seated like a jump, and the frame that lays its rows
    /// out finishes on the caret's own row (`caret_snap`). A snap that
    /// an edit still owes the frame stays owed (`snap_still_owed`).
    pub(super) fn show_caret(&mut self, page: &mut Page) {
        let view_h = page.view_h;
        // The caret moved on, so a seat still owed to the place it left
        // would move the view after this one.
        let owed = self.caret_snap;
        // So would a seat made for another place than the caret's, the
        // line of a jump the caret has left since.
        let left = match (self.pending_row, page.caret) {
            (Some(place), Some(caret)) => place.offset != caret.offset,
            _ => false,
        };
        if owed || left {
            self.pending_row = None;
        }
        self.caret_seat = None;
        self.leave_edit_view();
        self.caret_takes_over();
        let (Some(caret), Some(lay)) = (page.caret, page.lay) else {
            return;
        };
        let found = caret.geometry(lay, page.doc, page.fonts);
        let settled = match caret_row(found, lay.height, page.pending()) {
            CaretRow::Placed(b) => {
                let target = caret::snap(self.scroll_y, view_h, b);
                if target != self.scroll_y {
                    self.scroll_to(page, target);
                }
                true
            }
            // The kept view brings the caret back where it stood: no
            // seat is made beside it.
            CaretRow::Guess if self.keeps_caret_row() => false,
            // The seat of the caret's line is the request that waits:
            // a row kept at its height was the place the caret left.
            CaretRow::Guess | CaretRow::Unknown => {
                self.kept_view = None;
                self.seat_caret_line(page)
            }
        };
        self.caret_snap = snap_still_owed(owed, settled);
    }

    /// Seats the line of a caret whose row the layout does not hold. The
    /// block table answers for the line, as for a jump, and the line
    /// comes to the edge of the view it was beyond. A line that stands
    /// in the view stays where it is. True when nothing is left to wait
    /// for: the line stands in the view and no pass runs, or the layout
    /// has no answer and no pass that could bring one. While the pass
    /// runs, the row of the caret is still owed, and `caret_seat` keeps
    /// the view that the seat left.
    fn seat_caret_line(&mut self, page: &mut Page) -> bool {
        let view_h = page.view_h;
        let Some(caret) = page.caret else {
            return true;
        };
        let row_h = page.row_h();
        let pending = page.pending();
        match page.editor_row_y(caret.offset) {
            Some(y) if caret_line_in_view(y, row_h, self.scroll_y, view_h) => {
                if pending {
                    self.caret_seat = Some(CaretSeat {
                        offset: caret.offset,
                        scroll: self.scroll_y,
                    });
                }
                !pending
            }
            Some(y) => {
                // While the pass runs, the table is short of the lines
                // it has not placed: the seat waits for them in
                // `pending_row`, as a jump does. A seat that was spent
                // is kept, so that no frame makes it again.
                let below = caret_line_edge(y, row_h, self.scroll_y, view_h);
                self.seat_editor_on(
                    page,
                    Place {
                        offset: caret.offset,
                        below,
                        line: false,
                    },
                );
                self.caret_seat = self.pending_row.is_none().then_some(CaretSeat {
                    offset: caret.offset,
                    scroll: self.scroll_y,
                });
                false
            }
            None => !pending,
        }
    }

    /// The frame's part of bringing the view to the caret (`caret_snap`):
    /// the exact row once the layout holds it, and until then the seat
    /// of its line, made once (`caret_due`). True once the view is
    /// settled, the reader moved it, or nothing more can come.
    pub(super) fn settle_caret(&mut self, page: &mut Page) -> bool {
        let view_h = page.view_h;
        let (Some(caret), Some(lay)) = (page.caret, page.lay) else {
            return page.caret.is_none();
        };
        // `land` ran earlier in the frame, and a settled
        // layout leaves it no kept view. One left here would hold the
        // snap back with no pass to bring it.
        debug_assert!(
            page.pending() || !matches!(self.kept_view, Some(Kept::Edit(_))),
            "a view kept at an edit outlived its pass"
        );
        let found = caret.geometry(lay, page.doc, page.fonts);
        let height = lay.height;
        let found = match caret_row(found, height, page.pending()) {
            CaretRow::Placed(b) => match self.kept_view.and_then(|kept| kept.edit_holds(b, view_h))
            {
                // The view the edit kept shows the caret's row: it
                // comes back first, and the snap finds the row in it.
                Some(true) => return false,
                // The caret ended elsewhere than the edit, as after
                // Replace All: the kept view would take the view from
                // it once the snap has shown it.
                Some(false) => {
                    self.kept_view = None;
                    Some(b)
                }
                None => Some(b),
            },
            // The kept view brings the caret back where it stood, and
            // that is the caret's landing: no seat is made beside it,
            // or the seat would land the caret's line at an edge of
            // the view. The snap stays owed, and finds the caret in
            // view once the kept view is back.
            CaretRow::Guess | CaretRow::Unknown if self.keeps_caret_row() => return false,
            // A guess is not snapped to: the caret's line is seated as
            // a far caret's is, and lands when the pass reaches it.
            CaretRow::Guess | CaretRow::Unknown => None,
        };
        let due = caret_due(
            found.is_some(),
            self.caret_seat,
            caret.offset,
            self.scroll_y,
            page.pending(),
        );
        match (due, found) {
            (CaretDue::Snap, Some(b)) => {
                let target = caret::snap(self.scroll_y, view_h, b);
                self.scroll_y = scroll::clamp(target, height, view_h);
                self.caret_seat = None;
                true
            }
            (CaretDue::Seat, _) => self.seat_caret_line(page),
            (CaretDue::Wait, _) => false,
            _ => {
                self.caret_seat = None;
                true
            }
        }
    }

    /// The caret was seated after an edit or an undo step: the next
    /// placed frame snaps the view to it.
    pub(super) fn owe_caret(&mut self) {
        self.caret_snap = true;
        self.caret_seat = None;
        self.caret_takes_over();
    }

    /// The view goes to the caret, after an edit or a caret key: what
    /// waits for another place is dropped. That is the place of a drop
    /// of the thumb, a link's, a jump to the end, and a view kept by
    /// its top or its distance, which is the view the caret leaves: in
    /// a big file it would take the view back seconds later. The
    /// caret's own view stays: its row kept at its height, the view of
    /// an edit, the seat of its line.
    fn caret_takes_over(&mut self) {
        self.pending_anchor = None;
        self.pending_offset = None;
        self.thumb_landing = None;
        self.landing_settle = None;
        self.bottom_hold.clear();
        self.steps_held = 0;
        self.kept_view = self.kept_view.filter(|kept| kept.shows_caret());
    }

    /// A click placed the caret, and the view stays where the click
    /// found it.
    pub(super) fn caret_clicked(&mut self) {
        self.drop_waits();
    }

    /// Releases the landing a drop of the thumb left waiting, once the
    /// view is no longer where the drop left it: a wheel step, a key,
    /// a jump of the search or a caret's seat has moved it since.
    pub(super) fn release_thumb_landing(&mut self) {
        let Some(parked) = self.thumb_landing else {
            return;
        };
        if self.pending_offset.is_some() && self.scroll_y != parked {
            self.pending_offset = None;
        }
        if self.pending_offset.is_none() {
            self.thumb_landing = None;
        }
    }

    /// The thumb of the scrollbar names `offset` in a file that is
    /// still measured: the view lands there when the measure reaches
    /// it.
    pub(super) fn thumb_names(&mut self, offset: usize) {
        self.land_at(Place::top(offset));
    }

    /// The view lands on `place` of the source once the layout reaches
    /// it: a link, an entry of the outline, a step of the history
    /// while reading, the thumb of the scrollbar.
    pub(super) fn land_at(&mut self, place: Place) {
        self.drop_waits();
        self.pending_offset = Some(place);
    }

    /// The view lands on the heading `anchor` names once the layout
    /// places it.
    pub(super) fn land_at_anchor(&mut self, anchor: String) {
        self.drop_waits();
        self.pending_anchor = Some(anchor);
    }

    /// The place the thumb named still waits after `land`: the page
    /// goes to the end of the measured part, which the layout does not
    /// hold, so that the preview stands on an empty page.
    pub(super) fn thumb_waits(&mut self, page: &Page) {
        if self.pending_offset.is_some() {
            self.scroll_to(page, page.height());
            self.thumb_landing = Some(self.scroll_y);
        }
    }

    /// Drops what waits for the layout: a landing, a kept view, a
    /// jump to the end, the view owed to the caret. One request waits,
    /// the last one. A movement of the reader drops what waited before
    /// it, then asks for its own place: an older request would move the
    /// view after it, seconds later in a big file.
    pub(super) fn drop_waits(&mut self) {
        self.kept_view = None;
        self.pending_anchor = None;
        self.pending_offset = None;
        self.thumb_landing = None;
        self.pending_row = None;
        self.row_parked = None;
        self.landing_settle = None;
        self.bottom_hold.clear();
        self.caret_snap = false;
        self.caret_seat = None;
        self.steps_held = 0;
    }

    /// As `drop_waits`, for a request that is neither a movement nor a
    /// letter: the editor opened or left. The steps of the history
    /// that wait are places of the file, the same in both, and are
    /// still asked for.
    pub(super) fn drop_waits_keep_steps(&mut self) {
        let steps = self.steps_held;
        self.drop_waits();
        self.steps_held = steps;
    }

    /// A step of the history asked while the layout still owes
    /// places: it waits for the end of the measure. The place the step
    /// leaves is read from the layout, and so is the place it lands
    /// on: both are exact only in a layout that is whole. What waits
    /// already stays and lands first, and steps add up. A movement or
    /// a letter made meanwhile is the last request, and drops them
    /// (`drop_waits`, `caret_takes_over`).
    pub(super) fn hold_step(&mut self, forward: bool) {
        self.steps_held += if forward { 1 } else { -1 };
    }

    /// The steps of the history to make now, positive forward: the
    /// ones that waited, once the layout owes nothing more. None
    /// while it does.
    pub(super) fn take_steps(&mut self, page: &Page) -> Option<i32> {
        if self.steps_held == 0 || !page.whole() {
            return None;
        }
        Some(std::mem::take(&mut self.steps_held))
    }

    /// Whether a jump to the end waits for the measure to end.
    pub(super) fn end_waits(&self) -> bool {
        self.bottom_hold.held_at(self.scroll_y)
    }

    /// Holds the end again from where the view is, after something
    /// that is not a movement of the reader changed the scroll or
    /// dropped what waited (`end_waits`, read before it): a zoom step,
    /// the clamp of a new pass, the editor opened on a selection.
    pub(super) fn hold_end(&mut self) {
        self.bottom_hold.take(self.scroll_y, false);
    }

    /// How many requests wait for the layout. The caret's owed view is
    /// not counted: it follows the request that put the caret there.
    fn waiting(&self) -> usize {
        [
            self.kept_view.is_some(),
            self.pending_row.is_some(),
            self.pending_offset.is_some(),
            self.pending_anchor.is_some(),
            self.bottom_hold.held_at(self.scroll_y),
        ]
        .into_iter()
        .filter(|waits| *waits)
        .count()
    }

    /// Where the reader stands, as the history files it: the offset,
    /// the height of its line under the top of the view, whether it is
    /// a caret's, and whether the caret's row is away from the view.
    /// The caret while editing, else what shows at the top of the
    /// view. A landing that still waits for the layout answers before
    /// the screen, which has not moved yet (`waiting_place`).
    pub(super) fn standing(&self, page: &mut Page) -> Option<(usize, f32, bool, bool)> {
        let editing = page.editing && page.caret.is_some();
        let caret = page.caret.filter(|_| editing).map(|caret| caret.offset);
        let waiting = waiting_place(self.pending_row, self.pending_offset, caret);
        let stands = match (caret, waiting) {
            (Some(caret), Some(place)) => {
                let (below, away) = waiting_height(place, page.row_h(), page.view_h);
                (caret, below, true, away)
            }
            (None, Some(place)) => (place.offset, place.below, place.line, false),
            (Some(caret), None) => {
                let row = page.row_y_in_piece(caret);
                let (below, away) = row.map_or((0.0, false), |y| {
                    filed_height(y, page.row_h(), self.scroll_y, page.view_h)
                });
                (caret, below, true, away)
            }
            (None, None) => {
                let lay = page.lay?;
                let offset = scroll::top_offset(lay, page.doc, self.scroll_y);
                let below =
                    scroll::offset_top(lay, page.doc, offset).map_or(0.0, |y| y - self.scroll_y);
                (offset, below, false, false)
            }
        };
        Some(stands)
    }

    /// Keeps the place of the view across a new layout of the same
    /// text, for a layout made with rows of `row_h`. Nothing is taken
    /// while a landing waits, which says where the view goes, or while
    /// an earlier kept view waits, which is still the place of the
    /// reader.
    pub(super) fn keep_view_with(&mut self, page: &Page, row_h: f32) {
        if self.landing_waits() || self.end_waits() || self.kept_view.is_some() {
            return;
        }
        let Some(lay) = page.lay else {
            return;
        };
        let caret_row = match (page.editing, page.caret) {
            (true, Some(caret)) => page.editor_row_y(caret.offset),
            _ => None,
        };
        let kept = Kept::of(caret_row, row_h, self.scroll_y, page.view_h, || {
            scroll::TopPlace::of(lay, page.doc, self.scroll_y)
        });
        self.kept_view = kept.and_then(|kept| kept.under_snap(self.snap_owed(page)));
    }

    /// As `keep_view_with`, with the rows of the page as it is.
    pub(super) fn keep_view(&mut self, page: &Page) {
        self.keep_view_with(page, page.row_h());
    }

    /// Whether an edit still owes the view to its caret (`caret_snap`).
    pub(super) fn snap_owed(&self, page: &Page) -> bool {
        self.caret_snap && page.editing
    }

    /// Whether the view a new pass keeps shows the caret
    /// (`Kept::shows_caret`): `land` brings it back, and
    /// that is the caret's landing.
    fn keeps_caret_row(&self) -> bool {
        self.kept_view.is_some_and(Kept::shows_caret)
    }

    /// Drops the view an edit kept (`Kept::Edit`) once the caret has
    /// moved by a key or a click: that view showed the caret where the
    /// edit was, and the caret has left it.
    pub(super) fn leave_edit_view(&mut self) {
        if matches!(self.kept_view, Some(Kept::Edit(_))) {
            self.kept_view = None;
        }
    }

    /// What an edit at `offset` keeps of the view if it has to drop
    /// the layout, read while the layout and the text still agree
    /// (`Kept::of_edit`). None while a kept place or a landing waits:
    /// the scroll is then not the view the reader sees.
    pub(super) fn edit_view(&self, page: &Page, offset: usize) -> Option<Kept> {
        let waits = self.kept_view.is_some() || self.landing_waits();
        if waits || !page.editing {
            return None;
        }
        Kept::of_edit(
            page.editor_row_y(offset),
            page.row_h(),
            self.scroll_y,
            page.view_h,
        )
    }

    /// The edit dropped the layout, as in a file still measured: the
    /// new pass brings back the view of an edit made in view
    /// (`edit_view`, read before the edit).
    pub(super) fn keep_edit_view(&mut self, edit_view: Option<Kept>) {
        if self.kept_view.is_none() {
            self.kept_view = edit_view;
        }
    }

    /// A zoom step, before the layout is dropped. The view keeps its
    /// place: the caret's row in the editor, the top of the view on
    /// the page.
    pub(super) fn zoom_step(&mut self, page: &Page, ratio: f32) {
        let end_waits = self.end_waits();
        self.keep_view(page);
        self.kept_view = self.kept_view.map(|kept| kept.zoomed(ratio));
        // What the view shows while the pass runs, until the kept
        // place is reached.
        self.scroll_y *= ratio;
        // A jump to the end that waits is told from a movement of the
        // reader by the scroll it left, which the step has just
        // changed: the end still waits.
        if end_waits {
            self.hold_end();
        }
    }

    /// A new pass is about to start, with the layout of the old one
    /// still in the page when there is one.
    pub(super) fn pass_starts(&mut self, page: &Page) {
        // A new pass clamps the scroll to what it has placed: a place a
        // drop of the thumb waits for stays, as a landing like another.
        self.thumb_landing = None;
        self.row_parked = None;
        // A new width, of the window or of the sidebar, with the layout
        // of the old one still here: the view keeps its place.
        self.keep_view(page);
        // A jump to the end that waits says where the view goes, as a
        // landing does.
        let landing_waits = self.landing_waits() || self.end_waits();
        if landing_waits {
            // The landing says where the view goes. A view kept before
            // it would move the page after it.
            self.kept_view = None;
        } else if self.kept_view.is_none()
            && keeps_scroll(self.scroll_y, landing_waits)
            && !self.snap_owed(page)
        {
            self.kept_view = Some(Kept::Distance(self.scroll_y));
        }
    }

    /// A slice of the pass has run. A jump to the bottom taken while
    /// the document was still being placed is spent here, on the whole
    /// document.
    pub(super) fn pass_sliced(&mut self, page: &Page) {
        if self.bottom_hold.settle(self.scroll_y, !page.pending()) {
            self.scroll_to(page, page.height());
        }
    }

    /// Finishes a line landing on the frame that made it: the slide has
    /// drawn the rows around the landing, so the line's own row now
    /// answers and stands at its height. A row that still does not
    /// answer leaves the landing as it was.
    pub(super) fn settle_landing(&mut self, page: &Page) {
        let Some(place) = self.landing_settle.take() else {
            return;
        };
        let row = page
            .lay
            .and_then(|lay| caret::line_top(lay, page.doc, place.offset));
        if let Some(y) = row {
            self.scroll_to(page, caret::seated(y, place.below));
        }
    }

    /// Applies a scroll position or an anchor asked for before the pass
    /// had placed it: the kept view, the row, the offset, then the
    /// anchor.
    pub(super) fn land(&mut self, page: &mut Page) {
        if !self.waits() {
            return;
        }
        let height = page.height();
        let vh = page.view_h;
        let before_landings = self.scroll_y;
        if let Some(kept) = self.kept_view {
            let target = match kept {
                Kept::Distance(y) | Kept::Edit(y) => Some(y),
                Kept::Caret(below) => {
                    let below = kept_caret_height(below, vh, page.row_h());
                    page.caret
                        .and_then(|caret| page.row_landing(caret.offset, below))
                }
                Kept::Top(place) => page.lay.and_then(|lay| place.scroll(lay, page.doc)),
            };
            let settled = !page.pending();
            match target {
                Some(y) if scroll::reached(y, height, vh) => {
                    self.kept_view = None;
                    self.scroll_y = scroll::clamp(y, height, vh);
                }
                // The pass ended short of a kept place, as after a zoom
                // out near the end of the file: the view shows the end.
                Some(y) if settled && !matches!(kept, Kept::Distance(_) | Kept::Edit(_)) => {
                    self.kept_view = None;
                    self.scroll_y = scroll::clamp(y, height, vh);
                }
                _ if settled => self.kept_view = None,
                _ => {}
            }
        }
        // A row asked for before the layout reached it: the same
        // pending discipline as the offset target below, resolved
        // against rows rather than blocks, since the editor is indexed
        // by lines and a source view is one block.
        if let Some(place) = self.pending_row {
            let target = page.row_landing(place.offset, place.below);
            match target {
                Some(y) if scroll::reached(y, height, vh) || !page.pending() => {
                    self.pending_row = None;
                    // A kept view that still waits is older than the
                    // landing, and would move the view after it.
                    self.kept_view = None;
                    self.scroll_to(page, y);
                }
                None if page.lay.is_some() && !page.pending() => self.pending_row = None,
                _ => {}
            }
        }
        if let Some(place) = self.pending_offset {
            let Place { offset, below, .. } = place;
            // Held while the offset lies past the delivered source; the
            // worker's delivery brings the rest.
            let covered = offset < page.doc.source.len() || !page.parse_pending;
            if covered {
                let placed = if place.line {
                    page.line_landing(offset, below)
                } else {
                    page.lay.and_then(|lay| {
                        scroll::offset_top(lay, page.doc, offset).map(|y| caret::seated(y, below))
                    })
                };
                match placed {
                    Some(y) if scroll::reached(y, height, vh) || !page.pending() => {
                        self.pending_offset = None;
                        self.kept_view = None;
                        self.landing_settle = Some(place).filter(|place| place.line);
                        self.scroll_to(page, y);
                    }
                    None if !page.pending() && !page.parse_pending => {
                        self.pending_offset = None;
                    }
                    _ => {}
                }
            }
        }
        // A kept view that came back, or another landing that waited,
        // is not the reader's move: a seat of the editor that still
        // waits is counted from where they left the view.
        if self.scroll_y != before_landings
            && self.pending_row.is_some()
            && self.row_parked.is_some()
        {
            self.row_parked = Some(self.scroll_y);
        }
        let Some(name) = self.pending_anchor.clone() else {
            return;
        };
        match page.lay.and_then(|l| l.anchor_y(&name)) {
            Some(y) if scroll::reached(y, height, vh) || !page.pending() => {
                self.pending_anchor = None;
                self.kept_view = None;
                self.scroll_to(page, y);
            }
            // The pass ended without ever placing that heading.
            None if !page.pending() => self.pending_anchor = None,
            _ => {}
        }
    }

    /// Holds the scroll within what the pass has placed, before the
    /// window of the layout slides to it.
    pub(super) fn clamp(&mut self, page: &Page) {
        if let Some(lay) = page.lay {
            // The clamp of a new pass is not the reader's hand: a jump
            // to the end that waits is held from where the clamp
            // leaves the view.
            let end_waits = self.end_waits();
            self.scroll_y = scroll::clamp(self.scroll_y, lay.height, page.view_h);
            if end_waits {
                self.hold_end();
            }
            debug_assert!(
                self.scroll_y >= 0.0 && self.scroll_y <= lay.height.max(0.0),
                "the scroll {} is out of a page {} tall",
                self.scroll_y,
                lay.height
            );
        }
    }

    /// What a preview would show, in a view `page.view_h` tall: the
    /// place, its row, the file's line count and, for a place inside a
    /// line longer than `LONG_LINE`, the source bytes of the pieces
    /// around it. None when the layout shows the view and no seat of
    /// the editor waits, when no pass runs, and for a document that is
    /// not one block of lines.
    pub(super) fn wanted(&self, page: &Page) -> Option<WantedPlace> {
        let view_h = page.view_h;
        if !page.pass_active() || page.exporting || page.parse_pending {
            return None;
        }
        let doc = page.doc;
        let lines = match doc.blocks.first().map(|block| &block.kind) {
            Some(BlockKind::CodeBlock { lines, .. }) if doc.blocks.len() == 1 => lines,
            _ => return None,
        };
        let total = lines.len();
        if total == 0 || !(doc.code_file || doc.plain_file) {
            return None;
        }
        let lay = page.lay?;
        let pass = page.pass?;
        // A view that reaches past what is placed is the pass's own
        // first frames.
        if lay.height < self.scroll_y + view_h {
            return None;
        }
        let row_of = |offset: usize| scroll::source_row(doc, 0, offset).min(total - 1);
        // The first and the last line of the view, which the block
        // table answers exactly: the lines above them are measured.
        // `top_offset` reads one pixel under what it is given.
        // In a very long line the place is a piece of the line: the
        // layout holds the pieces around the view, not the line.
        let held = |y: f32| {
            let (line, byte) = lay.code_place_at(0, total, y + 1.0)?;
            pass.keeps(lay, line, byte)
        };
        let top = row_of(scroll::top_offset(lay, doc, self.scroll_y));
        // A drag of the scrollbar's thumb names its own landing
        // (`thumb_names`): the view follows the hand to it.
        let gesture = page.gesture;
        // A seat of the editor that waits is shown over a view the
        // layout holds too: the keys go to the caret, which is at that
        // place, not in the view. A press of the pointer brings the
        // page back, since a click reads the page.
        // With the search bar open the page shows too, with its
        // matches.
        let seat = !gesture
            && page.editing
            && !page.searching
            && self.pending_row.is_some()
            && self.row_parked.is_some();
        if !seat && held(self.scroll_y)? && held(self.scroll_y + view_h - 2.0)? {
            return None;
        }
        let landing = if gesture {
            None
        } else if let Some(place) = self.pending_row {
            Some((Wanted::Row(place), row_of(place.offset)))
        } else if let Some(place) = self.pending_offset {
            Some((Wanted::Offset(place), row_of(place.offset)))
        } else if let Some(Kept::Top(place)) = self.kept_view {
            Some((Wanted::Top(place), row_of(place.offset)))
        } else if let (Some(Kept::Caret(below)), Some(caret)) = (self.kept_view, page.caret) {
            Some((Wanted::Caret(caret.offset, below), row_of(caret.offset)))
        } else if self.bottom_hold.held_at(self.scroll_y) {
            Some((Wanted::Bottom, total - 1))
        } else {
            None
        };
        let (wanted, row) = landing.or_else(|| {
            let place = scroll::TopPlace::of(lay, doc, self.scroll_y)?;
            Some((Wanted::Top(place), top))
        })?;
        // The offset the preview is built around.
        let anchor = match &wanted {
            Wanted::Row(place) | Wanted::Offset(place) => place.offset,
            Wanted::Top(place) => place.offset,
            Wanted::Caret(offset, _) => *offset,
            Wanted::Bottom => lines.line_range(total - 1)?.end,
        };
        let (before, after) = paint::preview::pieces_for(view_h, page.row_h(), page.width);
        let piece = scroll::pieces_around(lay, doc, anchor, before, after);
        Some((wanted, row, total, piece, anchor))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use oryx::doc::images::MediaCache;
    use oryx::doc::load::FileKind;
    use oryx::doc::model::Document;
    use oryx::edit;
    use oryx::edit::caret::{self, Caret, Motion};
    use oryx::layout::{
        layout_begin, layout_more, layout_step, window_to, LayoutDoc, LayoutPass, ViewConfig,
    };
    use oryx::paint::scroll;
    use oryx::style::fonts::FontStore;
    use oryx::style::theme::Theme;
    use oryx::ui::history::{Entry, History};
    use proptest::prelude::*;

    use super::{Kept, Page, Place, View, WantedPlace};

    /// A text file in the window's place, with no window: the document,
    /// a real layout pass stepped a few lines a frame, the view, and
    /// the parts of `App` an action of the reader goes through before
    /// it reaches a rule of the view. `frame` calls the steps of a
    /// frame in the order of `App::redraw`, and each action is written
    /// as its function of `App` is: when one of them changes there, it
    /// changes here.
    struct Bench {
        text: String,
        doc: Document,
        lay: Option<LayoutDoc>,
        pass: Option<LayoutPass>,
        theme: Theme,
        fonts: FontStore,
        media: MediaCache,
        cfg: ViewConfig,
        /// The width of the window.
        width: f32,
        /// The width the layout was made for.
        laid_width: f32,
        view_h: f32,
        editing: bool,
        caret: Option<Caret>,
        view: View,
        history: History,
        /// How many steps of the pass a frame runs: a step places one
        /// line of the file.
        pace: usize,
    }

    /// What a test compares of the view between two frames.
    type Snapshot = (
        f32,
        Option<Kept>,
        Option<Place>,
        Option<Place>,
        Option<String>,
        (Option<f32>, Option<f32>, Option<Place>),
        (bool, bool, bool),
        Option<usize>,
    );

    impl Bench {
        fn new(text: String, editing: bool, pace: usize) -> Bench {
            let doc = edit::rendered_document(FileKind::Text, &text);
            Bench {
                text,
                doc,
                lay: None,
                pass: None,
                theme: Theme::default_dark(),
                fonts: FontStore::new(),
                media: MediaCache::offline(PathBuf::from(".")),
                cfg: ViewConfig::default(),
                width: 600.0,
                laid_width: 0.0,
                view_h: 200.0,
                editing,
                caret: editing.then(|| Caret::at(0)),
                view: View::default(),
                history: History::default(),
                pace,
            }
        }

        /// `App::page`.
        fn page(&mut self) -> (&mut View, Page<'_>) {
            (
                &mut self.view,
                Page {
                    doc: &self.doc,
                    lay: self.lay.as_ref(),
                    pass: self.pass.as_ref(),
                    theme: &self.theme,
                    fonts: &mut self.fonts,
                    media: &mut self.media,
                    cfg: &self.cfg,
                    width: self.laid_width,
                    view_h: self.view_h,
                    editing: self.editing,
                    caret: self.caret,
                    parse_pending: false,
                    exporting: false,
                    searching: false,
                    gesture: false,
                },
            )
        }

        fn pass_active(&self) -> bool {
            self.pass.as_ref().is_some_and(|pass| !pass.is_complete())
        }

        fn row_h(&mut self) -> f32 {
            self.page().1.row_h()
        }

        /// The slide of the layout's window to the view, in `redraw`.
        fn slide(&mut self) {
            if let Some(lay) = self.lay.as_mut() {
                window_to(
                    &self.doc,
                    &self.theme,
                    &mut self.fonts,
                    &mut self.media,
                    &self.cfg,
                    lay,
                    None,
                    self.view.scroll_y,
                    self.view_h,
                    true,
                );
                lay.index_more();
            }
        }

        /// `App::resolve_pending`, with no go to line that waits for a
        /// parse.
        fn resolve_pending(&mut self) {
            self.view.release_thumb_landing();
            self.view.release_parked_row();
            if !self.view.waits() {
                return;
            }
            let (view, mut page) = self.page();
            view.land(&mut page);
        }

        /// `App::make_held_steps`, in one file with no dialog.
        fn make_held_steps(&mut self) {
            let steps = {
                let (view, page) = self.page();
                view.take_steps(&page)
            };
            for _ in 0..steps.map_or(0, i32::abs) {
                self.back();
            }
        }

        /// One frame, in the order of `App::redraw`: the start of a
        /// pass, a slice, the landings, the clamp and the slide, the
        /// landing's row, the caret, the releases, the picture. Answers
        /// what the picture of the wait would show.
        fn frame(&mut self) -> Option<WantedPlace> {
            // The event loop between two frames, `App::about_to_wait`.
            self.make_held_steps();
            // `start_pass`.
            if self.lay.is_none() || self.laid_width != self.width {
                let (view, page) = self.page();
                view.pass_starts(&page);
                let (out, pass) = layout_begin(&self.doc, &self.cfg, self.width);
                self.lay = Some(out);
                self.pass = Some(pass);
                self.laid_width = self.width;
            }
            // `slice`: the steps, then the end of `layout_more`, which
            // an expired deadline keeps from placing anything more.
            if self.pass_active() {
                let lay = self.lay.as_mut().expect("a pass has a layout");
                let pass = self.pass.as_mut().expect("a pass is running");
                pass.retain_around(self.view.scroll_y, self.view_h);
                for _ in 0..self.pace {
                    let done = layout_step(
                        &self.doc,
                        &self.theme,
                        &mut self.fonts,
                        &mut self.media,
                        &self.cfg,
                        lay,
                        pass,
                    );
                    if done {
                        break;
                    }
                }
                layout_more(
                    &self.doc,
                    &self.theme,
                    &mut self.fonts,
                    &mut self.media,
                    &self.cfg,
                    lay,
                    pass,
                    Some(Instant::now()),
                );
                let (view, page) = self.page();
                view.pass_sliced(&page);
            }
            self.resolve_pending();
            {
                let (view, page) = self.page();
                view.clamp(&page);
            }
            self.slide();
            let before = self.view.scroll_y;
            {
                let (view, page) = self.page();
                view.settle_landing(&page);
            }
            if self.view.caret_snap && self.view.pending_row.is_none() {
                let (view, mut page) = self.page();
                view.caret_snap = !view.settle_caret(&mut page);
            }
            if self.view.scroll_y != before {
                self.slide();
            }
            self.view.release_thumb_landing();
            self.view.release_parked_row();
            let (view, page) = self.page();
            view.show_waiting_row(&page);
            let shown = view.wanted(&page);
            view.moved = false;
            shown
        }

        /// Frames until the pass has ended, then `more` frames.
        fn settle(&mut self, more: usize) {
            let mut frames = 0;
            while self.lay.is_none() || self.laid_width != self.width || self.pass_active() {
                self.frame();
                frames += 1;
                assert!(frames < 100_000, "the pass does not end");
            }
            for _ in 0..more {
                self.frame();
            }
        }

        fn snapshot(&self) -> Snapshot {
            let view = &self.view;
            (
                view.scroll_y,
                view.kept_view,
                view.pending_row,
                view.pending_offset,
                view.pending_anchor.clone(),
                (view.thumb_landing, view.row_parked, view.landing_settle),
                (
                    view.caret_snap,
                    view.caret_seat.is_some(),
                    view.bottom_hold.held_at(view.scroll_y),
                ),
                self.caret.map(|caret| caret.offset),
            )
        }

        /// The offset of the start of line `line`, the last line for a
        /// number past the end.
        fn line_start(&self, line: usize) -> usize {
            let starts: Vec<usize> = std::iter::once(0)
                .chain(self.text.match_indices('\n').map(|(at, _)| at + 1))
                .filter(|&at| at < self.text.len())
                .collect();
            starts[line.clamp(1, starts.len()) - 1]
        }

        /// `App::here`, in a file with a name.
        fn here(&mut self) -> Option<Entry> {
            let (view, mut page) = self.page();
            let (offset, below, editing, away) = view.standing(&mut page)?;
            Some(Entry {
                file: None,
                offset,
                below,
                editing,
                away,
            })
        }

        /// `App::push_jump`.
        fn push_jump(&mut self) {
            if let Some(here) = self.here() {
                self.history.jump(here);
            }
        }

        /// The wheel, `notches` down: `App::scroll_by`.
        fn wheel(&mut self, notches: f32) {
            let delta = notches * 3.0 * self.row_h();
            let (view, page) = self.page();
            view.scroll_by(&page, delta);
        }

        /// `App::go_to` in a code or text file.
        fn go_to(&mut self, line: usize) {
            let offset = self.line_start(line);
            self.push_jump();
            self.view.drop_waits();
            if self.editing {
                self.caret = Some(Caret::at(offset));
            }
            let place = Place::centered(offset, self.view_h, self.row_h());
            let (view, mut page) = self.page();
            view.seat_editor_on(&mut page, place);
        }

        /// A caret key: `App::step_caret`. `Ctrl+End` is `DocEnd`.
        fn key(&mut self, motion: Motion) -> bool {
            let step = (self.view_h - self.row_h()).max(self.row_h());
            let (Some(caret), Some(lay)) = (self.caret, self.lay.as_ref()) else {
                return false;
            };
            self.caret = Some(caret.step(motion, lay, &self.doc, &mut self.fonts, step));
            let (view, mut page) = self.page();
            view.show_caret(&mut page);
            true
        }

        /// The Top and Bottom commands of reading mode.
        fn go_edge(&mut self, bottom: bool) {
            self.push_jump();
            let (view, page) = self.page();
            if bottom {
                view.go_bottom(&page);
            } else {
                view.go_top(&page);
            }
        }

        /// A letter typed at the caret, by the path of `App::apply_edit`
        /// that drops the layout, then `App::seat_caret_after_edit`.
        fn letter(&mut self) -> bool {
            let Some(caret) = self.caret else {
                return false;
            };
            let at = caret.offset;
            let edit_view = {
                let (view, page) = self.page();
                view.edit_view(&page, at)
            };
            self.text.insert(at, 'x');
            self.doc = edit::rendered_document(FileKind::Text, &self.text);
            self.lay = None;
            self.view.keep_edit_view(edit_view);
            self.caret = Some(Caret::at(at + 1));
            self.view.owe_caret();
            true
        }

        /// A click in the text, at `x` and `y` of the view:
        /// `App::place_caret`.
        fn click(&mut self, x: f32, y: f32) -> bool {
            let Some(lay) = self.lay.as_ref() else {
                return false;
            };
            let y = y + self.view.scroll_y;
            let Some(placed) = caret::place(lay, &self.doc, &mut self.fonts, x, y) else {
                return false;
            };
            self.caret = Some(placed);
            self.view.caret_clicked();
            true
        }

        /// A zoom step: `App::set_zoom`.
        fn zoom(&mut self, zoom: f32) {
            if (zoom - self.cfg.zoom).abs() < f32::EPSILON {
                return;
            }
            let ratio = zoom / self.cfg.zoom;
            let (view, page) = self.page();
            view.zoom_step(&page, ratio);
            self.cfg.zoom = zoom;
            self.lay = None;
        }

        /// A drop of the scrollbar's thumb at `share` of its track:
        /// `App::drag_to`.
        fn thumb(&mut self, share: f32) {
            let measured = self.page().1.measured();
            if measured {
                let Some(lay) = self.lay.as_ref() else {
                    return;
                };
                let offset = scroll::offset_at_share(lay, &self.doc, share);
                self.view.thumb_names(offset);
                self.resolve_pending();
                let (view, page) = self.page();
                view.thumb_waits(&page);
                return;
            }
            let (view, page) = self.page();
            let target = share * (page.height() - page.view_h).max(0.0);
            view.jump_to(&page, target);
        }

        /// `Alt+Left`: `App::step_history` and `App::land_on`, in one
        /// file.
        fn back(&mut self) -> bool {
            if !self.page().1.whole() {
                let waits = self.view.steps_held != 0 || {
                    let here = self.here();
                    self.history
                        .peek(false, here.as_ref())
                        .or_else(|| self.history.peek(false, None))
                        .is_some()
                };
                if waits {
                    self.view.hold_step(false);
                }
                return false;
            }
            let here = self.here();
            let Some(target) = self.history.peek(false, here.as_ref()).cloned() else {
                return false;
            };
            self.history.step(false, here);
            self.view.drop_waits();
            let (view_h, row_h) = (self.view_h, self.row_h());
            if self.editing {
                let offset = caret::clamp(&self.doc, target.offset);
                self.caret = Some(Caret::at(offset));
                let place = Place::returned(offset, target.below, target.away, view_h, row_h, true);
                let (view, mut page) = self.page();
                view.seat_editor_on(&mut page, place);
            } else {
                self.view.pending_offset = Some(
                    Place::returned(
                        target.offset,
                        target.below,
                        target.away,
                        view_h,
                        row_h,
                        false,
                    )
                    .by_line(target.editing),
                );
            }
            true
        }
    }

    /// A text file of `lines` numbered lines, one in seven long enough
    /// to wrap in the window of the bench.
    fn numbered(lines: usize) -> String {
        (1..=lines)
            .map(|n| {
                if n % 7 == 0 {
                    format!("line {n} {}\n", "wraps and goes on ".repeat(8))
                } else {
                    format!("line {n}\n")
                }
            })
            .collect()
    }

    #[test]
    fn a_jump_in_a_measured_file_lands_where_it_lands_in_a_measured_one() {
        // The same jump in the editor of a file measured twelve lines a
        // frame, and of the file measured whole in its first frame.
        let lands = |pace: usize| {
            let mut bench = Bench::new(numbered(400), true, pace);
            bench.frame();
            bench.go_to(300);
            bench.settle(3);
            (bench.view.scroll_y, bench.snapshot())
        };
        let (slow, state) = lands(12);
        let (whole, _) = lands(usize::MAX);
        assert!(slow > 1000.0, "the view went down the file, to {slow}");
        assert_eq!(slow, whole, "the wait changes where the jump lands");
        assert_eq!(state.2, None, "no row waits once the measure has ended");
    }

    /// The editor of a file measured three lines a frame, the caret
    /// put on line 200 by a jump and a click in the view, the measure
    /// ended.
    fn editor_at_200() -> Bench {
        let mut bench = Bench::new(numbered(LINES), true, 3);
        bench.frame();
        bench.go_to(200);
        bench.settle(REST);
        bench.click(40.0, 100.0);
        bench.frame();
        bench
    }

    #[test]
    fn the_wheel_after_a_letter_is_not_undone_by_the_measure() {
        let mut bench = editor_at_200();
        bench.letter();
        for _ in 0..4 {
            bench.frame();
        }
        bench.wheel(2.0);
        bench.frame();
        let left = bench.view.scroll_y;
        bench.settle(REST);
        assert_eq!(
            bench.view.scroll_y, left,
            "the wheel left the view at {left}, and the measure moved it"
        );
    }

    #[test]
    fn the_wheel_after_a_jump_while_reading_is_not_undone() {
        let mut bench = Bench::new(numbered(LINES), false, 3);
        bench.frame();
        bench.go_to(300);
        for _ in 0..10 {
            bench.frame();
        }
        bench.wheel(2.0);
        bench.frame();
        let left = bench.view.scroll_y;
        assert!(left > 0.0, "the wheel moved the view");
        bench.settle(REST);
        assert_eq!(
            bench.view.scroll_y, left,
            "the wheel left the view at {left}, and the jump that waited moved it"
        );
    }

    #[test]
    fn the_wheel_after_a_drop_of_the_thumb_and_a_zoom_step_is_not_undone() {
        let mut bench = Bench::new(numbered(LINES), true, 3);
        bench.frame();
        bench.thumb(0.5);
        bench.frame();
        bench.zoom(1.25);
        for _ in 0..10 {
            bench.frame();
        }
        bench.wheel(2.0);
        bench.frame();
        let left = bench.view.scroll_y;
        bench.settle(REST);
        assert_eq!(
            bench.view.scroll_y, left,
            "the wheel left the view at {left}, and the place of the drop took it"
        );
    }

    #[test]
    fn the_end_asked_after_a_jump_that_waits_is_where_the_view_ends() {
        // G1 of Task 408.
        let ends = |pace: usize| {
            let mut bench = Bench::new(numbered(LINES), false, pace);
            bench.frame();
            bench.go_to(17);
            bench.frame();
            bench.go_edge(true);
            bench.settle(REST);
            bench.view.scroll_y
        };
        assert_eq!(
            ends(3),
            ends(usize::MAX),
            "the jump that waited landed after the end"
        );
    }

    #[test]
    fn the_top_asked_after_a_jump_that_waits_is_where_the_view_ends() {
        let mut bench = Bench::new(numbered(LINES), false, 3);
        bench.frame();
        bench.go_to(300);
        bench.frame();
        bench.go_edge(false);
        bench.settle(REST);
        assert_eq!(
            bench.view.scroll_y, 0.0,
            "the jump that waited landed after the top"
        );
    }

    #[test]
    fn a_letter_after_a_drop_of_the_thumb_stays_in_view() {
        // G2 of Task 408.
        let mut bench = Bench::new(numbered(LINES), true, 3);
        bench.frame();
        bench.thumb(0.14);
        bench.frame();
        bench.letter();
        bench.settle(REST);
        assert_eq!(bench.caret_row_shown(true), Ok(()));
    }

    #[test]
    fn a_caret_key_after_a_drop_of_the_thumb_stays_in_view() {
        let mut bench = Bench::new(numbered(LINES), true, 3);
        bench.frame();
        bench.thumb(0.5);
        bench.frame();
        bench.key(Motion::Down);
        bench.settle(REST);
        assert_eq!(bench.caret_row_shown(true), Ok(()));
    }

    #[test]
    fn the_wheel_at_the_top_releases_a_jump_to_the_end_that_waits() {
        // G5 of Task 408: the clamp leaves the step in place.
        let mut bench = Bench::new(numbered(LINES), false, 3);
        bench.frame();
        bench.go_edge(true);
        bench.frame();
        bench.wheel(-1.0);
        bench.settle(REST);
        assert_eq!(
            bench.view.scroll_y, 0.0,
            "the view went to the end after the wheel"
        );
    }

    #[test]
    fn a_jump_to_a_height_after_a_letter_is_not_undone_by_the_measure() {
        let mut bench = editor_at_200();
        bench.letter();
        for _ in 0..4 {
            bench.frame();
        }
        let (view, page) = bench.page();
        view.jump_to(&page, 40.0);
        bench.frame();
        let left = bench.view.scroll_y;
        bench.settle(REST);
        assert_eq!(
            bench.view.scroll_y, left,
            "the jump left the view at {left}, and the measure moved it"
        );
    }

    /// What starts the measure again under a request that waits: a
    /// zoom step, or a new width of the window, which the sidebar and
    /// a resize give.
    #[derive(Debug, Clone, Copy)]
    enum Again {
        Zoom,
        Width,
    }

    const AGAIN: [Again; 2] = [Again::Zoom, Again::Width];

    impl Bench {
        fn again(&mut self, how: Again) {
            match how {
                Again::Zoom => self.zoom(1.25),
                Again::Width => self.width = 480.0,
            }
        }

        /// The source row at the top of the view.
        fn top_row(&self) -> usize {
            let lay = self.lay.as_ref().expect("a layout");
            scroll::source_row(
                &self.doc,
                0,
                scroll::top_offset(lay, &self.doc, self.view.scroll_y),
            )
        }

        /// Whether the row holding `offset` is whole in the view.
        fn row_in_view(&mut self, offset: usize) -> bool {
            let (row_h, view_h, scroll) = (self.row_h(), self.view_h, self.view.scroll_y);
            let (_, page) = self.page();
            page.editor_row_y(offset)
                .is_some_and(|y| y >= scroll && y + row_h <= scroll + view_h)
        }

        /// The scroll that shows the end of the file.
        fn end_scroll(&self) -> f32 {
            let height = self.lay.as_ref().expect("a layout").height;
            (height - self.view_h).max(0.0)
        }
    }

    #[test]
    fn a_jump_of_the_editor_that_waits_lands_after_a_new_measure() {
        for how in AGAIN {
            for end in [false, true] {
                let mut bench = Bench::new(numbered(LINES), true, 3);
                bench.frame();
                if end {
                    bench.key(Motion::DocEnd);
                } else {
                    bench.go_to(300);
                }
                let caret = bench.caret.expect("a caret").offset;
                bench.frame();
                bench.again(how);
                bench.settle(REST);
                assert_eq!(
                    bench.caret.map(|caret| caret.offset),
                    Some(caret),
                    "{how:?}, end {end}"
                );
                assert_eq!(bench.caret_row_shown(true), Ok(()), "{how:?}, end {end}");
            }
        }
    }

    #[test]
    fn a_jump_while_reading_that_waits_lands_after_a_new_measure() {
        for how in AGAIN {
            let mut bench = Bench::new(numbered(LINES), false, 3);
            bench.frame();
            bench.go_to(300);
            bench.frame();
            bench.again(how);
            bench.settle(REST);
            let line = bench.line_start(300);
            assert!(
                bench.row_in_view(line),
                "{how:?}: line 300 is not in the view at {}",
                bench.view.scroll_y
            );
        }
    }

    #[test]
    fn a_drop_of_the_thumb_that_waits_lands_after_a_new_measure() {
        for how in AGAIN {
            for editing in [false, true] {
                let mut bench = Bench::new(numbered(LINES), editing, 3);
                bench.frame();
                bench.thumb(0.5);
                let place = bench.view.pending_offset.expect("the drop waits").offset;
                let row = scroll::source_row(&bench.doc, 0, place);
                bench.frame();
                bench.again(how);
                bench.settle(REST);
                assert_eq!(bench.top_row(), row, "{how:?}, editing {editing}");
            }
        }
    }

    #[test]
    fn the_end_that_waits_while_reading_is_reached_after_a_new_measure() {
        // Where the view ends, and the scroll of the end, for a zoom
        // step then for a new width.
        let ends = AGAIN.map(|how| {
            let mut bench = Bench::new(numbered(LINES), false, 3);
            bench.frame();
            for _ in 0..10 {
                bench.frame();
            }
            bench.go_edge(true);
            bench.frame();
            bench.again(how);
            bench.settle(REST);
            (bench.view.scroll_y, bench.end_scroll())
        });
        for (how, (scroll, end)) in AGAIN.iter().zip(ends) {
            assert_eq!(
                scroll, end,
                "{how:?}: the view is not at the end, of {ends:?}"
            );
        }
    }

    #[test]
    fn a_kept_view_that_waits_comes_back_after_a_second_new_measure() {
        for how in AGAIN {
            // While reading, the line at the top of the view.
            let mut bench = Bench::new(numbered(LINES), false, 3);
            bench.settle(REST);
            bench.wheel(40.0);
            bench.frame();
            let top = bench.top_row();
            assert!(top > 50, "the wheel went down the file, to row {top}");
            bench.zoom(1.5);
            bench.frame();
            bench.again(how);
            bench.settle(REST);
            assert_eq!(bench.top_row(), top, "{how:?}, reading");
            // In the editor, the caret's row.
            let mut bench = editor_at_200();
            bench.zoom(1.5);
            bench.frame();
            bench.again(how);
            bench.settle(REST);
            assert_eq!(bench.caret_row_shown(true), Ok(()), "{how:?}, editing");
        }
    }

    #[test]
    fn a_letter_stays_in_view_after_a_new_measure() {
        for how in AGAIN {
            // Typed in view.
            let mut bench = editor_at_200();
            bench.letter();
            bench.frame();
            bench.again(how);
            bench.settle(REST);
            assert_eq!(bench.caret_row_shown(true), Ok(()), "{how:?}, in view");
            // Typed with the caret out of view.
            let mut bench = editor_at_200();
            bench.wheel(-40.0);
            bench.frame();
            bench.letter();
            bench.frame();
            bench.again(how);
            bench.settle(REST);
            assert_eq!(bench.caret_row_shown(true), Ok(()), "{how:?}, out of view");
        }
    }

    /// Jumps to the lines of `jumps`, each one landed, then a zoom step
    /// that starts the measure again, a frame, and `steps` steps back.
    /// Answers the bench while the file is still measured.
    fn stepped_back(editing: bool, pace: usize, jumps: &[usize], steps: usize) -> Bench {
        let mut bench = Bench::new(numbered(LINES), editing, pace);
        bench.frame();
        for &line in jumps {
            bench.go_to(line);
            bench.settle(REST);
        }
        bench.zoom(1.25);
        bench.frame();
        for _ in 0..steps {
            bench.back();
        }
        bench
    }

    #[test]
    fn a_step_back_during_the_measure_ends_where_it_ends_in_a_measured_file() {
        for editing in [false, true] {
            for (jumps, steps) in [(&[300][..], 1), (&[100, 200, 300][..], 2)] {
                let ends = |pace: usize| {
                    let mut bench = stepped_back(editing, pace, jumps, steps);
                    bench.settle(REST);
                    (bench.view.scroll_y, bench.caret.map(|caret| caret.offset))
                };
                assert_eq!(
                    ends(3),
                    ends(usize::MAX),
                    "editing {editing}, jumps {jumps:?}, {steps} steps back"
                );
            }
        }
    }

    #[test]
    fn a_step_back_during_the_measure_waits_for_its_end() {
        let mut bench = stepped_back(true, 3, &[300], 1);
        let at_300 = bench.line_start(300);
        for _ in 0..5 {
            bench.frame();
            assert!(bench.pass_active(), "the file is still measured");
            assert_eq!(
                bench.caret.map(|caret| caret.offset),
                Some(at_300),
                "the step was made before the measure ended"
            );
        }
        bench.settle(REST);
        assert_eq!(bench.caret.map(|caret| caret.offset), Some(0));
        assert_eq!(bench.caret_row_shown(true), Ok(()));
    }

    #[test]
    fn the_wheel_drops_a_step_back_that_waits() {
        let mut bench = stepped_back(true, 3, &[300], 1);
        for _ in 0..10 {
            bench.frame();
        }
        bench.wheel(2.0);
        bench.frame();
        let left = bench.view.scroll_y;
        bench.settle(REST);
        assert_eq!(
            bench.view.scroll_y, left,
            "the step back moved the view after the wheel"
        );
        let at_300 = bench.line_start(300);
        assert_eq!(bench.caret.map(|caret| caret.offset), Some(at_300));
    }

    #[test]
    fn a_letter_drops_a_step_back_that_waits() {
        let mut bench = stepped_back(true, 3, &[300], 1);
        bench.frame();
        bench.letter();
        bench.settle(REST);
        let after = bench.line_start(300) + 1;
        assert_eq!(bench.caret.map(|caret| caret.offset), Some(after));
        assert_eq!(bench.caret_row_shown(true), Ok(()));
    }

    #[test]
    fn the_editor_opened_or_left_keeps_the_steps_that_wait() {
        let mut view = View {
            steps_held: -2,
            kept_view: Some(Kept::Distance(40.0)),
            ..View::default()
        };
        view.drop_waits_keep_steps();
        assert_eq!((view.steps_held, view.kept_view), (-2, None));
        view.drop_waits();
        assert_eq!(view.steps_held, 0);
    }

    /// An action of the reader, between two frames.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Act {
        /// Frames with no action: the measure goes on.
        Frames(usize),
        /// The wheel, in notches, down when positive.
        Wheel(i8),
        /// Go to line.
        GoTo(usize),
        /// `Ctrl+End`.
        End,
        /// `Ctrl+Home`.
        Home,
        /// A caret key in the editor: Left, Right, Up, Down, Page Up,
        /// Page Down. While reading, the page keys scroll.
        Key(u8),
        /// A letter typed at the caret.
        Letter,
        /// A click in the text.
        Click(u16, u16),
        /// A zoom step, in or back out.
        Zoom(bool),
        /// A new width of the window, narrow or back.
        Width(bool),
        /// A drop of the scrollbar's thumb, at a percentage of its track.
        Thumb(u8),
        /// `Alt+Left`.
        Back,
    }

    impl Act {
        /// The action names its place by itself, whatever the view
        /// shows: where it ends does not depend on how far the measure
        /// is.
        fn names_its_place(self) -> bool {
            matches!(
                self,
                Act::Frames(_)
                    | Act::GoTo(_)
                    | Act::End
                    | Act::Home
                    | Act::Letter
                    | Act::Zoom(_)
                    | Act::Width(_)
            )
        }

        /// The reader's hand on the view: after it, the view is where
        /// the hand left it.
        fn is_hand(self, editing: bool) -> bool {
            match self {
                Act::Wheel(notches) => notches != 0,
                Act::Key(key) => !editing && key >= 4,
                _ => false,
            }
        }
    }

    impl Bench {
        /// Does the action. True when it put the caret somewhere in
        /// the editor.
        fn act(&mut self, act: Act) -> bool {
            const KEYS: [Motion; 6] = [
                Motion::Left,
                Motion::Right,
                Motion::Up,
                Motion::Down,
                Motion::PageUp,
                Motion::PageDown,
            ];
            match act {
                Act::Frames(count) => {
                    for _ in 0..count {
                        self.frame();
                    }
                    false
                }
                Act::Wheel(notches) => {
                    self.wheel(f32::from(notches));
                    false
                }
                Act::GoTo(line) => {
                    self.go_to(line);
                    self.editing
                }
                Act::End if self.editing => self.key(Motion::DocEnd),
                Act::End => {
                    self.go_edge(true);
                    false
                }
                Act::Home if self.editing => self.key(Motion::DocStart),
                Act::Home => {
                    self.go_edge(false);
                    false
                }
                Act::Key(key) if self.editing => self.key(KEYS[usize::from(key) % 6]),
                Act::Key(key) if key >= 4 => {
                    let step = (self.view_h - self.row_h()).max(self.row_h());
                    let delta = if key == 4 { -step } else { step };
                    let (view, page) = self.page();
                    view.scroll_by(&page, delta);
                    false
                }
                Act::Key(_) => false,
                Act::Letter => self.letter(),
                Act::Click(x, y) if self.editing => self.click(f32::from(x), f32::from(y)),
                Act::Click(..) => false,
                Act::Zoom(up) => {
                    self.zoom(if up { 1.25 } else { 1.0 });
                    false
                }
                Act::Width(narrow) => {
                    self.width = if narrow { 480.0 } else { 600.0 };
                    false
                }
                Act::Thumb(percent) => {
                    self.thumb(f32::from(percent) / 100.0);
                    false
                }
                Act::Back => self.back() && self.editing,
            }
        }

        /// A fault of today's rules that the action is about to meet,
        /// found by this fuzzer and left as it is by Task 408, which
        /// changes no behavior. A sequence that meets one is not
        /// compared with the file measured whole. Each line goes when
        /// its fault is corrected.
        fn gap(&self, act: Act) -> Option<&'static str> {
            let waits = self.view.waits();
            match act {
                // Task 412: a click while the file is measured or the
                // view waits puts the caret on a row the layout holds,
                // not under the pointer, and a landing that waited then
                // takes the view from it.
                Act::Click(..) if self.editing && (waits || self.pass_active()) => Some("click"),
                // Not a fault, a limit of the comparison. While
                // reading, a zoom step or a new width keeps the top of
                // the view, and a jump that still waits keeps its line
                // at the height asked, or the end of the file: the two views are a row or so
                // apart, by how far the measure was.
                // A zoom step or a new width with the caret on a lower
                // row of a wrapped line: where the layout does not hold
                // the row when the kept view comes back, the line's
                // first row takes the caret's height, and the caret's
                // row can end under the view.
                Act::Zoom(_) | Act::Width(_) if self.editing && self.caret_line_wraps() => {
                    Some("kept caret on a wrapped line")
                }
                Act::Zoom(_) | Act::Width(_)
                    if !self.editing
                        && (self.view.landing_waits()
                            || self.view.bottom_hold.held_at(self.view.scroll_y)) =>
                {
                    Some("top or line")
                }
                _ => None,
            }
        }

        /// Whether the caret is on one of the long lines of `numbered`.
        fn caret_line_wraps(&self) -> bool {
            let Some(caret) = self.caret else {
                return false;
            };
            let at = caret.offset.min(self.text.len());
            let start = self.text[..at].rfind('\n').map_or(0, |found| found + 1);
            let end = self.text[at..]
                .find('\n')
                .map_or(self.text.len(), |found| at + found);
            end - start > 40
        }

        /// Whether the caret's row is in the view: whole, or only cut
        /// by an edge after a click, which moves no view.
        fn caret_row_shown(&mut self, whole: bool) -> Result<(), String> {
            let (view_h, scroll) = (self.view_h, self.view.scroll_y);
            let lay = self.lay.as_ref().expect("a layout");
            let found = self
                .caret
                .and_then(|caret| caret.geometry(lay, &self.doc, &mut self.fonts));
            let shown = found.is_some_and(|b| match whole {
                true => b.y >= scroll && b.y + b.h <= scroll + view_h,
                false => b.y + b.h > scroll && b.y < scroll + view_h,
            });
            if shown {
                return Ok(());
            }
            Err(format!(
                "the caret's row {:?} is not in the view at {scroll}",
                found.map(|b| (b.y, b.h))
            ))
        }

        /// The scroll is within the page and the caret is in the text,
        /// on a character.
        fn bounds(&self) -> Result<(), String> {
            let height = self.lay.as_ref().map_or(0.0, |lay| lay.height);
            let most = (height - self.view_h).max(0.0);
            let scroll = self.view.scroll_y;
            if !(0.0..=most).contains(&scroll) {
                return Err(format!("the scroll {scroll} is out of 0..={most}"));
            }
            match self.caret {
                Some(caret) if !self.text.is_char_boundary(caret.offset) => {
                    Err(format!("the caret {} is not on a character", caret.offset))
                }
                _ => Ok(()),
            }
        }
    }

    /// Where a sequence ends: the scroll, the caret, and the first
    /// known fault it met on its way (`Bench::gap`).
    type Ended = (Option<f32>, Option<usize>, Option<&'static str>);

    /// The lines of the file the fuzzer opens.
    const LINES: usize = 400;

    /// How many frames after the end of the measure the view is given
    /// to come to rest.
    const REST: usize = 4;

    fn acts() -> impl Strategy<Value = Vec<Act>> {
        let act = prop_oneof![
            4 => (1usize..6).prop_map(Act::Frames),
            3 => (-12i8..=12).prop_map(Act::Wheel),
            3 => (1usize..=LINES).prop_map(Act::GoTo),
            2 => Just(Act::End),
            1 => Just(Act::Home),
            3 => (0u8..6).prop_map(Act::Key),
            3 => Just(Act::Letter),
            2 => (0u16..560, 0u16..200).prop_map(|(x, y)| Act::Click(x, y)),
            2 => any::<bool>().prop_map(Act::Zoom),
            1 => any::<bool>().prop_map(Act::Width),
            2 => (0u8..=100).prop_map(Act::Thumb),
            2 => Just(Act::Back),
        ];
        prop::collection::vec(act, 1..14)
    }

    /// Runs a sequence of actions, a frame after each, then the measure
    /// to its end, and checks the properties of the waiting view.
    fn check(acts: &[Act], editing: bool, pace: usize) -> Result<Ended, String> {
        let mut bench = Bench::new(numbered(LINES), editing, pace);
        bench.frame();
        // The scroll the reader's hand left, when the hand was the last
        // to act: whatever waited before it, the view ends there.
        let mut hand = None;
        let mut gap = None;
        // In the editor, the last action put the caret somewhere and
        // the hand has not moved the view since: the caret's row is
        // owed to the view.
        let mut caret_shown = None;
        for (index, &act) in acts.iter().enumerate() {
            gap = gap.or(bench.gap(act));
            let put = bench.act(act);
            bench.frame();
            bench
                .bounds()
                .map_err(|what| format!("after act {index}, {act:?}: {what}"))?;
            match act {
                Act::Frames(_) | Act::Zoom(_) | Act::Width(_) => {}
                // A step back brings the view that was left, with the
                // caret out of it when the reader had scrolled away.
                Act::Wheel(_) | Act::Thumb(_) | Act::Back => caret_shown = None,
                // A click moves no view: a row the edge cuts stays cut.
                Act::Click(..) if put => caret_shown = Some(false),
                _ if put => caret_shown = Some(true),
                _ => {}
            }
            match act {
                Act::Frames(_) => {}
                _ if act.is_hand(editing) => hand = Some(bench.view.scroll_y),
                _ => hand = None,
            }
        }
        bench.settle(REST);
        bench.bounds().map_err(|what| format!("at rest: {what}"))?;
        let view = &bench.view;
        if view.waits() {
            return Err(format!(
                "at rest something waits: kept {:?}, row {:?}, offset {:?}, anchor {:?}",
                view.kept_view, view.pending_row, view.pending_offset, view.pending_anchor
            ));
        }
        if view.thumb_landing.is_some() || view.row_parked.is_some() {
            return Err(format!(
                "at rest a mark of a wait is left: thumb {:?}, parked {:?}",
                view.thumb_landing, view.row_parked
            ));
        }
        if view.bottom_hold.held_at(view.scroll_y) {
            return Err("at rest a jump to the end is still held".to_string());
        }
        if view.caret_snap {
            return Err("at rest the caret is still owed its view".to_string());
        }
        if view.steps_held != 0 {
            return Err("at rest a step of the history still waits".to_string());
        }
        let rest = bench.snapshot();
        bench.frame();
        let after = bench.snapshot();
        if rest != after {
            return Err(format!(
                "a frame with no action moved the view at rest:\n{rest:?}\n{after:?}"
            ));
        }
        if let Some(left) = hand {
            if bench.view.scroll_y != left {
                return Err(format!(
                    "the hand left the view at {left}, and it ended at {}",
                    bench.view.scroll_y
                ));
            }
        }
        if let (Some(whole), None) = (caret_shown, gap) {
            bench
                .caret_row_shown(whole)
                .map_err(|what| format!("at rest {what}"))?;
        }
        // In the editor the view follows the caret from where it was,
        // so only the caret is the same whatever the measure. While
        // reading, the scroll is.
        let scroll = (!editing).then_some(bench.view.scroll_y);
        Ok((scroll, bench.caret.map(|caret| caret.offset), gap))
    }

    /// The rules of the waiting view against a file measured a few
    /// lines a frame: random actions of the reader, in the editor and
    /// while reading, then the measure to its end.
    #[test]
    #[ignore]
    fn fuzz_view_wait() {
        let config = ProptestConfig {
            failure_persistence: None,
            // Each case lays a file out several times: fewer cases than
            // the other fuzzers, unless `PROPTEST_CASES` asks for more.
            cases: std::env::var("PROPTEST_CASES")
                .ok()
                .and_then(|cases| cases.parse().ok())
                .unwrap_or(64),
            ..ProptestConfig::default()
        };
        proptest!(config, |(
            acts in acts(),
            editing in any::<bool>(),
            pace in prop::sample::select(vec![3usize, 9, 31, 400]),
        )| {
            let ended = check(&acts, editing, pace);
            prop_assert!(
                ended.is_ok(),
                "{}\nediting {editing}, pace {pace}, acts {acts:?}",
                ended.unwrap_err()
            );
            // The wait changes nothing: actions that name their place
            // end where they end in a file measured whole at each
            // frame. While reading, the last movement decides alone
            // when it names its place, whatever came before it: the
            // wheel, the thumb or a step back, which depend on the
            // view they found.
            let known = matches!(ended, Ok((_, _, Some(_))));
            let last = acts.iter().rev().find(|act| !matches!(act, Act::Frames(_)));
            let decides = !editing && matches!(last, Some(Act::GoTo(_) | Act::End | Act::Home));
            if !known && (decides || acts.iter().all(|act| act.names_its_place())) {
                let whole = check(&acts, editing, usize::MAX);
                prop_assert_eq!(
                    &ended,
                    &whole,
                    "the measure changed where the view ends\nediting {}, pace {}, acts {:?}",
                    editing,
                    pace,
                    acts
                );
            }
        });
    }
}
