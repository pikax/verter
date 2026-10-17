//! Allocation footprint of a cached component-meta result.
//!
//! Composes the declared conventions of
//! [`verter_session_query::retention::allocation`] over the whole cached
//! payload — the native component-meta analysis and its resolution
//! template: every `String` / `Vec` backing by capacity, every `Box` block,
//! and each distinct `Arc` allocation once per measurement. Two results with
//! equal record counts therefore cost what their payloads actually keep
//! alive, never a per-record proxy.
//!
//! Every record is destructured exhaustively, so a new field fails to
//! compile until it is measured. `Copy` values own no heap and are charged
//! inline only ([`inline`] requires `Copy`, so a heap-owning field added to
//! one of them breaks the build here too).
//!
//! Some carriers keep their `Arc` fields private and expose only a borrow
//! of the payload; those payloads are deduplicated by address
//! ([`Meter::first_borrow`]). What a cached result does NOT pay for here:
//!
//! * graph nodes named by [`verter_type_engine::semantic_query::SemanticNodeId`]
//!   — the node arena owns their storage;
//! * interned language identities inside a registered source snapshot id —
//!   pooled identity strings carry their own charge.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_session_query::retention::allocation::{
    arc_block_bytes, arc_slice_block_bytes, arc_str_bytes, box_bytes, fact_vec_bytes,
    heap_block_bytes, vec_bytes, AliasSet,
};

use verter_language::parse_artifact::carrier_inventory as inv;
use verter_session_query::analysis::component_meta as cm;
use verter_session_query::analysis::type_expand as tx;
use verter_type_expr::facts as fx;
use verter_type_expr::locators as loc;
use verter_type_expr::publication as publ;
use verter_type_expr::span_origins as so;

use crate::component_meta_cached_result::{CachedComponentMetaResult, ResolutionTemplate};

/// Heap one cached result keeps alive beyond its own inline bytes.
#[must_use]
pub(crate) fn cached_result_heap_bytes(result: &CachedComponentMetaResult) -> usize {
    let CachedComponentMetaResult {
        analysis,
        resolution_template,
        canonical_id,
        whole_hash,
    } = result;
    let mut meter = Meter::default();
    let m = &mut meter;
    analysis.heap(m) + resolution_template.heap(m) + canonical_id.heap(m) + inline(whole_hash)
}

/// One measurement: the `Arc` allocations already counted, by handle and
/// by borrowed payload address.
#[derive(Default)]
struct Meter {
    aliases: AliasSet,
    /// Payload addresses of `Arc` allocations whose owner exposes only a
    /// borrow. Every such address names an allocation the measured value
    /// holds for the whole measurement.
    borrowed: FxHashSet<usize>,
}

impl Meter {
    /// `true` the first time the `Arc` payload at `value` is offered.
    fn first_borrow<T: ?Sized>(&mut self, value: &T) -> bool {
        self.borrowed
            .insert(std::ptr::from_ref(value).cast::<()>() as usize)
    }
}

/// Heap a payload value owns beyond its inline bytes.
trait PayloadHeap {
    fn heap(&self, m: &mut Meter) -> usize;
}

/// A `Copy` value owns no heap.
fn inline<T: Copy>(_value: &T) -> usize {
    0
}

/// An `Arc<T>` block and its payload's heap, once per measurement.
fn arc_heap<T: PayloadHeap>(value: &Arc<T>, m: &mut Meter) -> usize {
    if m.aliases.first(value) {
        arc_block_bytes(std::mem::size_of::<T>()) + (**value).heap(m)
    } else {
        0
    }
}

/// An `Arc<[T]>` payload its owner exposes only as a borrowed slice.
fn borrowed_arc_slice<T: PayloadHeap>(slice: &[T], m: &mut Meter) -> usize {
    if m.first_borrow(slice) {
        arc_block_bytes(std::mem::size_of_val(slice))
            + slice.iter().map(|item| item.heap(m)).sum::<usize>()
    } else {
        0
    }
}

/// An `Arc<T>` payload its owner exposes only as a borrow.
fn borrowed_arc_value<T: PayloadHeap>(value: &T, m: &mut Meter) -> usize {
    if m.first_borrow(value) {
        arc_block_bytes(std::mem::size_of::<T>()) + value.heap(m)
    } else {
        0
    }
}

/// An `Arc<str>` payload its owner exposes only as a borrowed `str`.
fn borrowed_arc_str(text: &str, m: &mut Meter) -> usize {
    if m.first_borrow(text) {
        arc_block_bytes(text.len())
    } else {
        0
    }
}

impl PayloadHeap for String {
    fn heap(&self, _m: &mut Meter) -> usize {
        heap_block_bytes(self.capacity())
    }
}

impl PayloadHeap for Arc<str> {
    fn heap(&self, m: &mut Meter) -> usize {
        arc_str_bytes(self, &mut m.aliases)
    }
}

impl<T: PayloadHeap> PayloadHeap for Vec<T> {
    fn heap(&self, m: &mut Meter) -> usize {
        vec_bytes(self) + self.iter().map(|item| item.heap(m)).sum::<usize>()
    }
}

impl<T: PayloadHeap> PayloadHeap for Arc<[T]> {
    fn heap(&self, m: &mut Meter) -> usize {
        let Some(block) = arc_slice_block_bytes(self, &mut m.aliases) else {
            return 0;
        };
        block + self.iter().map(|item| item.heap(m)).sum::<usize>()
    }
}

impl<T: PayloadHeap> PayloadHeap for Option<T> {
    fn heap(&self, m: &mut Meter) -> usize {
        self.as_ref().map_or(0, |value| value.heap(m))
    }
}

impl<T: PayloadHeap> PayloadHeap for Box<T> {
    fn heap(&self, m: &mut Meter) -> usize {
        box_bytes::<T>() + (**self).heap(m)
    }
}

impl<A: PayloadHeap, B: PayloadHeap, C: PayloadHeap> PayloadHeap for (A, B, C) {
    fn heap(&self, m: &mut Meter) -> usize {
        self.0.heap(m) + self.1.heap(m) + self.2.heap(m)
    }
}

/// `Copy` values: every byte inline.
macro_rules! inline_copy {
    ($($t:ty),* $(,)?) => {$(
        impl PayloadHeap for $t {
            fn heap(&self, _m: &mut Meter) -> usize {
                inline(self)
            }
        }
    )*};
}

/// Fieldless enums that are not `Copy`, matched exhaustively so a variant
/// that gains data fails to compile here.
macro_rules! fieldless {
    ($($t:path { $($v:ident),* $(,)? })*) => {$(
        impl PayloadHeap for $t {
            fn heap(&self, _m: &mut Meter) -> usize {
                use $t as Enum;
                match self {
                    $(Enum::$v)|* => 0,
                }
            }
        }
    )*};
}

inline_copy!(
    bool,
    u16,
    u32,
    u64,
    usize,
    [u8; 16],
    verter_span::Span,
    verter_type_engine::semantic_query::SemanticNodeId,
    verter_type_engine::semantic_query::ProjectionMode,
    verter_type_engine::semantic_query::ResultCompleteness,
    cm::PublicInstanceMemberKind,
    cm::PublicInstanceCompleteness,
    cm::BindingKindAnalysis,
    cm::AcceptedPropKind,
    cm::AcceptedEventKind,
    cm::AcceptedSurfaceCompleteness,
    verter_session_query::analysis::types::ImportBindingKind,
    verter_session_query::analysis::types::VueApiClassification,
    verter_session_query::analysis::types::AnalyzedMacroKind,
    verter_session_query::analysis::style::StyleAnalysisLang,
    verter_session_query::declarations::metadata::ResolvedDeclarationKind,
    fx::TopLevelOwnerId,
    fx::SchemaAbsence,
    fx::SemanticSourceFailure,
    fx::TypeParamVariance,
    fx::CallableOccurrenceProjection,
    fx::IndexSignaturePosition,
    loc::LocatorSymbolSpace,
    loc::TypeBodyPathStep,
    loc::MacroPayloadPosition,
    so::DeclContributorAnchor,
    so::SourceSynthetic,
    so::FunctionParamSelector,
    verter_type_expr::CanonicalIndexInt,
    verter_type_expr::SyntheticCarrierSurfaceKind,
    inv::SourceSpan,
    inv::SourceSlice,
    inv::SourceEncoding,
    inv::SourceSpaceId,
    inv::BlockId,
    inv::MarkupNodeId,
    inv::AttributeId,
    inv::InternedNameId,
    inv::BlockContentOriginFingerprint,
    inv::TransformChainFingerprint,
    inv::AttributeName,
    inv::AttributeQuote,
    inv::AttributeDynamicSyntax,
    inv::BlockRecoveryReason,
    inv::DirectiveModifier,
    inv::LazyDecodedText,
    inv::MarkupInterpolationFamily,
    inv::MarkupNamespace,
    inv::NamedAttributeSyntax,
    inv::RecoveredMarkupKind,
    inv::ScriptRole,
    inv::UnknownDirectiveReason,
    inv::UnknownMarkupReason,
    inv::VueDirectiveKind,
    verter_language::registered_source_authority::WholeSourceHash,
);

fieldless! {
    cm::MacroExpansionKind { DefineProps, DefineEmits, DefineSlots }
    cm::NoFallthroughReason {
        InheritAttrsFalse,
        MultiRoot,
        BranchNotSingleRoot,
        RootVFor,
        NoTemplate,
        EmptyTemplate,
        TextOrInterpolationRoot,
    }
    cm::GenericResolutionFailure {
        SpreadInput,
        DynamicKey,
        MissingType,
        UnsupportedExpression,
        MissingUsageLink,
        UnresolvedChildGenericSurface,
    }
    verter_session_query::analysis::types::ReactivityKind {
        None, Ref, Computed, Reactive, MaybeRef, Mutable,
    }
    verter_session_query::analysis::template::PropValueConstness { Const, Dynamic, Unknown }
    tx::ExpansionStopReason {
        BudgetExceeded,
        ProjectionWorkLimit,
        ConnectedQueryDepthLimit,
        MappedDepthExceeded,
        UnresolvedReference,
        IndeterminateConditional,
        InfiniteKeySpace,
        UnsupportedOperator,
        ConditionalContextTruncated,
        IdempotentArm,
        CyclicReference,
        CyclicInstantiation,
        InstantiationError,
        EmptyUnionArm,
        ConnectedMemoryLimit,
    }
    verter_session_query::type_solver::result::SolverExactness {
        ExactConcrete, ExactSymbolic, Incomplete,
    }
    verter_session_query::type_solver::result::ExecutionStatus {
        Completed, Cancelled, Interrupted, HardStop,
    }
    verter_type_expr::MemberVisibility { Public, Protected, Private }
    verter_type_expr::ObjectMethodKind { Method, Get, Set }
    verter_type_expr::PrimitiveName {
        String, Number, Boolean, Symbol, BigInt, Any, Unknown, Void, Never, Null, Undefined,
        Object,
    }
    verter_type_expr::PropCallableRoleUnresolvedReason {
        AnalysisUnavailable,
        MissingDependency,
        Cycle,
        BudgetExceeded,
        WorkLimitExceeded,
        Unsupported,
        Fault,
    }
    verter_type_expr::feature_roles::ReactiveWrapperUnresolvedReason {
        AnalysisUnavailable,
        RevisionMismatch,
        MissingDependency,
        Cycle,
        BudgetExceeded,
        WorkLimitExceeded,
        Cancelled,
        Unsupported,
        Fault,
    }
    publ::ResolutionExactness { ExactConcrete, ExactSymbolic, Incomplete }
    publ::ResolutionProvenance {
        SemanticEvaluator, SessionProjector, FrameworkSurface, FallthroughInheritance, Schema,
    }
    publ::ResolutionDiagnosticKind {
        BudgetExceeded,
        ProjectionWorkLimit,
        ConnectedQueryDepthLimit,
        MappedDepthExceeded,
        UnresolvedReference,
        IndeterminateConditional,
        InfiniteKeySpace,
        UnsupportedOperator,
        ConditionalContextTruncated,
        IdempotentArm,
        CyclicReference,
        CyclicInstantiation,
        InstantiationError,
        EmptyUnionArm,
        ConnectedMemoryLimit,
    }
    publ::AuthoredProvenance {
        MacroPayload, DeclarationBody, AugmentationBody, JsdocTypedefBody,
    }
    publ::PublicationPolicyReason { ImportedMacroCompound, ImportedIndexedAccess }
    publ::SemanticAuthority { Resolved, AuthoredFallback }
}

// ---------------------------------------------------------------------------
// Cached result and resolution template
// ---------------------------------------------------------------------------

impl PayloadHeap for ResolutionTemplate {
    fn heap(&self, m: &mut Meter) -> usize {
        let ResolutionTemplate {
            mode,
            whole_hash,
            resolved_macros,
            resolved_type_registry,
            resolved_type_registry_meta,
            evaluated_types,
            fact_versions,
            surface_identities,
            origin_graph,
            completeness,
            synthesis_should_suppress,
        } = self;
        mode.heap(m)
            + whole_hash.heap(m)
            + resolved_macros.heap(m)
            + resolved_type_registry.heap(m)
            + resolved_type_registry_meta.heap(m)
            + evaluated_types.heap(m)
            + fact_vec_bytes(fact_versions, &mut m.aliases)
            + surface_identities.heap(m)
            + origin_graph.heap(m)
            + completeness.heap(m)
            + synthesis_should_suppress.heap(m)
    }
}

impl PayloadHeap for crate::resolver_core::ResolvedMacroMeta {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            macro_index,
            macro_kind,
            type_name,
            import_source,
            surface_is_authoritative,
            declaration,
            native_props,
            jsdoc,
        } = self;
        macro_index.heap(m)
            + macro_kind.heap(m)
            + type_name.heap(m)
            + import_source.heap(m)
            + surface_is_authoritative.heap(m)
            + declaration.heap(m)
            + native_props.heap(m)
            + jsdoc.heap(m)
    }
}

impl PayloadHeap for crate::resolver_core::ResolvedTypeRegistryMeta {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { name, declaration } = self;
        name.heap(m) + declaration.heap(m)
    }
}

impl PayloadHeap for verter_session_query::declarations::metadata::ResolvedTypeDeclaration {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            requested_name,
            declaration_id,
            resolved_name,
            canonical_source,
            owner,
            span,
            kind,
            text,
        } = self;
        requested_name.heap(m)
            + declaration_id.heap(m)
            + resolved_name.heap(m)
            + canonical_source.heap(m)
            + owner.heap(m)
            + span.heap(m)
            + kind.heap(m)
            + text.heap(m)
    }
}

impl PayloadHeap for crate::resolver_core::ResolvedNativeProp {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            is_optional,
            type_annotation,
            visibility,
            span,
        } = self;
        name.heap(m)
            + is_optional.heap(m)
            + type_annotation.heap(m)
            + visibility.heap(m)
            + span.heap(m)
    }
}

impl PayloadHeap for crate::resolver_core::ResolvedJsdocBlock {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { description, tags } = self;
        description.heap(m) + tags.heap(m)
    }
}

impl PayloadHeap for crate::resolver_core::ResolvedJsdocTag {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            text,
            raw_type,
            subject_name,
            resolved_type,
        } = self;
        name.heap(m)
            + text.heap(m)
            + raw_type.heap(m)
            + subject_name.heap(m)
            + resolved_type.heap(m)
    }
}

impl PayloadHeap for verter_protocol::graph::snapshot::ResolvedJsdocTypeOutput {
    /// The display text and the graph snapshot's tables, which clones of
    /// one snapshot share.
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { display, graph } = self;
        display.heap(m) + graph.owned_heap_bytes(&mut m.aliases)
    }
}

impl PayloadHeap for crate::meta_resolve::SurfaceNodeIdentities {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            prop_node_ids,
            emit_node_ids,
            slot_binding_node_ids,
            binding_node_ids,
            registry_node_ids,
        } = self;
        prop_node_ids.heap(m)
            + emit_node_ids.heap(m)
            + slot_binding_node_ids.heap(m)
            + binding_node_ids.heap(m)
            + registry_node_ids.heap(m)
    }
}

impl PayloadHeap for verter_protocol::types::OriginGraphDto {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            nodes,
            edges,
            meta_strings,
        } = self;
        nodes.heap(m) + edges.heap(m) + meta_strings.heap(m)
    }
}

impl PayloadHeap for verter_protocol::types::OriginNodeDto {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { id, kind, label } = self;
        id.heap(m) + kind.heap(m) + label.heap(m)
    }
}

impl PayloadHeap for verter_protocol::types::OriginEdgeDto {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            source,
            target,
            kind,
            meta_index,
        } = self;
        source.heap(m) + target.heap(m) + kind.heap(m) + meta_index.heap(m)
    }
}

// ---------------------------------------------------------------------------
// Expanded component types
// ---------------------------------------------------------------------------

impl PayloadHeap for tx::ExpandedComponentTypes {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            props,
            define_props,
            define_emits,
            emits,
            define_slots,
            slot_bindings,
            bindings,
            exposed,
        } = self;
        props.heap(m)
            + define_props.heap(m)
            + define_emits.heap(m)
            + emits.heap(m)
            + define_slots.heap(m)
            + slot_bindings.heap(m)
            + bindings.heap(m)
            + exposed.heap(m)
    }
}

impl PayloadHeap for tx::ExpandedField {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            owner,
            authority,
            authored_evidence,
            optional,
            exactness,
            execution_status,
            diagnostics,
            declared_in_macro_type_arg,
        } = self;
        name.heap(m)
            + owner.heap(m)
            + authority.heap(m)
            + authored_evidence.heap(m)
            + optional.heap(m)
            + exactness.heap(m)
            + execution_status.heap(m)
            + diagnostics.heap(m)
            + declared_in_macro_type_arg.heap(m)
    }
}

impl PayloadHeap for tx::ExpandedMacroProps {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            macro_index,
            result,
        } = self;
        macro_index.heap(m) + result.heap(m)
    }
}

impl PayloadHeap for tx::ExpandedMacroObjectShape {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            macro_index,
            result,
        } = self;
        macro_index.heap(m) + result.heap(m)
    }
}

impl PayloadHeap for tx::ExpandedMacroExposed {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            macro_index,
            fields,
        } = self;
        macro_index.heap(m) + fields.heap(m)
    }
}

impl<T: PayloadHeap> PayloadHeap for tx::ExpansionResult<T> {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            value,
            exactness,
            execution_status,
            diagnostics,
        } = self;
        value.heap(m) + exactness.heap(m) + execution_status.heap(m) + diagnostics.heap(m)
    }
}

impl PayloadHeap for tx::ExpandedObjectShape {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            domain_guarantee,
            properties,
            index_signatures,
            call_signatures,
        } = self;
        domain_guarantee.heap(m)
            + properties.heap(m)
            + index_signatures.heap(m)
            + call_signatures.heap(m)
    }
}

impl PayloadHeap for tx::ExpandedProperty {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            ty,
            optional,
            readonly,
            visibility,
            declared_in_macro_type_arg,
        } = self;
        name.heap(m)
            + ty.heap(m)
            + optional.heap(m)
            + readonly.heap(m)
            + visibility.heap(m)
            + declared_in_macro_type_arg.heap(m)
    }
}

impl PayloadHeap for tx::ExpandedIndexSignature {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            key_type,
            value_type,
            readonly,
        } = self;
        key_type.heap(m) + value_type.heap(m) + readonly.heap(m)
    }
}

impl PayloadHeap for tx::ExpandedCallSignature {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            parameters,
            return_type,
            type_parameters,
        } = self;
        parameters.heap(m) + return_type.heap(m) + type_parameters.heap(m)
    }
}

impl PayloadHeap for tx::ExpandedParameter {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            ty,
            optional,
            rest,
        } = self;
        name.heap(m) + ty.heap(m) + optional.heap(m) + rest.heap(m)
    }
}

impl PayloadHeap for tx::ExpansionMetadata {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            exactness,
            execution_status,
            diagnostics,
        } = self;
        exactness.heap(m) + execution_status.heap(m) + diagnostics.heap(m)
    }
}

impl PayloadHeap for tx::ExpansionDiagnostic {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            reason,
            context,
            property_name,
        } = self;
        reason.heap(m) + context.heap(m) + property_name.heap(m)
    }
}

// ---------------------------------------------------------------------------
// Component-meta analysis
// ---------------------------------------------------------------------------

impl PayloadHeap for cm::ComponentMetaAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            domain_guarantees,
            props,
            events,
            slots,
            models,
            exposed,
            public_instance,
            ordered_sfc_structure,
            type_registry,
            components,
            template_refs,
            imports,
            bindings,
            vue_api_calls,
            styles,
            flags,
            root_reachability,
            accepted_props,
            accepted_events,
            accepted_surface_completeness,
            fallthrough_surface,
            macro_expansion_diagnostics,
            options_api,
            file_path,
        } = self;
        domain_guarantees.heap(m)
            + props.heap(m)
            + events.heap(m)
            + slots.heap(m)
            + models.heap(m)
            + exposed.heap(m)
            + public_instance.heap(m)
            + ordered_sfc_structure.heap(m)
            + type_registry.heap(m)
            + components.heap(m)
            + template_refs.heap(m)
            + imports.heap(m)
            + bindings.heap(m)
            + vue_api_calls.heap(m)
            + styles.heap(m)
            + flags.heap(m)
            + root_reachability.heap(m)
            + accepted_props.heap(m)
            + accepted_events.heap(m)
            + accepted_surface_completeness.heap(m)
            + fallthrough_surface.heap(m)
            + macro_expansion_diagnostics.heap(m)
            + options_api.heap(m)
            + file_path.heap(m)
    }
}

impl PayloadHeap for cm::MetadataFactGuarantee {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { kind, causes, open } = self;
        kind.heap(m) + causes.heap(m) + open.heap(m)
    }
}

impl PayloadHeap for cm::ComponentDomainGuarantees {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            props,
            events,
            slots,
        } = self;
        props.heap(m) + events.heap(m) + slots.heap(m)
    }
}

impl PayloadHeap for cm::MacroExpansionDiagnostics {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            macro_kind,
            macro_index,
            diagnostics,
            exactness,
            execution_status,
        } = self;
        macro_kind.heap(m)
            + macro_index.heap(m)
            + diagnostics.heap(m)
            + exactness.heap(m)
            + execution_status.heap(m)
    }
}

impl PayloadHeap for verter_session_query::analysis::types::JsdocTag {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { name, text } = self;
        name.heap(m) + text.heap(m)
    }
}

impl PayloadHeap for cm::PropAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            callable_role,
            publication,
            type_expansion,
            required,
            has_default,
            default_value,
            description,
            tags,
            declared_in_macro_type_arg,
        } = self;
        name.heap(m)
            + callable_role.heap(m)
            + publication.heap(m)
            + type_expansion.heap(m)
            + required.heap(m)
            + has_default.heap(m)
            + default_value.heap(m)
            + description.heap(m)
            + tags.heap(m)
            + declared_in_macro_type_arg.heap(m)
    }
}

impl PayloadHeap for cm::EventAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            payload,
            publication,
            return_publication,
            return_publication_scope,
            payload_expansion,
            raw_signature,
            description,
            tags,
        } = self;
        name.heap(m)
            + payload.heap(m)
            + publication.heap(m)
            + return_publication.heap(m)
            + return_publication_scope.heap(m)
            + payload_expansion.heap(m)
            + raw_signature.heap(m)
            + description.heap(m)
            + tags.heap(m)
    }
}

impl PayloadHeap for cm::SlotAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            is_scoped,
            bindings,
            is_required,
            return_type,
            return_publication,
            return_publication_scope,
            description,
            tags,
            declared_in_macro_type_arg,
        } = self;
        name.heap(m)
            + is_scoped.heap(m)
            + bindings.heap(m)
            + is_required.heap(m)
            + return_type.heap(m)
            + return_publication.heap(m)
            + return_publication_scope.heap(m)
            + description.heap(m)
            + tags.heap(m)
            + declared_in_macro_type_arg.heap(m)
    }
}

impl PayloadHeap for cm::SlotBindingAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            publication,
            type_expansion,
        } = self;
        name.heap(m) + publication.heap(m) + type_expansion.heap(m)
    }
}

impl PayloadHeap for cm::ModelAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { name, type_source } = self;
        name.heap(m) + type_source.heap(m)
    }
}

impl PayloadHeap for cm::ExposedAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            type_source,
            type_expansion,
            description,
            tags,
        } = self;
        name.heap(m)
            + type_source.heap(m)
            + type_expansion.heap(m)
            + description.heap(m)
            + tags.heap(m)
    }
}

impl PayloadHeap for cm::PublicInstanceAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            members,
            completeness,
        } = self;
        members.heap(m) + completeness.heap(m)
    }
}

impl PayloadHeap for cm::PublicInstanceMemberAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            kind,
            type_source,
            type_expansion,
            raw_type,
            description,
            tags,
        } = self;
        name.heap(m)
            + kind.heap(m)
            + type_source.heap(m)
            + type_expansion.heap(m)
            + raw_type.heap(m)
            + description.heap(m)
            + tags.heap(m)
    }
}

impl PayloadHeap for cm::OrderedSfcStructureAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            schema_version,
            artifact_token,
            inventory,
            source_space_tokens,
            block_tokens,
            markup_node_tokens,
            attribute_tokens,
        } = self;
        schema_version.heap(m)
            + artifact_token.heap(m)
            + arc_heap(inventory, m)
            + source_space_tokens.heap(m)
            + block_tokens.heap(m)
            + markup_node_tokens.heap(m)
            + attribute_tokens.heap(m)
    }
}

impl PayloadHeap for cm::ResolvedTypeAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            type_source,
            type_expansion,
        } = self;
        name.heap(m) + type_source.heap(m) + type_expansion.heap(m)
    }
}

impl PayloadHeap for cm::ComponentUsageAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            import_source,
            is_dynamic,
            props,
            has_spread,
            slots_used,
            static_classes,
            has_dynamic_class,
            v_models,
            v_model_entries,
            bindings,
            events,
        } = self;
        name.heap(m)
            + import_source.heap(m)
            + is_dynamic.heap(m)
            + props.heap(m)
            + has_spread.heap(m)
            + slots_used.heap(m)
            + static_classes.heap(m)
            + has_dynamic_class.heap(m)
            + v_models.heap(m)
            + v_model_entries.heap(m)
            + bindings.heap(m)
            + events.heap(m)
    }
}

impl PayloadHeap for cm::ComponentBindingUsageAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { name, modifiers } = self;
        name.heap(m) + modifiers.heap(m)
    }
}

impl PayloadHeap for cm::ComponentEventUsageAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            handler_expression,
            is_inline,
            modifiers,
        } = self;
        name.heap(m) + handler_expression.heap(m) + is_inline.heap(m) + modifiers.heap(m)
    }
}

impl PayloadHeap for cm::ComponentPropUsageAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            is_bound,
            constness,
            expression,
            referenced_bindings,
            from_spread,
            is_shorthand,
        } = self;
        name.heap(m)
            + is_bound.heap(m)
            + constness.heap(m)
            + expression.heap(m)
            + referenced_bindings.heap(m)
            + from_spread.heap(m)
            + is_shorthand.heap(m)
    }
}

impl PayloadHeap for cm::ComponentVModelUsageAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { binding_name } = self;
        binding_name.heap(m)
    }
}

impl PayloadHeap for cm::TemplateRefAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            is_dynamic,
            target_tag,
        } = self;
        name.heap(m) + is_dynamic.heap(m) + target_tag.heap(m)
    }
}

impl PayloadHeap for cm::ImportAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            source,
            is_type_only,
            bindings,
        } = self;
        source.heap(m) + is_type_only.heap(m) + bindings.heap(m)
    }
}

impl PayloadHeap for cm::ImportBindingAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            kind,
            imported_name,
            is_type_only,
        } = self;
        name.heap(m) + kind.heap(m) + imported_name.heap(m) + is_type_only.heap(m)
    }
}

impl PayloadHeap for cm::BindingAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            kind,
            reactivity_kind,
            return_wrapper_role,
            type_annotation,
            used_in_template,
            used_in_style,
        } = self;
        name.heap(m)
            + kind.heap(m)
            + reactivity_kind.heap(m)
            + return_wrapper_role.heap(m)
            + type_annotation.heap(m)
            + used_in_template.heap(m)
            + used_in_style.heap(m)
    }
}

impl PayloadHeap for cm::VueApiCallAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { api, arg_value } = self;
        api.heap(m) + arg_value.heap(m)
    }
}

impl PayloadHeap for cm::StyleAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            lang,
            scoped,
            is_module,
            module_name,
            block_ref,
            classes,
            ids,
            custom_properties,
            v_binds,
            selectors,
        } = self;
        lang.heap(m)
            + scoped.heap(m)
            + is_module.heap(m)
            + module_name.heap(m)
            + block_ref.heap(m)
            + classes.heap(m)
            + ids.heap(m)
            + custom_properties.heap(m)
            + v_binds.heap(m)
            + selectors.heap(m)
    }
}

impl PayloadHeap for cm::SelectorAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { text, specificity } = self;
        text.heap(m) + specificity.heap(m)
    }
}

impl PayloadHeap for cm::ComponentMetaFlags {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            async_setup,
            has_reactive_state,
            has_computed,
            has_watchers,
            has_lifecycle_hooks,
            has_provide,
            has_inject,
            has_inherit_attrs_false,
            has_store_usage,
            has_macro_failure,
        } = self;
        async_setup.heap(m)
            + has_reactive_state.heap(m)
            + has_computed.heap(m)
            + has_watchers.heap(m)
            + has_lifecycle_hooks.heap(m)
            + has_provide.heap(m)
            + has_inject.heap(m)
            + has_inherit_attrs_false.heap(m)
            + has_store_usage.heap(m)
            + has_macro_failure.heap(m)
    }
}

impl PayloadHeap for cm::RootReachability {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::NoFallthrough { reason } => reason.heap(m),
            Self::Branches { branches } => branches.heap(m),
        }
    }
}

impl PayloadHeap for cm::RootBranch {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            branch_index,
            condition_text,
            target,
            consumed,
            has_unknown_spread,
        } = self;
        branch_index.heap(m)
            + condition_text.heap(m)
            + target.heap(m)
            + consumed.heap(m)
            + has_unknown_spread.heap(m)
    }
}

impl PayloadHeap for cm::RootTargetRef {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::NativeElement { element_index, tag } => element_index.heap(m) + tag.heap(m),
            Self::DynamicComponentUsage {
                element_index,
                usage_index,
            } => element_index.heap(m) + usage_index.heap(m),
            Self::ComponentUsage {
                element_index,
                usage_index,
                name,
                import_source,
            } => element_index.heap(m) + usage_index.heap(m) + name.heap(m) + import_source.heap(m),
            Self::UnresolvedTarget {
                element_index,
                tag,
                reason,
            } => element_index.heap(m) + tag.heap(m) + reason.heap(m),
        }
    }
}

impl PayloadHeap for cm::UnresolvedRootTargetReason {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::UnsupportedBuiltin { tag } => tag.heap(m),
            Self::DynamicComponentIs
            | Self::SlotOutlet
            | Self::MissingUsageLink
            | Self::UnresolvedImport
            | Self::UnknownRootTarget => 0,
        }
    }
}

impl PayloadHeap for cm::ConsumedRootBindings {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            attrs,
            listeners,
            has_dynamic_attr_name,
            has_dynamic_listener_name,
        } = self;
        attrs.heap(m)
            + listeners.heap(m)
            + has_dynamic_attr_name.heap(m)
            + has_dynamic_listener_name.heap(m)
    }
}

impl PayloadHeap for cm::PartialBranchReason {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::GenericResolution { failure } => failure.heap(m),
            Self::DynamicAttrName | Self::DynamicListenerName | Self::UnknownSpread => 0,
        }
    }
}

impl PayloadHeap for cm::UnresolvedBranchReason {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Cycle { canonical_id } => canonical_id.heap(m),
            Self::UnresolvedChildImport { import_source } => import_source.heap(m),
            Self::RootTarget { reason } => reason.heap(m),
            Self::GenericResolution { failure } => failure.heap(m),
            Self::DynamicComponentIs | Self::ChildResolutionFailed => 0,
        }
    }
}

impl PayloadHeap for cm::MemberProvenance {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Declared => 0,
            Self::Inherited { sources } => sources.heap(m),
        }
    }
}

impl PayloadHeap for cm::InheritedSource {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::NativeTag { tag } => tag.heap(m),
            Self::Component { canonical_id } => canonical_id.heap(m),
        }
    }
}

impl PayloadHeap for cm::MemberAvailability {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Always => 0,
            Self::Conditional { branch_keys } => branch_keys.heap(m),
        }
    }
}

impl PayloadHeap for cm::AcceptedPropAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            callable_role,
            publication,
            type_source_scope,
            required,
            provenance,
            availability,
            kind,
        } = self;
        name.heap(m)
            + callable_role.heap(m)
            + publication.heap(m)
            + type_source_scope.heap(m)
            + required.heap(m)
            + provenance.heap(m)
            + availability.heap(m)
            + kind.heap(m)
    }
}

impl PayloadHeap for cm::AcceptedEventAnalysis {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            payload,
            payload_scope,
            raw_signature,
            provenance,
            availability,
            kind,
        } = self;
        name.heap(m)
            + payload.heap(m)
            + payload_scope.heap(m)
            + raw_signature.heap(m)
            + provenance.heap(m)
            + availability.heap(m)
            + kind.heap(m)
    }
}

impl PayloadHeap for cm::FallthroughSurface {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::None { reason } => reason.heap(m),
            Self::Branches { branches } => branches.heap(m),
        }
    }
}

impl PayloadHeap for cm::FallthroughPropEntry {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            callable_role,
            publication,
            type_source_scope,
            sources,
        } = self;
        name.heap(m)
            + callable_role.heap(m)
            + publication.heap(m)
            + type_source_scope.heap(m)
            + sources.heap(m)
    }
}

impl PayloadHeap for cm::FallthroughEventEntry {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            payload,
            payload_scope,
            raw_signature,
            sources,
        } = self;
        name.heap(m)
            + payload.heap(m)
            + payload_scope.heap(m)
            + raw_signature.heap(m)
            + sources.heap(m)
    }
}

impl PayloadHeap for cm::BranchStatus {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Resolved => 0,
            Self::PartiallyUnresolved { reasons } => reasons.heap(m),
            Self::Unresolved { reason } => reason.heap(m),
        }
    }
}

impl PayloadHeap for cm::ResolvedRootStep {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::NativeTag { tag } => tag.heap(m),
            Self::Component {
                canonical_id,
                component_name,
            } => canonical_id.heap(m) + component_name.heap(m),
            Self::Unresolved { tag, reason } => tag.heap(m) + reason.heap(m),
        }
    }
}

impl PayloadHeap for cm::FallthroughBranch {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            branch_key,
            condition_text,
            props,
            events,
            root_chain,
            status,
        } = self;
        branch_key.heap(m)
            + condition_text.heap(m)
            + props.heap(m)
            + events.heap(m)
            + root_chain.heap(m)
            + status.heap(m)
    }
}

// ---------------------------------------------------------------------------
// Typed publication and semantic type sources
// ---------------------------------------------------------------------------

impl PayloadHeap for verter_type_expr::TypeExprScope {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self(scope) = self;
        scope.heap(m)
    }
}

impl PayloadHeap for verter_type_expr::PropCallableRole {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::SvelteSnippet {
                symbol,
                exactness,
                provenance,
            } => symbol.heap(m) + exactness.heap(m) + provenance.heap(m),
            Self::Other => 0,
            Self::Unresolved { reason } => reason.heap(m),
        }
    }
}

impl PayloadHeap for verter_type_expr::ResolvedSymbolIdentity {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            canonical_id,
            owner,
            symbol,
        } = self;
        canonical_id.heap(m) + owner.heap(m) + symbol.heap(m)
    }
}

impl PayloadHeap for verter_type_expr::ReactiveWrapperRole {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Unresolved { reason } => reason.heap(m),
            Self::Ref
            | Self::ShallowRef
            | Self::ComputedRef
            | Self::ModelRef
            | Self::Reactive
            | Self::ShallowReactive
            | Self::None => 0,
        }
    }
}

impl PayloadHeap for publ::TypePublication {
    /// Fields are private; each is reached through its accessor.
    fn heap(&self, m: &mut Meter) -> usize {
        self.authority().heap(m) + self.evidence().map_or(0, |e| e.heap(m)) + self.result().heap(m)
    }
}

impl PayloadHeap for publ::ResolvedTypeAuthority {
    /// Fields are private: the outcome is borrowed in place, the diagnostic
    /// list is an `Arc` slice exposed only as a borrow.
    fn heap(&self, m: &mut Meter) -> usize {
        self.outcome().heap(m)
            + self.provenance().heap(m)
            + borrowed_arc_slice(self.diagnostics(), m)
    }
}

impl PayloadHeap for publ::ResolvedTypeOutcome {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Present { source, exactness } => arc_heap(source, m) + exactness.heap(m),
            Self::Absent { absence } => absence.heap(m),
            Self::Failed { failure } => failure.heap(m),
        }
    }
}

impl PayloadHeap for publ::TypedResolutionFailure {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::SourceConstruction(failure) => failure.heap(m),
        }
    }
}

impl PayloadHeap for publ::ResolutionDiagnostic {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            kind,
            context,
            property_name,
        } = self;
        kind.heap(m) + context.heap(m) + property_name.heap(m)
    }
}

impl PayloadHeap for publ::AuthoredTypeEvidence {
    /// Fields are private: the authored locator is borrowed in place, the
    /// text is an `Arc<str>` exposed only as a borrow.
    fn heap(&self, m: &mut Meter) -> usize {
        self.source().locator().heap(m)
            + borrowed_arc_str(self.text(), m)
            + self.provenance().heap(m)
    }
}

impl PayloadHeap for publ::PublicationResult {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Failed {
                failure,
                provenance,
            } => failure.heap(m) + provenance.heap(m),
            Self::Absent {
                absence,
                provenance,
            } => absence.heap(m) + provenance.heap(m),
            Self::Published {
                selected_source,
                semantic_authority,
                exactness,
                reason,
                provenance,
            } => {
                arc_heap(selected_source, m)
                    + semantic_authority.heap(m)
                    + exactness.heap(m)
                    + reason.heap(m)
                    + provenance.heap(m)
            }
        }
    }
}

impl PayloadHeap for publ::PublicationReason {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::ResolvedExactConcrete
            | Self::ResolvedExactSymbolic
            | Self::ResolvedIncomplete => 0,
            Self::AuthoredForIncomplete { policy } => policy.heap(m),
            Self::AuthoredSymbolicRepresentation { proof } => proof.heap(m),
        }
    }
}

impl PayloadHeap for publ::PublicationPolicy {
    fn heap(&self, m: &mut Meter) -> usize {
        self.incomplete_reason().heap(m)
            + self.symbolic_equivalence().map_or(0, |proof| proof.heap(m))
    }
}

impl PayloadHeap for publ::SymbolicEquivalenceProof {
    /// Both bound sources; the kind is inline.
    fn heap(&self, m: &mut Meter) -> usize {
        inline(&self.kind())
            + self.resolved_source().heap(m)
            + self.authored_source().locator().heap(m)
    }
}

impl PayloadHeap for publ::PublicationProvenance {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Resolved { provenance } => provenance.heap(m),
            Self::Authored { provenance } => provenance.heap(m),
        }
    }
}

impl PayloadHeap for fx::SourcePosition {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Absent(absence) => absence.heap(m),
            Self::Present(source) => source.heap(m),
            Self::Failed(failure) => failure.heap(m),
        }
    }
}

impl PayloadHeap for fx::SemanticTypeSource {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Authored(locator) => locator.heap(m),
            Self::Projected(fact) => fact.heap(m),
            Self::Synthesized(shape) => shape.heap(m),
            Self::Closed(fact) => fact.heap(m),
            Self::SyntheticSlotBinding(key) => arc_heap(key, m),
        }
    }
}

impl PayloadHeap for verter_type_expr::SyntheticCarrierKey {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            scope_canonical_id,
            surface_kind,
            slot_name,
            binding_name,
            value_node,
        } = self;
        scope_canonical_id.heap(m)
            + surface_kind.heap(m)
            + slot_name.heap(m)
            + binding_name.heap(m)
            + value_node.heap(m)
    }
}

impl PayloadHeap for loc::AuthoredAnchor {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            canonical_id,
            owner,
            symbol,
            space,
        } = self;
        canonical_id.heap(m) + owner.heap(m) + symbol.heap(m) + space.heap(m)
    }
}

impl PayloadHeap for loc::TypeBodySlot {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { anchor, path } = self;
        anchor.heap(m) + path.heap(m)
    }
}

impl PayloadHeap for loc::SymbolBodyLocator {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { anchor } = self;
        anchor.heap(m)
    }
}

impl PayloadHeap for loc::TypeArgLocator {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            anchor,
            path,
            arg_index,
        } = self;
        anchor.heap(m) + path.heap(m) + arg_index.heap(m)
    }
}

impl PayloadHeap for loc::MacroPayloadLocator {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            anchor,
            macro_index,
            payload,
        } = self;
        anchor.heap(m) + macro_index.heap(m) + payload.heap(m)
    }
}

impl PayloadHeap for loc::AuthoredAugmentationScope {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Global => 0,
            Self::Module { specifier } => specifier.heap(m),
        }
    }
}

impl PayloadHeap for loc::AugmentationBodyLocator {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            anchor,
            scope,
            path,
        } = self;
        anchor.heap(m) + scope.heap(m) + path.heap(m)
    }
}

impl PayloadHeap for loc::JsdocTypedefBodyLocator {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { anchor, path } = self;
        anchor.heap(m) + path.heap(m)
    }
}

impl PayloadHeap for loc::AuthoredBodyLocator {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::DeclBody(slot) => slot.heap(m),
            Self::AugmentationBody(locator) => locator.heap(m),
            Self::JsdocTypedefBody(locator) => locator.heap(m),
            Self::MacroPayload(locator) => locator.heap(m),
        }
    }
}

impl PayloadHeap for loc::FunctionReturnLocator {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Authored(slot) | Self::Jsdoc(slot) => slot.heap(m),
        }
    }
}

impl PayloadHeap for fx::ProjectedTypeFact {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Member(member) => member.heap(m),
            Self::IndexSignature(signature) => signature.heap(m),
            Self::CallSignature(signature) | Self::ConstructSignature(signature) => {
                signature.heap(m)
            }
            Self::Surface(surface) => surface.heap(m),
            Self::MemberPath { base, path } => base.heap(m) + path.heap(m),
            Self::CallableOccurrence {
                base,
                occurrence,
                projection,
            } => base.heap(m) + occurrence.heap(m) + projection.heap(m),
            Self::IndexPosition {
                base,
                signature_ordinal,
                position,
            } => base.heap(m) + signature_ordinal.heap(m) + position.heap(m),
        }
    }
}

impl PayloadHeap for fx::CallableOccurrenceHandle {
    /// Fields are private: the member path is an `Arc` slice exposed only
    /// as a borrow; the subject is an inline ordinal.
    fn heap(&self, m: &mut Meter) -> usize {
        borrowed_arc_slice(self.path(), m)
    }
}

impl PayloadHeap for fx::ProjectedMemberFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            key,
            optional,
            readonly,
            method_kind,
            has_implementation_body,
            visibility,
            declared_in_macro_type_arg,
            declaration_origin,
            ty,
            span_origin,
        } = self;
        key.heap(m)
            + optional.heap(m)
            + readonly.heap(m)
            + method_kind.heap(m)
            + has_implementation_body.heap(m)
            + visibility.heap(m)
            + declared_in_macro_type_arg.heap(m)
            + declaration_origin.heap(m)
            + ty.heap(m)
            + span_origin.heap(m)
    }
}

impl PayloadHeap for fx::ProjectedIndexSignatureFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            key_name,
            key_type,
            value_type,
            readonly,
            declaration_origin,
            span_origin,
        } = self;
        key_name.heap(m)
            + key_type.heap(m)
            + value_type.heap(m)
            + readonly.heap(m)
            + declaration_origin.heap(m)
            + span_origin.heap(m)
    }
}

impl PayloadHeap for fx::ProjectedSurfaceFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            members,
            call_signatures,
            construct_signatures,
            index_signatures,
            has_index_signature,
        } = self;
        members.heap(m)
            + call_signatures.heap(m)
            + construct_signatures.heap(m)
            + index_signatures.heap(m)
            + has_index_signature.heap(m)
    }
}

impl PayloadHeap for fx::FunctionSignatureFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            type_parameters,
            parameters,
            return_source,
            return_reference_head,
            has_implementation_body,
            spans_origin,
        } = self;
        type_parameters.heap(m)
            + parameters.heap(m)
            + return_source.heap(m)
            + return_reference_head.heap(m)
            + has_implementation_body.heap(m)
            + spans_origin.heap(m)
    }
}

impl PayloadHeap for fx::NarrowTypeParam {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            ordinal,
            constraint,
            default,
            is_const,
            variance,
        } = self;
        name.heap(m)
            + ordinal.heap(m)
            + constraint.heap(m)
            + default.heap(m)
            + is_const.heap(m)
            + variance.heap(m)
    }
}

impl PayloadHeap for fx::FunctionParamFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            optional,
            rest,
            has_ts_annotation,
            ty,
            span_origin,
        } = self;
        name.heap(m)
            + optional.heap(m)
            + rest.heap(m)
            + has_ts_annotation.heap(m)
            + ty.heap(m)
            + span_origin.heap(m)
    }
}

impl PayloadHeap for so::FunctionParamSpanOrigin {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { function, param } = self;
        function.heap(m) + param.heap(m)
    }
}

impl PayloadHeap for so::FunctionSpansOrigin {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::AliasBody { anchor } => anchor.heap(m),
            Self::Member {
                anchor,
                member_path,
            } => anchor.heap(m) + member_path.heap(m),
            Self::Synthetic(synthetic) => synthetic.heap(m),
        }
    }
}

impl PayloadHeap for so::MemberSpansOrigin {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Authored {
                anchor,
                member_path,
            } => anchor.heap(m) + member_path.heap(m),
            Self::Synthetic(synthetic) => synthetic.heap(m),
        }
    }
}

impl PayloadHeap for so::IndexSignatureSpansOrigin {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Authored {
                anchor,
                member_path,
            } => anchor.heap(m) + member_path.heap(m),
            Self::Synthetic(synthetic) => synthetic.heap(m),
        }
    }
}

impl PayloadHeap for fx::FunctionReturnSource {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Declared(locator) => locator.heap(m),
            Self::Flow(identity) => identity.heap(m),
            Self::Absent => 0,
        }
    }
}

impl PayloadHeap for fx::FlowFunctionReturnIdentity {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            anchor,
            function_part,
            overload_ordinal,
        } = self;
        anchor.heap(m) + function_part.heap(m) + overload_ordinal.heap(m)
    }
}

impl PayloadHeap for fx::FunctionPartIdentity {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::DeclarationBody | Self::Initializer => 0,
            Self::Member { member_path } => member_path.heap(m),
            Self::Other { ordinal } => ordinal.heap(m),
        }
    }
}

impl PayloadHeap for fx::AuthoredReferenceHeadFact {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::NotReference | Self::Unavailable => 0,
            Self::Bare { local_name, args } => local_name.heap(m) + args.heap(m),
            Self::Qualified {
                local_root,
                member_path,
                args,
            } => local_root.heap(m) + member_path.heap(m) + args.heap(m),
            Self::ImportType {
                specifier,
                member_path,
                args,
            } => specifier.heap(m) + member_path.heap(m) + args.heap(m),
        }
    }
}

impl PayloadHeap for fx::AuthoredReferenceArgLocator {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Value(locator) => locator.heap(m),
            Self::MacroPayload { payload, arg_index } => payload.heap(m) + arg_index.heap(m),
        }
    }
}

impl PayloadHeap for fx::KeyTypeShape {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::String | Self::Number | Self::Symbol => 0,
            Self::Other(slot) => slot.heap(m),
        }
    }
}

impl PayloadHeap for fx::DeclarationOrigin {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Declared(name) => name.heap(m),
            Self::Synthetic => 0,
        }
    }
}

impl<C: PayloadHeap, I: PayloadHeap> PayloadHeap for verter_type_expr::AuthoredPropertyKey<C, I> {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::String(text) => text.heap(m),
            Self::Number(index) => index.heap(m),
            Self::UniqueSymbol(identity) => identity.heap(m),
            Self::Computed(slot) => slot.heap(m),
        }
    }
}

impl<I: PayloadHeap> PayloadHeap for verter_type_expr::PropertyKey<I> {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::String(text) => text.heap(m),
            Self::Number(index) => index.heap(m),
            Self::UniqueSymbol(identity) => identity.heap(m),
        }
    }
}

impl PayloadHeap for fx::ValueDeclIdentityPart {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            canonical_id,
            owner,
            symbol,
            member_path,
        } = self;
        canonical_id.heap(m) + owner.heap(m) + symbol.heap(m) + member_path.heap(m)
    }
}

impl PayloadHeap for fx::ClosedTypeFact {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Leaf(leaf) => leaf.heap(m),
            Self::LeafUnion(leaves) => leaves.heap(m),
            Self::Object(object) => object.heap(m),
            Self::Function(signature) => signature.heap(m),
            Self::Tuple(tuple) => tuple.heap(m),
            Self::IndexedAccess(access) => access.heap(m),
        }
    }
}

impl PayloadHeap for fx::LeafTypeFact {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Primitive(primitive) => primitive.heap(m),
            Self::StringLiteral(text) | Self::NumberLiteral(text) | Self::Ref(text) => text.heap(m),
            Self::BooleanLiteral(value) => value.heap(m),
        }
    }
}

impl PayloadHeap for fx::ObjectShapeFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { members } = self;
        members.heap(m)
    }
}

impl PayloadHeap for fx::ObjectMemberFact {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Property(property) => property.heap(m),
            Self::Method(method) => method.heap(m),
            Self::CallSignature(signature) | Self::ConstructSignature(signature) => {
                signature.heap(m)
            }
            Self::IndexSignature(signature) => signature.heap(m),
            Self::Spread(spread) => spread.heap(m),
        }
    }
}

impl PayloadHeap for fx::ObjectPropertyFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            key,
            optional,
            readonly,
            visibility,
            ty,
            span_origin,
        } = self;
        key.heap(m)
            + optional.heap(m)
            + readonly.heap(m)
            + visibility.heap(m)
            + ty.heap(m)
            + span_origin.heap(m)
    }
}

impl PayloadHeap for fx::ObjectMethodFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            key,
            optional,
            method_kind,
            visibility,
            function,
            span_origin,
        } = self;
        key.heap(m)
            + optional.heap(m)
            + method_kind.heap(m)
            + visibility.heap(m)
            + function.heap(m)
            + span_origin.heap(m)
    }
}

impl PayloadHeap for fx::IndexSignatureFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            key_name,
            key_type,
            value_type,
            readonly,
            span_origin,
        } = self;
        key_name.heap(m)
            + key_type.heap(m)
            + value_type.heap(m)
            + readonly.heap(m)
            + span_origin.heap(m)
    }
}

impl PayloadHeap for fx::SpreadMemberFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { ty } = self;
        ty.heap(m)
    }
}

impl PayloadHeap for fx::TuplePayloadFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { readonly, elements } = self;
        readonly.heap(m) + elements.heap(m)
    }
}

impl PayloadHeap for fx::TupleElementFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            label,
            optional,
            rest,
            ty,
        } = self;
        label.heap(m) + optional.heap(m) + rest.heap(m) + ty.heap(m)
    }
}

impl PayloadHeap for fx::IndexedAccessFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { object, index_path } = self;
        object.heap(m) + index_path.heap(m)
    }
}

impl PayloadHeap for fx::ResolvedLocalShape {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Object(members) => members.heap(m),
            Self::Tuple(tuple) => tuple.heap(m),
            Self::IndexedAccess(access) => access.heap(m),
            Self::Leaf(leaf) => leaf.heap(m),
            Self::Ref(locator) => locator.heap(m),
        }
    }
}

impl PayloadHeap for fx::SynthesizedMemberFact {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            name,
            optional,
            ty,
            span_origin,
        } = self;
        name.heap(m) + optional.heap(m) + ty.heap(m) + span_origin.heap(m)
    }
}

impl PayloadHeap for fx::FactOrLocator {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Leaf(leaf) => leaf.heap(m),
            Self::LeafUnion(leaves) => leaves.heap(m),
            Self::Locator(slot) => slot.heap(m),
            Self::MacroPayload(locator) => locator.heap(m),
            Self::LeafObject(members) => members.heap(m),
        }
    }
}

impl PayloadHeap for fx::SynthesizedLeafMember {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { name, optional, ty } = self;
        name.heap(m) + optional.heap(m) + ty.heap(m)
    }
}

// ---------------------------------------------------------------------------
// Registered carrier inventory
// ---------------------------------------------------------------------------

impl PayloadHeap for inv::CarrierBlockInventory {
    /// Fields are private: each table is an `Arc` payload exposed only as a
    /// borrow. The memoised identity and structure hash are inline.
    fn heap(&self, m: &mut Meter) -> usize {
        borrowed_arc_slice(self.source_spaces(), m)
            + borrowed_arc_value(self.normalized_names(), m)
            + borrowed_arc_slice(self.blocks(), m)
            + borrowed_arc_value(self.markup(), m)
    }
}

impl PayloadHeap for inv::SourceSpaceDescriptor {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            id,
            identity,
            content_hash,
            byte_len,
            encoding,
            bytes,
        } = self;
        id.heap(m)
            + identity.heap(m)
            + content_hash.heap(m)
            + byte_len.heap(m)
            + encoding.heap(m)
            + bytes.heap(m)
    }
}

impl PayloadHeap for inv::SourceSpaceIdentity {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::RegisteredSnapshot { snapshot } => snapshot.heap(m),
            Self::DerivedTransformOutput {
                owner,
                origin_fingerprint,
                transform_chain_fingerprint,
                step_index,
            } => {
                owner.heap(m)
                    + origin_fingerprint.heap(m)
                    + transform_chain_fingerprint.heap(m)
                    + step_index.heap(m)
            }
        }
    }
}

impl PayloadHeap for verter_language::registered_source_authority::RegisteredSourceSnapshotId {
    /// Fixed-width digests and counters plus interned language identities,
    /// which are pooled (see the module docs).
    fn heap(&self, _m: &mut Meter) -> usize {
        0
    }
}

impl PayloadHeap for inv::ArtifactBlockRef {
    /// Fields are private: a fixed-width artifact identity and a block id.
    fn heap(&self, m: &mut Meter) -> usize {
        inline(self.artifact_identity()) + self.block_id().heap(m)
    }
}

impl PayloadHeap for inv::NormalizedNameTable {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self { values } = self;
        values.heap(m)
    }
}

impl PayloadHeap for inv::CarrierBlock {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Section { id, role, syntax } => id.heap(m) + role.heap(m) + syntax.heap(m),
            Self::MarkupRoot { id, node } => id.heap(m) + node.heap(m),
        }
    }
}

impl PayloadHeap for inv::SectionRole {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::TemplateHost => 0,
            Self::Script { role, dialect } => role.heap(m) + dialect.heap(m),
            Self::Style {
                dialect,
                scoped,
                module,
            } => dialect.heap(m) + scoped.heap(m) + module.heap(m),
            Self::Custom { normalized_name } => normalized_name.heap(m),
        }
    }
}

impl PayloadHeap for inv::ScriptSourceType {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::JavaScript | Self::TypeScript | Self::Jsx | Self::Tsx | Self::Missing => 0,
            Self::Custom {
                authored,
                normalized,
            } => authored.heap(m) + normalized.heap(m),
        }
    }
}

impl PayloadHeap for inv::StyleDialect {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Css
            | Self::Scss
            | Self::Sass
            | Self::Less
            | Self::Stylus
            | Self::PostCss
            | Self::Missing => 0,
            Self::Custom {
                authored,
                normalized,
            } => authored.heap(m) + normalized.heap(m),
        }
    }
}

impl PayloadHeap for inv::StyleModule {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::None | Self::Default => 0,
            Self::Named { name } => name.heap(m),
        }
    }
}

impl PayloadHeap for inv::TaggedSyntax {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            authored_name,
            normalized_name,
            opening_span,
            opening_name_span,
            attribute_insertion_anchor,
            content_span,
            closing_span,
            closing_name_span,
            full_span,
            termination,
            attributes,
        } = self;
        authored_name.heap(m)
            + normalized_name.heap(m)
            + opening_span.heap(m)
            + opening_name_span.heap(m)
            + attribute_insertion_anchor.heap(m)
            + content_span.heap(m)
            + closing_span.heap(m)
            + closing_name_span.heap(m)
            + full_span.heap(m)
            + termination.heap(m)
            + attributes.heap(m)
    }
}

impl PayloadHeap for inv::SyntaxTermination {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Closed | Self::SelfClosing | Self::Void | Self::UnclosedEof => 0,
            Self::Recovered {
                reason,
                recovery_span,
            } => reason.heap(m) + recovery_span.heap(m),
        }
    }
}

impl PayloadHeap for inv::CarrierAttribute {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Named {
                id,
                name,
                syntax,
                value,
                full_span,
                duplicate_of,
            } => {
                id.heap(m)
                    + name.heap(m)
                    + syntax.heap(m)
                    + value.heap(m)
                    + full_span.heap(m)
                    + duplicate_of.heap(m)
            }
            Self::Spread {
                id,
                full_span,
                open_span,
                expression_span,
                close_span,
                termination,
            }
            | Self::Attach {
                id,
                full_span,
                keyword_span: open_span,
                expression_span,
                close_span,
                termination,
            } => {
                id.heap(m)
                    + full_span.heap(m)
                    + open_span.heap(m)
                    + expression_span.heap(m)
                    + close_span.heap(m)
                    + termination.heap(m)
            }
            Self::Directive {
                id,
                family,
                prefix_span,
                local_name,
                argument,
                modifiers,
                value,
                full_span,
                duplicate_of,
            } => {
                id.heap(m)
                    + family.heap(m)
                    + prefix_span.heap(m)
                    + local_name.heap(m)
                    + argument.heap(m)
                    + modifiers.heap(m)
                    + value.heap(m)
                    + full_span.heap(m)
                    + duplicate_of.heap(m)
            }
        }
    }
}

impl PayloadHeap for inv::AttributeValue {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Missing => 0,
            Self::Static {
                raw,
                decoded,
                quote,
                value_span,
                inner_span,
            } => {
                raw.heap(m)
                    + decoded.heap(m)
                    + quote.heap(m)
                    + value_span.heap(m)
                    + inner_span.heap(m)
            }
            Self::Expression {
                syntax,
                full_span,
                open_span,
                expression_span,
                close_span,
                termination,
            } => {
                syntax.heap(m)
                    + full_span.heap(m)
                    + open_span.heap(m)
                    + expression_span.heap(m)
                    + close_span.heap(m)
                    + termination.heap(m)
            }
            Self::Mixed { full_span, parts } => full_span.heap(m) + parts.heap(m),
        }
    }
}

impl PayloadHeap for inv::AttributeValuePart {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Static { raw, decoded } => raw.heap(m) + decoded.heap(m),
            Self::Expression {
                syntax,
                full_span,
                open_span,
                expression_span,
                close_span,
                termination,
            } => {
                syntax.heap(m)
                    + full_span.heap(m)
                    + open_span.heap(m)
                    + expression_span.heap(m)
                    + close_span.heap(m)
                    + termination.heap(m)
            }
        }
    }
}

impl PayloadHeap for inv::DirectiveFamily {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Vue(kind) => kind.heap(m),
            Self::Svelte(kind) => kind.heap(m),
        }
    }
}

impl PayloadHeap for inv::SvelteDirectiveKind {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Bind
            | Self::On
            | Self::Use
            | Self::Class
            | Self::Style
            | Self::Let
            | Self::Transition
            | Self::In
            | Self::Out
            | Self::Animate
            | Self::Custom => 0,
            Self::Unknown {
                authored_family,
                reason,
            } => authored_family.heap(m) + reason.heap(m),
        }
    }
}

impl PayloadHeap for inv::DirectiveArgument {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::None => 0,
            Self::Static { name } => name.heap(m),
            Self::Dynamic {
                full_span,
                open_span,
                expression_span,
                close_span,
                termination,
            } => {
                full_span.heap(m)
                    + open_span.heap(m)
                    + expression_span.heap(m)
                    + close_span.heap(m)
                    + termination.heap(m)
            }
        }
    }
}

impl PayloadHeap for inv::MarkupSyntaxArena {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            roots,
            nodes,
            child_ids,
        } = self;
        roots.heap(m) + nodes.heap(m) + child_ids.heap(m)
    }
}

impl PayloadHeap for inv::MarkupSyntaxNode {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            id,
            root_block,
            parent,
            children,
            kind,
        } = self;
        id.heap(m)
            + root_block.heap(m)
            + parent.heap(m)
            + children.start.heap(m)
            + children.end.heap(m)
            + kind.heap(m)
    }
}

impl PayloadHeap for inv::MarkupNodeKind {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Element(element) => element.heap(m),
            Self::Text { content_span } => content_span.heap(m),
            Self::Comment {
                opening_span,
                content_span,
                closing_span,
                full_span,
                termination,
            } => {
                opening_span.heap(m)
                    + content_span.heap(m)
                    + closing_span.heap(m)
                    + full_span.heap(m)
                    + termination.heap(m)
            }
            Self::Interpolation {
                family,
                opening_span,
                expression_span,
                closing_span,
                full_span,
                termination,
            } => {
                family.heap(m)
                    + opening_span.heap(m)
                    + expression_span.heap(m)
                    + closing_span.heap(m)
                    + full_span.heap(m)
                    + termination.heap(m)
            }
            Self::SvelteControlBlock(block) => block.heap(m),
            Self::SvelteClause(clause) => clause.heap(m),
            Self::SvelteStandaloneTag(tag) => tag.heap(m),
            Self::Recovered {
                opening_span,
                opening_name_span,
                content_span,
                closing_span,
                closing_name_span,
                full_span,
                termination,
                expected,
                reason,
            } => {
                opening_span.heap(m)
                    + opening_name_span.heap(m)
                    + content_span.heap(m)
                    + closing_span.heap(m)
                    + closing_name_span.heap(m)
                    + full_span.heap(m)
                    + termination.heap(m)
                    + expected.heap(m)
                    + reason.heap(m)
            }
            Self::Unknown {
                opening_span,
                opening_name_span,
                content_span,
                closing_span,
                closing_name_span,
                full_span,
                termination,
                authored_head,
                reason,
            } => {
                opening_span.heap(m)
                    + opening_name_span.heap(m)
                    + content_span.heap(m)
                    + closing_span.heap(m)
                    + closing_name_span.heap(m)
                    + full_span.heap(m)
                    + termination.heap(m)
                    + authored_head.heap(m)
                    + reason.heap(m)
            }
        }
    }
}

impl PayloadHeap for inv::MarkupElementSyntax {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            authored_name,
            normalized_name,
            namespace,
            kind,
            opening_span,
            opening_name_span,
            attribute_insertion_anchor,
            content_span,
            closing_span,
            closing_name_span,
            full_span,
            self_closing,
            void_element,
            raw_text,
            termination,
            attributes,
        } = self;
        authored_name.heap(m)
            + normalized_name.heap(m)
            + namespace.heap(m)
            + kind.heap(m)
            + opening_span.heap(m)
            + opening_name_span.heap(m)
            + attribute_insertion_anchor.heap(m)
            + content_span.heap(m)
            + closing_span.heap(m)
            + closing_name_span.heap(m)
            + full_span.heap(m)
            + self_closing.heap(m)
            + void_element.heap(m)
            + raw_text.heap(m)
            + termination.heap(m)
            + attributes.heap(m)
    }
}

impl PayloadHeap for inv::MarkupElementKind {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Native
            | Self::Component
            | Self::DynamicComponent
            | Self::DynamicElement
            | Self::SvelteNestedStyle
            | Self::Unknown => 0,
            Self::SvelteSpecial(kind) => kind.heap(m),
        }
    }
}

impl PayloadHeap for inv::SvelteSpecialElementKind {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Head
            | Self::Window
            | Self::Document
            | Self::Body
            | Self::Element
            | Self::Boundary
            | Self::Options
            | Self::Component
            | Self::SelfRef
            | Self::Fragment => 0,
            Self::Unknown { authored_local } => authored_local.heap(m),
        }
    }
}

impl PayloadHeap for inv::SvelteControlBlockSyntax {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            head,
            opening_span,
            closing_span,
            full_span,
            termination,
        } = self;
        head.heap(m)
            + opening_span.heap(m)
            + closing_span.heap(m)
            + full_span.heap(m)
            + termination.heap(m)
    }
}

impl PayloadHeap for inv::SvelteControlBlockHead {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::If { condition } => condition.heap(m),
            Self::Each {
                iterable,
                item,
                index,
                key,
            } => iterable.heap(m) + item.heap(m) + index.heap(m) + key.heap(m),
            Self::Await {
                promise,
                inline_branch,
            } => promise.heap(m) + inline_branch.heap(m),
            Self::Key { expression } => expression.heap(m),
            Self::Snippet {
                authored_name,
                name_span,
                params_span,
            } => authored_name.heap(m) + name_span.heap(m) + params_span.heap(m),
        }
    }
}

impl PayloadHeap for inv::SvelteAwaitInlineBranch {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::None => 0,
            Self::Then {
                marker_span,
                head_span,
                binding,
            }
            | Self::Catch {
                marker_span,
                head_span,
                binding,
            } => marker_span.heap(m) + head_span.heap(m) + binding.heap(m),
        }
    }
}

impl PayloadHeap for inv::SvelteClauseSyntax {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            head,
            marker_span,
            full_span,
            termination,
        } = self;
        head.heap(m) + marker_span.heap(m) + full_span.heap(m) + termination.heap(m)
    }
}

impl PayloadHeap for inv::SvelteClauseHead {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Else => 0,
            Self::ElseIf { condition } => condition.heap(m),
            Self::Then { binding } | Self::Catch { binding } => binding.heap(m),
        }
    }
}

impl PayloadHeap for inv::SvelteStandaloneTagSyntax {
    fn heap(&self, m: &mut Meter) -> usize {
        let Self {
            family,
            opening_span,
            expression_span,
            closing_span,
            full_span,
            termination,
        } = self;
        family.heap(m)
            + opening_span.heap(m)
            + expression_span.heap(m)
            + closing_span.heap(m)
            + full_span.heap(m)
            + termination.heap(m)
    }
}

impl PayloadHeap for inv::SvelteStandaloneTagFamily {
    fn heap(&self, m: &mut Meter) -> usize {
        match self {
            Self::Render
            | Self::Html
            | Self::LegacyConst
            | Self::Const
            | Self::Let
            | Self::Debug
            | Self::Attach => 0,
            Self::Unknown {
                authored_name,
                reason,
            } => authored_name.heap(m) + reason.heap(m),
        }
    }
}
