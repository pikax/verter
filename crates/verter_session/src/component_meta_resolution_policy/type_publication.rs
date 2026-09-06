//! Structural policy for selecting an authored publication representation.
//!
//! The authored annotation SOURCE is the canonical form for
//! macro-participating imports. The contract runs BEFORE the rule walk on
//! prop / binding / accepted-prop published sources; classification is
//! structural (§3.4 Typed-IR-Only Resolver Rule): "role-bearing" means
//! "consumed by one of the owner's `defineProps` / `defineEmits` /
//! `defineModel` / `defineSlots` / `withDefaults` macros", NOT "identifier
//! ends in `Props`".

use verter_type_expr::facts::SemanticTypeSource;
use verter_type_expr::{
    AuthoredTypeEvidence, PublicationPolicy, PublicationPolicyReason, ResolutionExactness,
    SymbolicEquivalenceKind, SymbolicEquivalenceMint, SymbolicEquivalenceProof, TypePublication,
};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, IndexKey, NodeScopeId, ProjectionMode, ProjectionReductionContext, QueryError,
    SemanticNodeData, SemanticNodeId,
};

use super::core::{DeclLookup, PolicyCtx};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ProofPathStep {
    ObjectMember(u32, crate::semantic_query::AuthoredPropertyKey),
    ObjectCall(u32),
    ObjectConstruct(u32),
    ObjectIndexKey(u32),
    ObjectIndexValue(u32),
    ObjectKeyspace,
    UnionArm(u32),
    IntersectionArm(u32),
    ArrayElement,
    TupleElement(u32),
    TemplateExpression(u32),
    KeyOfBase,
    IndexedObject,
    IndexedIndex,
    MappedSource,
    MappedParameter,
    MappedKeyspace,
    MappedValue,
    MappedNameRemap,
    TypeParamConstraint,
    TypeParamDefault,
    ConditionalCheck,
    ConditionalExtends,
    ConditionalTrue,
    ConditionalFalse,
    FunctionParam(u32),
    FunctionReturn,
    FunctionTypeParamConstraint(u32),
    FunctionTypeParamDefault(u32),
    ReferenceArgument(u32),
    MergedContributor(u32),
    ObjectSpreadEffect(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProofReferenceIdentity {
    Decl(DeclIdentity),
    Bare {
        name: std::sync::Arc<str>,
        scope: NodeScopeId,
    },
    TypeParam {
        decl: DeclIdentity,
        param_index: u16,
    },
    /// The declaring identity a terminal nominal `typeof` carrier denotes.
    /// Distinct from [`Self::Decl`]: a `unique symbol` VALUE declaration is
    /// identified by its [`ValueDeclIdentityPart`], not by a type-decl
    /// [`DeclIdentity`].
    Nominal(verter_type_expr::facts::ValueDeclIdentityPart),
}

type ProofReferenceMap = rustc_hash::FxHashMap<Vec<ProofPathStep>, ProofReferenceIdentity>;

enum ProofWalkFrame {
    Enter(SemanticNodeId, Vec<ProofPathStep>),
    Exit(SemanticNodeId),
}

fn push_proof_child(
    stack: &mut Vec<ProofWalkFrame>,
    path: &[ProofPathStep],
    step: ProofPathStep,
    child: SemanticNodeId,
) {
    let mut child_path = path.to_vec();
    child_path.push(step);
    stack.push(ProofWalkFrame::Enter(child, child_path));
}

/// Collect identity-bearing references by structural position without
/// resolving them. This sidecar is the no-information-loss rail around the
/// normalized projection comparison below: the shared shape key intentionally
/// renders `DeclRef`/`BareRef` as display refs, so it cannot carry this proof.
fn proof_reference_map(root: SemanticNodeId, ctx: &PolicyCtx<'_, '_>) -> Option<ProofReferenceMap> {
    let mut references = ProofReferenceMap::default();
    let mut active = rustc_hash::FxHashSet::default();
    let mut stack = vec![ProofWalkFrame::Enter(root, Vec::new())];

    while let Some(frame) = stack.pop() {
        let (node, path) = match frame {
            ProofWalkFrame::Exit(node) => {
                active.remove(&node);
                continue;
            }
            ProofWalkFrame::Enter(node, path) if active.insert(node) => (node, path),
            ProofWalkFrame::Enter(_, _) => continue,
        };
        stack.push(ProofWalkFrame::Exit(node));
        let data = ctx.node_data(node)?;
        match data.as_ref() {
            SemanticNodeData::DeclRef { identity } => {
                references.insert(path, ProofReferenceIdentity::Decl(identity.clone()));
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                references.insert(path.clone(), ProofReferenceIdentity::Decl(base.clone()));
                for (index, arg) in args.iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::ReferenceArgument(index as u32),
                        *arg,
                    );
                }
            }
            SemanticNodeData::BareRef(_) => {
                let (name, scope) = data.bare_ref_head().expect("BareRef carrier head");
                references.insert(
                    path.clone(),
                    ProofReferenceIdentity::Bare {
                        name: std::sync::Arc::clone(name),
                        scope: scope.clone(),
                    },
                );
                for (index, arg) in data.carrier_type_args().iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::ReferenceArgument(index as u32),
                        *arg,
                    );
                }
            }
            SemanticNodeData::TypeParam {
                decl,
                param_index,
                constraint,
                default,
                ..
            } => {
                references.insert(
                    path.clone(),
                    ProofReferenceIdentity::TypeParam {
                        decl: decl.clone(),
                        param_index: *param_index,
                    },
                );
                if let Some(constraint) = constraint {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::TypeParamConstraint,
                        *constraint,
                    );
                }
                if let Some(default) = default {
                    push_proof_child(&mut stack, &path, ProofPathStep::TypeParamDefault, *default);
                }
            }
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                whole_hash,
            }) => {
                references.insert(
                    path,
                    ProofReferenceIdentity::Decl(DeclIdentity {
                        canonical_id: std::sync::Arc::clone(canonical_id),
                        owner: *owner,
                        whole_hash: *whole_hash,
                        decl_name: std::sync::Arc::clone(name),
                    }),
                );
            }
            SemanticNodeData::Alias(target) => {
                stack.push(ProofWalkFrame::Enter(*target, path));
            }
            SemanticNodeData::Object(surface) => {
                for (index, member) in surface.positive_members().iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::ObjectMember(index as u32, member.key.clone()),
                        member.value,
                    );
                }
                for (index, signature) in surface.call_signatures.iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::ObjectCall(index as u32),
                        *signature,
                    );
                }
                for (index, signature) in surface.construct_signatures.iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::ObjectConstruct(index as u32),
                        *signature,
                    );
                }
                for (index, signature) in surface.index_signatures.iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::ObjectIndexKey(index as u32),
                        signature.key_type,
                    );
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::ObjectIndexValue(index as u32),
                        signature.value_type,
                    );
                }
                if let Some(keyspace) = surface.keyspace {
                    push_proof_child(&mut stack, &path, ProofPathStep::ObjectKeyspace, keyspace);
                }
            }
            SemanticNodeData::Union(arms) => {
                for (index, arm) in arms.iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::UnionArm(index as u32),
                        *arm,
                    );
                }
            }
            SemanticNodeData::Intersection(arms) => {
                for (index, arm) in arms.iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::IntersectionArm(index as u32),
                        *arm,
                    );
                }
            }
            SemanticNodeData::Array { element, .. } => {
                push_proof_child(&mut stack, &path, ProofPathStep::ArrayElement, *element);
            }
            SemanticNodeData::Tuple { elements, .. } => {
                for (index, element) in elements.iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::TupleElement(index as u32),
                        element.value,
                    );
                }
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                for (index, expression) in expressions.iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::TemplateExpression(index as u32),
                        *expression,
                    );
                }
            }
            SemanticNodeData::KeyOf { base } => {
                push_proof_child(&mut stack, &path, ProofPathStep::KeyOfBase, *base);
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                push_proof_child(&mut stack, &path, ProofPathStep::IndexedObject, *object);
                if let IndexKey::Computed(index) = index {
                    push_proof_child(&mut stack, &path, ProofPathStep::IndexedIndex, *index);
                }
            }
            SemanticNodeData::Mapped { source, mapper } => {
                push_proof_child(&mut stack, &path, ProofPathStep::MappedSource, *source);
                push_proof_child(
                    &mut stack,
                    &path,
                    ProofPathStep::MappedParameter,
                    mapper.parameter_node,
                );
                push_proof_child(
                    &mut stack,
                    &path,
                    ProofPathStep::MappedKeyspace,
                    mapper.key_space,
                );
                push_proof_child(
                    &mut stack,
                    &path,
                    ProofPathStep::MappedValue,
                    mapper.value_expr,
                );
                if let Some(name_remap) = mapper.name_remap {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::MappedNameRemap,
                        name_remap,
                    );
                }
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                push_proof_child(&mut stack, &path, ProofPathStep::ConditionalCheck, *check);
                push_proof_child(
                    &mut stack,
                    &path,
                    ProofPathStep::ConditionalExtends,
                    *extends,
                );
                push_proof_child(
                    &mut stack,
                    &path,
                    ProofPathStep::ConditionalTrue,
                    *true_branch_ref,
                );
                push_proof_child(
                    &mut stack,
                    &path,
                    ProofPathStep::ConditionalFalse,
                    *false_branch_ref,
                );
            }
            SemanticNodeData::Signature {
                params,
                return_type,
                type_parameters,
                ..
            } => {
                for (index, param) in params.iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::FunctionParam(index as u32),
                        param.ty,
                    );
                }
                push_proof_child(
                    &mut stack,
                    &path,
                    ProofPathStep::FunctionReturn,
                    *return_type,
                );
                for (index, parameter) in type_parameters.iter().enumerate() {
                    if let Some(constraint) = parameter.constraint {
                        push_proof_child(
                            &mut stack,
                            &path,
                            ProofPathStep::FunctionTypeParamConstraint(index as u32),
                            constraint,
                        );
                    }
                    if let Some(default) = parameter.default {
                        push_proof_child(
                            &mut stack,
                            &path,
                            ProofPathStep::FunctionTypeParamDefault(index as u32),
                            default,
                        );
                    }
                }
            }
            SemanticNodeData::MergedDecl { contributors } => {
                for (index, contributor) in contributors.iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::MergedContributor(index as u32),
                        *contributor,
                    );
                }
            }
            SemanticNodeData::TypeOf(_) | SemanticNodeData::ImportType(_) => {
                for (index, arg) in data.carrier_type_args().iter().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::ReferenceArgument(index as u32),
                        *arg,
                    );
                }
            }
            // A terminal nominal carrier names a VALUE declaration; its
            // declaring identity is exactly the identity-bearing reference
            // this map exists to retain, because the shape comparison below
            // renders only the carrier's head.
            SemanticNodeData::TypeOfNominal(_) => {
                let identity = data
                    .typeof_nominal_identity()
                    .expect("TypeOfNominal carrier identity");
                references.insert(path, ProofReferenceIdentity::Nominal(identity.clone()));
            }
            SemanticNodeData::ObjectSpreadProgram(program) => {
                for (index, child) in program.child_nodes().enumerate() {
                    push_proof_child(
                        &mut stack,
                        &path,
                        ProofPathStep::ObjectSpreadEffect(index as u32),
                        child,
                    );
                }
            }
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::InferRef { .. }
            | SemanticNodeData::RawFallback { .. }
            // A sealed callable carrier records no reference identity and
            // opens only to its two sanctioned consumers — terminal here.
            | SemanticNodeData::DeferredCallable(_)
            | SemanticNodeData::SyntheticBinding { .. } => {}
        }
    }

    Some(references)
}

#[cfg(test)]
pub(super) fn proof_reference_maps_match_for_test(
    left: SemanticNodeId,
    right: SemanticNodeId,
    ctx: &PolicyCtx<'_, '_>,
) -> Option<bool> {
    Some(proof_reference_map(left, ctx)? == proof_reference_map(right, ctx)?)
}

fn push_optional_proof_pair(
    stack: &mut Vec<(SemanticNodeId, SemanticNodeId)>,
    left: Option<SemanticNodeId>,
    right: Option<SemanticNodeId>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            stack.push((left, right));
            true
        }
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

/// Compare node projections by semantic type fields, normalizing each
/// structural occurrence through the shared dispatch at `Published(Navigate)`.
/// Source spans, declaration origins, and merge/macro provenance are excluded:
/// they explain where an equivalent projection came from, not what type it is.
/// Reference identities remain exact here and are additionally guarded by the
/// positional map in [`symbolic_projection_equivalent`].
fn normalized_projection_shape_equivalent(
    left: SemanticNodeId,
    right: SemanticNodeId,
    ctx: &PolicyCtx<'_, '_>,
) -> Option<bool> {
    let dispatch = ProjectSemanticDispatch::new(ctx.resolver_ctx());
    let reduction = ProjectionReductionContext::published(ProjectionMode::Navigate);
    let mut visited = rustc_hash::FxHashSet::default();
    let mut stack = vec![(left, right)];

    while let Some((left, right)) = stack.pop() {
        if left == right {
            continue;
        }
        let left = dispatch
            .normalize_node_for_structural_fact_demand(left, reduction)
            .into_complete_node()?;
        let right = dispatch
            .normalize_node_for_structural_fact_demand(right, reduction)
            .into_complete_node()?;
        if left == right || !visited.insert((left, right)) {
            continue;
        }
        let left_data = ctx.node_data(left)?;
        let right_data = ctx.node_data(right)?;

        if let SemanticNodeData::Alias(target) = left_data.as_ref() {
            stack.push((*target, right));
            continue;
        }
        if let SemanticNodeData::Alias(target) = right_data.as_ref() {
            stack.push((left, *target));
            continue;
        }

        match (left_data.as_ref(), right_data.as_ref()) {
            (SemanticNodeData::Object(left), SemanticNodeData::Object(right)) => {
                if left.positive_members().len() != right.positive_members().len()
                    || left.call_signatures.len() != right.call_signatures.len()
                    || left.construct_signatures.len() != right.construct_signatures.len()
                    || left.index_signatures.len() != right.index_signatures.len()
                    || left.has_known_index_signature() != right.has_known_index_signature()
                    || !push_optional_proof_pair(&mut stack, left.keyspace, right.keyspace)
                {
                    return Some(false);
                }
                for (left, right) in left
                    .positive_members()
                    .iter()
                    .zip(right.positive_members().iter())
                {
                    if left.key != right.key
                        || left.optional != right.optional
                        || left.readonly != right.readonly
                        || left.method_kind != right.method_kind
                        || left.visibility != right.visibility
                    {
                        return Some(false);
                    }
                    stack.push((left.value, right.value));
                }
                stack.extend(
                    left.call_signatures
                        .iter()
                        .copied()
                        .zip(right.call_signatures.iter().copied()),
                );
                stack.extend(
                    left.construct_signatures
                        .iter()
                        .copied()
                        .zip(right.construct_signatures.iter().copied()),
                );
                for (left, right) in left
                    .index_signatures
                    .iter()
                    .zip(right.index_signatures.iter())
                {
                    if left.readonly != right.readonly {
                        return Some(false);
                    }
                    stack.push((left.key_type, right.key_type));
                    stack.push((left.value_type, right.value_type));
                }
            }
            (
                left @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)),
                right @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)),
            ) if left.discriminant_index() == right.discriminant_index() => {
                let left = left.composite_members().expect("composite arm");
                let right = right.composite_members().expect("composite arm");
                if left.len() != right.len() {
                    return Some(false);
                }
                stack.extend(left.iter().copied().zip(right.iter().copied()));
            }
            (SemanticNodeData::Primitive(left), SemanticNodeData::Primitive(right)) => {
                if left != right {
                    return Some(false);
                }
            }
            (SemanticNodeData::Literal(left), SemanticNodeData::Literal(right)) => {
                if left != right {
                    return Some(false);
                }
            }
            (SemanticNodeData::Opaque(left), SemanticNodeData::Opaque(right)) => {
                if left != right {
                    return Some(false);
                }
            }
            (
                SemanticNodeData::Array {
                    element: left,
                    readonly: left_readonly,
                },
                SemanticNodeData::Array {
                    element: right,
                    readonly: right_readonly,
                },
            ) => {
                if left_readonly != right_readonly {
                    return Some(false);
                }
                stack.push((*left, *right));
            }
            (
                SemanticNodeData::Tuple {
                    elements: left,
                    readonly: left_readonly,
                },
                SemanticNodeData::Tuple {
                    elements: right,
                    readonly: right_readonly,
                },
            ) => {
                if left_readonly != right_readonly || left.len() != right.len() {
                    return Some(false);
                }
                for (left, right) in left.iter().zip(right.iter()) {
                    if left.label != right.label
                        || left.optional != right.optional
                        || left.rest != right.rest
                    {
                        return Some(false);
                    }
                    stack.push((left.value, right.value));
                }
            }
            (
                SemanticNodeData::TemplateLiteral {
                    quasis: left_quasis,
                    expressions: left_expressions,
                },
                SemanticNodeData::TemplateLiteral {
                    quasis: right_quasis,
                    expressions: right_expressions,
                },
            ) => {
                if left_quasis != right_quasis || left_expressions.len() != right_expressions.len()
                {
                    return Some(false);
                }
                stack.extend(
                    left_expressions
                        .iter()
                        .copied()
                        .zip(right_expressions.iter().copied()),
                );
            }
            (SemanticNodeData::KeyOf { base: left }, SemanticNodeData::KeyOf { base: right }) => {
                stack.push((*left, *right))
            }
            (
                SemanticNodeData::IndexedAccess {
                    object: left_object,
                    index: left_index,
                },
                SemanticNodeData::IndexedAccess {
                    object: right_object,
                    index: right_index,
                },
            ) => {
                stack.push((*left_object, *right_object));
                match (left_index, right_index) {
                    (IndexKey::String(left), IndexKey::String(right)) if left == right => {}
                    (IndexKey::Number(left), IndexKey::Number(right)) if left == right => {}
                    (IndexKey::Computed(left), IndexKey::Computed(right)) => {
                        stack.push((*left, *right));
                    }
                    _ => return Some(false),
                }
            }
            (
                SemanticNodeData::Mapped {
                    source: left_source,
                    mapper: left_mapper,
                },
                SemanticNodeData::Mapped {
                    source: right_source,
                    mapper: right_mapper,
                },
            ) => {
                if left_mapper.optionality != right_mapper.optionality
                    || left_mapper.readonly != right_mapper.readonly
                    || left_mapper.kind != right_mapper.kind
                    || !push_optional_proof_pair(
                        &mut stack,
                        left_mapper.name_remap,
                        right_mapper.name_remap,
                    )
                {
                    return Some(false);
                }
                stack.push((*left_source, *right_source));
                stack.push((left_mapper.parameter_node, right_mapper.parameter_node));
                stack.push((left_mapper.key_space, right_mapper.key_space));
                stack.push((left_mapper.value_expr, right_mapper.value_expr));
            }
            (SemanticNodeData::TypeOf(_), SemanticNodeData::TypeOf(_)) => {
                let (left_root, left_path) = left_data.typeof_head().expect("TypeOf carrier head");
                let (right_root, right_path) =
                    right_data.typeof_head().expect("TypeOf carrier head");
                let left_args = left_data.carrier_type_args();
                let right_args = right_data.carrier_type_args();
                if left_root != right_root
                    || left_path != right_path
                    || left_args.len() != right_args.len()
                {
                    return Some(false);
                }
                stack.extend(left_args.iter().copied().zip(right_args.iter().copied()));
            }
            // Terminal nominal carriers compare by head AND declaring
            // identity: two carriers with the same authored head shape but
            // different declaring symbols are DIFFERENT types, and the
            // equivalence proof must never certify them equal.
            (SemanticNodeData::TypeOfNominal(_), SemanticNodeData::TypeOfNominal(_)) => {
                let (left_root, left_path) = left_data.typeof_head().expect("TypeOf carrier head");
                let (right_root, right_path) =
                    right_data.typeof_head().expect("TypeOf carrier head");
                let left_identity = left_data
                    .typeof_nominal_identity()
                    .expect("TypeOfNominal carrier identity");
                let right_identity = right_data
                    .typeof_nominal_identity()
                    .expect("TypeOfNominal carrier identity");
                if left_root != right_root
                    || left_path != right_path
                    || left_identity != right_identity
                {
                    return Some(false);
                }
            }
            // A deferred shell and a terminal nominal carrier are different
            // semantic classes; an equivalence proof over that pair is a
            // mismatch, not a match.
            (SemanticNodeData::TypeOf(_), SemanticNodeData::TypeOfNominal(_))
            | (SemanticNodeData::TypeOfNominal(_), SemanticNodeData::TypeOf(_)) => {
                return Some(false);
            }
            (
                SemanticNodeData::TypeParam {
                    decl: left_decl,
                    param_index: left_index,
                    constraint: left_constraint,
                    default: left_default,
                    ..
                },
                SemanticNodeData::TypeParam {
                    decl: right_decl,
                    param_index: right_index,
                    constraint: right_constraint,
                    default: right_default,
                    ..
                },
            ) => {
                if left_decl != right_decl
                    || left_index != right_index
                    || !push_optional_proof_pair(&mut stack, *left_constraint, *right_constraint)
                    || !push_optional_proof_pair(&mut stack, *left_default, *right_default)
                {
                    return Some(false);
                }
            }
            (
                SemanticNodeData::Infer {
                    name: left_name,
                    binder: left_binder,
                },
                SemanticNodeData::Infer {
                    name: right_name,
                    binder: right_binder,
                },
            ) => {
                if left_name != right_name || left_binder != right_binder {
                    return Some(false);
                }
            }
            (
                SemanticNodeData::Conditional {
                    check: left_check,
                    extends: left_extends,
                    true_branch_ref: left_true,
                    false_branch_ref: left_false,
                    distributive: left_distributive,
                },
                SemanticNodeData::Conditional {
                    check: right_check,
                    extends: right_extends,
                    true_branch_ref: right_true,
                    false_branch_ref: right_false,
                    distributive: right_distributive,
                },
            ) => {
                if left_distributive != right_distributive {
                    return Some(false);
                }
                stack.push((*left_check, *right_check));
                stack.push((*left_extends, *right_extends));
                stack.push((*left_true, *right_true));
                stack.push((*left_false, *right_false));
            }
            (
                SemanticNodeData::Signature {
                    kind: left_kind,
                    params: left_params,
                    return_type: left_return,
                    type_parameters: left_type_parameters,
                    ..
                },
                SemanticNodeData::Signature {
                    kind: right_kind,
                    params: right_params,
                    return_type: right_return,
                    type_parameters: right_type_parameters,
                    ..
                },
            ) => {
                if left_kind != right_kind
                    || left_params.len() != right_params.len()
                    || left_type_parameters.len() != right_type_parameters.len()
                {
                    return Some(false);
                }
                for (left, right) in left_params.iter().zip(right_params.iter()) {
                    if left.name != right.name
                        || left.optional != right.optional
                        || left.rest != right.rest
                    {
                        return Some(false);
                    }
                    stack.push((left.ty, right.ty));
                }
                stack.push((*left_return, *right_return));
                for (left, right) in left_type_parameters
                    .iter()
                    .zip(right_type_parameters.iter())
                {
                    if left.name != right.name
                        || !push_optional_proof_pair(&mut stack, left.constraint, right.constraint)
                        || !push_optional_proof_pair(&mut stack, left.default, right.default)
                    {
                        return Some(false);
                    }
                }
            }
            (
                SemanticNodeData::DeclRef { identity: left },
                SemanticNodeData::DeclRef { identity: right },
            ) => {
                if left != right {
                    return Some(false);
                }
            }
            (
                SemanticNodeData::InstantiationRef {
                    base: left_base,
                    args: left_args,
                },
                SemanticNodeData::InstantiationRef {
                    base: right_base,
                    args: right_args,
                },
            ) => {
                if left_base != right_base || left_args.len() != right_args.len() {
                    return Some(false);
                }
                stack.extend(left_args.iter().copied().zip(right_args.iter().copied()));
            }
            (
                SemanticNodeData::MergedDecl { contributors: left },
                SemanticNodeData::MergedDecl {
                    contributors: right,
                },
            ) => {
                if left.len() != right.len() {
                    return Some(false);
                }
                stack.extend(left.iter().copied().zip(right.iter().copied()));
            }
            (SemanticNodeData::BareRef(_), SemanticNodeData::BareRef(_)) => {
                let (left_name, left_scope) =
                    left_data.bare_ref_head().expect("BareRef carrier head");
                let (right_name, right_scope) =
                    right_data.bare_ref_head().expect("BareRef carrier head");
                let left_args = left_data.carrier_type_args();
                let right_args = right_data.carrier_type_args();
                if left_name != right_name
                    || left_scope != right_scope
                    || left_args.len() != right_args.len()
                {
                    return Some(false);
                }
                stack.extend(left_args.iter().copied().zip(right_args.iter().copied()));
            }
            (SemanticNodeData::ImportType(_), SemanticNodeData::ImportType(_)) => {
                let left_head = left_data
                    .import_type_head()
                    .expect("ImportType carrier head");
                let right_head = right_data
                    .import_type_head()
                    .expect("ImportType carrier head");
                let left_args = left_data.carrier_type_args();
                let right_args = right_data.carrier_type_args();
                if left_head != right_head || left_args.len() != right_args.len() {
                    return Some(false);
                }
                stack.extend(left_args.iter().copied().zip(right_args.iter().copied()));
            }
            (
                SemanticNodeData::RawFallback { value: left },
                SemanticNodeData::RawFallback { value: right },
            ) => {
                if left != right {
                    return Some(false);
                }
            }
            (
                SemanticNodeData::SyntheticBinding {
                    id: left_id,
                    value_node: left_value,
                },
                SemanticNodeData::SyntheticBinding {
                    id: right_id,
                    value_node: right_value,
                },
            ) => {
                if left_id != right_id || left_value != right_value {
                    return Some(false);
                }
            }
            (
                SemanticNodeData::ObjectSpreadProgram(left),
                SemanticNodeData::ObjectSpreadProgram(right),
            ) => {
                let left_children: Vec<_> = left.child_nodes().collect();
                let right_children: Vec<_> = right.child_nodes().collect();
                if left_children.len() != right_children.len() {
                    return Some(false);
                }
                stack.extend(left_children.into_iter().zip(right_children));
            }
            (
                SemanticNodeData::InferRef {
                    name: left_name,
                    binder: left_binder,
                },
                SemanticNodeData::InferRef {
                    name: right_name,
                    binder: right_binder,
                },
            ) => {
                if left_name != right_name || left_binder != right_binder {
                    return Some(false);
                }
            }
            _ => return Some(false),
        }
    }

    Some(true)
}

/// Policy-only equivalence for ExactSymbolic authored selection.
///
/// Raw references aligned at the same structural position must retain the
/// same declaration/scope identity. Each structural occurrence is then
/// normalized lockstep by the ONE shared resolver under
/// `Published(Navigate)`. Any identity-bearing carrier that survives that
/// demand must match exactly.
pub(super) fn symbolic_projection_equivalent(
    resolved: SemanticNodeId,
    authored: SemanticNodeId,
    ctx: &PolicyCtx<'_, '_>,
) -> Option<bool> {
    let resolved_raw_refs = proof_reference_map(resolved, ctx)?;
    let authored_raw_refs = proof_reference_map(authored, ctx)?;
    if resolved_raw_refs.iter().any(|(path, identity)| {
        authored_raw_refs
            .get(path)
            .is_some_and(|other| other != identity)
    }) {
        return Some(false);
    }

    normalized_projection_shape_equivalent(resolved, authored, ctx)
}

/// Mint a symbolic-representation proof only when both bound sources retain
/// reference identity and normalize losslessly to the same projection through
/// the shared resolver.
fn validate_symbolic_equivalence(
    kind: SymbolicEquivalenceKind,
    resolved_source: &SemanticTypeSource,
    evidence: &AuthoredTypeEvidence,
    ctx: &PolicyCtx<'_, '_>,
) -> Option<SymbolicEquivalenceProof> {
    let resolved = ctx.raise_source(resolved_source)?;
    let authored_source = evidence.source().to_semantic_source();
    let authored = ctx.raise_source(&authored_source)?;
    if symbolic_projection_equivalent(resolved.node(), authored.node(), ctx) != Some(true) {
        return None;
    }

    // SAFETY: the identity-preserving normalized-projection comparison above
    // returned `Some(true)` for these exact bound sources.
    let mint = unsafe { SymbolicEquivalenceMint::new_unchecked() };
    Some(SymbolicEquivalenceProof::from_lossless_projection(
        &mint,
        kind,
        resolved_source.clone(),
        evidence.source().clone(),
    ))
}

/// If the user's authored annotation SOURCE contains imported
/// macro-participating references that the evaluator eagerly resolved into
/// structural shapes (e.g. `ButtonProps[]` became `Array<Object{href,
/// disabled, label}>`), restore the symbolic form by publishing the
/// authored source. Both sides raise through the ONE shared dispatch and
/// classify node-domain; no text is ever reparsed.
///
/// "Macro-participating" is structural — see §3.4. The set of
/// participating identities is built once in
/// `apply_component_meta_resolution_policy` and threaded via
/// `PolicyCtx::macro_participating_idents`.
///
/// **Only fires for COMPOUND raw shapes** — a bare
/// `Ref(macro-participating)` raw annotation needs no restoration: the
/// normalized macro rows publish the shallow reference carrier directly
/// (shallow-by-default). Restoring bare references here would
/// over-correct cases like `avatar: AvatarProps` where the evaluator's
/// substituted Object body is the intended public shape.
///
/// Returns `true` if the published source was replaced.
pub(super) fn macro_compound_publication_policy(
    publication: &TypePublication,
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<PublicationPolicy> {
    let evidence = publication.evidence()?;
    let resolved_source = publication.authority().source()?;
    let raw_source = evidence.source().to_semantic_source();
    let raw_node = ctx.raise_source(&raw_source)?.node();

    if is_bare_macro_participating_ref(raw_node, ctx) {
        return None;
    }

    let mut participating_refs: Vec<(String, usize)> = Vec::new();
    collect_macro_participating_refs(raw_node, ctx, &mut participating_refs);
    if participating_refs.is_empty() {
        return None;
    }

    for (name, _) in &participating_refs {
        let imported = ctx
            .locate_declaration(name)
            .is_some_and(|decl| decl.canonical_source != ctx.owner_canonical);
        if !imported {
            return None;
        }
    }

    let policy = match publication.authority().exactness()? {
        ResolutionExactness::Incomplete => PublicationPolicy::allow_authored_for_incomplete(
            PublicationPolicyReason::ImportedMacroCompound,
        ),
        ResolutionExactness::ExactConcrete | ResolutionExactness::ExactSymbolic => {
            PublicationPolicy::exact_only()
        }
    };
    let proof = validate_symbolic_equivalence(
        SymbolicEquivalenceKind::ImportedMacroCompound,
        resolved_source,
        evidence,
        ctx,
    );
    Some(match proof {
        Some(proof) => policy.with_symbolic_equivalence(proof),
        None => policy,
    })
}

/// A reference head directly at the raw root (unwrapping one alias hop)
/// whose name resolves to a macro-participating root identity.
fn is_bare_macro_participating_ref(node: SemanticNodeId, ctx: &PolicyCtx<'_, '_>) -> bool {
    if let Some((name, _)) = ctx.node_ref_head(node) {
        return ctx.is_macro_participating(name.as_str());
    }
    match ctx.node_data(node).as_deref() {
        Some(SemanticNodeData::Alias(target)) => is_bare_macro_participating_ref(*target, ctx),
        _ => false,
    }
}

/// Collect every reference head `(name, type-argument arity)` pair where
/// `name` resolves to one of the owner's macro-participating root
/// identities. Tracks both name and type-argument arity to disambiguate
/// generic vs. non-generic forms. Walks the raw node's composition spine
/// (unions / intersections / arrays / tuples / indexed-access arms /
/// reference args) — visited-guarded, since raised nodes may be shared.
fn collect_macro_participating_refs(
    root: SemanticNodeId,
    ctx: &PolicyCtx<'_, '_>,
    out: &mut Vec<(String, usize)>,
) {
    let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    let mut worklist: Vec<SemanticNodeId> = vec![root];
    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }
        if let Some((name, args)) = ctx.node_ref_head(node) {
            if ctx.is_macro_participating(name.as_str()) {
                let entry = (name, args.len());
                if !out.contains(&entry) {
                    out.push(entry);
                }
            }
            worklist.extend(args);
            continue;
        }
        match ctx.node_data(node).as_deref() {
            Some(SemanticNodeData::Alias(target)) => worklist.push(*target),
            Some(composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_))) => {
                let arms = composite.composite_members().expect("composite arm");
                worklist.extend(arms.iter().copied());
            }
            Some(SemanticNodeData::Array { element, .. }) => worklist.push(*element),
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                worklist.extend(elements.iter().map(|element| element.value));
            }
            Some(SemanticNodeData::IndexedAccess { object, index }) => {
                worklist.push(*object);
                if let IndexKey::Computed(index_node) = index {
                    worklist.push(*index_node);
                }
            }
            _ => {}
        }
    }
}

/// Whether the slot binding's authored annotation SOURCE describes an
/// indexed access that transits through an imported declaration. When
/// true, the caller restores the symbolic form from the authored source
/// and skips the expansion walk.
pub(super) fn imported_indexed_publication_policy(
    publication: &TypePublication,
    ctx: &mut PolicyCtx<'_, '_>,
) -> Option<PublicationPolicy> {
    let evidence = publication.evidence()?;
    let resolved_source = publication.authority().source()?;
    let raw_source = evidence.source().to_semantic_source();
    let hot = ctx.raise_source(&raw_source)?;
    if !raw_indexed_access_root_is_imported(hot.node(), ctx) {
        return None;
    }

    let policy = match publication.authority().exactness()? {
        ResolutionExactness::Incomplete => PublicationPolicy::allow_authored_for_incomplete(
            PublicationPolicyReason::ImportedIndexedAccess,
        ),
        ResolutionExactness::ExactConcrete | ResolutionExactness::ExactSymbolic => {
            PublicationPolicy::exact_only()
        }
    };
    let proof = validate_symbolic_equivalence(
        SymbolicEquivalenceKind::ImportedIndexedAccess,
        resolved_source,
        evidence,
        ctx,
    );
    Some(match proof {
        Some(proof) => policy.with_symbolic_equivalence(proof),
        None => policy,
    })
}

/// Returns true when the raised node is an `IndexedAccess` whose deref
/// chain transits through a reference to an imported declaration. The
/// "indexed root" is the chain starting from the indexed access's `object`
/// and the property body that the access selects from the root's
/// declaration body.
fn raw_indexed_access_root_is_imported(node: SemanticNodeId, ctx: &mut PolicyCtx<'_, '_>) -> bool {
    let Some(data) = ctx.node_data(node) else {
        return false;
    };
    let SemanticNodeData::IndexedAccess { object, index } = data.as_ref() else {
        return false;
    };
    let object = *object;
    // Index must be a string key — that is the member-path the policy can
    // statically inspect inside the root's declaration body.
    let member = match index {
        IndexKey::String(member) => member.to_string(),
        _ => return false,
    };
    // For the slot binding case we expect a reference to a declaration at
    // the object position.
    let Some((name, _)) = ctx.node_ref_head(object) else {
        return false;
    };
    let Some(DeclLookup {
        canonical_source,
        owner,
        body,
    }) = ctx.locate_declaration(name.as_str())
    else {
        return false;
    };
    // The root's declaration body must raise to an Object whose `member`
    // property value contains an imported reference (or itself resolves to
    // an imported declaration). The root's own location is not the trigger.
    let Some(body_hot) = ctx.raise_source_in_scope(&body, &canonical_source, owner) else {
        return false;
    };
    let property_value = match ctx.node_data(body_hot.node()).as_deref() {
        Some(SemanticNodeData::Object(surface)) => surface
            .positive_members()
            .iter()
            .find(|candidate| candidate.key.as_string() == Some(member.as_str()))
            .map(|candidate| candidate.value),
        _ => None,
    };
    let Some(property_value) = property_value else {
        return false;
    };
    node_contains_imported_ref(property_value, ctx)
}

/// Walks the raised node graph and returns true on the first reference
/// whose declaration resolves to an imported (non-owner) declaration.
/// References whose declarations cannot be located are ignored — they
/// cannot be proven imported. A cross-file `import("…")` carrier is, by
/// construction, a reference to an imported declaration.
fn node_contains_imported_ref(root: SemanticNodeId, ctx: &mut PolicyCtx<'_, '_>) -> bool {
    let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    let mut worklist: Vec<SemanticNodeId> = vec![root];
    while let Some(node) = worklist.pop() {
        if !visited.insert(node) {
            continue;
        }
        if let Some((name, args)) = ctx.node_ref_head(node) {
            if let Some(DeclLookup {
                canonical_source,
                owner,
                ..
            }) = ctx.locate_declaration(name.as_str())
            {
                if canonical_source != ctx.owner_canonical || owner != ctx.owner {
                    return true;
                }
            }
            worklist.extend(args);
            continue;
        }
        let Some(data) = ctx.node_data(node) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::ImportType(_) => return true,
            SemanticNodeData::Alias(target) => worklist.push(*target),
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                let arms = composite.composite_members().expect("composite arm");
                worklist.extend(arms.iter().copied());
            }
            SemanticNodeData::Array { element, .. } | SemanticNodeData::KeyOf { base: element } => {
                worklist.push(*element)
            }
            SemanticNodeData::Tuple { elements, .. } => {
                worklist.extend(elements.iter().map(|element| element.value));
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                worklist.push(*object);
                if let IndexKey::Computed(index_node) = index {
                    worklist.push(*index_node);
                }
            }
            SemanticNodeData::Object(surface) => {
                worklist.extend(surface.positive_members().iter().map(|member| member.value));
                worklist.extend(surface.call_signatures.iter().copied());
                worklist.extend(surface.construct_signatures.iter().copied());
                for signature in surface.index_signatures.iter() {
                    worklist.push(signature.key_type);
                    worklist.push(signature.value_type);
                }
            }
            SemanticNodeData::Signature {
                params,
                return_type,
                ..
            } => {
                worklist.extend(params.iter().map(|param| param.ty));
                worklist.push(*return_type);
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                worklist.push(*check);
                worklist.push(*extends);
                worklist.push(*true_branch_ref);
                worklist.push(*false_branch_ref);
            }
            SemanticNodeData::Mapped { source, .. } => worklist.push(*source),
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                worklist.extend(expressions.iter().copied());
            }
            SemanticNodeData::MergedDecl { contributors } => {
                worklist.extend(contributors.iter().copied());
            }
            _ => {}
        }
    }
    false
}
