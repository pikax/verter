//! The declaration-body structural producer surface.
//!
//! This surface exposes [`emit_decl_body_arm`] — the witness-gated decl-body
//! structural producer entry. Its production caller is the declaration-body
//! producer (`crate::project_semantic_dispatch`), which lowers a
//! declaration's body to the dormant semantic-graph carriers through the ONE
//! shared structural lowerer. Lowering a declaration body and lowering a Vue
//! macro type argument are the SAME structural operation; both reach the raw
//! lowerer ([`super::lower`]) through a witness-gated wrapper, so there is
//! exactly one structural-carrier producer.
//!
//! ## Witness-gated boundary
//!
//! The raw structural lowerer is PRIVATE to [`super::lower`]. This surface
//! mints a [`DeclBodyProducerWitness`] — a zero-data capability proof whose
//! field is private and whose constructor is confined to this surface — and
//! presents it to [`super::lower::emit_decl_body_arm`]. No foreign module can
//! forge the witness, so the decl-body producer cannot be re-opened anywhere
//! but here, and a third structural-carrier producer is unrepresentable by
//! construction.

use verter_type_expr::TypeExpr;

use crate::semantic_query::{HotTypeRef, NodeScopeId};
use crate::semantic_query_memo::SemanticGraphStore;

use super::lower::{self, StructuralLowerContext, StructuralLowerError};

/// Compile-time capability proof that the declaration-body producer is
/// invoking the shared structural lowerer through its sanctioned decl-body
/// entry ([`super::lower::emit_decl_body_arm`]).
///
/// The field is PRIVATE and the constructor is confined to this surface, so
/// no other module — not even a sibling under
/// [`crate::structural_carrier_producer`] — can forge one. The wrapper in
/// [`super::lower`] can NAME the type (it is module-visible) but cannot
/// construct it. A would-be second structural-carrier producer therefore
/// cannot present this witness and cannot reach the lowerer.
pub(in crate::structural_carrier_producer) struct DeclBodyProducerWitness {
    _private: (),
}

impl DeclBodyProducerWitness {
    /// Mint the decl-body capability proof. Private to this surface
    /// (`decl_body_surface`): only [`emit_decl_body_arm`] constructs it.
    fn new() -> Self {
        Self { _private: () }
    }
}

/// Lower a declaration body's owned [`TypeExpr`] into the dormant
/// semantic-graph carriers, rooted at the owner-supplied `scope`, performing
/// no resolution — the witness-gated decl-body structural producer entry.
///
/// Its production caller is the declaration-body producer; this surface mints
/// the [`DeclBodyProducerWitness`] internally and presents it to the
/// witnessed [`super::lower::emit_decl_body_arm`], which calls the private
/// structural lowerer. The emitted root is an unresolved carrier graph
/// (`BareRef` / `ImportType` / operator shells) identical to what the macro
/// surface produces — resolution is a later demand-time concern.
#[allow(dead_code)] // The production caller is the declaration-body producer.
pub(crate) fn emit_decl_body_arm(
    graph: &SemanticGraphStore,
    expr: &TypeExpr,
    scope: NodeScopeId,
    ctx: &StructuralLowerContext<'_>,
) -> Result<HotTypeRef, StructuralLowerError> {
    lower::emit_decl_body_arm(graph, expr, scope, ctx, &DeclBodyProducerWitness::new())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use verter_type_expr::{PrimitiveName, TypeExpr};

    use super::emit_decl_body_arm;
    use crate::semantic_query::{NodeScopeId, SemanticNodeData, SemanticNodeId};
    use crate::semantic_query_memo::SemanticGraphStore;
    use crate::structural_carrier_producer::lower::{BinderScope, StructuralLowerContext};
    use crate::types::HostConfig;
    use crate::VerterHost;

    /// A real declaration-bound file scope — deliberately NOT `Global`/empty,
    /// so the emitted carrier roots at the owner-supplied lexical scope.
    fn fixture_scope() -> NodeScopeId {
        NodeScopeId::File {
            canonical_id: Arc::from("/decl_fixture.ts"),
            whole_hash: [9u8; 16],
            local_scope: None,
        }
    }

    /// Read an interned node's payload.
    fn node(graph: &SemanticGraphStore, id: SemanticNodeId) -> Arc<SemanticNodeData> {
        graph.node_data(id).expect("interned node must exist")
    }

    /// The witness-gated decl-body entry lowers a real `TypeExpr` to a real
    /// `HotTypeRef` carrier through the shared structural lowerer. A bare
    /// `Foo<Bar>` reference stays a `BareRef` carrier (no resolution), proving
    /// the entry produces a genuine structural-graph handle and that the
    /// witness gate compiles end-to-end — NOT a stub that returns a placeholder.
    #[test]
    fn emit_decl_body_arm_lowers_real_type_expr_to_carrier() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        // `Foo<Bar>` — an unresolved generic reference. Structural lowering
        // emits a `BareRef` carrier (never a resolved `DeclRef`).
        let expr = TypeExpr::Ref {
            name: Arc::from("Foo"),
            type_arguments: Arc::from(
                vec![TypeExpr::Ref {
                    name: Arc::from("Bar"),
                    type_arguments: Arc::from(Vec::new().into_boxed_slice()),
                }]
                .into_boxed_slice(),
            ),
        };

        let binders: [BinderScope; 0] = [];
        let ctx = StructuralLowerContext::new(&binders);
        let handle = emit_decl_body_arm(&graph, &expr, fixture_scope(), &ctx)
            .expect("the witness-gated decl-body entry must lower a resolvable shape");

        // The emitted root is a genuine interned carrier — an unresolved
        // `BareRef`, NOT a resolved `DeclRef` and NOT a stub placeholder.
        let root = node(&graph, handle.node());
        match root.as_ref() {
            SemanticNodeData::BareRef(_) => {}
            other => panic!("decl-body root must be an unresolved BareRef carrier, got {other:?}"),
        }

        // A plain primitive lowers to its `Primitive` carrier too — a second
        // discriminating shape so the entry is not pinned to one variant.
        let prim = TypeExpr::Primitive(PrimitiveName::String);
        let prim_handle = emit_decl_body_arm(&graph, &prim, fixture_scope(), &ctx)
            .expect("the witness-gated decl-body entry must lower a primitive");
        assert!(
            matches!(
                node(&graph, prim_handle.node()).as_ref(),
                SemanticNodeData::Primitive(_)
            ),
            "a primitive decl-body lowers to a Primitive carrier"
        );
    }
}
