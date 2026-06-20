//! The structural-carrier producer — the single owner of the query-free
//! [`TypeExpr`](verter_type_expr::TypeExpr) → dormant semantic-graph carrier
//! lowering.
//!
//! Verter has exactly ONE structural-carrier producer: the raw lowerer
//! [`lower::lower_type_expr_structural`] is PRIVATE to its module (no
//! visibility modifier), and every production caller reaches it through a
//! WITNESS-GATED wrapper. One such surface exists today:
//!
//! - [`macro_surface`] — the Vue SFC macro hot mirror, the single-entry
//!   producer of a macro type-argument's mode-neutral graph handle. It
//!   reaches the lowerer through [`lower::emit_macro_arg`], presenting the
//!   [`macro_surface::MacroProducerWitness`].
//!
//! The witness carries a private field and a constructor confined to its
//! owning surface, so no foreign module — and no sibling under this owner
//! module — can forge one. A SECOND structural-carrier producer would have to
//! either name the private raw lowerer (a compile error: it has no visibility
//! modifier) or forge a witness (a compile error: the constructor is
//! confined). The single-engine producer rule is therefore enforced by the
//! type system, not a source scanner.
//!
//! The owner module's boundary stays NARROW: it owns only the raw lowerer
//! ([`lower`]), the witness-gated producer surface ([`macro_surface`]), and
//! their tests — no second production producer surface may be added here.

pub(in crate::structural_carrier_producer) mod lower;

pub(crate) mod macro_surface;

// External consumers reach the macro hot mirror through these re-exports; the
// raw structural lowerer stays UNREACHABLE (it is private to `lower`, and
// `lower` itself is confined to this owner module).
pub(crate) use macro_surface::{macro_type_arg_hot_ref, MacroHotMirror};
