//! Heap a memo family's key keeps alive.
//!
//! A [`FamilyKey`] sits inline in the family map and in the retention
//! budget's ledger, but most of its identity lives behind pointers:
//! argument and path slices, declaration and file names, boxed composite
//! identities, interned relation and call records, root attachments.
//! [`family_key_heap_bytes`] walks every one of those allocations under the
//! conventions of [`verter_session_query::retention::allocation`]: a `Box`
//! costs its pointee's block plus the pointee's own heap, each `Arc`
//! allocation (slices, names, interned records, root owners) is counted
//! once per [`AliasSet`], and a field whose type is `Copy` owns no heap —
//! [`inline`] holds that as a bound, so a field that gains an allocation
//! stops compiling here instead of going uncounted.
//!
//! What the walk does not count: the graph nodes `SemanticNodeId` operands
//! name (the arena's), the tables behind a signature descriptor or call
//! substitution id (the signature epoch's), and inline fields (the block
//! of whatever embeds the key).
//!
//! Every match is exhaustive and destructures every field it can see, so a
//! new variant or field must be dispositioned here; a sealed key (a
//! lowering locator, an instantiation key, an authored operand) is
//! measured through its accessors, and a key whose fields are private to
//! its own module measures itself there.
//!
//! [`query_key_heap_bytes`] applies the same walk to a full
//! [`SemanticQueryKey`], for the prepared identity a cost receipt shares.

use std::sync::Arc;

use verter_session_query::retention::allocation::{
    arc_slice_block_bytes, arc_str_bytes, arc_str_list_bytes, box_bytes, heap_block_bytes, AliasSet,
};
use verter_type_expr::locators::{AuthoredAnchor, AuthoredAugmentationScope, AuthoredBodyLocator};

use super::family::FamilyKey;
use crate::semantic_query::allocation_footprint::OwnedHeap as _;
use crate::semantic_query::{
    ApparentDemandScope, CallArgKey, Demand, EvalPolicy, FlowFunctionSlotIdentity,
    FlowInputContext, FlowReturnContext, FlowReturnKey, InferenceContextKey, IntersectionInputRef,
    MaterializationCycleGateKey, ObjectProjectionSelector, ProgramPointId, ProjectionDemand,
    RelateMemoKey, RelationContext, ResolveCallContext, ResolveCallKey, ResolveDeclKey,
    ResolvedDeclSlotIdentity, ReturnProjectionDemand, ScopeId, SemanticContextId, SemanticQueryKey,
    ValueRootKey, ValueRootSlotIdentity,
};

/// Every heap allocation reachable from `key` beyond its inline bytes, by
/// capacity, each distinct `Arc` allocation once per `aliases`.
///
/// Measuring a key a second time under the same `aliases` counts exactly
/// what a CLONE of it allocates afresh: a clone shares every `Arc` the
/// first measurement recorded and deep-copies only its boxes (and the
/// owned bytes inside them).
/// The heap a clone of `key` allocates afresh: its boxes and their
/// contents, every `Arc` being shared with `key`.
///
/// A clone shares the key's owned names too: they are handed back with the
/// figure, for the clone's holder to hold.
pub(super) fn family_key_clone_heap_bytes(
    key: &FamilyKey,
) -> verter_session_query::retention::Footprint {
    let mut aliases = AliasSet::new();
    let _ = family_key_heap_bytes(key, &mut aliases);
    let bytes = family_key_heap_bytes(key, &mut aliases);
    verter_session_query::retention::Footprint {
        bytes,
        handles: aliases.take_handles(),
    }
}

pub(super) fn family_key_heap_bytes(key: &FamilyKey, aliases: &mut AliasSet) -> usize {
    match key {
        FamilyKey::ResolveDecl(key) => resolve_decl_heap_bytes(key, aliases),
        FamilyKey::Instantiate {
            base,
            args,
            resolve_env_hash,
            body_source,
            provenance,
            merge_role,
            vue_heritage_policy,
        } => {
            decl_slot_heap_bytes(base, aliases)
                + copy_slice_bytes(args, aliases)
                + inline(resolve_env_hash)
                + inline(body_source)
                + inline(provenance)
                + inline(merge_role)
                + inline(vue_heritage_policy)
        }
        FamilyKey::InstantiateAuthored { identity } => {
            box_block_bytes(identity) + identity.key_heap_bytes(aliases)
        }
        FamilyKey::ProjectMember { base, member } => inline(base) + text_bytes(member, aliases),
        FamilyKey::IndexedAccess { base, index } => inline(base) + index.owned_heap_bytes(aliases),
        FamilyKey::KeyOf {
            base,
            provenance,
            merge_role,
            vue_heritage_policy,
        } => inline(base) + inline(provenance) + inline(merge_role) + inline(vue_heritage_policy),
        FamilyKey::MappedType {
            source,
            mapper,
            provenance,
            merge_role,
            vue_heritage_policy,
        } => {
            inline(source)
                + mapper_heap_bytes(mapper)
                + inline(provenance)
                + inline(merge_role)
                + inline(vue_heritage_policy)
        }
        FamilyKey::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
            pending,
        } => {
            pending_heap_bytes(pending, aliases)
                + inline(check)
                + inline(extends)
                + inline(true_branch)
                + inline(false_branch)
                + inline(distributive)
        }
        FamilyKey::TypeOf {
            value_root,
            path,
            resolve_env_hash,
            provenance,
            merge_role,
            vue_heritage_policy,
        } => {
            value_root_heap_bytes(value_root, aliases)
                + box_block_bytes(path)
                + text_bytes(path, aliases)
                + inline(resolve_env_hash)
                + inline(provenance)
                + inline(merge_role)
                + inline(vue_heritage_policy)
        }
        FamilyKey::ReduceUnion {
            members,
            nullability,
        } => copy_slice_bytes(members, aliases) + inline(nullability),
        FamilyKey::ReduceIntersection {
            input,
            purpose,
            context,
        } => {
            intersection_input_heap_bytes(input, aliases)
                + inline(purpose)
                + semantic_context_heap_bytes(context, aliases)
        }
        FamilyKey::ProjectObjectSpread { identity } => {
            box_block_bytes(identity) + identity.key_heap_bytes(aliases)
        }
        FamilyKey::ProjectPath {
            base,
            path,
            provenance,
            merge_role,
            vue_heritage_policy,
        } => {
            inline(base)
                + path.owned_heap_bytes(aliases)
                + inline(provenance)
                + inline(merge_role)
                + inline(vue_heritage_policy)
        }
        FamilyKey::ResolveMacroPayload {
            owner,
            macro_index,
            macro_kind,
            type_args,
            resolve_env_hash,
        } => {
            decl_slot_heap_bytes(owner, aliases)
                + copy_slice_bytes(type_args, aliases)
                + inline(macro_index)
                + inline(macro_kind)
                + inline(resolve_env_hash)
        }
        FamilyKey::ResolveClassSurface {
            decl_slot,
            type_args,
            side,
            parse_env_hash,
            resolve_env_hash,
        } => {
            decl_slot_heap_bytes(decl_slot, aliases)
                + copy_slice_bytes(type_args, aliases)
                + inline(side)
                + inline(parse_env_hash)
                + inline(resolve_env_hash)
        }
        FamilyKey::ResolveAmbientNamespace {
            namespace_slot,
            type_args,
            parse_env_hash,
            resolve_env_hash,
        } => {
            decl_slot_heap_bytes(namespace_slot, aliases)
                + copy_slice_bytes(type_args, aliases)
                + inline(parse_env_hash)
                + inline(resolve_env_hash)
        }
        FamilyKey::ResolveEnum {
            enum_slot,
            resolve_env_hash,
        } => decl_slot_heap_bytes(enum_slot, aliases) + inline(resolve_env_hash),
        FamilyKey::ResolveOverloadSet {
            callee,
            kind,
            type_args,
            resolve_env_hash,
        } => {
            copy_slice_bytes(type_args, aliases)
                + inline(callee)
                + inline(kind)
                + inline(resolve_env_hash)
        }
        FamilyKey::ClassifyBroadRuntime { subject, context } => {
            box_block_bytes(subject) + broad_subject_heap_bytes(subject, aliases) + inline(context)
        }
        FamilyKey::Relate { key } => key.key_heap_bytes(aliases),
        FamilyKey::ApparentType {
            base,
            type_env_hash,
            lib_env_hash,
            project_identity,
            demand_scope,
        } => {
            apparent_scope_heap_bytes(demand_scope, aliases)
                + inline(base)
                + inline(type_env_hash)
                + inline(lib_env_hash)
                + inline(project_identity)
        }
        FamilyKey::TemplateLiteralReduce {
            pattern,
            args,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
        } => {
            text_bytes(pattern, aliases)
                + copy_slice_bytes(args, aliases)
                + inline(resolve_env_hash)
                + inline(type_env_hash)
                + inline(lib_env_hash)
                + inline(project_identity)
        }
        FamilyKey::FlowNarrowingAt {
            point,
            flow,
            parse_env_hash,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
            substitution,
        } => {
            program_point_heap_bytes(point, aliases)
                + flow.key_heap_bytes(aliases)
                + inline(parse_env_hash)
                + inline(resolve_env_hash)
                + inline(type_env_hash)
                + inline(lib_env_hash)
                + inline(project_identity)
                + inline(substitution)
        }
        FamilyKey::ContextualTypeAt {
            point,
            contextual,
            parse_env_hash,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
            substitution,
        } => {
            program_point_heap_bytes(point, aliases)
                + contextual.key_heap_bytes(aliases)
                + inline(parse_env_hash)
                + inline(resolve_env_hash)
                + inline(type_env_hash)
                + inline(lib_env_hash)
                + inline(project_identity)
                + inline(substitution)
        }
        FamilyKey::LowerLocator { key } => {
            box_block_bytes(key) + locator_key_heap_bytes(key, aliases)
        }
        FamilyKey::ClassifyMaterializationCycleGate { key } => {
            box_block_bytes(key) + cycle_gate_key_heap_bytes(key, aliases)
        }
        FamilyKey::ClassifyTruthinessDomain { subject } => inline(subject),
        FamilyKey::AwaitedNormalize {
            operand,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
        }
        | FamilyKey::AsyncReturnPayload {
            operand,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
        } => {
            inline(operand)
                + inline(resolve_env_hash)
                + inline(type_env_hash)
                + inline(lib_env_hash)
                + inline(project_identity)
        }
        FamilyKey::SignaturesOfType {
            subject,
            kind,
            context,
        } => inline(subject) + inline(kind) + semantic_context_heap_bytes(context, aliases),
        FamilyKey::ReadSignatureResult { key } => read_signature_key_heap_bytes(key, aliases),
        FamilyKey::FlowReturn { key } => {
            box_block_bytes(key) + flow_return_key_heap_bytes(key, aliases)
        }
        FamilyKey::ResolveCall { key } => key.key_heap_bytes(aliases),
    }
}

/// A field whose type is `Copy` owns no heap: the bound is the proof.
pub(super) const fn inline<T: Copy>(_: &T) -> usize {
    0
}

/// An `Arc` slice of heap-free elements: its block alone.
pub(super) fn copy_slice_bytes<S: KeySlice + ?Sized>(slice: &S, aliases: &mut AliasSet) -> usize {
    slice.key_slice_bytes(aliases)
}

/// A key's slice of inline elements: a raw shared slice the key counts, or
/// an owned slice charged by its own cell.
pub(super) trait KeySlice {
    fn key_slice_bytes(&self, aliases: &mut AliasSet) -> usize;
}

impl<T: Copy> KeySlice for Arc<[T]> {
    fn key_slice_bytes(&self, aliases: &mut AliasSet) -> usize {
        arc_slice_block_bytes(self, aliases).unwrap_or(0)
    }
}

impl<T: Copy + verter_session_query::retention::owned::Measured> KeySlice
    for verter_type_expr::OwnedSlice<T>
{
    fn key_slice_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.offer(aliases)
    }
}

/// A key's text: a raw shared string the key counts, or an owned one
/// charged by its own cell.
pub(super) trait KeyText {
    fn key_text_bytes(&self, aliases: &mut AliasSet) -> usize;
}

impl KeyText for Arc<str> {
    fn key_text_bytes(&self, aliases: &mut AliasSet) -> usize {
        arc_str_bytes(self, aliases)
    }
}

impl KeyText for verter_type_expr::OwnedStr {
    fn key_text_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.offer(aliases)
    }
}

impl KeyText for Arc<[Arc<str>]> {
    fn key_text_bytes(&self, aliases: &mut AliasSet) -> usize {
        arc_str_list_bytes(self, aliases)
    }
}

impl KeyText for verter_type_expr::OwnedSlice<verter_type_expr::OwnedStr> {
    fn key_text_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.offer(aliases)
    }
}

impl<T: KeyText + ?Sized> KeyText for Box<T> {
    fn key_text_bytes(&self, aliases: &mut AliasSet) -> usize {
        (**self).key_text_bytes(aliases)
    }
}

/// The heap a key's text keeps, by its kind ([`KeyText`]).
pub(super) fn text_bytes<T: KeyText + ?Sized>(text: &T, aliases: &mut AliasSet) -> usize {
    text.key_text_bytes(aliases)
}

/// The block a `Box` holds its pointee in. Never aliased: a clone of the
/// box deep-copies it.
// Takes the box itself: the allocation measured is the box's, so only a
// boxed field may be passed.
#[allow(clippy::borrowed_box)]
fn box_block_bytes<T>(_: &Box<T>) -> usize {
    box_bytes::<T>()
}

/// A declaration slot's file and symbol names.
pub(super) fn decl_slot_heap_bytes(
    slot: &ResolvedDeclSlotIdentity,
    aliases: &mut AliasSet,
) -> usize {
    let ResolvedDeclSlotIdentity {
        defining_canonical,
        owner,
        merged_symbol_name,
        symbol_space,
        env,
    } = slot;
    text_bytes(defining_canonical, aliases)
        + text_bytes(merged_symbol_name, aliases)
        + inline(owner)
        + inline(symbol_space)
        + inline(env)
}

fn scope_heap_bytes(scope: &ScopeId, aliases: &mut AliasSet) -> usize {
    let ScopeId {
        canonical_id,
        owner,
        local_scope,
        binder_scope_id,
    } = scope;
    text_bytes(canonical_id, aliases)
        + inline(owner)
        + inline(local_scope)
        + inline(binder_scope_id)
}

fn resolve_decl_heap_bytes(key: &ResolveDeclKey, aliases: &mut AliasSet) -> usize {
    let ResolveDeclKey { scope, name } = key;
    scope_heap_bytes(scope, aliases) + text_bytes(name, aliases)
}

fn value_root_heap_bytes(slot: &ValueRootSlotIdentity, aliases: &mut AliasSet) -> usize {
    let ValueRootSlotIdentity {
        root: ValueRootKey { scope, name },
        project_identity,
        type_env_hash,
        lib_env_hash,
    } = slot;
    scope_heap_bytes(scope, aliases)
        + text_bytes(name, aliases)
        + inline(project_identity)
        + inline(type_env_hash)
        + inline(lib_env_hash)
}

fn program_point_heap_bytes(point: &ProgramPointId, aliases: &mut AliasSet) -> usize {
    let ProgramPointId {
        canonical_id,
        offset,
    } = point;
    text_bytes(canonical_id, aliases) + inline(offset)
}

/// A context identity is a handle to a self-charged interned record: it
/// owns no heap of its own.
fn semantic_context_heap_bytes(context: &SemanticContextId, _aliases: &mut AliasSet) -> usize {
    let _: &SemanticContextId = context;
    0
}

/// A compact intersection input owns nothing; a recipe input owns its
/// interned recipe graph.
fn intersection_input_heap_bytes(input: &IntersectionInputRef, aliases: &mut AliasSet) -> usize {
    match input {
        IntersectionInputRef::Empty => 0,
        IntersectionInputRef::Unary(node) => inline(node),
        IntersectionInputRef::Binary(left, right) => inline(left) + inline(right),
        IntersectionInputRef::Recipe(recipe) => recipe.key_heap_bytes(aliases),
    }
}

/// An authored locator's anchor names and its body path or augmentation
/// specifier.
fn authored_locator_heap_bytes(locator: &AuthoredBodyLocator, aliases: &mut AliasSet) -> usize {
    use verter_type_expr::locators::{
        AugmentationBodyLocator, JsdocTypedefBodyLocator, MacroPayloadLocator, TypeBodySlot,
    };
    match locator {
        AuthoredBodyLocator::DeclBody(TypeBodySlot { anchor, path }) => {
            anchor_heap_bytes(anchor, aliases) + copy_slice_bytes(path, aliases)
        }
        AuthoredBodyLocator::AugmentationBody(AugmentationBodyLocator {
            anchor,
            scope,
            path,
        }) => {
            let scope = match scope {
                AuthoredAugmentationScope::Global => 0,
                AuthoredAugmentationScope::Module { specifier } => text_bytes(specifier, aliases),
            };
            anchor_heap_bytes(anchor, aliases) + scope + copy_slice_bytes(path, aliases)
        }
        AuthoredBodyLocator::JsdocTypedefBody(JsdocTypedefBodyLocator { anchor, path }) => {
            anchor_heap_bytes(anchor, aliases) + copy_slice_bytes(path, aliases)
        }
        AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
            anchor,
            macro_index,
            payload,
        }) => anchor_heap_bytes(anchor, aliases) + inline(macro_index) + inline(payload),
    }
}

fn anchor_heap_bytes(anchor: &AuthoredAnchor, aliases: &mut AliasSet) -> usize {
    let AuthoredAnchor {
        canonical_id,
        owner,
        symbol,
        space,
    } = anchor;
    text_bytes(canonical_id, aliases) + text_bytes(symbol, aliases) + inline(owner) + inline(space)
}

/// A sealed authored operand: its locator. The operand's lexical scope is
/// derived from that locator and shares its anchor's file name; its binder
/// frame and split environment are inline.
pub(super) fn authored_operand_heap_bytes(
    authored: &crate::semantic_query::operand::AuthoredOperandQueryIdentity,
    aliases: &mut AliasSet,
) -> usize {
    authored_locator_heap_bytes(authored.locator(), aliases)
}

/// A force precision's residual path, when it has one.
pub(super) fn force_projection_heap_bytes(
    projection: &crate::semantic_query::operand::SemanticOperandForceProjection,
    aliases: &mut AliasSet,
) -> usize {
    use crate::semantic_query::operand::SemanticOperandForceProjection;
    match projection {
        SemanticOperandForceProjection::WholeSurface
        | SemanticOperandForceProjection::KeyDomain => 0,
        SemanticOperandForceProjection::Path(path) => path.owned_heap_bytes(aliases),
    }
}

/// An object-projection selector's property keys.
pub(super) fn object_selector_heap_bytes(
    selector: &ObjectProjectionSelector,
    aliases: &mut AliasSet,
) -> usize {
    match selector {
        ObjectProjectionSelector::Key(key) => key.owned_heap_bytes(aliases),
        ObjectProjectionSelector::RelationShape(keys) => keys.owned_heap_bytes(aliases),
        ObjectProjectionSelector::Surface | ObjectProjectionSelector::ExcessEligibility => 0,
        ObjectProjectionSelector::IndexDomain(domain)
        | ObjectProjectionSelector::EnumerableValueEnvelope(domain) => inline(domain),
        ObjectProjectionSelector::Signature(kind) => inline(kind),
    }
}

fn relation_context_heap_bytes(context: &RelationContext, aliases: &mut AliasSet) -> usize {
    let RelationContext {
        resolve_env_hash,
        type_env_hash,
        lib_env_hash,
        project_identity,
        substitution,
        projection_reduction,
        semantic_context,
    } = context;
    semantic_context_heap_bytes(semantic_context, aliases)
        + inline(resolve_env_hash)
        + inline(type_env_hash)
        + inline(lib_env_hash)
        + inline(project_identity)
        + inline(substitution)
        + inline(projection_reduction)
}

fn inference_context_heap_bytes(context: &InferenceContextKey, aliases: &mut AliasSet) -> usize {
    let InferenceContextKey {
        inferable_params,
        variance_phase,
        pass_kind,
        candidate_priority,
        no_infer_mask,
        const_param_policy,
        contextual_inference_mode,
    } = context;
    inferable_params.key_heap_bytes(aliases)
        + inline(variance_phase)
        + inline(pass_kind)
        + inline(candidate_priority)
        + inline(no_infer_mask)
        + inline(const_param_policy)
        + inline(contextual_inference_mode)
}

/// The payload an interned relation record holds.
pub(super) fn relate_payload_heap_bytes(key: &RelateMemoKey, aliases: &mut AliasSet) -> usize {
    let RelateMemoKey {
        source,
        target,
        relation,
        policy,
        source_freshness,
        inference_context,
        context,
    } = key;
    optional_inference_heap_bytes(inference_context, aliases)
        + relation_context_heap_bytes(context, aliases)
        + inline(source)
        + inline(target)
        + inline(relation)
        + inline(policy)
        + inline(source_freshness)
}

fn optional_inference_heap_bytes(
    context: &Option<InferenceContextKey>,
    aliases: &mut AliasSet,
) -> usize {
    context.as_ref().map_or(0, |inference| {
        inference_context_heap_bytes(inference, aliases)
    })
}

/// A mapper is node ids and modifiers.
fn mapper_heap_bytes(mapper: &crate::semantic_query::MapperKey) -> usize {
    let crate::semantic_query::MapperKey {
        parameter_node,
        key_space,
        value_expr,
        optionality,
        readonly,
        name_remap,
        kind,
        over_type_variable,
        modifiers_type,
    } = mapper;
    inline(parameter_node)
        + inline(key_space)
        + inline(value_expr)
        + inline(optionality)
        + inline(readonly)
        + inline(name_remap)
        + inline(kind)
        + inline(over_type_variable)
        + inline(modifiers_type)
}

/// A conditional's shared pending-substitution record and its frames.
fn pending_heap_bytes(
    pending: &Option<
        verter_session_query::retention::owned::Owned<
            crate::semantic_query::ConditionalPendingSubstitution,
        >,
    >,
    aliases: &mut AliasSet,
) -> usize {
    pending.as_ref().map_or(0, |pending| {
        // An owned allocation the key shares with the payload: charged by
        // its own cell.
        pending.offer(aliases)
    })
}

/// A broad-runtime subject's owner slot and member route.
fn broad_subject_heap_bytes(
    subject: &crate::locator_identity::BroadRuntimeSubjectLocator,
    aliases: &mut AliasSet,
) -> usize {
    use crate::locator_identity::BroadRuntimeSubjectRoute;
    let route = match subject.route() {
        BroadRuntimeSubjectRoute::Payload => 0,
        BroadRuntimeSubjectRoute::Member(name) => text_bytes(name, aliases),
    };
    decl_slot_heap_bytes(subject.owner(), aliases) + inline(&subject.macro_index()) + route
}

fn apparent_scope_heap_bytes(scope: &ApparentDemandScope, aliases: &mut AliasSet) -> usize {
    match scope {
        ApparentDemandScope::Anchored => 0,
        ApparentDemandScope::Rootless { canonical } => text_bytes(canonical, aliases),
    }
}

/// A lowering key's slot and authored locator.
fn locator_key_heap_bytes(
    key: &crate::locator_identity::LocatorLoweringKey,
    aliases: &mut AliasSet,
) -> usize {
    decl_slot_heap_bytes(key.slot(), aliases)
        + authored_locator_heap_bytes(key.locator(), aliases)
        + inline(&key.parse_env_hash())
        + inline(&key.resolve_env_hash())
}

fn cycle_gate_key_heap_bytes(key: &MaterializationCycleGateKey, aliases: &mut AliasSet) -> usize {
    let MaterializationCycleGateKey {
        root,
        parse_env_hash,
        resolve_env_hash,
    } = key;
    decl_slot_heap_bytes(root, aliases) + inline(parse_env_hash) + inline(resolve_env_hash)
}

/// A signature-result demand: ids into the signature epoch's tables and
/// its semantic context.
fn read_signature_key_heap_bytes(
    key: &crate::signature_kernel::ReadSignatureResultKey,
    aliases: &mut AliasSet,
) -> usize {
    let crate::signature_kernel::ReadSignatureResultKey {
        descriptor,
        call_substitution,
        projection,
        evaluation,
        semantic_context,
    } = key;
    inline(descriptor)
        + inline(call_substitution)
        + inline(projection)
        + inline(evaluation)
        + semantic_context_heap_bytes(semantic_context, aliases)
}

/// The payload an interned call-resolution record holds.
pub(super) fn resolve_call_payload_heap_bytes(
    key: &ResolveCallKey,
    aliases: &mut AliasSet,
) -> usize {
    let ResolveCallKey {
        point,
        callee,
        kind,
        receiver,
        args,
        explicit_type_args,
        flow,
        context,
    } = key;
    let arguments = arc_slice_block_bytes(args, aliases).map_or(0, |block| {
        block
            + args
                .iter()
                .map(|argument| call_argument_heap_bytes(argument, aliases))
                .sum::<usize>()
    });
    let ResolveCallContext {
        parse_env_hash,
        resolve_env_hash,
        type_env_hash,
        lib_env_hash,
        project_identity,
        substitution,
    } = context;
    program_point_heap_bytes(point, aliases)
        + arguments
        + copy_slice_bytes(explicit_type_args, aliases)
        + flow.key_heap_bytes(aliases)
        + substitution.key_heap_bytes(aliases)
        + inline(callee)
        + inline(kind)
        + inline(receiver)
        + inline(parse_env_hash)
        + inline(resolve_env_hash)
        + inline(type_env_hash)
        + inline(lib_env_hash)
        + inline(project_identity)
}

fn call_argument_heap_bytes(argument: &CallArgKey, aliases: &mut AliasSet) -> usize {
    match argument {
        CallArgKey::Eager {
            ty,
            spread,
            literal_mode,
            context_sensitive,
            const_view,
            first_pass,
        } => {
            inline(ty)
                + inline(spread)
                + inline(literal_mode)
                + inline(context_sensitive)
                + inline(const_view)
                + inline(first_pass)
        }
        CallArgKey::ProgramExpression {
            point,
            spread,
            literal_mode,
            context_sensitive,
        } => {
            program_point_heap_bytes(point, aliases)
                + inline(spread)
                + inline(literal_mode)
                + inline(context_sensitive)
        }
    }
}

/// A flow-return identity: its function slot, argument and contextual
/// parameter lists, type substitution, demanded path, and the result
/// contract's canonical bytes. The contract exposes only its bytes'
/// length — a clone allocates exactly that — so it is counted by length.
fn flow_return_key_heap_bytes(key: &FlowReturnKey, aliases: &mut AliasSet) -> usize {
    let FlowReturnKey {
        function,
        normalized_type_args,
        context,
        demand,
        input,
        result_contract,
    } = key;
    let FlowFunctionSlotIdentity {
        declaration_slot,
        function_part,
        overload_ordinal,
    } = function;
    let part = match function_part {
        verter_type_expr::facts::FunctionPartIdentity::DeclarationBody
        | verter_type_expr::facts::FunctionPartIdentity::Initializer => 0,
        verter_type_expr::facts::FunctionPartIdentity::Member { member_path } => {
            copy_slice_bytes(member_path, aliases)
        }
        verter_type_expr::facts::FunctionPartIdentity::Other { ordinal } => inline(ordinal),
    };
    let FlowReturnContext {
        parse_env_hash,
        resolve_env_hash,
        type_env_hash,
        lib_env_hash,
        project_identity,
        result_evaluation,
        type_substitution,
        policy,
    } = context;
    let ReturnProjectionDemand {
        point: Demand {
            projection,
            policy: demand_policy,
        },
    } = demand;
    let FlowInputContext {
        contextual_parameters,
    } = input;
    decl_slot_heap_bytes(declaration_slot, aliases)
        + part
        + inline(overload_ordinal)
        + copy_slice_bytes(normalized_type_args, aliases)
        + type_substitution.key_heap_bytes(aliases)
        + projection_demand_heap_bytes(projection, aliases)
        + eval_policy_heap_bytes(demand_policy)
        + copy_slice_bytes(contextual_parameters, aliases)
        + heap_block_bytes(result_contract.canonical_bytes().len())
        + inline(parse_env_hash)
        + inline(resolve_env_hash)
        + inline(type_env_hash)
        + inline(lib_env_hash)
        + inline(project_identity)
        + inline(result_evaluation)
        + inline(policy)
}

fn projection_demand_heap_bytes(demand: &ProjectionDemand, aliases: &mut AliasSet) -> usize {
    let ProjectionDemand {
        path,
        facets,
        member_demand,
        call_signatures,
        construct_signatures,
        index_signatures,
        display_needs,
    } = demand;
    path.owned_heap_bytes(aliases)
        + inline(facets)
        + inline(member_demand)
        + inline(call_signatures)
        + inline(construct_signatures)
        + inline(index_signatures)
        + inline(display_needs)
}

fn eval_policy_heap_bytes(policy: &EvalPolicy) -> usize {
    let EvalPolicy {
        alias_preservation,
        normalization_depth,
        generic_open,
        operator_reduction,
        carrier_stop,
        surface_role,
        provenance,
        merge_role,
    } = policy;
    inline(alias_preservation)
        + inline(normalization_depth)
        + inline(generic_open)
        + inline(operator_reduction)
        + inline(carrier_stop)
        + inline(surface_role)
        + inline(provenance)
        + inline(merge_role)
}

/// Every heap allocation reachable from a query key beyond its inline
/// bytes, each distinct `Arc` allocation once per `aliases`. A prepared
/// identity holds a key beside the family projected from it; the family
/// shares the key's slices and names, so measuring both under one
/// `aliases` counts each allocation once.
pub(super) fn query_key_heap_bytes(key: &SemanticQueryKey, aliases: &mut AliasSet) -> usize {
    match key {
        SemanticQueryKey::ResolveDecl(key) => resolve_decl_heap_bytes(key, aliases),
        SemanticQueryKey::Instantiate(key) => {
            let source = key.source().authored_source().map_or(0, |source| {
                box_bytes::<crate::semantic_query::AuthoredOperandSource>()
                    + authored_operand_heap_bytes(source.identity(), aliases)
                    + force_projection_heap_bytes(source.projection(), aliases)
            });
            decl_slot_heap_bytes(key.base(), aliases)
                + copy_slice_bytes(key.args(), aliases)
                + source
                + inline(&key.context())
        }
        SemanticQueryKey::ProjectMember { base, member, mode } => {
            inline(base) + text_bytes(member, aliases) + inline(mode)
        }
        SemanticQueryKey::IndexedAccess { base, index, mode } => {
            inline(base) + index.owned_heap_bytes(aliases) + inline(mode)
        }
        SemanticQueryKey::KeyOf { base, context } => inline(base) + inline(context),
        SemanticQueryKey::MappedType {
            source,
            mapper,
            context,
        } => inline(source) + mapper_heap_bytes(mapper) + inline(context),
        SemanticQueryKey::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
            pending,
        } => {
            pending_heap_bytes(pending, aliases)
                + inline(check)
                + inline(extends)
                + inline(true_branch)
                + inline(false_branch)
                + inline(distributive)
        }
        SemanticQueryKey::TypeOf {
            value_root,
            path,
            context,
        } => {
            value_root_heap_bytes(value_root, aliases) + text_bytes(path, aliases) + inline(context)
        }
        SemanticQueryKey::ReduceUnion {
            members,
            nullability,
        } => copy_slice_bytes(members, aliases) + inline(nullability),
        SemanticQueryKey::ReduceIntersection {
            input,
            purpose,
            context,
        } => {
            intersection_input_heap_bytes(input, aliases)
                + inline(purpose)
                + semantic_context_heap_bytes(context, aliases)
        }
        SemanticQueryKey::ProjectObjectSpread {
            program,
            selector,
            context,
        } => inline(program) + object_selector_heap_bytes(selector, aliases) + inline(context),
        SemanticQueryKey::ProjectPath {
            base,
            path,
            context,
        } => inline(base) + path.owned_heap_bytes(aliases) + inline(context),
        SemanticQueryKey::Relate {
            source,
            target,
            relation,
            policy,
            source_freshness,
            inference_context,
            context,
        } => {
            optional_inference_heap_bytes(inference_context, aliases)
                + relation_context_heap_bytes(context, aliases)
                + inline(source)
                + inline(target)
                + inline(relation)
                + inline(policy)
                + inline(source_freshness)
        }
        SemanticQueryKey::ResolveMacroPayload {
            owner,
            macro_index,
            macro_kind,
            type_args,
            context,
        } => {
            decl_slot_heap_bytes(owner, aliases)
                + copy_slice_bytes(type_args, aliases)
                + inline(macro_index)
                + inline(macro_kind)
                + inline(context)
        }
        SemanticQueryKey::ResolveClassSurface {
            decl_slot,
            type_args,
            side,
            context,
        } => {
            decl_slot_heap_bytes(decl_slot, aliases)
                + copy_slice_bytes(type_args, aliases)
                + inline(side)
                + inline(context)
        }
        SemanticQueryKey::ResolveAmbientNamespace {
            namespace_slot,
            type_args,
            context,
        } => {
            decl_slot_heap_bytes(namespace_slot, aliases)
                + copy_slice_bytes(type_args, aliases)
                + inline(context)
        }
        SemanticQueryKey::ResolveEnum { enum_slot, context } => {
            decl_slot_heap_bytes(enum_slot, aliases) + inline(context)
        }
        SemanticQueryKey::ResolveOverloadSet {
            callee,
            kind,
            type_args,
            context,
        } => copy_slice_bytes(type_args, aliases) + inline(callee) + inline(kind) + inline(context),
        SemanticQueryKey::ClassifyBroadRuntime { subject, context } => {
            broad_subject_heap_bytes(subject, aliases) + inline(context)
        }
        SemanticQueryKey::ApparentType { base, context } => {
            let crate::semantic_query::ApparentTypeContext {
                type_env_hash,
                lib_env_hash,
                project_identity,
                demand_scope,
            } = context;
            apparent_scope_heap_bytes(demand_scope, aliases)
                + inline(base)
                + inline(type_env_hash)
                + inline(lib_env_hash)
                + inline(project_identity)
        }
        SemanticQueryKey::TemplateLiteralReduce {
            pattern,
            args,
            context,
        } => text_bytes(pattern, aliases) + copy_slice_bytes(args, aliases) + inline(context),
        SemanticQueryKey::FlowNarrowingAt {
            point,
            flow,
            context,
        } => {
            program_point_heap_bytes(point, aliases)
                + flow.key_heap_bytes(aliases)
                + inline(context)
        }
        SemanticQueryKey::ContextualTypeAt {
            point,
            contextual,
            context,
        } => {
            program_point_heap_bytes(point, aliases)
                + contextual.key_heap_bytes(aliases)
                + inline(context)
        }
        SemanticQueryKey::LowerLocator { key } => locator_key_heap_bytes(key, aliases),
        SemanticQueryKey::ClassifyMaterializationCycleGate(key) => {
            cycle_gate_key_heap_bytes(key, aliases)
        }
        SemanticQueryKey::FlowReturn(key) => {
            box_block_bytes(key) + flow_return_key_heap_bytes(key, aliases)
        }
        SemanticQueryKey::ResolveCall(key) => {
            box_block_bytes(key) + resolve_call_payload_heap_bytes(key, aliases)
        }
        SemanticQueryKey::ClassifyTruthinessDomain { subject } => inline(subject),
        SemanticQueryKey::SignaturesOfType {
            subject,
            kind,
            context,
        } => inline(subject) + inline(kind) + semantic_context_heap_bytes(context, aliases),
        SemanticQueryKey::ReadSignatureResult(key) => read_signature_key_heap_bytes(key, aliases),
        SemanticQueryKey::AwaitedNormalize { operand, context }
        | SemanticQueryKey::AsyncReturnPayload { operand, context } => {
            inline(operand) + inline(context)
        }
    }
}

#[cfg(test)]
#[path = "family_footprint_tests.rs"]
mod tests;
