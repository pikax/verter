//! Sole forcing authority for closed semantic operands.
//!
//! Every route from a sealed operand to a semantic node runs through
//! [`ProjectSemanticDispatch::force_semantic_operand`], which combines the
//! operand's sealed identity with the force request's complete projection
//! context exactly once and then dispatches ordinary semantic queries. The
//! module owns the seal (`mint_*`), the seal's admission checks, and that
//! one boundary; it is not a second resolver and never interprets typed IR
//! itself.

// A plain library build sees this boundary and its sealing helpers as
// unreachable: the forcing boundary and its co-located tests are the only
// callers. Scoped to `not(test)`
// deliberately: under `cfg(test)` — the configuration
// `clippy --all-targets` compiles — dead-code analysis stays ARMED, so a
// helper that neither production nor the co-located suite exercises still
// surfaces as a genuine orphan.
#![cfg_attr(not(test), allow(dead_code))]

use std::sync::Arc;

use verter_semantic::analysis::type_eval::AugmentationScopeKind;
use verter_type_expr::locators::{
    AuthoredAugmentationScope, AuthoredBodyLocator, TypeBodyPathStep,
};

use crate::fact_signature_helpers::{ReadSetSignature, ReadSetSignatureExt as _};
use crate::locator_identity::{
    semantic_space_for_locator_space, LibEnvHash, LocatorLoweringKey, ParseEnvHash,
    ProjectIdentityDim, ResolveEnvHash, SlotEnvIdentity, TypeEnvHash,
};
use crate::resolver_core::{BudgetDomain, BudgetExceededFailure};
use crate::semantic_query::operand::{
    authored_anchor, AuthoredSemanticOperand, ForcedSemanticOperand, OperandBinderIdentity,
    OperandSplitEnv, SemanticOperand, SemanticOperandEvidence, SemanticOperandForceRequest,
    SemanticOperandMintError, SemanticOperandParts,
};
use crate::semantic_query::{
    DeclarationSlotSeed, DepSignature, IndexKey, InstantiateBodySource, InstantiateContext,
    InstantiateKey, PathSegment, QueryError, QueryResult, ResolvedDeclSlotIdentity,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};

use super::{ProjectSemanticDispatch, SemanticOperandAuthority};

type OperandEvidenceEntry = (SemanticQueryKey, SemanticOperandEvidence);

struct OperandEvidenceGuard<'a> {
    stack: &'a std::cell::RefCell<smallvec::SmallVec<[OperandEvidenceEntry; 2]>>,
    previous_len: usize,
}

struct AuthoredBinderBinding {
    index: u16,
    parameter_nodes: Vec<SemanticNodeId>,
    default: Option<SemanticNodeId>,
}

/// Declared header type-parameter arity the seal can enforce. There is
/// deliberately no "unknown / defer to force" state: a readable prepared
/// header yields `Exact` (or `DisagreeingOverloads` for an overload
/// group), and an unreadable one is a typed mint error — surplus
/// substitution arguments that bind nothing still hash into
/// `InstantiateKey.args`, so admitting them on an unreadable header is
/// exactly the family fragmentation the seal exists to refuse.
enum HeaderArity {
    /// Surplus above `n` is a typed refusal.
    Exact(usize),
    /// Overload signatures do not share one arity; any non-empty substitution
    /// is refused rather than fragmenting InstantiateKey.args.
    DisagreeingOverloads,
}

fn unique_signature_arity(
    signatures: &[verter_type_expr::facts::FunctionSignatureFact],
) -> HeaderArity {
    let Some(first) = signatures.first() else {
        return HeaderArity::Exact(0);
    };
    let arity = first.type_parameters.len();
    if signatures
        .iter()
        .all(|signature| signature.type_parameters.len() == arity)
    {
        HeaderArity::Exact(arity)
    } else {
        HeaderArity::DisagreeingOverloads
    }
}

impl<'a> OperandEvidenceGuard<'a> {
    /// Push evidence scoped to exactly `target` — the force's own key.
    /// `merge_active_operand_evidence_for_build` only merges an entry into
    /// the cold build whose key equals `target`, so a nested build the
    /// target key's own construction triggers never inherits it.
    fn push(
        stack: &'a std::cell::RefCell<smallvec::SmallVec<[OperandEvidenceEntry; 2]>>,
        target: &SemanticQueryKey,
        evidence: impl IntoIterator<Item = SemanticOperandEvidence>,
    ) -> Self {
        let previous_len = stack.borrow().len();
        stack
            .borrow_mut()
            .extend(evidence.into_iter().map(|item| (target.clone(), item)));
        Self {
            stack,
            previous_len,
        }
    }
}

impl Drop for OperandEvidenceGuard<'_> {
    fn drop(&mut self) {
        self.stack.borrow_mut().truncate(self.previous_len);
    }
}

/// The forced candidate's evidence unioned with the evidence of the input
/// operands the force consumed, deduplicated and bounded by the same
/// signature cap the seal applies. Overflow is the typed
/// [`QueryError::SignatureOverflow`] refusal, never a silently truncated
/// root set.
fn union_operand_evidence(
    inputs: &[SemanticOperandEvidence],
    produced: SemanticOperandEvidence,
) -> Result<SemanticOperandEvidence, QueryError> {
    if inputs.is_empty() {
        return Ok(produced);
    }
    let mut facts: Vec<crate::resolver_core::FactVersionRef> =
        produced.read_set().facts.iter().cloned().collect();
    let mut roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> =
        produced.self_roots().to_vec();
    let mut deps: Vec<DepSignature> = produced.dep_signatures().to_vec();
    for input in inputs {
        if input.read_set().overflowed {
            return Err(QueryError::SignatureOverflow);
        }
        for fact in input.read_set().facts.iter() {
            if !facts.contains(fact) {
                facts.push(fact.clone());
                if facts.len() > crate::resolver_core::FACT_SIGNATURE_CAP {
                    return Err(QueryError::SignatureOverflow);
                }
            }
        }
        for root in input.self_roots().iter() {
            if !roots.contains(root) {
                roots.push(root.clone());
            }
        }
        for dep in input.dep_signatures().iter() {
            if !deps.contains(dep) {
                deps.push(Arc::clone(dep));
            }
        }
    }
    Ok(SemanticOperandEvidence::seal(
        ReadSetSignature::new(Arc::from(facts.into_boxed_slice())),
        Arc::from(roots.into_boxed_slice()),
        Arc::from(deps.into_boxed_slice()),
        &SemanticOperandAuthority::mint_for_forcing_boundary(),
    ))
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Seal a published runtime node to this graph and generation.
    /// `pub(super)`: the forcing authority lives in this module tree, and
    /// no consumer outside it can mint an operand.
    pub(super) fn mint_node_semantic_operand(
        &self,
        forced: &ForcedSemanticOperand,
    ) -> Result<SemanticOperand, SemanticOperandMintError> {
        if forced.store_identity() != self.graph().operand_store_identity()
            || forced.generation() != self.ctx.project_type_store().current_project_generation()
        {
            return Err(SemanticOperandMintError::ForeignNode);
        }
        let evidence = forced.evidence().clone();
        if evidence.read_set().overflowed {
            return Err(SemanticOperandMintError::SignatureOverflow);
        }
        Ok(SemanticOperand::node(
            forced.store_identity(),
            forced.generation(),
            forced.node(),
            evidence,
            SemanticOperandAuthority::mint_for_forcing_boundary(),
        ))
    }

    /// Seal an exact authored locator and authority-issued node operands used
    /// as its substitution. Callers cannot supply scope, binder, environment,
    /// store, or generation axes independently. `pub(super)`: minting is
    /// confined to the forcing authority's module tree.
    pub(super) fn mint_authored_semantic_operand(
        &self,
        locator: AuthoredBodyLocator,
        substitution: Arc<[SemanticOperand]>,
    ) -> Result<SemanticOperand, SemanticOperandMintError> {
        let anchor = authored_anchor(&locator);
        self.verify_authored_anchor(&locator, anchor)?;
        let binder = OperandBinderIdentity::for_locator(&locator);
        match self.declared_header_arity(&locator, anchor)? {
            HeaderArity::Exact(declared) => {
                if substitution.len() > declared {
                    return Err(SemanticOperandMintError::SubstitutionArity {
                        expected: declared,
                        actual: substitution.len(),
                    });
                }
                if let Some(ordinal) = binder.bound_ordinal() {
                    if ordinal as usize >= declared {
                        return Err(SemanticOperandMintError::BoundOrdinalOutOfRange {
                            ordinal,
                            declared,
                        });
                    }
                }
            }
            HeaderArity::DisagreeingOverloads => {
                if !substitution.is_empty() {
                    return Err(SemanticOperandMintError::SubstitutionArity {
                        expected: 0,
                        actual: substitution.len(),
                    });
                }
            }
        }
        let generation = self.ctx.project_type_store().current_project_generation();
        let store_identity = self.graph().operand_store_identity();
        let (substitution, substitution_evidence) =
            self.seal_substitution(&substitution, store_identity, generation)?;
        let split_env = self
            .stable_operand_env(anchor.canonical_id.as_ref())
            .ok_or(SemanticOperandMintError::UnstableEnvironment)?;
        let substitution_runtime =
            (!substitution.is_empty()).then_some((store_identity, generation));
        Ok(SemanticOperand::from_authored_authority(
            locator,
            substitution,
            split_env,
            substitution_evidence,
            substitution_runtime,
            SemanticOperandAuthority::mint_for_forcing_boundary(),
        ))
    }

    /// Fail-closed admission for a top-level declaration anchor: the
    /// declaration the locator names must exist, in the symbol space the
    /// locator names. Sealing an operand for an absent symbol would defer
    /// the refusal to a generic miss at force time, and sealing one whose
    /// symbol lives in the other space would lower it under the wrong
    /// binder frame — both are typed refusals at the seal instead.
    ///
    /// Only `DeclBody` anchors have a prepared-declaration authority to
    /// check against. An augmentation body, a JSDoc typedef body, and a
    /// macro payload carry their own scoped authority (an augmentation
    /// scope, a comment payload, an SFC macro), and a namespace anchor
    /// declares no binder frame at all, so those arms admit unvalidated and
    /// resolve through their own producers.
    fn verify_authored_anchor(
        &self,
        locator: &AuthoredBodyLocator,
        anchor: &verter_type_expr::locators::AuthoredAnchor,
    ) -> Result<(), SemanticOperandMintError> {
        use verter_type_expr::locators::LocatorSymbolSpace;
        if !matches!(locator, AuthoredBodyLocator::DeclBody(_)) {
            return Ok(());
        }
        let canonical = anchor.canonical_id.as_ref();
        let symbol = anchor.symbol.as_ref();
        let in_type_space = || {
            self.ctx
                .prepared_type_decl_return_only(canonical, anchor.owner, symbol)
                .is_some()
        };
        let in_value_space = || {
            self.ctx
                .prepared_value_decl_return_only(canonical, anchor.owner, symbol)
                .is_some()
        };
        let (found, other) = match anchor.space {
            LocatorSymbolSpace::Type => (in_type_space(), LocatorSymbolSpace::Value),
            LocatorSymbolSpace::Value => (in_value_space(), LocatorSymbolSpace::Type),
            LocatorSymbolSpace::Namespace => return Ok(()),
        };
        if found {
            return Ok(());
        }
        let found_in_other = match other {
            LocatorSymbolSpace::Type => in_type_space(),
            LocatorSymbolSpace::Value => in_value_space(),
            LocatorSymbolSpace::Namespace => false,
        };
        Err(if found_in_other {
            SemanticOperandMintError::WrongAnchorSpace {
                expected: anchor.space,
                actual: other,
            }
        } else {
            SemanticOperandMintError::MissingAuthoredDeclaration
        })
    }

    /// How many header type parameters the anchor declaration declares.
    /// The seal enforces the answer against surplus substitution
    /// arguments and out-of-range bound ordinals, so an anchor whose
    /// header cannot be read authoritatively is a typed mint error —
    /// never a "defer the surplus to force" admission.
    ///
    /// `Exact(n)` is a fact the seal enforces; the two `Exact(0)` arms
    /// for JSDoc typedef bodies and SFC macro payloads are structural,
    /// not absences: both deref to `type_parameters: Vec::new()`.
    ///
    /// A value-space declaration's generics live on its selected signature
    /// (or the unique arity of the overload group). An augmentation body's
    /// inner declaration is prepared through the owner bundle.
    fn declared_header_arity(
        &self,
        locator: &AuthoredBodyLocator,
        anchor: &verter_type_expr::locators::AuthoredAnchor,
    ) -> Result<HeaderArity, SemanticOperandMintError> {
        use verter_type_expr::locators::LocatorSymbolSpace;
        match locator {
            AuthoredBodyLocator::DeclBody(slot) => match anchor.space {
                LocatorSymbolSpace::Type => self
                    .ctx
                    .prepared_type_decl_return_only(
                        anchor.canonical_id.as_ref(),
                        anchor.owner,
                        anchor.symbol.as_ref(),
                    )
                    .map(|prepared| HeaderArity::Exact(prepared.type_parameters.len()))
                    .ok_or(SemanticOperandMintError::MissingAuthoredDeclaration),
                LocatorSymbolSpace::Value => self.value_decl_header_arity(slot, anchor),
                LocatorSymbolSpace::Namespace => Ok(HeaderArity::Exact(0)),
            },
            AuthoredBodyLocator::JsdocTypedefBody(_) | AuthoredBodyLocator::MacroPayload(_) => {
                Ok(HeaderArity::Exact(0))
            }
            AuthoredBodyLocator::AugmentationBody(aug) => {
                self.augmentation_header_arity(aug, anchor)
            }
        }
    }

    fn value_decl_header_arity(
        &self,
        slot: &verter_type_expr::locators::TypeBodySlot,
        anchor: &verter_type_expr::locators::AuthoredAnchor,
    ) -> Result<HeaderArity, SemanticOperandMintError> {
        let prepared = self
            .ctx
            .prepared_value_decl_return_only(
                anchor.canonical_id.as_ref(),
                anchor.owner,
                anchor.symbol.as_ref(),
            )
            .ok_or(SemanticOperandMintError::MissingAuthoredDeclaration)?;
        if let Some(TypeBodyPathStep::ValueSignature { ordinal }) = slot.path.first() {
            return prepared
                .signatures
                .get(*ordinal as usize)
                .map(|signature| HeaderArity::Exact(signature.type_parameters.len()))
                .ok_or(SemanticOperandMintError::UnresolvedLocatorPath);
        }
        match unique_signature_arity(&prepared.signatures) {
            // A CLASS is the one value declaration whose value position
            // (the constructor object) is generic through a TYPE-SPACE
            // header: its own signature list is empty, and the header
            // that binds `class Holder<T>` lives on the prepared type
            // declaration. Every other zero-signature value declaration
            // (function, const, let, var, enum) that merely SHARES a
            // name with a generic type declaration declares no header of
            // its own — consulting the type-space header for it would
            // let surplus arguments seal and fragment the value family,
            // so it stays at its own `Exact(0)`.
            HeaderArity::Exact(0)
                if matches!(
                    prepared.kind,
                    verter_semantic::analysis::type_eval::ValueDeclKind::Class
                ) =>
            {
                Ok(self
                    .ctx
                    .prepared_type_decl_return_only(
                        anchor.canonical_id.as_ref(),
                        anchor.owner,
                        anchor.symbol.as_ref(),
                    )
                    .map(|prepared| HeaderArity::Exact(prepared.type_parameters.len()))
                    .unwrap_or(HeaderArity::Exact(0)))
            }
            other => Ok(other),
        }
    }

    fn augmentation_header_arity(
        &self,
        aug: &verter_type_expr::locators::AugmentationBodyLocator,
        anchor: &verter_type_expr::locators::AuthoredAnchor,
    ) -> Result<HeaderArity, SemanticOperandMintError> {
        use verter_type_expr::locators::LocatorSymbolSpace;
        // A value-space augmentation inner declaration has no value-side
        // prepare through the owner bundle; the honest arity read is the
        // structural `Exact(0)` (surplus is refused, under-supply stays
        // legal), never an unreadable-header admission.
        if anchor.space != LocatorSymbolSpace::Type {
            return Ok(HeaderArity::Exact(0));
        }
        let bundle = self
            .ctx
            .prepared_decl_bundle(anchor.canonical_id.as_ref())
            .ok_or(SemanticOperandMintError::MissingAuthoredDeclaration)?;
        let scope_kind = match &aug.scope {
            AuthoredAugmentationScope::Global => AugmentationScopeKind::Global,
            AuthoredAugmentationScope::Module { specifier } => {
                AugmentationScopeKind::Module(specifier.as_ref().to_string())
            }
        };
        match bundle.prepare_augmentation_type_decl_in(
            &scope_kind,
            anchor.owner,
            anchor.symbol.as_ref(),
        ) {
            Ok(Some(prepared)) => Ok(HeaderArity::Exact(prepared.type_parameters.len())),
            Ok(None) | Err(_) => Err(SemanticOperandMintError::MissingAuthoredDeclaration),
        }
    }

    fn seal_substitution(
        &self,
        operands: &[SemanticOperand],
        store_identity: u64,
        generation: u64,
    ) -> Result<(Arc<[SemanticNodeId]>, SemanticOperandEvidence), SemanticOperandMintError> {
        let mut nodes = Vec::with_capacity(operands.len());
        let mut facts = Vec::new();
        let mut roots = Vec::new();
        let mut deps: Vec<DepSignature> = Vec::new();
        for operand in operands {
            // An `Authored` (not-yet-forced) operand supplied as a
            // substitution argument is not a wrong-store/generation node —
            // it is a substitution slot that was never bound to a concrete
            // runtime node in the first place.
            let SemanticOperandParts::Node {
                store_identity: operand_store,
                generation: operand_generation,
                node,
                evidence,
            } = operand.parts(SemanticOperandAuthority::mint_for_forcing_boundary())
            else {
                return Err(SemanticOperandMintError::UnboundSubstitution);
            };
            if operand_store != store_identity || operand_generation != generation {
                return Err(SemanticOperandMintError::ForeignNode);
            }
            nodes.push(node);
            if evidence.read_set().overflowed {
                return Err(SemanticOperandMintError::SignatureOverflow);
            }
            for fact in evidence.read_set().facts.iter() {
                if !facts.contains(fact) {
                    facts.push(fact.clone());
                    if facts.len() > crate::resolver_core::FACT_SIGNATURE_CAP {
                        return Err(SemanticOperandMintError::SignatureOverflow);
                    }
                }
            }
            for root in evidence.self_roots().iter() {
                if !roots.contains(root) {
                    roots.push(root.clone());
                }
            }
            for dep in evidence.dep_signatures().iter() {
                if !deps.contains(dep) {
                    deps.push(Arc::clone(dep));
                }
            }
        }
        Ok((
            Arc::from(nodes.into_boxed_slice()),
            SemanticOperandEvidence::seal(
                ReadSetSignature::new(Arc::from(facts.into_boxed_slice())),
                Arc::from(roots.into_boxed_slice()),
                Arc::from(deps.into_boxed_slice()),
                &SemanticOperandAuthority::mint_for_forcing_boundary(),
            ),
        ))
    }

    fn read_operand_env(&self, canonical: &str) -> OperandSplitEnv {
        let host = self.ctx.host_for_fact_tracer_install();
        let env = host.host_view_env_hashes_for(canonical);
        OperandSplitEnv::new(
            ParseEnvHash::from_env_hash(env.parse_env_hash),
            ResolveEnvHash::from_env_hash(env.resolve_env_hash),
            TypeEnvHash::from_env_hash(env.type_env_hash),
            LibEnvHash::from_env_hash(env.lib_env_hash),
            ProjectIdentityDim::from_project_identity(
                host.host_view_project_identity_for(canonical).fold_u32(),
            ),
            SemanticOperandAuthority::mint_for_forcing_boundary(),
        )
    }

    /// The publication epoch every env dimension is derived from: the
    /// identity of the currently published workspace root plus the
    /// monotonic content generation. The five dimensions live on that one
    /// immutable root (`env_hashes_by_project` / `project_identity_hashes`
    /// / the ownership snapshot), so an unchanged epoch across a read
    /// window is a witness that the whole tuple came from ONE snapshot.
    ///
    /// The root `Arc` is returned, not just its address: the caller pins it
    /// for the whole window, which is what makes pointer equality a sound
    /// identity test (a dropped root's allocation could otherwise be reused
    /// by the replacement).
    fn operand_env_epoch(
        &self,
    ) -> (
        Option<Arc<verter_workspace::published_state::PublishedRoot>>,
        u64,
    ) {
        let workspace = self.ctx.host_for_fact_tracer_install().workspace();
        (workspace.published_root(), workspace.content_generation())
    }

    /// Seal all five environment dimensions as ONE atomic observation.
    ///
    /// The composite read is bracketed by the publication epoch above and
    /// repeated, so acceptance requires BOTH that the two reads agree AND
    /// that no republication or content generation landed between them —
    /// value agreement alone cannot rule out a change-and-change-back
    /// between the two halves of a torn composite read. A window that never
    /// settles is the typed instability refusal, never a spliced tuple.
    fn stable_operand_env(&self, canonical: &str) -> Option<OperandSplitEnv> {
        // bounded-loop: at most three paired snapshots before typed instability.
        for _ in 0..3 {
            let (before_root, before_generation) = self.operand_env_epoch();
            let first = self.read_operand_env(canonical);
            // Test-only repeating seam: a real workspace republication landing
            // between the two halves of the composite read, with every read
            // VALUE unchanged. Production has no hook here.
            #[cfg(test)]
            self.ctx
                .host_for_fact_tracer_install()
                .test_force
                .semantic_operand_env_window_seam
                .fire_repeating();
            let second = self.read_operand_env(canonical);
            let (after_root, after_generation) = self.operand_env_epoch();
            let same_root = match (&before_root, &after_root) {
                (None, None) => true,
                (Some(before), Some(after)) => Arc::ptr_eq(before, after),
                _ => false,
            };
            if first == second && same_root && before_generation == after_generation {
                return Some(first);
            }
        }
        None
    }

    fn typed_operand_partial(
        &self,
        reasons: crate::semantic_query::PartialReasonSet,
    ) -> QueryError {
        if reasons.contains(crate::semantic_query::PartialReasonSet::CANCELLED) {
            return QueryError::Cancelled;
        }
        if reasons.contains(crate::semantic_query::PartialReasonSet::BUDGET_EXCEEDED)
            || reasons.contains(crate::semantic_query::PartialReasonSet::PROJECTION_WORK_LIMIT)
            || reasons
                .contains(crate::semantic_query::PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT)
        {
            let (limit, actual) = crate::request_context::current_request_budget()
                .map(|budget| {
                    (
                        budget.effective_projection_op_budget(),
                        budget.projection_ops_executed_count() as u64,
                    )
                })
                .unwrap_or((0, 0));
            return QueryError::BudgetExceeded(BudgetExceededFailure {
                domain: BudgetDomain::ProjectionOperation,
                limit,
                actual,
                context: "semantic-operand-force".to_string(),
            });
        }
        if reasons.contains(crate::semantic_query::PartialReasonSet::SUPERSEDED_GENERATION) {
            return QueryError::StaleSemanticOperand;
        }
        if reasons.contains(crate::semantic_query::PartialReasonSet::UNSTABLE_STATE) {
            return QueryError::UnstableState { attempts: 1 };
        }
        QueryError::IncompleteSemanticOperand { reasons }
    }

    fn charge_operand_work(&self, context: &str) -> Result<(), QueryError> {
        let Some(budget) = crate::request_context::current_request_budget() else {
            return Ok(());
        };
        if !budget.check_projection_op_count() {
            return Ok(());
        }
        crate::request_context::mark_request_result_inference_budget_exceeded();
        self.fold_into_top_build_local_taint(true, true);
        Err(QueryError::BudgetExceeded(BudgetExceededFailure {
            domain: BudgetDomain::ProjectionOperation,
            limit: budget.effective_projection_op_budget(),
            actual: budget.projection_ops_executed_count() as u64,
            context: context.to_string(),
        }))
    }

    fn reject_if_cancelled(&self) -> Result<(), QueryError> {
        if !self.ctx.is_cancelled() {
            return Ok(());
        }
        crate::request_context::mark_request_result_cancelled();
        self.fold_into_top_build_local_taint(true, true);
        Err(QueryError::Cancelled)
    }

    fn merge_operand_evidence(&self, evidence: &SemanticOperandEvidence) -> Result<(), QueryError> {
        if evidence.read_set().overflowed {
            self.fold_into_top_build_local_taint(false, true);
            return Err(QueryError::SignatureOverflow);
        }
        let self_root_canonicals: Vec<Arc<str>> = evidence
            .self_roots()
            .iter()
            .map(|(canonical, _)| Arc::clone(canonical))
            .collect();
        if !evidence
            .read_set()
            .validate_with_self_roots(self.ctx, &self_root_canonicals)
        {
            self.fold_into_top_build_local_taint(false, true);
            return Err(QueryError::StaleSemanticOperand);
        }
        evidence.read_set().bubble(self.ctx);
        for signature in evidence.dep_signatures().iter() {
            crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, signature);
        }
        self.deposit_operand_self_roots(evidence.self_roots());
        Ok(())
    }

    /// Merge operand evidence into the cold build for `current_key` — ONLY
    /// entries pushed FOR that exact key, never a transitively nested build
    /// `current_key`'s own construction triggers underneath it (a
    /// substitution-independent `LowerLocator` or other child candidate must
    /// not inherit the operand's roots; see [`OperandEvidenceGuard::push`]).
    pub(super) fn merge_active_operand_evidence_for_build(&self, current_key: &SemanticQueryKey) {
        let evidence = {
            let stack = self.active_operand_evidence.borrow();
            if stack.is_empty() {
                return;
            }
            stack.clone()
        };
        for (target, evidence) in &evidence {
            if target != current_key {
                continue;
            }
            if let Err(error) = self.merge_operand_evidence(evidence) {
                // `merge_operand_evidence` already folded `cache_suppress`
                // (never warm-admit this entry). A torn/overflowed injected
                // evidence set must ALSO taint `result_is_partial` — a stale
                // node discovered here (revalidated at cold-build time, not
                // just at the force's preflight check) must never let the
                // build finish as a complete `Value`; without this, an edit
                // landing between preflight and cold execution would let a
                // stale/overflowed operand escape as a fully-formed result.
                let reasons = if matches!(error, QueryError::StaleSemanticOperand) {
                    crate::semantic_query::PartialReasonSet::SUPERSEDED_GENERATION
                } else {
                    crate::semantic_query::PartialReasonSet::PROPAGATED
                };
                self.fold_into_top_build_local_taint_with(true, true, reasons);
            }
        }
    }

    fn exact_locator_key(
        &self,
        authored: &AuthoredSemanticOperand,
    ) -> Result<LocatorLoweringKey, QueryError> {
        let anchor = authored_anchor(authored.locator());
        let (parse, resolve, type_env, lib_env, project) = authored.split_env().parts();
        let slot = DeclarationSlotSeed::new(
            Arc::clone(&anchor.canonical_id),
            anchor.owner,
            Arc::clone(&anchor.symbol),
            semantic_space_for_locator_space(anchor.space),
        )
        .finalize(SlotEnvIdentity::new(type_env, lib_env, project));
        LocatorLoweringKey::new_unsubstituted(slot, authored.locator().clone(), parse, resolve)
            .map_err(|_| QueryError::StaleSemanticOperand)
    }

    /// Force-dispatches the declaration's own `LowerLocator` shape, then
    /// binds `args` onto its type-parameter binders via
    /// [`Self::authored_binders_in`] + the shared
    /// [`Self::substitute_semantic_type_param`] — the same substitution
    /// primitive `build_instantiate`'s declaration-source path applies, over
    /// binder identities that ALREADY exist in `root`.
    ///
    /// `authored_binders_in` re-derives those binder ids by walking the
    /// lowered graph rather than reusing
    /// [`Self::build_type_param_binder_frame`] (the constructor
    /// `locator_binder_frame_from_narrow_params` /
    /// `build_lower_locator` route through) directly: that constructor's
    /// bound (constraint/default) content lowers under a snapshot LEASE
    /// scoped to the originating `LowerLocator` build
    /// (`transient_type_parts` — see the type-parameter-bound confinement
    /// block in the module docs), and that lease is gone by the time this
    /// force-level call runs on the already-published `root` node. Walking
    /// the interned, lease-independent graph is what lets this step recover
    /// binder identity without a second dereference of the operand's own
    /// locator — re-invoking the lease-scoped constructor here is not an
    /// available option, only a second `LowerLocator` dispatch would be,
    /// which would violate the one-dereference-per-force bound
    /// (`cold_force_dereferences_exact_locator_once_and_warm_family_does_not_grow`).
    /// Both routes still mint/intern the SAME content-addressed
    /// `DeclHeader`-mode binder ids for the same `(scope, owner_symbol,
    /// ordinal)`, so a walk-discovered binder and a frame-discovered binder
    /// for the same declaration are never distinct nodes.
    pub(super) fn build_authored_instantiation(
        &self,
        slot: &ResolvedDeclSlotIdentity,
        locator: &AuthoredBodyLocator,
        args: &Arc<[SemanticNodeId]>,
        instantiate_context: InstantiateContext,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        let parse_env = match instantiate_context.body_source() {
            InstantiateBodySource::FileBacked(parse) => parse,
            InstantiateBodySource::NonFile => {
                return (
                    QueryResult::Error(QueryError::Miss),
                    self.project_generation_signature(),
                )
                    .into()
            }
        };
        if let Err(error) = self.reject_if_cancelled() {
            return (
                QueryResult::Error(error),
                self.project_generation_signature(),
            )
                .into();
        }
        if let Err(error) = self.charge_operand_work("semantic-operand-lower-locator") {
            return (
                QueryResult::Error(error),
                self.project_generation_signature(),
            )
                .into();
        }
        let lower_key = match LocatorLoweringKey::new_unsubstituted(
            slot.clone(),
            locator.clone(),
            parse_env,
            ResolveEnvHash::from_env_hash(instantiate_context.resolve_env_hash()),
        ) {
            Ok(key) => SemanticQueryKey::LowerLocator { key },
            // Same failure class `exact_locator_key` maps to
            // `StaleSemanticOperand` — a torn/unstable slot construction is
            // a staleness signal, not a genuine absence; unify the mapping
            // regardless of which site observes the construction failure
            // first.
            Err(_) => {
                return (
                    QueryResult::Error(QueryError::StaleSemanticOperand),
                    self.project_generation_signature(),
                )
                    .into()
            }
        };
        self.record_dispatch_intent_counters(&lower_key);
        let lower = self.execute_read(lower_key);
        crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, &lower.dep_signature);
        let mut root = match lower.value {
            QueryResult::Value(node) => node,
            QueryResult::Recursive(node) => {
                return (QueryResult::Recursive(node), lower.dep_signature).into()
            }
            QueryResult::Error(error) => {
                return (QueryResult::Error(error), lower.dep_signature).into()
            }
        };

        // Test-only one-shot seam: the nested locator-lowering child has
        // completed (and published its own candidate) at this point, while
        // the force's own result is still being built. Admission tests fire
        // a cancellation or budget drain here. Production has no hook.
        #[cfg(test)]
        self.ctx
            .host_for_fact_tracer_install()
            .test_force
            .semantic_operand_post_child_seam
            .fire_once();

        let anchor = authored_anchor(locator);
        let binders = match self.authored_binders_in(root, anchor) {
            Ok(binders) => binders,
            Err(error) => return (QueryResult::Error(error), lower.dep_signature).into(),
        };
        let mut prior = Vec::<(SemanticNodeId, SemanticNodeId)>::new();
        for AuthoredBinderBinding {
            index,
            parameter_nodes,
            default,
        } in binders
        {
            if let Err(error) = self.reject_if_cancelled() {
                return (QueryResult::Error(error), lower.dep_signature).into();
            }
            let mut replacement = args.get(index as usize).copied().or(default);
            if let Some(mut value) = replacement.take() {
                for (parameter, argument) in &prior {
                    value = self.substitute_semantic_type_param(value, *parameter, *argument);
                }
                for parameter in parameter_nodes {
                    root = self.substitute_semantic_type_param(root, parameter, value);
                    prior.push((parameter, value));
                }
            }
        }

        if let Err(error) = self.reject_if_cancelled() {
            return (QueryResult::Error(error), lower.dep_signature).into();
        }
        if let Err(error) = self.charge_operand_work("semantic-operand-project") {
            return (QueryResult::Error(error), lower.dep_signature).into();
        }
        // The final projection dispatches through the SAME `ProjectPath`
        // query family every other caller uses — never a direct builder
        // call — so a plain `ProjectPath` request for the identical
        // (root, empty path, context) triple shares this admission/memo
        // entry instead of the force path computing an uncached duplicate.
        let project_key = SemanticQueryKey::ProjectPath {
            base: root,
            path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            context: instantiate_context.projection_reduction(),
        };
        self.record_dispatch_intent_counters(&project_key);
        let projected = self.execute_read(project_key);
        crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, &projected.dep_signature);
        let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
            (projected.value, projected.dep_signature).into();
        if let Some(indexed) = self
            .ctx
            .ensure_indexed_ready_serve(slot.defining_canonical.as_ref())
            .map(|serve| serve.indexed)
        {
            output = output.with_observed_self_roots(vec![(
                Arc::clone(&slot.defining_canonical),
                indexed.whole_hash,
            )]);
        }
        output
    }

    /// Post-lowering binder-identity recovery for `build_authored_instantiation`
    /// — see that method's doc comment for why this walks the already-interned
    /// `root` graph instead of calling `build_type_param_binder_frame` /
    /// `locator_binder_frame_from_narrow_params` directly (lease lifetime).
    fn authored_binders_in(
        &self,
        root: SemanticNodeId,
        anchor: &verter_type_expr::locators::AuthoredAnchor,
    ) -> Result<Vec<AuthoredBinderBinding>, QueryError> {
        let mut found =
            std::collections::BTreeMap::<u16, (Vec<SemanticNodeId>, Option<SemanticNodeId>)>::new();
        let mut visited = rustc_hash::FxHashSet::default();
        // ONE bounded-work charge for the whole scan: the walk is bounded
        // by its visited set and one force performs exactly one scan, so
        // charging per visited node would scale the force's projection
        // charge with the lowered body's WIDTH — a sibling-heavy object
        // could trip the budget on scan volume alone, with no additional
        // semantic work performed.
        self.charge_operand_work("semantic-operand-binder-scan")?;
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if !visited.insert(node) {
                continue;
            }
            self.reject_if_cancelled()?;
            let Some(data) = self.graph().node_data(node) else {
                continue;
            };
            match data.as_ref() {
                SemanticNodeData::TypeParam {
                    decl,
                    param_index,
                    default,
                    ..
                } if decl.canonical_id == anchor.canonical_id
                    && decl.owner == anchor.owner
                    && decl.decl_name == anchor.symbol =>
                {
                    let entry = found.entry(*param_index).or_default();
                    if !entry.0.contains(&node) {
                        entry.0.push(node);
                    }
                    entry.1 = entry.1.or(*default);
                    // A later parameter's default may itself reference an
                    // earlier parameter (`type D<T = string, U = T> = U`).
                    // Traverse into the default so that earlier binder is
                    // discovered too — otherwise, when the body never
                    // references it directly, its own default (`string`)
                    // never applies and the chain resolves to an unbound
                    // `T` shell instead.
                    if let Some(default) = default {
                        stack.push(*default);
                    }
                }
                SemanticNodeData::Alias(child) => stack.push(*child),
                composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                    stack.extend(
                        composite
                            .composite_members()
                            .expect("composite arm")
                            .iter()
                            .copied(),
                    );
                }
                SemanticNodeData::Array { element, .. } => stack.push(*element),
                SemanticNodeData::Tuple { elements, .. } => {
                    stack.extend(elements.iter().map(|element| element.value));
                }
                SemanticNodeData::Object(surface) => {
                    stack.extend(surface.positive_members().iter().map(|member| member.value));
                    stack.extend(surface.call_signatures.iter().copied());
                    stack.extend(surface.construct_signatures.iter().copied());
                    for signature in surface.index_signatures.iter() {
                        stack.push(signature.key_type);
                        stack.push(signature.value_type);
                    }
                    stack.extend(surface.keyspace);
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    stack.extend(expressions.iter().copied());
                }
                SemanticNodeData::KeyOf { base } => stack.push(*base),
                SemanticNodeData::IndexedAccess { object, index } => {
                    stack.push(*object);
                    if let IndexKey::Computed(index) = index {
                        stack.push(*index);
                    }
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    stack.push(*source);
                    stack.push(mapper.key_space);
                    stack.push(mapper.value_expr);
                    stack.extend(mapper.name_remap);
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    ..
                } => stack.extend([*check, *extends, *true_branch_ref, *false_branch_ref]),
                SemanticNodeData::InstantiationRef { args, .. } => {
                    stack.extend(args.iter().copied());
                }
                SemanticNodeData::ObjectSpreadProgram(program) => {
                    stack.extend(program.child_nodes())
                }
                SemanticNodeData::MergedDecl { contributors } => {
                    stack.extend(contributors.iter().copied());
                }
                SemanticNodeData::Signature {
                    params,
                    return_type,
                    type_parameters,
                    ..
                } => {
                    stack.extend(params.iter().map(|param| param.ty));
                    stack.push(*return_type);
                    for parameter in type_parameters.iter() {
                        stack.extend(parameter.constraint);
                        stack.extend(parameter.default);
                    }
                }
                // The unresolved carriers expose their structural type
                // arguments only through the sanctioned single accessor;
                // descending them keeps a `BareRef<Arg>`'s argument — which
                // may itself reference the anchor's binder — discoverable.
                SemanticNodeData::BareRef(_)
                | SemanticNodeData::TypeOf(_)
                | SemanticNodeData::ImportType(_) => {
                    stack.extend(data.carrier_type_args().iter().copied());
                }
                // Leaves, identity carriers, and value-side carriers: no
                // binder-bearing semantic child to scan. A `DeferredCallable`'s
                // parts are readable only by its two owning executors, and a
                // `SyntheticBinding`'s `value_node` is value-side provenance
                // that never hosts the anchor's header binders. A nominal
                // `typeof` carrier holds only a value root, a member path, and
                // the declaring identity — all names, no semantic node — so it
                // is a true leaf and cannot hide a binder.
                //
                // NO `_` wildcard: a new `SemanticNodeData` variant fails to
                // compile here, forcing its author to classify whether it can
                // carry a binder-bearing child instead of silently dropping
                // it from the scan.
                SemanticNodeData::Primitive(_)
                | SemanticNodeData::Literal(_)
                | SemanticNodeData::Opaque(_)
                | SemanticNodeData::Infer { .. }
                | SemanticNodeData::InferRef { .. }
                | SemanticNodeData::DeclRef { .. }
                | SemanticNodeData::RawFallback { .. }
                | SemanticNodeData::DeferredCallable(_)
                | SemanticNodeData::TypeOfNominal(_)
                | SemanticNodeData::SyntheticBinding { .. } => {}
                // A `TypeParam` of a DIFFERENT declaration (the guarded arm
                // above did not match): its constraint/default carry that
                // other declaration's meaning, not this anchor's binders.
                SemanticNodeData::TypeParam { .. } => {}
            }
        }
        Ok(found
            .into_iter()
            .map(
                |(index, (parameter_nodes, default))| AuthoredBinderBinding {
                    index,
                    parameter_nodes,
                    default,
                },
            )
            .collect())
    }

    pub(super) fn force_semantic_operand(
        &self,
        operand: &SemanticOperand,
        request: SemanticOperandForceRequest,
    ) -> QueryResult<ForcedSemanticOperand> {
        if let Err(error) = self.reject_if_cancelled() {
            return QueryResult::Error(error);
        }
        if let Err(error) = self.charge_operand_work("semantic-operand-force") {
            return QueryResult::Error(error);
        }
        let mut projection_evidence = smallvec::SmallVec::<[SemanticOperandEvidence; 2]>::new();
        let context = request.into_context();
        let key = match operand.parts(SemanticOperandAuthority::mint_for_forcing_boundary()) {
            SemanticOperandParts::Node {
                store_identity,
                generation,
                node,
                evidence,
            } => {
                if store_identity != self.graph().operand_store_identity() {
                    return QueryResult::Error(QueryError::ForeignSemanticOperand);
                }
                if generation != self.ctx.project_type_store().current_project_generation() {
                    return QueryResult::Error(QueryError::StaleSemanticOperand);
                }
                if let Err(error) = self.merge_operand_evidence(evidence) {
                    return QueryResult::Error(error);
                }
                projection_evidence.push(evidence.clone());
                SemanticQueryKey::ProjectPath {
                    base: node,
                    path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
                    context,
                }
            }
            SemanticOperandParts::Authored(authored) => {
                if let Some((store_identity, generation)) = authored.substitution_runtime() {
                    if store_identity != self.graph().operand_store_identity() {
                        return QueryResult::Error(QueryError::ForeignSemanticOperand);
                    }
                    if generation != self.ctx.project_type_store().current_project_generation() {
                        return QueryResult::Error(QueryError::StaleSemanticOperand);
                    }
                }
                let anchor = authored_anchor(authored.locator());
                let Some(live_env) = self.stable_operand_env(anchor.canonical_id.as_ref()) else {
                    return QueryResult::Error(QueryError::UnstableState { attempts: 3 });
                };
                if live_env != authored.split_env() {
                    return QueryResult::Error(QueryError::StaleSemanticOperand);
                }
                if let Err(error) = self.merge_operand_evidence(authored.substitution_evidence()) {
                    return QueryResult::Error(error);
                }
                projection_evidence.push(authored.substitution_evidence().clone());
                let locator_key = match self.exact_locator_key(authored) {
                    Ok(key) => key,
                    Err(error) => return QueryResult::Error(error),
                };
                let (parse, resolve, _, _, _) = authored.split_env().parts();
                let instantiate_context = InstantiateContext::file_backed(
                    context,
                    resolve.get(),
                    parse,
                    super::BodySourceWitness::mint_for_dispatch_factory(),
                );
                // An empty-path TYPE-space `DeclBody` operand addresses the
                // whole declaration body under the body frame — exactly the
                // answer the declaration-source `Instantiate` query
                // computes through its own lease-scoped binder-frame
                // builder. Sealing that force under the authored source
                // would fork the family: the identical (decl, args,
                // context) would live twice, once for the compiler's
                // ordinary dispatches and once for forces. Nested locator
                // positions — and the value-space, augmentation, typedef,
                // and macro arms, whose answers are not the declaration's
                // Instantiate shell — keep locator identity in the key.
                let whole_declaration = authored.addresses_whole_type_declaration();
                SemanticQueryKey::Instantiate(if whole_declaration {
                    InstantiateKey::new(
                        locator_key.slot().clone(),
                        Arc::clone(authored.substitution()),
                        instantiate_context,
                    )
                } else {
                    InstantiateKey::new_authored(
                        locator_key.slot().clone(),
                        authored.query_identity(),
                        Arc::clone(authored.substitution()),
                        instantiate_context,
                        SemanticOperandAuthority::mint_for_forcing_boundary(),
                    )
                })
            }
        };

        if let Err(error) = self.reject_if_cancelled() {
            return QueryResult::Error(error);
        }
        self.record_dispatch_intent_counters(&key);
        let _evidence_guard = OperandEvidenceGuard::push(
            &self.active_operand_evidence,
            &key,
            projection_evidence.iter().cloned(),
        );
        let (read, evidence) = self.execute_read_with_operand_evidence(key);
        crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, &read.dep_signature);
        if evidence
            .as_ref()
            .is_some_and(|evidence| evidence.read_set().overflowed)
            || read.walker_diagnostics.iter().any(|diagnostic| {
                matches!(
                    diagnostic,
                    crate::project_semantic_dispatch::walk::ShallowDiagnostic::SignatureOverflow
                )
            })
        {
            return QueryResult::Error(QueryError::SignatureOverflow);
        }
        if read.result_is_partial {
            return match read.value {
                QueryResult::Error(error) => QueryResult::Error(error),
                QueryResult::Recursive(node) => QueryResult::Recursive(node),
                QueryResult::Value(_) => {
                    QueryResult::Error(self.typed_operand_partial(read.partial_reason_classes()))
                }
            };
        }
        match read.value {
            QueryResult::Error(error) => QueryResult::Error(error),
            QueryResult::Recursive(node) => QueryResult::Recursive(node),
            QueryResult::Value(node) => {
                let Some(evidence) = evidence else {
                    return QueryResult::Error(QueryError::IncompleteSemanticOperand {
                        reasons: crate::semantic_query::PartialReasonSet::PROPAGATED,
                    });
                };
                // The candidate's own evidence UNIONED with the input
                // operand's. Warm and cold forces must return the same
                // evidence for the same operand: only the COLD winner's
                // build observes the injected input evidence, so a warm
                // repetition (or a candidate first built by a different
                // caller carrying different roots for the same interned
                // node) would otherwise hand back a strictly smaller root
                // set — and a `mint -> force -> mint` chain would silently
                // drop the original producer's roots. Unioning here makes
                // the force's OWN observable output path-independent.
                let evidence = match union_operand_evidence(&projection_evidence, evidence) {
                    Ok(evidence) => evidence,
                    Err(error) => return QueryResult::Error(error),
                };
                QueryResult::Value(ForcedSemanticOperand::minted(
                    self.graph().operand_store_identity(),
                    self.ctx.project_type_store().current_project_generation(),
                    node,
                    evidence,
                    SemanticOperandAuthority::mint_for_forcing_boundary(),
                ))
            }
        }
    }
}

/// Authority-issued authored-instantiate key fixture: the ONE sanctioned
/// route from an authored locator to an authored-source
/// [`SemanticQueryKey::Instantiate`] for out-of-module tests that have no
/// live dispatch. The operand is sealed through the same derivation the
/// production mint applies (`SemanticOperand::from_authored_authority`),
/// so the issued key carries the exact sealed lexical/binder/split-env
/// identity — tests never fabricate an operand, an environment tuple, or
/// an evidence set themselves.
#[cfg(test)]
pub(crate) fn authored_instantiate_key_fixture(
    base: ResolvedDeclSlotIdentity,
    locator: AuthoredBodyLocator,
    args: Arc<[SemanticNodeId]>,
    split_env_axes: (
        ParseEnvHash,
        ResolveEnvHash,
        TypeEnvHash,
        LibEnvHash,
        ProjectIdentityDim,
    ),
    context: InstantiateContext,
) -> SemanticQueryKey {
    let (parse_env_hash, resolve_env_hash, type_env_hash, lib_env_hash, project_identity) =
        split_env_axes;
    let authority = SemanticOperandAuthority::mint_for_forcing_boundary();
    let operand = SemanticOperand::from_authored_authority(
        locator,
        Arc::clone(&args),
        OperandSplitEnv::new(
            parse_env_hash,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
            authority,
        ),
        SemanticOperandEvidence::seal(
            ReadSetSignature::empty(),
            Arc::from([]),
            Arc::from([]),
            &authority,
        ),
        None,
        authority,
    );
    let SemanticOperandParts::Authored(authored) = operand.parts(authority) else {
        unreachable!("the fixture seals an authored operand")
    };
    SemanticQueryKey::Instantiate(InstantiateKey::new_authored(
        base,
        authored.query_identity(),
        args,
        context,
        authority,
    ))
}
