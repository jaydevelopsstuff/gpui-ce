//! Masked text, as a password field shows it: each character of the stored
//! text laid out as a bullet. The layout's offsets are the bullets', so
//! [`DocumentView`] maps them to and from the storage's wherever the state
//! asks the layout something.

use gpui::{
    Bounds, CaretMovement, CaretPosition, CaretSelection, CaretSelectionMovement, Pixels, Point,
    ShapedText, SharedString, TextMovement, TextSelectionKind,
};
use smallvec::SmallVec;
use std::{ops::Range, sync::Arc};

/// What a masked character is drawn as: `•`, as AppKit's secure fields and
/// WebKit's password inputs draw them.
pub(super) const MASK: char = '\u{2022}';

/// The text a masked field lays out for `text`.
pub(super) fn masked_text(text: &str) -> SharedString {
    SharedString::from(MASK.to_string().repeat(text.chars().count()))
}

/// The laid-out document, with offsets in the storage's terms.
#[derive(Clone, Copy)]
pub(super) struct DocumentView<'a> {
    document: &'a Arc<ShapedText>,
    /// The stored text, when the layout is its mask
    masked: Option<&'a str>,
}

impl<'a> DocumentView<'a> {
    pub(super) fn new(document: &'a Arc<ShapedText>, masked: Option<&'a str>) -> Self {
        Self { document, masked }
    }

    /// The layout itself, in its own offsets, for painting.
    pub(super) fn shaped(&self) -> &'a Arc<ShapedText> {
        self.document
    }

    /// The stored text's length, in UTF-8 bytes.
    pub(super) fn len(&self) -> usize {
        self.masked.map_or(self.document.text.len(), str::len)
    }

    /// The nearest character boundary of the stored text at or before
    /// `index`, within the laid-out text.
    pub(super) fn floor_char_boundary(&self, index: usize) -> usize {
        match self.masked {
            Some(text) => text.floor_char_boundary(index.min(text.len())),
            None => self.document.text.floor_char_boundary(index),
        }
    }

    pub(super) fn line_count(&self) -> usize {
        self.document.line_count()
    }

    /// A storage offset as the layout's.
    fn to_layout(&self, index: usize) -> usize {
        match self.masked {
            Some(text) => {
                let index = text.floor_char_boundary(index.min(text.len()));
                text[..index].chars().count() * MASK.len_utf8()
            }
            None => index,
        }
    }

    /// A layout offset as the storage's.
    fn to_storage(&self, index: usize) -> usize {
        match self.masked {
            Some(text) => text
                .char_indices()
                .nth(index / MASK.len_utf8())
                .map_or(text.len(), |(offset, _)| offset),
            None => index,
        }
    }

    fn caret_to_layout(&self, caret: CaretPosition) -> CaretPosition {
        CaretPosition {
            index: self.to_layout(caret.index),
            ..caret
        }
    }

    fn caret_to_storage(&self, caret: CaretPosition) -> CaretPosition {
        CaretPosition {
            index: self.to_storage(caret.index),
            ..caret
        }
    }

    fn selection_to_storage(&self, selection: CaretSelection) -> CaretSelection {
        CaretSelection {
            anchor: self.caret_to_storage(selection.anchor),
            caret: self.caret_to_storage(selection.caret),
        }
    }

    fn range_to_layout(&self, range: Range<usize>) -> Range<usize> {
        self.to_layout(range.start)..self.to_layout(range.end)
    }

    fn range_to_storage(&self, range: Range<usize>) -> Range<usize> {
        self.to_storage(range.start)..self.to_storage(range.end)
    }

    pub(super) fn visual_position_for_caret(
        &self,
        caret: CaretPosition,
        line_height: Pixels,
    ) -> Option<Point<Pixels>> {
        self.document
            .visual_position_for_caret(self.caret_to_layout(caret), line_height)
    }

    pub(super) fn logical_cluster_before(&self, caret: CaretPosition) -> Option<Range<usize>> {
        self.document
            .logical_cluster_before(self.caret_to_layout(caret))
            .map(|range| self.range_to_storage(range))
    }

    pub(super) fn logical_cluster_after(&self, caret: CaretPosition) -> Option<Range<usize>> {
        self.document
            .logical_cluster_after(self.caret_to_layout(caret))
            .map(|range| self.range_to_storage(range))
    }

    pub(super) fn caret_movement(
        &self,
        caret: CaretPosition,
        movement: TextMovement,
        vertical_navigation_x: Option<Pixels>,
    ) -> CaretMovement {
        let moved = self.document.caret_movement(
            self.caret_to_layout(caret),
            movement,
            vertical_navigation_x,
        );
        CaretMovement {
            result: self.caret_to_storage(moved.result),
            vertical_navigation_x: moved.vertical_navigation_x,
        }
    }

    pub(super) fn selection_movement(
        &self,
        selection: CaretSelection,
        movement: TextMovement,
        extend: bool,
        vertical_navigation_x: Option<Pixels>,
        line_height: Pixels,
    ) -> CaretSelectionMovement {
        let selection = CaretSelection {
            anchor: self.caret_to_layout(selection.anchor),
            caret: self.caret_to_layout(selection.caret),
        };
        let moved = self.document.selection_movement(
            selection,
            movement,
            extend,
            vertical_navigation_x,
            line_height,
        );
        CaretSelectionMovement {
            result: self.selection_to_storage(moved.result),
            vertical_navigation_x: moved.vertical_navigation_x,
        }
    }

    pub(super) fn closest_caret_for_pixel_point(
        &self,
        point: Point<Pixels>,
        line_height: Pixels,
    ) -> Result<CaretPosition, CaretPosition> {
        self.document
            .closest_caret_for_pixel_point(point, line_height)
            .map(|caret| self.caret_to_storage(caret))
            .map_err(|caret| self.caret_to_storage(caret))
    }

    pub(super) fn selection_from_pixel_point(
        &self,
        point: Point<Pixels>,
        line_height: Pixels,
        kind: TextSelectionKind,
    ) -> Range<usize> {
        self.range_to_storage(
            self.document
                .selection_from_pixel_point(point, line_height, kind),
        )
    }

    pub(super) fn selection_bounds(
        &self,
        range: Range<usize>,
        line_height: Pixels,
    ) -> SmallVec<[Bounds<Pixels>; 4]> {
        self.document
            .selection_bounds(self.range_to_layout(range), line_height)
    }
}
