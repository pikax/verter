//! The generated-order walk over a fragment's pieces. The writer and every
//! map product read pieces through it; there is no other.

use super::fragment::EmissionFragment;
use super::piece::Piece;

/// One visited piece and where it lands in the output.
pub(crate) struct Visit<'v, 's> {
    pub(crate) fragment: &'v EmissionFragment<'s>,
    pub(crate) piece: &'v Piece<'s>,
    pub(crate) text: &'v str,
    /// Output offset of the piece's first byte.
    pub(crate) at: u32,
}

pub(crate) trait PieceVisitor<'s> {
    fn visit(&mut self, visit: Visit<'_, 's>);
}

/// A declared slot of the walked fragment, filled with another fragment:
/// the marked piece is replaced by `with`'s pieces.
pub(crate) struct Fill<'f, 's> {
    pub(crate) piece: u32,
    pub(crate) with: &'f EmissionFragment<'s>,
}

/// Visit `fragment`'s pieces in generated order starting at output offset
/// `at`, with `fills` (ordered by piece) written in place of their marked
/// pieces. Returns the output offset after the fragment.
pub(crate) fn walk<'s>(
    fragment: &EmissionFragment<'s>,
    fills: &[Fill<'_, 's>],
    at: u32,
    visitor: &mut impl PieceVisitor<'s>,
) -> u32 {
    let mut fills = fills.iter().peekable();
    let mut at = at;
    for (index, piece) in fragment.pieces().iter().enumerate() {
        if let Some(fill) = fills.next_if(|fill| fill.piece == index as u32) {
            at = walk(fill.with, &[], at, visitor);
        } else {
            visitor.visit(Visit {
                fragment,
                piece,
                text: fragment.piece_text(piece),
                at,
            });
            at += piece.len();
        }
    }
    at
}
