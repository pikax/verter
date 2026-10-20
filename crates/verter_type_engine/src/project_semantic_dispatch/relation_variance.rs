//! Type-argument variance — the checker's `getVariances` and
//! `relateVariances`, inside the relation authority.
//!
//! Two references to one generic interface or class relate by their type
//! arguments under each type parameter's variance before any structural
//! comparison (`structuredTypeRelatedTo`), and so do two applications of
//! one type alias whose parameters carry a variance annotation. A
//! parameter's variance is its annotation (`in`, `out`, `in out`) when it
//! has one, and otherwise the variance the checker MEASURES: it relates
//! the declaration instantiated with a marker subtype at that parameter to
//! the one instantiated with the marker supertype (every other parameter
//! left as itself), in both directions, and, when both hold, the one with
//! an unrelated marker to the supertype one (an independent parameter).
//!
//! Measuring is an ordinary relation of the two marker instantiations, so
//! its answer is memoized in the relation memo like any other: there is no
//! variance table. The markers are type parameters of the measured
//! declaration no other relation can name ([`MARKER_PARAM_INDEX`]), so a
//! pair carrying them is only ever related while that declaration's
//! variance is measured. A measurement is a relation of its own — it opens
//! a fresh relation chain, as the checker's `isTypeAssignableTo` inside
//! `getVariances` starts a fresh `checkTypeRelatedTo`.
//!
//! While a declaration's variance is being measured, a relation between
//! two other references to it answers as the checker's does while its
//! `getVariances` is in progress: `Unknown`, which holds — the variance
//! is measured from the occurrences not nested in its own recursive
//! instantiations (`interface R<T> { v: R<[T]>; t: T }` is covariant in
//! `T` through `t`). That answer depends on the measurement being open,
//! and every relation it is reached from carries the measurement's own
//! markers, which only that measurement relates — so each is always
//! computed with the measurement open, and publishes like any other. A
//! reference whose measurement is open BELOW another declaration's (two
//! declarations measuring each other) makes the inner measurement depend
//! on the outer one, so nothing of the enclosing build is memoized: each root relation measures what it needs as if it
//! were the first, as the checker does when it meets that declaration
//! first. Every measurement is therefore a function of the declaration
//! alone, whatever was related before it.

use std::sync::Arc;
use verter_type_expr::OwnedStr;

use super::relation_predicates::{assignable, result_and};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, InferBinding, NodeScopeId, PrimitiveKind, QueryError, RelateMemoKey,
    RelationResult, SemanticNodeData, SemanticNodeId,
};

/// The first `param_index` of a variance marker: a marker is a type
/// parameter of the measured declaration past every authored one,
/// `MARKER_PARAM_INDEX + 3 × parameter + role`.
const MARKER_PARAM_INDEX: u16 = 0x8000;

/// A variance marker's role (the checker's `markerSubType`,
/// `markerSuperType` and `markerOtherType`): the sub-marker is constrained
/// to the super-marker, and the other marker relates to neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerRole {
    Sub = 0,
    Super = 1,
    Other = 2,
}

/// A type parameter's variance — the checker's `VarianceFlags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Variance {
    /// `VarianceMask`: 0 invariant, 1 covariant, 2 contravariant,
    /// 3 bivariant, 4 independent.
    mask: u8,
    /// `Unmeasurable`: the measurement could not be read off the marker
    /// relations, so arguments relate by identity and a failure falls
    /// back to the structural comparison.
    unmeasurable: bool,
    /// `Unreliable`: the measurement's relations met the marker where the
    /// checker reports it unreliable (a rest parameter holding it), so a
    /// failure falls back to the structural comparison.
    unreliable: bool,
}

impl Variance {
    const INVARIANT: u8 = 0;
    const COVARIANT: u8 = 1;
    const CONTRAVARIANT: u8 = 2;
    const BIVARIANT: u8 = 3;
    const INDEPENDENT: u8 = 4;

    const fn exact(mask: u8) -> Self {
        Self {
            mask,
            unmeasurable: false,
            unreliable: false,
        }
    }
}

/// What the variance of a reference's declaration is at this point.
enum DeclarationVariance {
    /// One variance per type parameter.
    Known(Vec<Variance>),
    /// The declaration's variance is being measured: the checker's
    /// in-progress `emptyArray`, whose relation answers this.
    InProgress(RelationResult),
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// Two references to one generic declaration relate by their type
    /// arguments under its parameters' variances (`structuredTypeRelatedTo`
    /// on two references to one target, before its structural
    /// comparison). `None` relates them structurally: the operands are not
    /// two such references, one of them is a measurement's marker
    /// instantiation (`isMarkerType` — a measurement compares structure),
    /// the declaration's variance does not apply (an alias with no
    /// annotation), or the arguments did not relate and the variances
    /// allow a structural fallback (`relateVariances`).
    ///
    /// An inference session relates structurally: its candidates are the
    /// ones the member descent deposits.
    pub(super) fn relate_by_variance(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        if self.relation_session_active() {
            return None;
        }
        let (declaration, source_args) = self.generic_reference(source)?;
        let (target_declaration, target_args) = self.generic_reference(target)?;
        if !same_declaration(&declaration, &target_declaration)
            || source_args.len() != target_args.len()
            || self.variance_measurement(source).is_some()
            || self.variance_measurement(target).is_some()
        {
            return None;
        }
        let variances = match self.parameter_variances(&declaration, source_args.len(), false)? {
            DeclarationVariance::Known(variances) => variances,
            DeclarationVariance::InProgress(result) => return Some(result),
        };
        self.relate_variances(&source_args, &target_args, &variances, bindings)
    }

    /// Two references to one generic declaration under an inference session,
    /// the target's type arguments holding an `infer` site, infer from
    /// their type arguments pairwise (`inferFromTypeArguments`: a
    /// contravariant parameter's argument contravariantly, any other's
    /// covariantly) and relate by them (`Box<"a">` against `Box<infer P>`
    /// infers `"a"`, whatever `Box` expands to) — an alias's parameters
    /// measured as the checker's `getAliasVariances` measures them. `None`
    /// relates the pair as before: not two such references, a variance that
    /// is unmeasurable, unreliable or being measured, or type arguments that
    /// do not relate — their deposits rolled back.
    pub(super) fn infer_from_type_arguments(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        use super::relation::InferPosition;
        if !self.relation_session_active() {
            return None;
        }
        let source_ref = self.generic_reference(source);
        let target_ref = self.generic_reference(target);
        let (declaration, source_args) = source_ref?;
        let (target_declaration, target_args) = target_ref?;
        if !same_declaration(&declaration, &target_declaration)
            || source_args.len() != target_args.len()
            || !target_args
                .iter()
                .any(|arg| self.subtree_contains_infer(*arg) || self.call_session_infers_bare(*arg))
            || self.variance_measurement(source).is_some()
            || self.variance_measurement(target).is_some()
        {
            return None;
        }
        // Distinct, unconstrained bare binders each receive exactly one
        // argument. Replacing them with those arguments reproduces the source
        // reference itself, irrespective of variance. This inference consumes
        // no declaration-body or variance-measurement approximation.
        let owns_whole_pattern = {
            let txn = self.dispatch_txn.borrow();
            let session = txn.active_session().map(|session| session.id);
            (0..txn.reentry().depth()).any(|index| {
                txn.reentry().frame(index).is_some_and(|frame| {
                    session.is_some()
                        && frame
                            .relation()
                            .is_some_and(|state| state.opened_session == session)
                        && frame
                            .identity
                            .as_relate()
                            .is_some_and(|(key, _)| key.target == target)
                })
            })
        };
        // A call's distinct bare type parameters recover the source's
        // arguments the same way (`inferFromTypes` over two applications of
        // one alias infers from their alias arguments before expanding
        // either): the call's applicability check relates the instantiated
        // signature afterwards, so the identity needs no ownership of the
        // whole pattern.
        let mut binders = rustc_hash::FxHashSet::default();
        let mut params = rustc_hash::FxHashSet::default();
        let identity_inference =
            target_args
                .iter()
                .all(|arg| match self.graph().node_data(*arg).as_deref() {
                    Some(SemanticNodeData::Infer {
                        binder,
                        constraint: None,
                        ..
                    }) => owns_whole_pattern && binders.insert(binder.clone()),
                    Some(SemanticNodeData::TypeParam { .. }) => {
                        self.call_session_infers_bare(*arg) && params.insert(*arg)
                    }
                    _ => false,
                });
        let variances = if identity_inference {
            vec![Variance::exact(Variance::COVARIANT); source_args.len()]
        } else {
            let DeclarationVariance::Known(variances) =
                self.parameter_variances(&declaration, source_args.len(), true)?
            else {
                return None;
            };
            variances
        };
        if variances
            .iter()
            .any(|variance| variance.unmeasurable || variance.unreliable)
        {
            return None;
        }
        let checkpoint = self.relation_session_checkpoint();
        let bindings_len = bindings.len();
        let mut acc = assignable(bindings);
        for ((&source, &target), variance) in
            source_args.iter().zip(target_args.iter()).zip(&variances)
        {
            let related = match variance.mask {
                Variance::CONTRAVARIANT => {
                    self.relate_member(target, source, bindings, InferPosition::ContravariantParam)
                }
                Variance::INDEPENDENT => {
                    // Inferred from all the same; no argument decides the
                    // relation.
                    let _ = self.relate_member(source, target, bindings, InferPosition::Covariant);
                    continue;
                }
                Variance::INVARIANT => result_and(
                    self.relate_member(source, target, bindings, InferPosition::Covariant),
                    self.relate_member(target, source, bindings, InferPosition::ContravariantParam),
                ),
                _ => self.relate_member(source, target, bindings, InferPosition::Covariant),
            };
            acc = result_and(acc, related);
            if !matches!(acc, RelationResult::Assignable { .. }) {
                break;
            }
        }
        if !matches!(acc, RelationResult::Assignable { .. }) {
            self.relation_session_rollback(&checkpoint);
            bindings.truncate(bindings_len);
            return None;
        }
        Some(acc)
    }

    /// The checker's `relateVariances` over `typeArgumentsRelatedTo`: each
    /// argument pair relates under its parameter's variance, an
    /// independent parameter's never. When one does not relate, `None`
    /// falls back to the structural comparison if a parameter is
    /// unmeasurable or the target passes `void` for a covariant parameter;
    /// otherwise the variance decides: not assignable.
    fn relate_variances(
        &self,
        source_args: &[SemanticNodeId],
        target_args: &[SemanticNodeId],
        variances: &[Variance],
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        use super::relation::InferPosition;
        let mut acc = assignable(bindings);
        for ((&source, &target), variance) in source_args.iter().zip(target_args).zip(variances) {
            if variance.unreliable {
                // `typeArgumentsRelatedTo` reports an unreliable
                // parameter's argument to an enclosing measurement.
                self.note_relation_unreliable();
            }
            let related = if variance.mask == Variance::INDEPENDENT {
                continue;
            } else if variance.unmeasurable {
                self.relate_type_arguments_identically(source, target)
            } else {
                let covariant = |bindings: &mut Vec<InferBinding>| {
                    self.relate_member(source, target, bindings, InferPosition::Covariant)
                };
                let contravariant = |bindings: &mut Vec<InferBinding>| {
                    self.relate_member(target, source, bindings, InferPosition::Covariant)
                };
                match variance.mask {
                    Variance::COVARIANT => covariant(bindings),
                    Variance::CONTRAVARIANT => contravariant(bindings),
                    Variance::BIVARIANT => match contravariant(bindings) {
                        RelationResult::NotAssignable => covariant(bindings),
                        related => related,
                    },
                    _ => match covariant(bindings) {
                        RelationResult::NotAssignable => RelationResult::NotAssignable,
                        related => result_and(related, contravariant(bindings)),
                    },
                }
            };
            acc = result_and(acc, related);
            if matches!(acc, RelationResult::NotAssignable) {
                break;
            }
        }
        if !matches!(acc, RelationResult::NotAssignable) {
            return Some(acc);
        }
        let structural_fallback = variances
            .iter()
            .any(|variance| variance.unmeasurable || variance.unreliable)
            || variances
                .iter()
                .zip(target_args)
                .any(|(variance, &target)| {
                    variance.mask == Variance::COVARIANT
                        && matches!(
                            self.graph().node_data(target).as_deref(),
                            Some(SemanticNodeData::Primitive(PrimitiveKind::Void))
                        )
                });
        (!structural_fallback).then_some(RelationResult::NotAssignable)
    }

    /// Two type arguments of an unmeasurable parameter relate only when
    /// they are identical (`compareTypesIdentical`). One node is identical
    /// to itself; any other pair is left to the structural comparison the
    /// unmeasurable parameter falls back to, which relates identical
    /// arguments as well.
    fn relate_type_arguments_identically(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelationResult {
        if source == target {
            assignable(&[])
        } else {
            RelationResult::NotAssignable
        }
    }

    /// The variances of `declaration`'s `arity` type parameters, when two
    /// of its references relate by them: an interface's or class's always
    /// (the lib's `Array` and `ReadonlyArray` are covariant, as the checker
    /// fixes them), an alias's when one of its parameters carries an
    /// annotation. A parameter's annotation is its variance; any other is
    /// measured — unless the declaration's variance is being measured.
    fn parameter_variances(
        &self,
        declaration: &DeclIdentity,
        arity: usize,
        measure_aliases: bool,
    ) -> Option<DeclarationVariance> {
        use verter_session_query::declarations::TypeDeclKind;
        use verter_type_expr::facts::TypeParamVariance;
        let prepared = self.ctx.prepared_type_decl_return_only(
            declaration.canonical_id.as_ref(),
            declaration.owner,
            declaration.decl_name.as_ref(),
        )?;
        if prepared.type_parameters.len() != arity {
            return None;
        }
        if self.is_lib_environment_canonical(&declaration.canonical_id)
            && matches!(declaration.decl_name.as_ref(), "Array" | "ReadonlyArray")
        {
            return Some(DeclarationVariance::Known(vec![
                Variance::exact(
                    Variance::COVARIANT
                );
                arity
            ]));
        }
        let annotated = prepared
            .type_parameters
            .iter()
            .map(|param| match param.variance {
                TypeParamVariance::Unannotated => None,
                TypeParamVariance::Out => Some(Variance::exact(Variance::COVARIANT)),
                TypeParamVariance::In => Some(Variance::exact(Variance::CONTRAVARIANT)),
                TypeParamVariance::InOut => Some(Variance::exact(Variance::INVARIANT)),
            })
            .collect::<Vec<_>>();
        if !measure_aliases
            && prepared.kind == TypeDeclKind::Alias
            && annotated.iter().all(Option::is_none)
        {
            return None;
        }
        if annotated.iter().any(Option::is_none) {
            if let Some(result) = self.variance_in_progress(declaration) {
                return Some(DeclarationVariance::InProgress(result));
            }
        }
        let names: Vec<Arc<str>> = prepared
            .type_parameters
            .iter()
            .map(|param| Arc::from(param.name.as_str()))
            .collect();
        Some(DeclarationVariance::Known(
            annotated
                .into_iter()
                .enumerate()
                .map(|(index, annotation)| {
                    annotation.unwrap_or_else(|| self.measure_variance(declaration, &names, index))
                })
                .collect(),
        ))
    }

    /// The checker's measurement of parameter `index` of `declaration`
    /// (`getVariancesWorker`): the declaration with the sub-marker there
    /// relates to it with the super-marker (covariant), the reverse
    /// (contravariant), and when both hold the other marker's to the
    /// super-marker's too (independent). A marker relation the engine
    /// cannot decide leaves the parameter unmeasurable.
    fn measure_variance(
        &self,
        declaration: &DeclIdentity,
        names: &[Arc<str>],
        index: usize,
    ) -> Variance {
        let with =
            |role: MarkerRole| self.variance_marker_instantiation(declaration, names, index, role);
        let (sub, sup) = (with(MarkerRole::Sub), with(MarkerRole::Super));
        let mut unreliable = false;
        let mut relate = |source, target| {
            let (related, reported) = self.variance_marker_relation(source, target);
            unreliable |= reported;
            related
        };
        let (Some(covariant), Some(contravariant)) = (relate(sub, sup), relate(sup, sub)) else {
            return Variance {
                mask: Variance::INVARIANT,
                unmeasurable: true,
                unreliable,
            };
        };
        let mut mask = u8::from(covariant) * Variance::COVARIANT
            + u8::from(contravariant) * Variance::CONTRAVARIANT;
        if mask == Variance::BIVARIANT {
            match relate(with(MarkerRole::Other), sup) {
                Some(true) => mask = Variance::INDEPENDENT,
                Some(false) => {}
                None => {
                    return Variance {
                        mask,
                        unmeasurable: true,
                        unreliable,
                    }
                }
            }
        }
        Variance {
            mask,
            unmeasurable: false,
            unreliable,
        }
    }

    /// One marker relation of a measurement, assignability as the
    /// checker's `isTypeAssignableTo` asks it (`None` when undecided),
    /// and whether it reported an unreliable marker — computed, or replayed
    /// from the memo, where its footprint carries the report.
    fn variance_marker_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> (Option<bool>, bool) {
        self.dispatch_txn
            .borrow_mut()
            .relation
            .last_relation_unreliable = false;
        let observation = self.observe_read(|| {
            let step = self.execute_relate(self.relate_key_for(source, target));
            let unreliable = self.dispatch_txn.borrow().relation.last_relation_unreliable;
            (step, unreliable)
        });
        let accepted = observation.observations_are_complete()
            && matches!(
                observation.value().0,
                super::dispatch_txn::RelationStep::Assignable { .. }
                    | super::dispatch_txn::RelationStep::NotAssignable
            );
        let result = if accepted {
            observation.adopt(self)
        } else {
            // An unmeasurable marker comparison contributes no variance
            // proof. The caller relates the original structures instead.
            observation.discard()
        };
        let Ok((step, unreliable)) = result else {
            return (None, false);
        };
        let related = if accepted {
            match step {
                super::dispatch_txn::RelationStep::Assignable { .. } => Some(true),
                super::dispatch_txn::RelationStep::NotAssignable => Some(false),
                _ => None,
            }
        } else {
            None
        };
        (related, unreliable)
    }

    /// Record, on the relation frame on top of the stack, that its
    /// computation met a marker the checker reports as unreliable.
    pub(super) fn note_relation_unreliable(&self) {
        let mut txn = self.dispatch_txn.borrow_mut();
        let reentry = txn.reentry_mut();
        let Some(top) = reentry.depth().checked_sub(1) else {
            return;
        };
        if let Some(state) = reentry
            .frame_mut_for_update(top)
            .and_then(super::dispatch_txn::ObligationFrame::relation_mut)
        {
            state.chain.recursion.unreliable = true;
        }
    }

    /// Close the unreliable report of the relation frame at `idx`, keyed
    /// `key`: the report is the transaction's last, for a measurement
    /// reading it, and part of the frame below's computation — unless the
    /// frame is a measurement of its own, whose reports stay its own (the
    /// checker restores the enclosing handler after `getVariances`).
    pub(super) fn close_relation_unreliable(
        &self,
        idx: usize,
        key: &RelateMemoKey,
        unreliable: bool,
    ) {
        let measurement = self.relation_key_is_variance_measurement(key);
        let mut txn = self.dispatch_txn.borrow_mut();
        txn.relation.last_relation_unreliable = unreliable;
        if !unreliable || measurement {
            return;
        }
        let Some(parent) = idx.checked_sub(1) else {
            return;
        };
        if let Some(state) = txn
            .reentry_mut()
            .frame_mut_for_update(parent)
            .and_then(super::dispatch_txn::ObligationFrame::relation_mut)
        {
            state.chain.recursion.unreliable = true;
        }
    }

    /// Whether `signature` has a rest parameter whose type holds a
    /// variance marker — one the checker's `compareSignaturesRelated`
    /// reports unreliable.
    pub(super) fn signature_rest_holds_variance_marker(&self, signature: SemanticNodeId) -> bool {
        let graph = self.graph();
        let Some(data) = graph.node_data(signature) else {
            return false;
        };
        let SemanticNodeData::Signature { params, .. } = &*data else {
            return false;
        };
        let Some(rest) = params.iter().find(|param| param.rest).map(|param| param.ty) else {
            return false;
        };
        drop(data);
        let mut stack = vec![rest];
        let mut seen: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            if self.variance_marker_of(node).is_some() {
                return true;
            }
            match graph.node_data(node).as_deref() {
                Some(SemanticNodeData::Array { element, .. }) => stack.push(*element),
                Some(SemanticNodeData::Tuple { elements, .. }) => {
                    stack.extend(elements.iter().map(|element| element.value));
                }
                Some(SemanticNodeData::Union(members)) => {
                    stack.extend(members.members_arc().iter().copied());
                }
                Some(SemanticNodeData::Intersection(members)) => {
                    stack.extend(members.members_arc().iter().copied());
                }
                _ => {}
            }
        }
        false
    }

    /// `declaration` applied to its own type parameters, the one at `index`
    /// replaced by the marker of `role` (the checker's `createMarkerType`).
    fn variance_marker_instantiation(
        &self,
        declaration: &DeclIdentity,
        names: &[Arc<str>],
        index: usize,
        role: MarkerRole,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let args: Vec<SemanticNodeId> = names
            .iter()
            .enumerate()
            .map(|(position, name)| {
                if position == index {
                    self.variance_marker(declaration, name, index, role)
                } else {
                    graph.intern_node_with_scope(
                        SemanticNodeData::TypeParam {
                            decl: declaration.clone(),
                            param_index: position as u16,
                            constraint: None,
                            default: None,
                            display_name: OwnedStr::from(name),
                        },
                        declaration_scope(declaration),
                    )
                }
            })
            .collect();
        graph.intern_node_with_scope(
            SemanticNodeData::InstantiationRef {
                base: declaration.clone(),
                args: crate::semantic_query::PayloadArray::from(args.into_boxed_slice()),
            },
            declaration_scope(declaration),
        )
    }

    /// The marker of `role` for parameter `index` (named `name`) of
    /// `declaration`; the sub-marker is constrained to the super-marker.
    fn variance_marker(
        &self,
        declaration: &DeclIdentity,
        name: &Arc<str>,
        index: usize,
        role: MarkerRole,
    ) -> SemanticNodeId {
        let constraint = (role == MarkerRole::Sub)
            .then(|| self.variance_marker(declaration, name, index, MarkerRole::Super));
        let prefix = match role {
            MarkerRole::Sub => "sub-",
            MarkerRole::Super => "super-",
            MarkerRole::Other => "other-",
        };
        self.graph().intern_node_with_scope(
            SemanticNodeData::TypeParam {
                decl: declaration.clone(),
                param_index: MARKER_PARAM_INDEX + 3 * index as u16 + role as u16,
                constraint,
                default: None,
                display_name: OwnedStr::from(format!("{prefix}{name}")),
            },
            declaration_scope(declaration),
        )
    }

    /// The variance marker `node` is: its declaration, parameter and role.
    pub fn variance_marker_of(
        &self,
        node: SemanticNodeId,
    ) -> Option<(DeclIdentity, u16, MarkerRole)> {
        let data = self.graph().node_data(node)?;
        let SemanticNodeData::TypeParam {
            decl, param_index, ..
        } = &*data
        else {
            return None;
        };
        // A class's polymorphic `this` binder sits past every marker
        // position; it is no marker.
        if *param_index == super::substitute::THIS_BINDER_INDEX {
            return None;
        }
        let offset = param_index.checked_sub(MARKER_PARAM_INDEX)?;
        let role = match offset % 3 {
            0 => MarkerRole::Sub,
            1 => MarkerRole::Super,
            _ => MarkerRole::Other,
        };
        Some((decl.clone(), offset / 3, role))
    }

    /// The declaration and parameter `node` measures, when it is a
    /// measurement's marker instantiation: its declaration applied to its
    /// own type parameters with one of them a marker (`isMarkerType`).
    fn variance_measurement(&self, node: SemanticNodeId) -> Option<(DeclIdentity, u16)> {
        // Marker detection is metadata-only, also while the transaction is borrowed.
        let (declaration, args) = self.raw_generic_reference(node)?;
        let mut measured = None;
        for (position, &arg) in args.iter().enumerate() {
            if let Some((marker_declaration, parameter, _)) = self.variance_marker_of(arg) {
                if measured.is_some()
                    || usize::from(parameter) != position
                    || !same_declaration(&marker_declaration, &declaration)
                {
                    return None;
                }
                measured = Some(parameter);
                continue;
            }
            let data = self.graph().node_data(arg)?;
            let SemanticNodeData::TypeParam {
                decl, param_index, ..
            } = &*data
            else {
                return None;
            };
            if usize::from(*param_index) != position || !same_declaration(decl, &declaration) {
                return None;
            }
        }
        measured.map(|parameter| (declaration, parameter))
    }

    /// Whether `key` is a measurement's marker relation: both operands
    /// measure one parameter of one declaration. Such a relation starts a
    /// relation chain of its own.
    pub(super) fn relation_key_is_variance_measurement(&self, key: &RelateMemoKey) -> bool {
        self.variance_measured_by(key).is_some()
    }

    fn variance_measured_by(&self, key: &RelateMemoKey) -> Option<DeclIdentity> {
        let (source, parameter) = self.variance_measurement(key.source)?;
        let (target, target_parameter) = self.variance_measurement(key.target)?;
        (parameter == target_parameter && same_declaration(&source, &target)).then_some(source)
    }

    /// The checker's in-progress `getVariances` for `declaration`: when its
    /// measurement is open, the relation answers `Unknown`, which holds.
    /// Measured by the NEAREST open measurement, the relations above it
    /// carry its markers and are always related with it open; measured
    /// further down, the nearer measurement depends on it, and nothing of
    /// the enclosing build is memoized.
    /// `None` when no measurement of `declaration` is open.
    fn variance_in_progress(&self, declaration: &DeclIdentity) -> Option<RelationResult> {
        let (nearest, measuring) = {
            let txn = self.dispatch_txn.borrow();
            let reentry = txn.reentry();
            let mut nearest = None;
            let mut measuring = None;
            for index in (0..reentry.depth()).rev() {
                let Some((key, _)) = reentry
                    .frame(index)
                    .and_then(|frame| frame.identity.as_relate())
                else {
                    continue;
                };
                let Some(measured) = self.variance_measured_by(key) else {
                    continue;
                };
                nearest.get_or_insert(index);
                if same_declaration(&measured, declaration) {
                    measuring = Some(index);
                    break;
                }
            }
            (nearest, measuring)
        };
        let measuring = measuring?;
        if nearest != Some(measuring) {
            self.fold_into_top_build_local_taint_with(
                false,
                true,
                crate::semantic_query::PartialReasonSet::empty(),
            );
        }
        Some(assignable(&[]))
    }

    /// The generic declaration `node` applies and its type arguments: an
    /// application (`InstantiationRef`), or a declaration's reference to
    /// itself lowered inside its own body (`RecursiveRef`), whose file is
    /// the node's origin scope.
    fn generic_reference(
        &self,
        node: SemanticNodeId,
    ) -> Option<(DeclIdentity, Arc<[SemanticNodeId]>)> {
        let context =
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            );
        let mut node = node;
        let mut visited = rustc_hash::FxHashSet::default();
        while visited.insert(node) {
            let data = self.graph().node_data(node)?;
            match data.as_ref() {
                SemanticNodeData::Alias(next) => node = *next,
                SemanticNodeData::IndexedAccess { .. } => {
                    let next = self
                        .evaluate_deferred_semantic_node_with_context(node, context)
                        .into_active_query_build_node(self);
                    if next == node {
                        break;
                    }
                    node = next;
                }
                _ => break,
            }
        }
        if let Some(SemanticNodeData::DeclRef { identity }) =
            self.graph().node_data(node).as_deref()
        {
            return Some((
                identity.clone(),
                self.declared_application_arguments(identity, &[], context)?,
            ));
        }
        let (declaration, args) = self.raw_generic_reference(node)?;
        let args = self.declared_application_arguments(&declaration, &args, context)?;
        Some((declaration, args))
    }

    /// The interface or class reference a type-alias application stands
    /// for, when the other operand is a reference to that declaration.
    ///
    /// The checker resolves an alias reference where it is written: the
    /// application IS the type its body instantiates to, and when that type
    /// is a reference to a generic interface or class, two references to
    /// one declaration infer from and relate by their type arguments
    /// (`MessageBase<X> extends UIMessage<infer M>` infers through the
    /// `UIMessage<…>` the alias selects, never through its members).
    /// `None` when `node` applies no alias, when its instantiation is not a
    /// reference, or when that reference names a declaration other than
    /// `other`'s.
    pub(super) fn alias_application_reference_to(
        &self,
        node: SemanticNodeId,
        other: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        use verter_session_query::declarations::TypeDeclKind;
        let declaration_of = |node: SemanticNodeId| -> Option<DeclIdentity> {
            match self.graph().node_data(node)?.as_ref() {
                SemanticNodeData::DeclRef { identity } => Some(identity.clone()),
                SemanticNodeData::InstantiationRef { base, .. } => Some(base.clone()),
                _ => None,
            }
        };
        let is_alias = |declaration: &DeclIdentity| {
            self.ctx
                .prepared_type_decl_return_only(
                    declaration.canonical_id.as_ref(),
                    declaration.owner,
                    declaration.decl_name.as_ref(),
                )
                .is_some_and(|prepared| prepared.kind == TypeDeclKind::Alias)
        };
        let target = declaration_of(other)?;
        if is_alias(&target) {
            return None;
        }
        let transit = crate::semantic_query::ProjectionReductionContext::structural_transit();
        let mut current = node;
        let mut visited = rustc_hash::FxHashSet::default();
        while visited.insert(current) {
            if let Some(SemanticNodeData::Alias(inner)) = self.graph().node_data(current).as_deref()
            {
                current = *inner;
                continue;
            }
            let (declaration, args) = match self.graph().node_data(current)?.as_ref() {
                SemanticNodeData::DeclRef { identity } => (identity.clone(), Arc::from([])),
                SemanticNodeData::InstantiationRef { base, args } => {
                    (base.clone(), Arc::clone(args.as_arc()))
                }
                _ => return None,
            };
            if same_declaration(&declaration, &target) {
                return (current != node).then_some(current);
            }
            if !is_alias(&declaration) {
                return None;
            }
            let key = crate::semantic_query::SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    self.type_slot_for(
                        declaration.canonical_id.clone(),
                        declaration.owner,
                        declaration.decl_name.clone(),
                    ),
                    args,
                    self.instantiate_context_for(&declaration.canonical_id, transit),
                ),
            );
            let crate::semantic_query::QueryResult::Value(output) =
                crate::semantic_query::SemanticQueryApi::execute_type_node(self, key)
            else {
                return None;
            };
            current = self
                .evaluate_deferred_semantic_node_with_context(output.value, transit)
                .into_active_query_build_node(self);
        }
        None
    }

    fn raw_generic_reference(
        &self,
        node: SemanticNodeId,
    ) -> Option<(DeclIdentity, Arc<[SemanticNodeId]>)> {
        let graph = self.graph();
        let data = graph.node_data(node)?;
        match &*data {
            SemanticNodeData::InstantiationRef { base, args } => {
                Some((base.clone(), Arc::clone(args.as_arc())))
            }
            SemanticNodeData::Opaque(QueryError::RecursiveRef { name, args })
                if !args.is_empty() =>
            {
                let Some(NodeScopeId::File {
                    canonical_id,
                    owner,
                    whole_hash,
                    ..
                }) = graph.node_scope(node)
                else {
                    return None;
                };
                Some((
                    DeclIdentity {
                        canonical_id,
                        owner,
                        whole_hash,
                        decl_name: name.clone(),
                    },
                    Arc::from(args.operands()),
                ))
            }
            _ => None,
        }
    }
}

/// The origin scope of the nodes a measurement of `declaration` interns —
/// its markers and marker instantiations: the declaration's own file at
/// the content generation its identity pins. They are the declaration's
/// derived nodes, released with that generation (a measurement after an
/// edit interns under the new generation, never beside a stale one), and
/// at most six per type parameter: three markers and three instantiations.
pub fn declaration_scope(declaration: &DeclIdentity) -> NodeScopeId {
    NodeScopeId::File {
        canonical_id: declaration.canonical_id.clone(),
        owner: declaration.owner,
        whole_hash: declaration.whole_hash,
        local_scope: None,
    }
}

/// Whether two declaration identities name one declaration.
fn same_declaration(a: &DeclIdentity, b: &DeclIdentity) -> bool {
    a.canonical_id == b.canonical_id && a.owner == b.owner && a.decl_name == b.decl_name
}

/// How the structural descent relates a pair holding a variance marker.
pub(super) enum MarkerPair {
    /// Neither operand is a marker.
    NotMarker,
    /// A marker against a union or an intersection: the pair distributes
    /// over its arms, as any other pair does.
    Distribute,
    /// The pair's verdict.
    Decided(RelationResult),
    /// The pair relates as this one: a marker source as its constraint's
    /// `unknown`.
    Relate(SemanticNodeId, SemanticNodeId),
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// A pair holding a variance marker, related as the checker relates
    /// its marker type parameters: the sub-marker to the super-marker of
    /// its parameter, a marker to the same marker; a marker source to any
    /// other target as its constraint — the super-marker, whose own is
    /// `unknown` — would; and nothing but `never` or `any` (decided
    /// before) to a marker target.
    pub(super) fn variance_marker_pair(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> MarkerPair {
        let source_marker = self.variance_marker_of(source);
        let target_marker = self.variance_marker_of(target);
        if source_marker.is_none() && target_marker.is_none() {
            return MarkerPair::NotMarker;
        }
        let graph = self.graph();
        let kind = |node: SemanticNodeId| graph.node_data(node);
        let composite = |node: SemanticNodeId| {
            matches!(
                kind(node).as_deref(),
                Some(SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_))
            )
        };
        let type_param = |node: SemanticNodeId| {
            matches!(
                kind(node).as_deref(),
                Some(SemanticNodeData::TypeParam { .. })
            )
        };
        match (source_marker, target_marker) {
            (Some(source_marker), Some(target_marker)) => {
                let related = source == target
                    || (source_marker.2 == MarkerRole::Sub
                        && target_marker.2 == MarkerRole::Super
                        && source_marker.1 == target_marker.1
                        && same_declaration(&source_marker.0, &target_marker.0));
                MarkerPair::Decided(if related {
                    assignable(&[])
                } else {
                    RelationResult::NotAssignable
                })
            }
            (Some(_), None) | (None, Some(_)) if composite(source) || composite(target) => {
                MarkerPair::Distribute
            }
            (Some(_), None) if !type_param(target) => MarkerPair::Relate(
                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown)),
                target,
            ),
            _ => MarkerPair::Decided(RelationResult::NotAssignable),
        }
    }
}
