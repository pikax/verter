//! ACCIDENTAL-REGRESSION CANARY for the output-materialization carrier
//! escape fence (NOT the complete enforcer).
//!
//! The COMPLETE safe-Rust mechanism that keeps a capability-free `TypeExpr`
//! out of the sealed carriers `OutputTypeExpr` / `MaterializedOutputTypeExpr`
//! is PAYLOAD UNREACHABILITY: the inner `TypeExpr` lives in the deeply-private
//! `carrier::payload` vault in
//! `src/project_semantic_dispatch/output_materialization.rs`, so in safe Rust
//! OUTSIDE that vault there is NO readable `TypeExpr` field to return — a
//! capability-free unwrap is unrepresentable by field access, auto-deref, an
//! arbitrary trait impl (the escape-trait surface is unbounded — `Deref`,
//! `Index`, `AsRef`, … — so a finite trait list could never be complete), or
//! an inherent method. The only production APIs returning `TypeExpr` /
//! `&TypeExpr` are the capability-gated `into_type_expr` / `type_expr`
//! accessors.
//!
//! The `assert_not_impl_any!` assertions below are a `const _` CANARY that
//! catches COMMON ACCIDENTAL `Deref<Target = TypeExpr>` / `AsRef<TypeExpr>` /
//! `Borrow<TypeExpr>` regressions on the re-exported carrier names early and
//! crate-wide; they are NOT, and must not be documented as, the complete
//! enforcer (a hostile arbitrary trait the list does not name would slip past
//! the canary but is still unrepresentable by the payload vault in safe Rust).
//! Completeness comes from the vault, not from this trait enumeration. This
//! canary does not cover guard deletion, deliberate edits inside the trusted
//! vault, or unsafe code unless the crate forbids unsafe globally.
//!
//! Companion structural guards (the bounded residuals the compiler cannot
//! express) live in the integration suite at
//! `tests/cases/output_projector_residual_guards.rs`:
//! `output_projector_owner_registration_inventory` (the sanctioned sink set +
//! the EXACT module-topology confinement of the owner file — banning
//! item/include/attribute macro injection and any module other than the
//! intended inline `projector` / `carrier` / `carrier::payload` /
//! `projector::sealed` shape),
//! `output_carriers_have_no_inherent_typeexpr_escape_method` (a closed
//! item/signature allowlist over the carrier/vault modules — no production
//! method returning `TypeExpr` / `&TypeExpr` without a capability param), and
//! `output_carrier_payload_fields_are_private` (every `TypeExpr`-bearing
//! payload field private regardless of type-name spelling). The out-of-crate
//! visibility boundary is pinned by the trybuild fixture
//! `output_projector_not_impl_outside_crate.rs`.
use static_assertions::assert_not_impl_any;
use verter_type_expr::TypeExpr;

use super::output_materialization::{MaterializedOutputTypeExpr, OutputProjector, OutputTypeExpr};
use crate::semantic_query::SemanticNodeId;

// Carrier trait-escape CANARY (crate-wide, every profile). A
// `Deref<Target = TypeExpr>` makes `*carrier` a bare `&TypeExpr` for any
// holder; an `AsRef<TypeExpr>` / `Borrow<TypeExpr>` is the same escape by a
// different trait. This canary catches the COMMON ACCIDENTAL forms early — but
// the escape-trait surface is UNBOUNDED, so this finite list is not the
// complete enforcer: the complete safe-Rust mechanism is the payload vault (no
// readable `TypeExpr` field outside `carrier::payload`), which makes ALL such
// escapes — named or not — unrepresentable in safe Rust. If one of these named
// impls is accidentally added, the build fails on the offending `const _`
// below, in EVERY profile.
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

// Owner-seal fence (crate-wide, every profile). `OutputProjector` is sealed
// against its private `sealed::Sealed` supertrait, so it can only be
// implemented in the owner module for the sanctioned sink capability types. A
// representative NON-SINK crate type (`SemanticNodeId`, the graph node id this
// reverse boundary materialises FROM) must therefore never be an
// `OutputProjector`. If a future edit makes a non-sink type a sink (a sealed
// impl in the owner + the trait impl), this `const _` fails to compile — the
// in-crate compile-time witness that the sink set stays closed to the owner.
assert_not_impl_any!(SemanticNodeId: OutputProjector);
