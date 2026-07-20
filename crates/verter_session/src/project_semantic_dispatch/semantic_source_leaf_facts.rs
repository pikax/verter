//! Closed LEAF-fact projection and lowering for the semantic-source bridge.
//!
//! The [`LeafTypeFact`] half of [`super::semantic_source`]: projecting a
//! RESOLVED semantic-graph node into its complete closed leaf / leaf-union
//! fact (the ONE shared node→closed-fact projection every publication
//! surface reads), building the closed payload-tuple SOURCE from realized
//! call parameters, and lowering a closed leaf fact (or a shallow
//! named-symbol reference) back into a transient graph handle through the
//! shared in-scope lowerer. Pure node↔fact bridging — no reduction, no
//! dispatch execution beyond the shared lowerer entries.

use std::sync::Arc;

use verter_type_expr::facts::{
    ClosedTypeFact, FactOrLocator, LeafTypeFact, SemanticTypeSource, TuplePayloadFact,
};
use verter_type_expr::locators::SymbolBodyLocator;

use super::semantic_source::SourceRaiseContext;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    HotTypeRef, ProjectionReductionContext, SemanticNodeData, SemanticNodeId,
};

impl ProjectSemanticDispatch<'_> {
    /// Project a RESOLVED node into its complete closed LEAF fact when it is
    /// one (a primitive / string / number / boolean literal). `None` for any
    /// richer shape — the caller publishes its content-free source instead;
    /// a leaf is the only node class whose fact is complete by itself.
    pub(crate) fn node_leaf_fact(&self, node: SemanticNodeId) -> Option<LeafTypeFact> {
        match super::node_data_for(self.ctx, node).as_deref() {
            Some(SemanticNodeData::Primitive(kind)) => Some(LeafTypeFact::Primitive(
                super::raise::semantic_primitive_to_primitive_name(*kind),
            )),
            Some(SemanticNodeData::Literal(value)) => match value {
                verter_type_expr::LiteralValue::String(text) => {
                    Some(LeafTypeFact::StringLiteral(text.clone()))
                }
                verter_type_expr::LiteralValue::Number(number) => {
                    Some(LeafTypeFact::NumberLiteral(format!("{number}")))
                }
                verter_type_expr::LiteralValue::Boolean(flag) => {
                    Some(LeafTypeFact::BooleanLiteral(*flag))
                }
                // No leaf-fact arm exists for a BigInt literal.
                verter_type_expr::LiteralValue::BigInt(_) => None,
            },
            _ => None,
        }
    }

    /// Project a RESOLVED node into its complete closed LEAF-UNION fact when
    /// it is a `Union` whose every member is itself a complete leaf. `None`
    /// for any other shape (including a union with one non-leaf arm — partial
    /// facts are never published). The ONE shared node→closed-union
    /// projection every publication surface reads — never re-derived
    /// per-surface.
    pub(crate) fn node_leaf_union_fact(&self, node: SemanticNodeId) -> Option<Arc<[LeafTypeFact]>> {
        let members = match super::node_data_for(self.ctx, node).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.clone(),
            _ => return None,
        };
        let leaves: Option<Vec<LeafTypeFact>> = members
            .iter()
            .map(|&member| self.node_leaf_fact(member))
            .collect();
        leaves.map(|leaves| Arc::from(leaves.into_boxed_slice()))
    }

    /// The complete closed element fact for a tuple-element/param position:
    /// the LEAF fact when the node decided one, else the LEAF-UNION fact when
    /// the node is a union of complete leaves, else `None` — a richer shape
    /// has no content-free element fact, so the caller fails its composite
    /// closed instead of publishing a partial fact.
    pub(crate) fn node_leaf_fact_or_union(&self, node: SemanticNodeId) -> Option<FactOrLocator> {
        if let Some(leaf) = self.node_leaf_fact(node) {
            return Some(FactOrLocator::Leaf(leaf));
        }
        self.node_leaf_union_fact(node)
            .map(FactOrLocator::LeafUnion)
    }

    /// Build the closed payload-tuple SOURCE from realized call parameters in
    /// the node domain: each param projects to its complete closed element
    /// fact (a LEAF, or a LEAF-UNION for an instantiated `string | number`
    /// element), preserving label / optionality / rest / ORDER. `None` when
    /// ANY param is richer — a partial closed tuple is never published (the
    /// caller publishes the projected CALLABLE-PARAMS replay route instead,
    /// or its honest typed failure when no replay base exists). Pure
    /// node→fact projection: no reduction, no dispatch execution, no
    /// `TypeExpr` materialisation.
    pub(crate) fn closed_params_tuple_source(
        &self,
        params: &[crate::semantic_query::FunctionParam],
    ) -> Option<SemanticTypeSource> {
        use verter_type_expr::facts::TupleElementFact;
        let elements: Option<Vec<TupleElementFact>> = params
            .iter()
            .map(|param| {
                self.node_leaf_fact_or_union(param.ty)
                    .map(|ty| TupleElementFact {
                        label: param.name.as_ref().map(|name| name.to_string()),
                        optional: param.optional,
                        rest: param.rest,
                        ty,
                    })
            })
            .collect();
        Some(SemanticTypeSource::Closed(ClosedTypeFact::Tuple(
            TuplePayloadFact {
                readonly: false,
                elements: Arc::from(elements?.into_boxed_slice()),
            },
        )))
    }

    /// Lower a closed LEAF fact: the closed-grammar `TypeExpr` projection
    /// lowered through the shared in-scope lowerer (so a bare `Ref` leaf
    /// resolves its reference head under the raise scope's name resolution).
    pub(super) fn raise_leaf_fact(
        &self,
        leaf: &LeafTypeFact,
        ctx: &SourceRaiseContext<'_>,
    ) -> Option<HotTypeRef> {
        let expr = super::lower::leaf_type_fact_expr(leaf);
        self.lower_type_expr_in_owner_scope_with_context(
            ctx.scope_canonical_id,
            ctx.scope_owner,
            &expr,
            ctx.context,
        )
        .map(HotTypeRef::new)
    }

    /// Lower a shallow named-symbol reference (`ResolvedLocalShape::Ref`): the
    /// bare reference lowers in the symbol anchor's exact canonical + lexical
    /// owner so its head resolves against the declaring region's name
    /// resolution, staying a carrier the consuming dispatch resolves on
    /// demand. An EMPTY anchor canonical is the analyzer's producer-local
    /// convention (same as `absolutize_locator` in
    /// [`super::semantic_source`]) — both canonical and owner come from the
    /// caller's exact raise scope.
    pub(super) fn raise_symbol_ref(
        &self,
        symbol: &SymbolBodyLocator,
        ctx: &SourceRaiseContext<'_>,
    ) -> Option<HotTypeRef> {
        let expr = super::lower::leaf_type_fact_expr(&LeafTypeFact::Ref(
            symbol.anchor.symbol.as_ref().to_string(),
        ));
        let (canonical, owner) = if symbol.anchor.canonical_id.is_empty() {
            (ctx.scope_canonical_id, ctx.scope_owner)
        } else {
            (symbol.anchor.canonical_id.as_ref(), symbol.anchor.owner)
        };
        self.lower_type_expr_in_owner_scope_with_context(
            canonical,
            owner,
            &expr,
            ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            ),
        )
        .map(HotTypeRef::new)
    }
}
