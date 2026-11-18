//! The Vue semantic snapshot: every framework fact the compiler reads about
//! one component, produced once and only for the demanded families.

mod binding;
mod origin;
mod snapshot;

pub(crate) use binding::resolve_simple_expr_with;
pub use binding::{
    is_simple_ident, BindingAccess, VueBindingFact, VueBindingTable, VueBindingView, UNREF_PREFIX,
};
pub use origin::{
    build_origin_table, FrameworkApi, FrameworkOrigin, FrameworkOriginTable, OriginEvidence,
};
pub(crate) use snapshot::build_semantic_snapshot_with_expressions;
pub use snapshot::{
    build_semantic_snapshot, VueComponentBindingMatch, VueComponentClass, VueComponentTagFacts,
    VueSemanticInputs, VueSemanticRefusal, VueSemanticSnapshot, NO_COMPONENT_TAG,
};
