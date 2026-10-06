//! ACCIDENTAL-REGRESSION CANARY for the output-materialization fence (NOT the
//! complete enforcer).
//!
//! The COMPLETE safe-Rust mechanism that keeps a `TypeExpr` out of the sealed
//! carriers `OutputTypeExpr` / `MaterializedOutputTypeExpr` without the
//! engine's output authority is PAYLOAD UNREACHABILITY: the inner `TypeExpr`
//! lives in the deeply-private `carrier::payload` vault in
//! `src/project_semantic_dispatch/output_materialization.rs`, so in safe Rust
//! OUTSIDE that vault there is NO readable `TypeExpr` field to return. The
//! only production APIs returning `TypeExpr` / `&TypeExpr` take
//! `&OutputAuthority`.
//!
//! The `assert_not_impl_any!` assertions below are a `const _` CANARY that
//! catches COMMON ACCIDENTAL `Deref<Target = TypeExpr>` / `AsRef<TypeExpr>` /
//! `Borrow<TypeExpr>` regressions on the carrier names, and any accidental
//! duplication path for the authority itself. They are NOT the complete
//! enforcer (the escape-trait surface is unbounded); completeness comes from
//! the vault and the authority's private construction. This canary does not
//! cover guard deletion, deliberate edits inside the trusted vault, or unsafe
//! code unless the crate forbids unsafe globally.
//!
//! The authority's acquisition and reconstruction boundaries are pinned by the
//! trybuild fixtures `output_authority_not_forgeable`, `output_authority_not_duplicable`,
//! `output_authority_not_recoverable_from_query_access` and
//! `output_authority_not_reminted_from_live_stores`.
use static_assertions::assert_not_impl_any;
use verter_type_expr::TypeExpr;

use super::engine_resources::OutputAuthority;
use super::output_materialization::{MaterializedOutputTypeExpr, OutputTypeExpr};

// Carrier trait-escape CANARY (crate-wide, every profile). A
// `Deref<Target = TypeExpr>` makes `*carrier` a bare `&TypeExpr` for any
// holder; an `AsRef<TypeExpr>` / `Borrow<TypeExpr>` is the same escape by a
// different trait. If one of these named impls is accidentally added, the
// build fails on the offending `const _` below, in EVERY profile.
assert_not_impl_any!(
    OutputTypeExpr:
        std::ops::Deref<Target = TypeExpr>,
        std::convert::AsRef<TypeExpr>,
        std::borrow::Borrow<TypeExpr>
);
assert_not_impl_any!(
    MaterializedOutputTypeExpr:
        std::ops::Deref<Target = TypeExpr>,
        std::convert::AsRef<TypeExpr>,
        std::borrow::Borrow<TypeExpr>
);

// Authority duplication CANARY (crate-wide, every profile). The authority is
// minted once per engine; a `Clone` / `Copy` impl would let any borrower mint
// an owned duplicate, and a `Default` impl would be an unbound constructor.
assert_not_impl_any!(OutputAuthority: Clone, Copy, Default);
