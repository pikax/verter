//! The structural-carrier producer — the single owner of the query-free
//! [`TypeExpr`](verter_type_expr::TypeExpr) → dormant semantic-graph carrier
//! lowering.
//!
//! Verter has exactly ONE structural-carrier producer, and it is confined to
//! a SINGLE implementation module: [`macro_arg_producer`]. That module owns
//! everything producer-capable — the raw lowerer `lower_type_expr_structural`,
//! the `<script setup generic="…">` binder-seed builder
//! `build_script_setup_seed_frames`, and the macro hot-mirror builder
//! `build_macro_hot_ref` — ALL of which are PRIVATE to the module (no
//! visibility modifier). The ONLY crate-visible items it exposes are
//! [`macro_type_arg_hot_ref`] (the sole production entry that lowers a macro
//! type-argument) and the [`MacroHotMirror`] artifact child it populates.
//!
//! The single-producer guarantee has TWO regimes. The FOREIGN case is
//! COMPILER-CONFINED: no foreign module can NAME the private lowerer, the
//! private binder-seed builder, or the private mirror builder (an E0603 /
//! E0433 compile error), so a second producer in a foreign module is
//! unrepresentable by construction. The SAME-MODULE case is NOT
//! compiler-confined — Rust privacy is module-scoped, so a SECOND producer
//! written INSIDE [`macro_arg_producer`] CAN name the module-private builders;
//! that residual is POLICED by the strengthened single-producer architecture
//! guards (cfg-satisfiability + producer-exposure over fn/value/trait entries +
//! no-codegen-surface + no-query/dispatch + scoped derive-shadow), NOT by
//! construction. The irreducible residual is trust in the one sanctioned
//! producer implementation plus compiler bugs / build substitution /
//! out-of-tree proc-macros.
//!
//! The owner module's boundary stays NARROW: it contains only
//! [`macro_arg_producer`], this `mod.rs`, and the producer's test modules — no
//! second production producer surface may be added here.

mod macro_arg_producer;

// External consumers reach the macro hot mirror through these re-exports; the
// raw structural lowerer, the binder-seed builder, and the mirror builder stay
// UNREACHABLE (they are private to `macro_arg_producer`, the single producer
// module).
pub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};
