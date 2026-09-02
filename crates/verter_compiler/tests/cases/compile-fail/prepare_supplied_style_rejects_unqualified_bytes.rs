//! Caller-preprocessed style bytes cannot enter the compiler as a bare string.
//!
//! The admission point runs a plain-CSS grammar over its input, so bytes that
//! have not left their authored dialect behind produce a "verified plain CSS"
//! artifact for source that is nothing of the kind.
//!
//! What the signature enforces is exactly this: bytes cannot arrive without a
//! stated byte space and a stated producer — `&str` is neither. It does not
//! prove the assertion true. Plain CSS is a subset of every dialect this
//! compiler parses, so no grammar check separates CSS from SCSS that happens
//! to be CSS-shaped; the assertion belongs to the admitting boundary, and
//! requiring the tool's identity alongside it is what keeps it attached to a
//! party that actually ran or accepted that tool.

use verter_compiler::style_planner::prepare_supplied_style;

fn main() {
    let _ = prepare_supplied_style("$brand: red;\n.a { color: $brand; }");
}
