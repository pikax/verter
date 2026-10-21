//! The dispatcher's public construction surface.
//!
//! A dispatcher method that builds or resolves graph nodes returns construction
//! ordinals: labels that name nodes but own none. Inside the engine those
//! labels are composed under the operation or builder owner that roots them;
//! outside it, every such result is delivered here, in the [`Constructed`]
//! carrier that owns the construction that produced it. The label-returning
//! methods themselves are crate-private in production builds (test builds keep
//! them as fixture seams), so a consumer outside the engine reaches a
//! construction result only through its owner.

use std::sync::Arc;

use super::{ProjectSemanticDispatch, SemanticNodeId};
use crate::semantic_query::{
    HotTypeRef, ObjectProjectionFormula, ProjectionMode, ProjectionReductionContext, QueryResult,
    SemanticQueryKey, SemanticQueryValue, SurfaceMember,
};
use crate::semantic_query_memo::node_roots::Constructed;

/// A dispatcher's construction results, each in its owning carrier
/// ([`ProjectSemanticDispatch::owning`]).
///
/// The carrier owns everything its result's build interned, rooted or read,
/// never the dispatcher: a discarded carrier releases it while the dispatcher
/// lives, and a kept carrier (or any clone of it) outlives the dispatcher and
/// every builder scope until its final holder drops. A builder owner the
/// caller holds open keeps a share too, as it keeps every construction made
/// under it.
pub struct OwningDispatch<'d, 'a, C: crate::resolver_core::ResolverCapabilities> {
    pub(super) dispatch: &'d ProjectSemanticDispatch<'a, C>,
}

/// Declare carrier-delivering forms of crate-private construction methods:
/// each runs the method as one construction ([`ProjectSemanticDispatch::construct`])
/// and returns its value in the carrier that owns it.
macro_rules! owning_methods {
    ($(
        $(#[$meta:meta])*
        fn $name:ident(&self $(, $arg:ident : $ty:ty)* $(,)?) -> $ret:ty;
    )*) => {
        impl<C: crate::resolver_core::ResolverCapabilities> OwningDispatch<'_, '_, C> {
            $(
                $(#[$meta])*
                pub fn $name(&self $(, $arg: $ty)*) -> Constructed<$ret> {
                    self.dispatch.construct(|| self.dispatch.$name($($arg),*))
                }
            )*
        }
    };
}

owning_methods! {
    /// Intern a typed opaque fallback in its owning carrier.
    fn opaque(&self, error: crate::semantic_query::QueryError) -> SemanticNodeId;

    /// Re-enter the shared dispatch over a hot handle at `mode`; the
    /// resolved subject node, owned by its carrier.
    fn resolve_hot_handle_at_mode(&self, handle: HotTypeRef, mode: ProjectionMode) -> SemanticNodeId;

    /// Re-enter the shared dispatch over a hot handle under the exact
    /// reduction `context`; the resolved subject node, owned by its carrier.
    fn resolve_hot_handle_with_context(
        &self,
        handle: HotTypeRef,
        context: ProjectionReductionContext,
    ) -> SemanticNodeId;

    /// A named declaration's body as a hot handle, executed through the
    /// shared `Instantiate` query and owned by its carrier.
    fn decl_body_hot_ref(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        symbol: &str,
        args: Arc<[SemanticNodeId]>,
        prc: ProjectionReductionContext,
    ) -> Option<HotTypeRef>;

    /// Raise a semantic type source to a graph handle.
    fn raise_semantic_type_source_to_hot(
        &self,
        source: &verter_type_expr::facts::SemanticTypeSource,
        ctx: super::semantic_source::SourceRaiseContext<'_>,
    ) -> super::semantic_source::SourceRaiseOutcome;

    /// Raise a semantic type source to a graph handle, refusing a partial
    /// raise as a typed failure.
    fn raise_semantic_type_source_to_hot_strict(
        &self,
        source: &verter_type_expr::facts::SemanticTypeSource,
        scope_canonical_id: &str,
        scope_owner: verter_type_expr::TopLevelOwnerId,
        context: ProjectionReductionContext,
    ) -> Result<Option<HotTypeRef>, super::semantic_source::StrictSourceRaiseFailure>;

    /// Raise an authored body locator to a graph handle.
    fn raise_authored_locator_to_hot(
        &self,
        locator: &verter_type_expr::locators::AuthoredBodyLocator,
        context: ProjectionReductionContext,
    ) -> super::semantic_source::SourceRaiseOutcome;

    /// Demand `node`'s validated structural form.
    fn demand_validated_structural_node(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<SemanticNodeId>;

    /// Normalize `node` for a structural fact demand.
    fn normalize_node_for_structural_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> super::evaluate::StructuralFactDemandOutcome;

    /// Peel `node`'s uninstantiated carriers for a structural fact demand.
    fn peel_node_for_uninstantiated_carrier_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> super::evaluate::StructuralFactDemandOutcome;

    /// Resolve the subject node a carrier stands for.
    fn resolve_carrier_subject_node(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> SemanticNodeId;

    /// Apply a deferred conditional's pending substitution for one branch.
    fn apply_conditional_branch_pending(
        &self,
        node: SemanticNodeId,
        pending: Option<&crate::semantic_query::ConditionalPendingSubstitution>,
        true_branch: bool,
    ) -> SemanticNodeId;

    /// Bind a callee's receiver.
    fn bind_callee_receiver(&self, callee: SemanticNodeId, receiver: SemanticNodeId) -> SemanticNodeId;

    /// Lower a type expression in a file scope at `mode`.
    fn lower_type_expr_in_scope_with_mode(
        &self,
        scope_canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
        mode: ProjectionMode,
    ) -> Option<SemanticNodeId>;

    /// Lower a type expression in a top-level owner's scope at `mode`.
    fn lower_type_expr_in_owner_scope_with_mode(
        &self,
        scope_canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        expr: &verter_type_expr::TypeExpr,
        mode: ProjectionMode,
    ) -> Option<SemanticNodeId>;

    /// Demand the terminal symbol instantiation `node` names.
    fn demand_terminal_symbol_instantiation(
        &self,
        node: SemanticNodeId,
        expected: &[verter_type_expr::feature_roles::ResolvedSymbolIdentity],
    ) -> super::symbol_identity::TerminalSymbolInstantiationDemandOutcome;

    /// The value of member `name` of `object`.
    fn indexed_member_value(
        &self,
        canonical: &str,
        object: SemanticNodeId,
        name: &Arc<str>,
    ) -> Option<SemanticNodeId>;

    /// The literal provenance of a call argument's `authored` nested
    /// positions, its value reads resolved in `canonical`'s `owner`
    /// scope: the record the argument's call key carries.
    fn resolve_argument_literal_provenance(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        authored: Option<&verter_type_expr::IndexedLiteralProvenance>,
    ) -> Option<Arc<crate::semantic_query::ArgumentLiteralProvenance>>;

    /// Resolve a call through its indexed callee; the call's return type.
    fn execute_indexed_resolve_call(
        &self,
        key: crate::semantic_query::ResolveCallKey,
    ) -> Option<SemanticNodeId>;

    /// Widen a nominal `typeof` carrier.
    fn widened_nominal_typeof(&self, node: SemanticNodeId) -> Option<SemanticNodeId>;

    /// The shared reducer's outcome, under `nullability`, for a written
    /// union that may absorb a literal.
    fn reduced_written_union(
        &self,
        node: SemanticNodeId,
        nullability: verter_session_query::flow::policy::NullabilityPolicy,
    ) -> Option<QueryResult<SemanticNodeId>>;

    /// Widen every owner-local nominal `typeof` in `node`.
    fn widen_owner_local_nominal_typeofs(&self, node: SemanticNodeId, canonical: &str) -> SemanticNodeId;

    /// A function's return node: declared, flow-derived or absent.
    fn execute_function_return_source(
        &self,
        source: &verter_type_expr::facts::FunctionReturnSource,
        scope_canonical_id: &str,
    ) -> super::flow_return::FunctionReturnNode;

    /// Project an object spread program for a consumer.
    fn project_object_spread_for_consumer(
        &self,
        program: SemanticNodeId,
        selector: crate::semantic_query::ObjectProjectionSelector,
        projection_reduction: ProjectionReductionContext,
    ) -> QueryResult<ObjectProjectionFormula>;

    /// The positive members of a macro spread formula.
    fn spread_formula_positive_members_for_macro(
        &self,
        formula: &ObjectProjectionFormula,
    ) -> Result<Vec<SurfaceMember>, crate::semantic_query::PartialReasonSet>;

    /// Execute a flow return, reporting whether a degraded read was observed.
    fn execute_flow_return_observing_degraded_read(
        &self,
        key: crate::semantic_query::FlowReturnKey,
    ) -> (crate::semantic_query::FlowReturnStep, bool);

    /// Execute a fact query.
    fn execute_fact(
        &self,
        key: SemanticQueryKey,
    ) -> Result<
        crate::semantic_query::FactResult<SemanticQueryValue>,
        crate::semantic_query::ExecutionAbort,
    >;
}

/// Operand topology of a construction result. Implementations reuse the
/// query/payload visitors; generic envelopes keep their represented operands.
/// This is private to the engine so a caller cannot omit an operand.
pub(crate) trait ConstructionResult {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId));
}

impl ConstructionResult for SemanticNodeId {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        visit(*self);
    }
}
impl ConstructionResult for HotTypeRef {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        visit(self.node());
    }
}
impl<T: ConstructionResult> ConstructionResult for Option<T> {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        if let Some(value) = self {
            value.operands(visit);
        }
    }
}
impl<T: ConstructionResult, E: ConstructionResult> ConstructionResult for Result<T, E> {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        match self {
            Ok(value) => value.operands(visit),
            Err(error) => error.operands(visit),
        }
    }
}
impl<T: ConstructionResult> ConstructionResult for Vec<T> {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        for value in self {
            value.operands(visit);
        }
    }
}
impl<T: ConstructionResult> ConstructionResult for Arc<[T]> {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        for value in self.iter() {
            value.operands(visit);
        }
    }
}
impl<T: ConstructionResult> ConstructionResult for QueryResult<T> {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        match self {
            Self::Value(value) => value.operands(visit),
            Self::Recursive(node) => visit(*node),
            Self::Error(error) => error.for_each_retained_node(visit),
        }
    }
}
impl<T: ConstructionResult> ConstructionResult for crate::semantic_query::FactResult<T> {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        match self {
            Self::Complete(value) | Self::Approximate { value, .. } => value.operands(visit),
            Self::Unavailable { .. } => {}
        }
    }
}
impl<A: ConstructionResult, B: ConstructionResult> ConstructionResult for (A, B) {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        self.0.operands(visit);
        self.1.operands(visit);
    }
}
impl ConstructionResult for crate::semantic_query::PartialReasonSet {
    fn operands(&self, _: &mut dyn FnMut(SemanticNodeId)) {}
}
impl ConstructionResult for bool {
    fn operands(&self, _: &mut dyn FnMut(SemanticNodeId)) {}
}
impl ConstructionResult for crate::semantic_query::ExecutionAbort {
    fn operands(&self, _: &mut dyn FnMut(SemanticNodeId)) {}
}
impl ConstructionResult for crate::semantic_query::QueryError {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        self.for_each_retained_node(visit);
    }
}
impl ConstructionResult for SemanticQueryValue {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        self.for_each_retained_node(visit);
    }
}
impl ConstructionResult for ObjectProjectionFormula {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        self.for_each_retained_node(visit);
    }
}
impl ConstructionResult for Arc<crate::semantic_query::ArgumentLiteralProvenance> {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        for position in self.positions.iter() {
            for step in position.path.iter() {
                if let crate::semantic_query::ArgumentPathStep::Member(key) = step {
                    visit(*key);
                }
            }
            for literal in position
                .literals
                .iter()
                .flat_map(|literals| literals.iter())
            {
                visit(*literal);
            }
        }
        for read in self.fresh_reads.iter() {
            for step in read.iter() {
                if let crate::semantic_query::ArgumentPathStep::Member(key) = step {
                    visit(*key);
                }
            }
        }
    }
}
impl ConstructionResult for SurfaceMember {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        crate::semantic_query_memo::construction_copy::ConstructionCopy::for_each_operand(
            self, visit,
        );
    }
}
impl ConstructionResult for super::semantic_source::SourceRaiseOutcome {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        match self {
            Self::Raised(hot) => hot.operands(visit),
            Self::Failed(error) => error.for_each_retained_node(visit),
            Self::Absent => {}
        }
    }
}
impl ConstructionResult for super::semantic_source::StrictSourceRaiseFailure {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        match self {
            Self::QueryFailure(error) => error.for_each_retained_node(visit),
            Self::InteriorMiss(_) | Self::UnknownMaterializing(_) => {}
        }
    }
}
impl ConstructionResult for super::evaluate::StructuralFactDemandOutcome {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        match self {
            Self::Complete(node) | Self::Recovered { node, .. } => visit(*node),
            Self::Partial(_) => {}
        }
    }
}
impl ConstructionResult for super::symbol_identity::TerminalSymbolInstantiationDemandOutcome {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        if let Self::Complete(Some(value)) = self {
            value.args.iter().copied().for_each(visit);
        }
    }
}
impl ConstructionResult for super::flow_return::FunctionReturnNode {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        match self {
            Self::Declared(hot) => hot.operands(visit),
            Self::Flow(result) => result.for_each_retained_node(visit),
            Self::Aborted(_) | Self::DeclaredMiss | Self::NoValue(_) | Self::Absent => {}
        }
    }
}
impl ConstructionResult for crate::semantic_query::FlowReturnStep {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        match self {
            Self::Complete(result) => result.for_each_retained_node(visit),
            // Hold owns its key's operands through the key's interned identities.
            Self::Hold(_) | Self::Aborted(_) | Self::NoValue(_) => {}
        }
    }
}

impl<T: ConstructionResult> ConstructionResult
    for crate::semantic_query::surface_resolution::SurfaceResolution<T>
{
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        if let Some(value) = self.represented_value() {
            value.operands(visit);
        }
    }
}
impl ConstructionResult for super::callable_view::PositionalParamNode {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        visit(self.ty);
    }
}
impl ConstructionResult for super::callable_view::SlotCallableNodeParts {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        self.first_param.operands(visit);
        self.return_type.operands(visit);
    }
}
impl ConstructionResult for crate::semantic_query::FunctionParam {
    fn operands(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        visit(self.ty);
    }
}
