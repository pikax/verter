//! The structural-carrier producer — the single owner of the query-free
//! [`TypeExpr`](verter_type_expr::TypeExpr) → dormant semantic-graph carrier
//! lowering.
//!
//! Verter has exactly ONE structural-carrier producer: the raw lowerer
//! [`lower::lower_type_expr_structural`] is PRIVATE to its module (no
//! visibility modifier), and every production caller reaches it through a
//! WITNESS-GATED wrapper. Two such surfaces exist:
//!
//! - [`macro_surface`] — the Vue SFC macro hot mirror, the single-entry
//!   producer of a macro type-argument's mode-neutral graph handle. It
//!   reaches the lowerer through [`lower::emit_macro_arg`], presenting the
//!   [`macro_surface::MacroProducerWitness`].
//! - [`decl_body_surface`] — the declaration-body structural producer. It
//!   reaches the lowerer through [`lower::emit_decl_body_arm`], presenting the
//!   [`decl_body_surface::DeclBodyProducerWitness`].
//!
//! Both witnesses carry a private field and a constructor confined to their
//! owning surface, so no foreign module — and no sibling under this owner
//! module — can forge one. A THIRD structural-carrier producer would have to
//! either name the private raw lowerer (a compile error: it has no visibility
//! modifier) or forge a witness (a compile error: the constructors are
//! confined). The single-engine producer rule is therefore enforced by the
//! type system, not a source scanner.
//!
//! The owner module's boundary stays NARROW: it owns only the raw lowerer
//! ([`lower`]), the two witness-gated producer surfaces ([`macro_surface`],
//! [`decl_body_surface`]), and their tests — no third producer surface may be
//! added here.

pub(in crate::structural_carrier_producer) mod lower;

pub(crate) mod decl_body_surface;
pub(crate) mod macro_surface;

// External consumers reach the macro hot mirror through these re-exports; the
// raw structural lowerer stays UNREACHABLE (it is private to `lower`, and
// `lower` itself is confined to this owner module).
pub(crate) use macro_surface::{macro_type_arg_hot_ref, MacroHotMirror};
