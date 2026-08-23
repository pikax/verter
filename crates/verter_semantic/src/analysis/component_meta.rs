//! Component metadata extraction from analysis snapshots.
//!
//! Pure analysis-domain types and extraction logic for component-meta.
//! This module does NOT depend on `verter_session` — all resolved data
//! is pre-supplied via [`ComponentMetaInput`].
//!
//! # Ownership boundary
//!
//! - All types in [`ComponentMetaInput`] are owned by `verter_semantic::analysis`
//! - The host constructs the input by projecting from its internal snapshots
//! - [`ComponentMetaAnalysis`] is the analysis-domain result (no serde)
//! - Conversion to transport-facing DTOs happens via `verter_protocol` and its adapter layers

use crate::analysis::types::{
    AnalysisFlags, AnalyzedBinding, AnalyzedExposeField, AnalyzedImport, AnalyzedMacro,
    AnalyzedMacroKind, AnalyzedOptionsApi, AnalyzedPropField, AnalyzedSlotField, ImportBindingKind,
    JsdocTag, StoreUsage, VueApiCallSite,
};
use verter_type_expr::facts::{SemanticTypeSource, SourcePosition};
use verter_type_expr::locators::{AuthoredBodyLocator, MacroPayloadLocator};
use verter_type_expr::{
    AuthoredSourceMint, AuthoredTypeEvidence, PublicationPolicy, ResolutionDiagnostic,
    ResolutionDiagnosticKind, ResolutionExactness, ResolutionProvenance, ResolvedTypeAuthority,
    ResolvedTypeOutcome, TypeExprScope, TypePublication,
};
/// The authored SOURCE of an analyzer field's payload position: the
/// content-free locator wrapped as the four-source `Authored` arm. Consumers
/// demand the typed body through the shared dispatch from it.
fn authored_payload_source(payload: Option<&MacroPayloadLocator>) -> Option<SemanticTypeSource> {
    payload.map(|locator| {
        SemanticTypeSource::Authored(AuthoredBodyLocator::MacroPayload(locator.clone()))
    })
}

/// The authored source POSITION of an analyzer field's payload: the stamped
/// authored locator when the analyzer recorded one, else the PROVEN
/// unannotated schema absence — the analyzer's payload slot IS the authored
/// annotation record, so its absence is structural (an array-form emit, a
/// runtime prop, an untyped binding), never a failure.
fn authored_payload_position(payload: Option<&MacroPayloadLocator>) -> SourcePosition {
    authored_payload_source(payload)
        .map(SourcePosition::Present)
        .unwrap_or_else(SourcePosition::unannotated)
}

fn authored_type_evidence(
    payload: Option<&MacroPayloadLocator>,
    text: Option<&str>,
) -> Option<AuthoredTypeEvidence> {
    // SAFETY: this helper is the analyzer-row producer join: `payload` and
    // `text` are read from the same `Analyzed*Field`.
    let mint = unsafe { AuthoredSourceMint::new_unchecked() };
    Some(AuthoredTypeEvidence::from_macro_payload(
        &mint,
        payload?,
        std::sync::Arc::from(text?),
    ))
}

fn exactness_from_expansion(
    metadata: Option<&crate::analysis::type_expand::ExpansionMetadata>,
    position: &SourcePosition,
) -> ResolutionExactness {
    metadata.map_or_else(
        || match position.present() {
            Some(
                SemanticTypeSource::Closed(_)
                | SemanticTypeSource::Projected(_)
                | SemanticTypeSource::Synthesized(_),
            ) => ResolutionExactness::ExactConcrete,
            Some(SemanticTypeSource::Authored(_) | SemanticTypeSource::SyntheticSlotBinding(_)) => {
                ResolutionExactness::ExactSymbolic
            }
            None => ResolutionExactness::Incomplete,
        },
        |metadata| match metadata.exactness {
            crate::analysis::type_expand::ExpansionExactness::ExactConcrete => {
                ResolutionExactness::ExactConcrete
            }
            crate::analysis::type_expand::ExpansionExactness::ExactSymbolic => {
                ResolutionExactness::ExactSymbolic
            }
            crate::analysis::type_expand::ExpansionExactness::Incomplete => {
                ResolutionExactness::Incomplete
            }
        },
    )
}

fn diagnostic_kind(
    reason: crate::analysis::type_expand::ExpansionStopReason,
) -> ResolutionDiagnosticKind {
    use crate::analysis::type_expand::ExpansionStopReason as Source;
    match reason {
        Source::BudgetExceeded => ResolutionDiagnosticKind::BudgetExceeded,
        Source::ProjectionWorkLimit => ResolutionDiagnosticKind::ProjectionWorkLimit,
        Source::ConnectedQueryDepthLimit => ResolutionDiagnosticKind::ConnectedQueryDepthLimit,
        Source::MappedDepthExceeded => ResolutionDiagnosticKind::MappedDepthExceeded,
        Source::UnresolvedReference => ResolutionDiagnosticKind::UnresolvedReference,
        Source::IndeterminateConditional => ResolutionDiagnosticKind::IndeterminateConditional,
        Source::InfiniteKeySpace => ResolutionDiagnosticKind::InfiniteKeySpace,
        Source::UnsupportedOperator => ResolutionDiagnosticKind::UnsupportedOperator,
        Source::ConditionalContextTruncated => {
            ResolutionDiagnosticKind::ConditionalContextTruncated
        }
        Source::IdempotentArm => ResolutionDiagnosticKind::IdempotentArm,
        Source::CyclicReference => ResolutionDiagnosticKind::CyclicReference,
        Source::CyclicInstantiation => ResolutionDiagnosticKind::CyclicInstantiation,
        Source::InstantiationError => ResolutionDiagnosticKind::InstantiationError,
        Source::EmptyUnionArm => ResolutionDiagnosticKind::EmptyUnionArm,
    }
}

fn diagnostics_from_expansion(
    metadata: Option<&crate::analysis::type_expand::ExpansionMetadata>,
) -> std::sync::Arc<[ResolutionDiagnostic]> {
    metadata.map_or_else(
        || std::sync::Arc::from([]),
        |metadata| {
            metadata
                .diagnostics
                .iter()
                .map(|diagnostic| ResolutionDiagnostic {
                    kind: diagnostic_kind(diagnostic.reason),
                    context: std::sync::Arc::from(diagnostic.context.as_str()),
                    property_name: diagnostic
                        .property_name
                        .as_deref()
                        .map(std::sync::Arc::from),
                })
                .collect::<Vec<_>>()
                .into()
        },
    )
}

fn publication_from_position(
    position: &SourcePosition,
    metadata: Option<&crate::analysis::type_expand::ExpansionMetadata>,
    evidence: Option<AuthoredTypeEvidence>,
    provenance: ResolutionProvenance,
) -> TypePublication {
    TypePublication::from_source_position(
        position,
        exactness_from_expansion(metadata, position),
        provenance,
        diagnostics_from_expansion(metadata),
        evidence,
        &PublicationPolicy::exact_only(),
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Input view
// ═══════════════════════════════════════════════════════════════════════════

/// One host-resolved prop row: the prop analysis field plus the member
/// VALUE's session-resolved SOURCE POSITION — the prop-type authority the
/// extraction publishes (the flat evaluated lanes contribute metadata only).
#[derive(Debug, Clone)]
pub struct ResolvedPropInput {
    /// The prop analysis field.
    pub field: AnalyzedPropField,
    /// The member value's published source position.
    pub authority: ResolvedTypeAuthority,
    pub authored_evidence: Option<AuthoredTypeEvidence>,
    /// Typed callable role established by the session resolver.
    pub callable_role: verter_type_expr::PropCallableRole,
}

/// One host-resolved emit row: the emit analysis field plus the payload's
/// session-resolved SOURCE POSITION — the emit-payload authority the
/// extraction publishes (the flat evaluated lanes contribute metadata only).
#[derive(Debug, Clone)]
pub struct ResolvedEmitInput {
    /// Resolver-minted opaque producer/name-arm identity.
    pub id: verter_type_expr::facts::ResolvedEmitOccurrenceId,
    /// Event-name literal arm.
    pub name: String,
    /// Exact authored producer span when available.
    pub span: verter_span::Span,
    /// Display-only payload type.
    pub payload_type: Option<String>,
    /// Exact authored payload locator when available.
    pub payload: Option<verter_type_expr::locators::MacroPayloadLocator>,
    /// Scope paired with [`Self::payload_type`].
    pub payload_expr_scope: Option<TypeExprScope>,
    /// Authored producer description.
    pub description: Option<String>,
    /// Authored producer JSDoc tags.
    pub tags: Vec<crate::analysis::types::JsdocTag>,
    /// The payload's published source position.
    pub payload_source: SourcePosition,
    /// Atomic payload publication owned by this occurrence.
    pub payload_publication: TypePublication,
    /// Producer-owned callable return publication. Property/event-map rows use
    /// the implicit `void` return and carry `None`.
    pub return_publication: Option<TypePublication>,
    /// Scope used to raise [`Self::return_publication`]'s selected source.
    pub return_publication_scope: Option<TypeExprScope>,
}

/// One host-resolved expose row: the expose analysis field plus the member
/// VALUE's session-resolved SOURCE POSITION — the exposed-type authority the
/// extraction publishes for type-argument surface members.
#[derive(Debug, Clone)]
pub struct ResolvedExposeInput {
    /// The expose analysis field.
    pub field: AnalyzedExposeField,
    /// The member value's published source position.
    pub type_source: SourcePosition,
}

/// One host-resolved script binding's whole-return reactive-wrapper role.
///
/// The session resolves the role of a composable call's AUTHORED return
/// annotation on demand, through the one shared type-resolution engine, and
/// hands the closed typed answer to the extraction. `binding_index` addresses
/// [`ComponentMetaInput::bindings`] positionally; a binding with no row was
/// never demanded (a distinct state from a demanded-and-degraded row, which
/// carries `ReactiveWrapperRole::Unresolved { reason }`).
#[derive(Debug, Clone)]
pub struct ResolvedBindingReactivityInput {
    /// Index into [`ComponentMetaInput::bindings`].
    pub binding_index: usize,
    /// The resolved whole-return wrapper role — exact, a completed
    /// non-wrapper proof, or a typed degradation.
    pub return_wrapper_role: verter_type_expr::ReactiveWrapperRole,
}

/// Input view for component-meta extraction.
///
/// All fields reference existing `verter_semantic::analysis` types.
/// The host constructs this by projecting from its internal snapshot; the
/// prop / emit / expose rows carry the session-resolved SOURCE POSITIONS the
/// normalized macro surface produced — the extraction's source authority.
#[derive(Debug, Clone, Default)]
pub struct ResolvedMacroInput {
    /// Index of the macro in `ComponentMetaInput.macros`.
    pub macro_index: usize,
    /// Host-resolved props for the macro, each with its member-value source.
    pub props: Vec<ResolvedPropInput>,
    /// Host-resolved emits for the macro, each with its payload source.
    pub emits: Vec<ResolvedEmitInput>,
    /// Host-resolved slots for the macro.
    pub slots: Vec<AnalyzedSlotField>,
    /// Producer-owned return publications aligned one-for-one with [`Self::slots`].
    /// `None` is the proven no-return-source case; a typed failure remains a
    /// `Some(TypePublication)` and must not be collapsed into absence.
    pub slot_return_publications: Vec<Option<TypePublication>>,
    /// Host-resolved exposed rows for the macro (`defineExpose<T>()`
    /// type-argument surface members with their span-sliced JSDoc), each
    /// with its member-value source.
    pub exposed: Vec<ResolvedExposeInput>,
    /// Producer-owned prop names with authored runtime defaults.
    pub default_keys: Vec<String>,
}

pub struct ComponentMetaInput<'a> {
    pub macros: &'a [AnalyzedMacro],
    pub bindings: &'a [AnalyzedBinding],
    pub imports: &'a [AnalyzedImport],
    pub template: Option<&'a crate::analysis::template::TemplateAnalysisSnapshot>,
    pub options_api: Option<&'a AnalyzedOptionsApi>,
    pub analysis_flags: AnalysisFlags,
    pub styles: &'a [crate::analysis::style::StyleBlockAnalysis],
    pub vue_api_calls: &'a [VueApiCallSite],
    pub store_usages: &'a [StoreUsage],
    pub resolved_macros: &'a [ResolvedMacroInput],
    /// Session-resolved whole-return wrapper roles for the composable-call
    /// bindings the host demanded. Empty when nothing was demanded.
    pub resolved_binding_reactivity: &'a [ResolvedBindingReactivityInput],
    pub resolved_type_registry: &'a [ResolvedTypeAnalysis],
    pub evaluated_types: Option<&'a crate::analysis::type_expand::ExpandedComponentTypes>,
    pub file_path: &'a str,
}

// ═══════════════════════════════════════════════════════════════════════════
// Domain result types
// ═══════════════════════════════════════════════════════════════════════════

/// Analysis-domain component metadata. No serde — only used in Rust.
/// Converted to `FfiComponentMeta` at the NAPI/WASM boundary.
#[derive(Debug, Clone)]
pub struct ComponentMetaAnalysis {
    pub props: Vec<PropAnalysis>,
    pub events: Vec<EventAnalysis>,
    pub slots: Vec<SlotAnalysis>,
    pub models: Vec<ModelAnalysis>,
    pub exposed: Vec<ExposedAnalysis>,
    /// Host-populated public-instance sidecar derived from runtime-observable members.
    pub public_instance: Option<PublicInstanceAnalysis>,
    /// Host-populated, content-free structure projected from the registered artifact.
    pub ordered_sfc_structure: Option<OrderedSfcStructureAnalysis>,
    pub type_registry: Vec<ResolvedTypeAnalysis>,
    pub components: Vec<ComponentUsageAnalysis>,
    pub template_refs: Vec<TemplateRefAnalysis>,
    pub imports: Vec<ImportAnalysis>,
    pub bindings: Vec<BindingAnalysis>,
    pub vue_api_calls: Vec<VueApiCallAnalysis>,
    pub styles: Vec<StyleAnalysis>,
    pub flags: ComponentMetaFlags,
    /// Root reachability classification for fallthrough inheritance.
    /// Extracted from template facts only — host owns all inheritance semantics.
    pub root_reachability: RootReachability,
    /// Accepted props: declared props + inherited attrs (host-populated).
    pub accepted_props: Vec<AcceptedPropAnalysis>,
    /// Accepted events: declared emits + inherited listeners (host-populated).
    pub accepted_events: Vec<AcceptedEventAnalysis>,
    /// Whether the accepted surface is exact or a lower bound.
    pub accepted_surface_completeness: AcceptedSurfaceCompleteness,
    /// Branch-structured inherited surface (host-populated).
    pub fallthrough_surface: FallthroughSurface,
    /// Macro-wide expansion diagnostics that apply to the entire macro, not to a
    /// specific property. Lifted out of per-field `type_expansion.diagnostics` to
    /// avoid duplication across every prop/event/slot in the same macro.
    pub macro_expansion_diagnostics: Vec<MacroExpansionDiagnostics>,
    pub options_api: bool,
    pub file_path: String,
}

/// Which macro kind produced the expansion diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroExpansionKind {
    DefineProps,
    DefineEmits,
    DefineSlots,
}

/// Macro-wide expansion diagnostics that are not specific to any property.
/// Stored once per macro instead of duplicated on every field.
#[derive(Debug, Clone)]
pub struct MacroExpansionDiagnostics {
    pub macro_kind: MacroExpansionKind,
    pub macro_index: usize,
    pub diagnostics: Vec<crate::analysis::type_expand::ExpansionDiagnostic>,
    pub exactness: crate::analysis::type_expand::ExpansionExactness,
    pub execution_status: crate::analysis::type_expand::ExpansionExecutionStatus,
}

/// Analyzed prop from `defineProps` / Options API `props`.
#[derive(Debug, Clone)]
pub struct PropAnalysis {
    pub name: String,
    /// Typed callable role; display text never participates in classification.
    pub callable_role: verter_type_expr::PropCallableRole,
    /// The resolved type SOURCE POSITION: the evaluated source unless the
    /// expansion is incomplete and an authored payload exists (the symbolic
    /// fallback); a PROVEN schema absence when the position carries no
    /// annotation; a typed failure when the REQUIRED value position's source
    /// could not be constructed (fails output materialization).
    pub publication: TypePublication,
    /// Completeness and diagnostics from native expansion when available.
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    /// The author's own annotation is carried separately as bundled evidence
    /// inside `publication`.
    pub required: bool,
    pub has_default: bool,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<JsdocTag>,
    /// True iff the SFC author explicitly wrote this prop name as a member
    /// of the `defineProps<T>()` type argument's own body. Propagates the
    /// per-prop provenance fact carried by [`AnalyzedPropField`] so the
    /// `verter_audit::PublishedSurfacePolicy::Refined` projection can
    /// distinguish author-declared names (Vue intrinsics like `class` /
    /// `style` and `on{Event}` shadows of declared emits the author *kept*
    /// on purpose) from names that arrived via heritage / utility-type
    /// expansion (HTMLAttributes inheritance, etc.).
    pub declared_in_macro_type_arg: bool,
}

/// Analyzed event from `defineEmits`.
#[derive(Debug, Clone)]
pub struct EventAnalysis {
    pub name: String,
    /// Legacy semantic source lane retained for accepted/fallthrough mechanics.
    /// Public contract consumers use `publication`.
    pub payload: SourcePosition,
    /// Producer-owned payload publication. This retains typed authority,
    /// exactness, diagnostics, and provenance through terminal materialization.
    pub publication: TypePublication,
    /// Producer-owned callable return publication. `None` denotes the
    /// property/event-map implicit `void` return.
    pub return_publication: Option<TypePublication>,
    /// Scope used to raise [`Self::return_publication`]'s selected source.
    pub return_publication_scope: Option<TypeExprScope>,
    pub payload_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    pub raw_signature: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<JsdocTag>,
}

/// Analyzed slot from `defineSlots` / template.
#[derive(Debug, Clone)]
pub struct SlotAnalysis {
    pub name: String,
    pub is_scoped: bool,
    pub bindings: Vec<SlotBindingAnalysis>,
    pub is_required: bool,
    pub return_type: Option<String>,
    /// Producer-owned typed return publication. Source outcome, exactness,
    /// diagnostics, provenance, and authored evidence travel together to the
    /// terminal output sink.
    pub return_publication: Option<TypePublication>,
    /// Scope used to raise [`Self::return_publication`]'s selected source.
    pub return_publication_scope: Option<TypeExprScope>,
    pub description: Option<String>,
    pub tags: Vec<JsdocTag>,
    /// Producer fact: does this slot come from the component's own AUTHORED
    /// slots surface — a member of the resolved `defineSlots<T>()` macro
    /// surface (inline body, referenced interface, or its heritage) or a
    /// template `<slot>` element? `false` only for rows arriving purely
    /// through the evaluated type-expansion channel with no authored
    /// counterpart — the residual channel VNode-transport keys could leak
    /// through. Consumed by `@verter/component-meta/published-surface`'s
    /// `Compat` / `Refined` slot blocklist (an author-declared slot is
    /// never blocked, whatever its name).
    pub declared_in_macro_type_arg: bool,
}

/// A single binding property on a scoped slot.
#[derive(Debug, Clone)]
pub struct SlotBindingAnalysis {
    pub name: String,
    /// The resolved binding type SOURCE POSITION (`Absent` = display text
    /// only; the typed binding channel is host-raised).
    pub publication: TypePublication,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
}

/// Analyzed model from `defineModel`.
#[derive(Debug, Clone)]
pub struct ModelAnalysis {
    pub name: String,
    /// The model value's resolved type SOURCE POSITION (`Absent` = untyped
    /// model).
    pub type_source: SourcePosition,
}

/// Analyzed exposed member from `defineExpose`.
#[derive(Debug, Clone)]
pub struct ExposedAnalysis {
    pub name: String,
    /// The exposed member's resolved type SOURCE POSITION (`Absent` =
    /// untyped binding).
    pub type_source: SourcePosition,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    pub description: Option<String>,
    /// JSDoc tags from the exposed member's leading `/** ... */` block.
    pub tags: Vec<JsdocTag>,
}

/// Host-populated public-instance sidecar exposed by the official API.
#[derive(Debug, Clone)]
pub struct PublicInstanceAnalysis {
    pub members: Vec<PublicInstanceMemberAnalysis>,
    pub completeness: PublicInstanceCompleteness,
}

/// A single runtime-observable public-instance member.
#[derive(Debug, Clone)]
pub struct PublicInstanceMemberAnalysis {
    pub name: String,
    pub kind: PublicInstanceMemberKind,
    /// The member's resolved type SOURCE POSITION (`Absent` = untyped
    /// member).
    pub type_source: SourcePosition,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    pub raw_type: Option<String>,
    pub description: Option<String>,
    /// JSDoc tags carried onto the public member from its source surface.
    pub tags: Vec<JsdocTag>,
}

/// Content-free schema-8 structure authority. Token arrays are indexed only
/// by the corresponding canonical local IDs; public identity is the token.
#[derive(Debug, Clone)]
pub struct OrderedSfcStructureAnalysis {
    pub schema_version: u32,
    pub artifact_token: String,
    pub inventory:
        std::sync::Arc<verter_language::parse_artifact::carrier_inventory::CarrierBlockInventory>,
    pub source_space_tokens: std::sync::Arc<[String]>,
    pub block_tokens: std::sync::Arc<[String]>,
    pub markup_node_tokens: std::sync::Arc<[String]>,
    pub attribute_tokens: std::sync::Arc<[String]>,
}

/// What kind of member this public-instance entry represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicInstanceMemberKind {
    Prop,
    SlotContainer,
    Exposed,
}

/// Whether the host believes the surfaced public-instance contract is complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicInstanceCompleteness {
    Exact,
    Partial,
}

/// A named resolved type available for schema expansion.
#[derive(Debug, Clone)]
pub struct ResolvedTypeAnalysis {
    pub name: String,
    /// The registry entry's resolved type SOURCE POSITION (registry entries
    /// always resolve a present source; the position type keeps the lane
    /// uniform with every other materialized output lane).
    pub type_source: SourcePosition,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
}

/// A component usage discovered in the template.
#[derive(Debug, Clone)]
pub struct ComponentUsageAnalysis {
    pub name: String,
    pub import_source: Option<String>,
    pub is_dynamic: bool,
    pub props: Vec<ComponentPropUsageAnalysis>,
    pub has_spread: bool,
    pub slots_used: Vec<String>,
    pub static_classes: Vec<String>,
    pub has_dynamic_class: bool,
    pub v_models: Vec<String>,
    pub v_model_entries: Vec<ComponentVModelUsageAnalysis>,
    /// Framework-neutral two-way bindings (the Svelte `bind:` family). Empty for
    /// Vue.
    pub bindings: Vec<ComponentBindingUsageAnalysis>,
    /// Framework-neutral events (the legacy Svelte `on:` directive only — a
    /// plain `on*` attribute is a prop, never an event). Empty for Vue.
    pub events: Vec<ComponentEventUsageAnalysis>,
}

/// A two-way binding passed to a child component (the Svelte `bind:` family).
#[derive(Debug, Clone)]
pub struct ComponentBindingUsageAnalysis {
    /// The bound local member name (`value` in `bind:value`).
    pub name: String,
    /// The `|modifier` list, in source order.
    pub modifiers: Vec<String>,
}

/// An event listened on a child component via the legacy Svelte `on:`
/// directive. A plain `on*` attribute is a prop, never an event (the
/// props/events split is syntactic — the child component-meta, not a name
/// guess, decides which passed props are callback events).
#[derive(Debug, Clone)]
pub struct ComponentEventUsageAnalysis {
    /// The event name — the legacy directive local (`click` from `on:click`).
    pub name: String,
    /// The handler expression text, when present.
    pub handler_expression: Option<String>,
    /// Whether the handler is an inline function expression.
    pub is_inline: bool,
    /// The `|modifier` list, in source order.
    pub modifiers: Vec<String>,
}

/// A single prop passed to a child component in the template.
#[derive(Debug, Clone)]
pub struct ComponentPropUsageAnalysis {
    pub name: String,
    pub is_bound: bool,
    pub constness: crate::analysis::template::PropValueConstness,
    pub expression: Option<String>,
    pub referenced_bindings: Vec<String>,
    pub from_spread: bool,
    pub is_shorthand: bool,
}

/// A v-model directive used on a child component in the template.
#[derive(Debug, Clone)]
pub struct ComponentVModelUsageAnalysis {
    pub binding_name: String,
}

/// A template ref usage.
#[derive(Debug, Clone)]
pub struct TemplateRefAnalysis {
    pub name: String,
    pub is_dynamic: bool,
    pub target_tag: String,
}

/// A script import.
#[derive(Debug, Clone)]
pub struct ImportAnalysis {
    pub source: String,
    pub is_type_only: bool,
    pub bindings: Vec<ImportBindingAnalysis>,
}

/// A single imported binding.
#[derive(Debug, Clone)]
pub struct ImportBindingAnalysis {
    pub name: String,
    pub kind: ImportBindingKind,
    pub imported_name: Option<String>,
    pub is_type_only: bool,
}

/// Declaration kind for a script binding in the component-meta result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKindAnalysis {
    Const,
    Let,
    Var,
    Function,
    AsyncFunction,
    Class,
}

/// A script-level binding.
#[derive(Debug, Clone)]
pub struct BindingAnalysis {
    pub name: String,
    pub kind: BindingKindAnalysis,
    pub reactivity_kind: crate::analysis::types::ReactivityKind,
    /// The EXACTNESS carrier for a composable binding's whole-return type.
    ///
    /// [`Self::reactivity_kind`] is a collapsed decoration vocabulary with no
    /// degraded arm, so it cannot distinguish "proven `Ref`" from "proven not a
    /// Vue wrapper" from "could not be resolved, and here is why". This field
    /// carries that distinction:
    ///
    /// - `None` — no whole-return role was demanded for this binding (it is not
    ///   a whole-value composable call, or the value-space walk already decided
    ///   it). NOT a claim about reactivity.
    /// - `Some(Ref | ShallowRef | ComputedRef | ModelRef | Reactive |
    ///   ShallowReactive)` — the exact package-backed `vue` wrapper family the
    ///   callee's authored return annotation resolves to.
    /// - `Some(ReactiveWrapperRole::None)` — a COMPLETED proof that the return
    ///   type is not a Vue wrapper. This is not a proof of non-reactivity:
    ///   `reactive()` returns `UnwrapNestedRefs<T>`, not `Reactive<T>`, so it
    ///   never downgrades [`Self::reactivity_kind`].
    /// - `Some(ReactiveWrapperRole::Unresolved { reason })` — a typed
    ///   degradation with its exact reason.
    pub return_wrapper_role: Option<verter_type_expr::ReactiveWrapperRole>,
    pub type_annotation: Option<String>,
    pub used_in_template: bool,
    pub used_in_style: bool,
}

/// A Vue API call site.
#[derive(Debug, Clone)]
pub struct VueApiCallAnalysis {
    pub api: crate::analysis::types::VueApiClassification,
    pub arg_value: Option<String>,
}

/// Analysis of a single style block.
#[derive(Debug, Clone)]
pub struct StyleAnalysis {
    pub lang: crate::analysis::style::StyleAnalysisLang,
    pub scoped: bool,
    pub is_module: bool,
    pub module_name: Option<String>,
    /// Sealed artifact-bound block identity carried through from the style
    /// analysis; the wire boundary revalidates it against the ordered
    /// structure before minting a public block token.
    pub block_ref: Option<verter_language::parse_artifact::carrier_inventory::ArtifactBlockRef>,
    pub classes: Vec<String>,
    pub ids: Vec<String>,
    pub custom_properties: Vec<String>,
    pub v_binds: Vec<String>,
    pub selectors: Vec<SelectorAnalysis>,
}

/// A CSS selector plus specificity.
#[derive(Debug, Clone)]
pub struct SelectorAnalysis {
    pub text: String,
    pub specificity: (u32, u32, u32),
}

/// Capability flags derived from script analysis.
#[derive(Debug, Clone, Default)]
pub struct ComponentMetaFlags {
    pub async_setup: bool,
    pub has_reactive_state: bool,
    pub has_computed: bool,
    pub has_watchers: bool,
    pub has_lifecycle_hooks: bool,
    pub has_provide: bool,
    pub has_inject: bool,
    pub has_inherit_attrs_false: bool,
    pub has_store_usage: bool,
    /// D123 — marks a macro-impacting lowering failure (a
    /// `verter_session::owned_artifacts::eval_program::LoweringError`).
    /// Currently always `false`: `extract_flags` emits the default and
    /// no production path flips it; consumers detect macro-impacting
    /// failures via the structured `macro_expansion_diagnostics`
    /// entries instead. Per D117, the public `getComponentMeta` API
    /// still returns `Option<ComponentMetaPayload>` (NOT `Result`);
    /// macro-impacting lowering failures surface as a populated
    /// payload, so NAPI does not throw exceptions.
    pub has_macro_failure: bool,
}

// ═══════════════════════════════════════════════════════════════════════════
// Root Reachability
// ═══════════════════════════════════════════════════════════════════════════

/// Classification of a component's template root structure for fallthrough
/// inheritance resolution. Extracted from `TemplateAnalysisSnapshot` facts only.
///
/// The host-owned resolver uses this to determine whether and how fallthrough
/// inheritance applies. Analysis extracts facts; the host owns all semantics.
#[derive(Debug, Clone, PartialEq)]
pub enum RootReachability {
    /// No fallthrough inheritance is possible.
    NoFallthrough { reason: NoFallthroughReason },
    /// One or more conditional branches, each with exactly one root target.
    /// A single-element vec means an unconditional single root.
    Branches { branches: Vec<RootBranch> },
}

/// Why a component has no fallthrough surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoFallthroughReason {
    /// `defineOptions({ inheritAttrs: false })` or Options API `inheritAttrs: false`.
    InheritAttrsFalse,
    /// Multiple unconditional root elements (fragment).
    MultiRoot,
    /// A conditional branch does not resolve to exactly one root target.
    BranchNotSingleRoot,
    /// Root element has `v-for` (produces multiple DOM nodes).
    RootVFor,
    /// No `<template>` block in the SFC.
    NoTemplate,
    /// `<template>` exists but has no children.
    EmptyTemplate,
    /// Root children are only text/interpolation nodes (no element root).
    TextOrInterpolationRoot,
}

/// A single root render branch in a component's template.
#[derive(Debug, Clone, PartialEq)]
pub struct RootBranch {
    /// Branch index in normalized source order (after transparent `<template v-if>` expansion).
    pub branch_index: u16,
    /// Condition text for diagnostics and UI display only. Serialized to FFI/JSON for
    /// debugging purposes but never used for identity, hashing, equality, or cache keys.
    /// Semantic identity comes from `branch_index` / `branch_key`, not condition text.
    pub condition_text: Option<String>,
    /// What the root target is.
    pub target: RootTargetRef,
    /// Attrs/listeners explicitly consumed on the root element.
    pub consumed: ConsumedRootBindings,
    /// Whether `v-bind="obj"` spread (without argument) is used on the root.
    pub has_unknown_spread: bool,
}

/// The kind of root render target.
#[derive(Debug, Clone, PartialEq)]
pub enum RootTargetRef {
    /// Native HTML element (e.g., `<div>`, `<input>`).
    NativeElement {
        /// Index into `TemplateAnalysisSnapshot.elements`.
        element_index: u32,
        /// Tag name (lowercase).
        tag: String,
    },
    /// Dynamic `<component :is>` root with a stable link to `TemplateComponentUsage`.
    DynamicComponentUsage {
        /// Index into `TemplateAnalysisSnapshot.elements`.
        element_index: u32,
        /// Index into `TemplateAnalysisSnapshot.components`.
        usage_index: u32,
    },
    /// Resolved component with a stable link to `TemplateComponentUsage`.
    ComponentUsage {
        /// Index into `TemplateAnalysisSnapshot.elements`.
        element_index: u32,
        /// Index into `TemplateAnalysisSnapshot.components`.
        usage_index: u32,
        /// PascalCase component name.
        name: String,
        /// Import source path for cross-file resolution.
        import_source: Option<String>,
    },
    /// Dynamic, slot, built-in, or otherwise unresolvable root target.
    UnresolvedTarget {
        /// Index into `TemplateAnalysisSnapshot.elements`.
        element_index: u32,
        /// Tag name as written.
        tag: String,
        /// Why this target cannot be resolved.
        reason: UnresolvedRootTargetReason,
    },
}

/// Why a root target cannot be resolved for fallthrough inheritance.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnresolvedRootTargetReason {
    /// `<component :is="...">` — dynamic component.
    DynamicComponentIs,
    /// `<slot>` outlet as root.
    SlotOutlet,
    /// Vue built-in with special render behavior (Teleport, Transition, etc.).
    UnsupportedBuiltin { tag: String },
    /// Component element without a matching `TemplateComponentUsage` entry.
    MissingUsageLink,
    /// Component whose import source could not be resolved.
    UnresolvedImport,
    /// Catch-all for unrecognized root target patterns.
    UnknownRootTarget,
}

/// Attrs/listeners explicitly bound on the root element (consumed, not inherited).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConsumedRootBindings {
    /// Static attr names consumed on the root (e.g., `disabled`, `placeholder`).
    /// Does NOT include `class` or `style` (Vue always merges those).
    pub attrs: Vec<String>,
    /// Canonical listener names consumed on the root (e.g., `click` from `@click` or `:onClick`).
    pub listeners: Vec<String>,
    /// Whether a computed/dynamic attr name is bound (e.g., `:[expr]`).
    /// When true, the branch is a lower bound — some consumed attrs are unknown.
    pub has_dynamic_attr_name: bool,
    /// Whether a computed/dynamic listener name is bound (e.g., `@[expr]`, `v-on="obj"`,
    /// or spread with unknown keys). When true, the branch is a lower bound.
    pub has_dynamic_listener_name: bool,
}

/// Why generic-root specialization could not resolve a concrete instantiation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GenericResolutionFailure {
    SpreadInput,
    DynamicKey,
    MissingType,
    UnsupportedExpression,
    MissingUsageLink,
    UnresolvedChildGenericSurface,
}

/// Known lower-bound causes for a partially resolved fallthrough branch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PartialBranchReason {
    DynamicAttrName,
    DynamicListenerName,
    UnknownSpread,
    GenericResolution { failure: GenericResolutionFailure },
}

/// Why a fallthrough branch could not be resolved at all.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnresolvedBranchReason {
    Cycle { canonical_id: String },
    DynamicComponentIs,
    ChildResolutionFailed,
    UnresolvedChildImport { import_source: Option<String> },
    RootTarget { reason: UnresolvedRootTargetReason },
    GenericResolution { failure: GenericResolutionFailure },
}

// ═══════════════════════════════════════════════════════════════════════════
// Fallthrough Inheritance Types (host-populated)
// ═══════════════════════════════════════════════════════════════════════════

/// How a member arrived on the accepted surface.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberProvenance {
    /// Member is declared locally (defineProps / defineEmits / Options API).
    Declared,
    /// Member is inherited from one or more fallthrough sources.
    Inherited { sources: Vec<InheritedSource> },
}

/// A single inheritance source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum InheritedSource {
    /// Inherited from a native HTML element.
    NativeTag { tag: String },
    /// Inherited from a child component.
    Component { canonical_id: String },
}

/// Whether a member is always available or only in certain branches.
#[derive(Debug, Clone, PartialEq)]
pub enum MemberAvailability {
    /// Available in all branches (unconditional single-root or all branches).
    Always,
    /// Available only in specific branches.
    Conditional { branch_keys: Vec<String> },
}

/// Kind of accepted prop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptedPropKind {
    /// Locally declared prop.
    DeclaredProp,
    /// Inherited HTML attribute.
    Attr,
}

/// Kind of accepted event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptedEventKind {
    /// Locally declared emit.
    DeclaredEmit,
    /// Inherited native listener.
    Listener,
}

/// Whether the accepted surface is exact or only a lower bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptedSurfaceCompleteness {
    /// All accepted members are known exactly.
    Exact,
    /// Some members may be missing due to unresolved or partial branches.
    LowerBound,
}

/// An accepted prop on the computed call-site surface.
#[derive(Debug, Clone)]
pub struct AcceptedPropAnalysis {
    pub name: String,
    /// Typed callable role preserved through accepted-surface inheritance.
    pub callable_role: verter_type_expr::PropCallableRole,
    /// The accepted prop's resolved type SOURCE POSITION (`Absent` =
    /// untyped / cross-branch divergent).
    pub publication: TypePublication,
    /// The canonical file scope `type_source`'s SCOPE-RELATIVE names (bare
    /// `Ref` leaf spellings, producer-local anchors at any nesting depth)
    /// resolve under — the PRODUCING owner of an inherited source, carried
    /// positionally per the cross-owner effective-scope invariant. `None` =
    /// the analysis owner itself (own/declared rows, intrinsic attr rows).
    pub type_source_scope: Option<String>,
    pub required: bool,
    pub provenance: MemberProvenance,
    pub availability: MemberAvailability,
    pub kind: AcceptedPropKind,
}

/// An accepted event on the computed call-site surface.
#[derive(Debug, Clone)]
pub struct AcceptedEventAnalysis {
    pub name: String,
    /// The accepted event's resolved payload SOURCE POSITION (`Absent` =
    /// untyped / cross-branch divergent).
    pub payload: SourcePosition,
    /// The canonical file scope `payload`'s SCOPE-RELATIVE names resolve
    /// under — the PRODUCING owner of an inherited source, carried
    /// positionally (see [`AcceptedPropAnalysis::type_source_scope`]).
    /// `None` = the analysis owner itself.
    pub payload_scope: Option<String>,
    pub raw_signature: Option<String>,
    pub provenance: MemberProvenance,
    pub availability: MemberAvailability,
    pub kind: AcceptedEventKind,
}

/// The branch-structured inherited surface. Declared members do NOT appear here.
#[derive(Debug, Clone, PartialEq)]
pub enum FallthroughSurface {
    /// No fallthrough inheritance.
    None { reason: NoFallthroughReason },
    /// Branch-structured inherited props and events.
    Branches { branches: Vec<FallthroughBranch> },
}

/// An inherited prop entry in a fallthrough branch.
#[derive(Debug, Clone, PartialEq)]
pub struct FallthroughPropEntry {
    pub name: String,
    /// Typed callable role inherited from the producing prop.
    pub callable_role: verter_type_expr::PropCallableRole,
    /// The inherited prop's resolved type SOURCE POSITION (`Absent` =
    /// untyped).
    pub publication: TypePublication,
    /// The canonical file scope `type_source`'s SCOPE-RELATIVE names resolve
    /// under — the PRODUCING owner (the terminal origin of a multi-hop
    /// inheritance chain), carried positionally per the cross-owner
    /// effective-scope invariant. `None` = the intrinsic/native case with no
    /// producing file (the branch owner's scope applies).
    pub type_source_scope: Option<String>,
    pub sources: Vec<InheritedSource>,
}

/// An inherited event entry in a fallthrough branch.
#[derive(Debug, Clone, PartialEq)]
pub struct FallthroughEventEntry {
    pub name: String,
    /// The inherited event's resolved payload SOURCE POSITION (`Absent` =
    /// untyped).
    pub payload: SourcePosition,
    /// The canonical file scope `payload`'s SCOPE-RELATIVE names resolve
    /// under — the PRODUCING owner (the terminal origin of a multi-hop
    /// inheritance chain), carried positionally (see
    /// [`FallthroughPropEntry::type_source_scope`]).
    pub payload_scope: Option<String>,
    pub raw_signature: Option<String>,
    pub sources: Vec<InheritedSource>,
}

/// Status of a fallthrough branch.
#[derive(Debug, Clone, PartialEq)]
pub enum BranchStatus {
    /// All members in this branch are exactly known.
    Resolved,
    /// Some members are known but the branch may have additional unknown members.
    PartiallyUnresolved { reasons: Vec<PartialBranchReason> },
    /// This branch could not be resolved at all.
    Unresolved { reason: UnresolvedBranchReason },
}

/// A single step in the root resolution chain.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedRootStep {
    /// Native HTML element target.
    NativeTag { tag: String },
    /// Resolved child component target.
    Component {
        canonical_id: String,
        component_name: String,
    },
    /// Unresolved root target.
    Unresolved {
        tag: String,
        reason: UnresolvedBranchReason,
    },
}

/// A single branch in the fallthrough surface.
#[derive(Debug, Clone, PartialEq)]
pub struct FallthroughBranch {
    /// Deterministic branch key (e.g., "0", "0.1", "2.0.3").
    pub branch_key: String,
    /// Condition text for diagnostics only.
    pub condition_text: Option<String>,
    /// Inherited props in this branch (after subtraction).
    pub props: Vec<FallthroughPropEntry>,
    /// Inherited events in this branch (after subtraction).
    pub events: Vec<FallthroughEventEntry>,
    /// Chain of root steps traversed to produce this branch.
    pub root_chain: Vec<ResolvedRootStep>,
    /// Resolution status of this branch.
    pub status: BranchStatus,
}

// ═══════════════════════════════════════════════════════════════════════════
// Root Reachability Extraction
// ═══════════════════════════════════════════════════════════════════════════

/// Vue built-in components with special render behavior that are not valid
/// fallthrough targets.
const UNSUPPORTED_BUILTINS: &[&str] = &[
    "Teleport",
    "teleport",
    "Transition",
    "transition",
    "TransitionGroup",
    "transition-group",
    "KeepAlive",
    "keep-alive",
    "Suspense",
    "suspense",
];

/// Extract root reachability facts from template analysis.
///
/// This is a pure fact extraction — no fallthrough semantics, no merge logic.
/// The host-owned resolver consumes this to compute the actual inherited surface.
pub fn extract_root_reachability(
    template: Option<&crate::analysis::template::TemplateAnalysisSnapshot>,
    flags: &ComponentMetaFlags,
) -> RootReachability {
    // Rule 1: inheritAttrs: false → no fallthrough
    if flags.has_inherit_attrs_false {
        return RootReachability::NoFallthrough {
            reason: NoFallthroughReason::InheritAttrsFalse,
        };
    }

    // Rule 2: no template
    let template = match template {
        Some(t) => t,
        None => {
            return RootReachability::NoFallthrough {
                reason: NoFallthroughReason::NoTemplate,
            }
        }
    };

    // Rule 3: empty template (no elements at all)
    if template.elements.is_empty() {
        return RootReachability::NoFallthrough {
            reason: NoFallthroughReason::EmptyTemplate,
        };
    }

    // Collect root elements (parent_index == None)
    let root_elements: Vec<(u32, &crate::analysis::template::TemplateElement)> = template
        .elements
        .iter()
        .enumerate()
        .filter(|(_, el)| el.parent_index.is_none())
        .map(|(i, el)| (i as u32, el))
        .collect();

    // Rule 4: no root elements but template has content (text/interpolation only)
    if root_elements.is_empty() {
        return RootReachability::NoFallthrough {
            reason: NoFallthroughReason::TextOrInterpolationRoot,
        };
    }

    // Partition root elements into independent roots and conditional branches.
    // An element with has_v_else or has_v_else_if is a branch continuation,
    // not an independent root.
    let mut independent_roots: Vec<Vec<(u32, &crate::analysis::template::TemplateElement)>> =
        Vec::new();
    let mut current_chain: Vec<(u32, &crate::analysis::template::TemplateElement)> = Vec::new();

    for (idx, el) in &root_elements {
        if el.has_v_else || el.has_v_else_if {
            // Continuation of the current chain
            current_chain.push((*idx, el));
        } else {
            // New independent root — flush any pending chain
            if !current_chain.is_empty() {
                independent_roots.push(std::mem::take(&mut current_chain));
            }
            current_chain.push((*idx, el));
        }
    }
    if !current_chain.is_empty() {
        independent_roots.push(current_chain);
    }

    // Rule 6: multiple unconditional independent roots → fragment
    if independent_roots.len() > 1 {
        return RootReachability::NoFallthrough {
            reason: NoFallthroughReason::MultiRoot,
        };
    }

    // Exactly one independent root (possibly with v-if chain branches)
    let chain = &independent_roots[0];

    let mut branches = Vec::with_capacity(chain.len());
    for (branch_index, (root_element_index, root_el)) in chain.iter().enumerate() {
        let (element_index, actual_el) =
            match normalize_root_actual_target(*root_element_index, template) {
                Ok(actual) => actual,
                Err(reason) => {
                    return RootReachability::NoFallthrough { reason };
                }
            };

        let target = classify_root_target(element_index, actual_el, template);
        let consumed = extract_consumed_root_bindings(actual_el);
        let has_unknown_spread = actual_el
            .directives
            .iter()
            .any(|d| d.name == "bind" && d.argument.is_none());

        branches.push(RootBranch {
            branch_index: branch_index as u16,
            condition_text: root_el.v_if_condition.clone(),
            target,
            consumed,
            has_unknown_spread,
        });
    }

    RootReachability::Branches { branches }
}

/// Normalize a root element to the actual single render target that participates
/// in fallthrough inheritance.
///
/// Transparent root `<template v-if>` wrappers are unwrapped recursively, but
/// only if they yield exactly one direct child element and no direct text or
/// interpolation content.
fn normalize_root_actual_target(
    element_index: u32,
    template: &crate::analysis::template::TemplateAnalysisSnapshot,
) -> Result<(u32, &crate::analysis::template::TemplateElement), NoFallthroughReason> {
    let el = &template.elements[element_index as usize];

    if el.v_for.is_some() {
        return Err(NoFallthroughReason::RootVFor);
    }

    if el.tag != "template" {
        return Ok((element_index, el));
    }

    if !el.text_children.is_empty() {
        return Err(NoFallthroughReason::BranchNotSingleRoot);
    }

    let child_elements: Vec<u32> = template
        .elements
        .iter()
        .enumerate()
        .filter(|(_, child)| child.parent_index == Some(element_index))
        .map(|(idx, _)| idx as u32)
        .collect();

    if child_elements.len() != 1 {
        return Err(NoFallthroughReason::BranchNotSingleRoot);
    }

    normalize_root_actual_target(child_elements[0], template)
}

/// Classify a root element into a `RootTargetRef`.
fn classify_root_target(
    element_index: u32,
    el: &crate::analysis::template::TemplateElement,
    template: &crate::analysis::template::TemplateAnalysisSnapshot,
) -> RootTargetRef {
    let tag = &el.tag;

    // Check for slot outlet
    if tag == "slot" {
        return RootTargetRef::UnresolvedTarget {
            element_index,
            tag: tag.clone(),
            reason: UnresolvedRootTargetReason::SlotOutlet,
        };
    }

    // Check for unsupported Vue built-ins
    if UNSUPPORTED_BUILTINS.contains(&tag.as_str()) {
        return RootTargetRef::UnresolvedTarget {
            element_index,
            tag: tag.clone(),
            reason: UnresolvedRootTargetReason::UnsupportedBuiltin { tag: tag.clone() },
        };
    }

    if el.is_component {
        // Check for dynamic component
        if tag == "component" {
            return match el.component_usage_index.and_then(|idx| {
                template
                    .components
                    .get(idx as usize)
                    .map(|usage| (idx, usage))
            }) {
                Some((usage_index, usage)) if usage.props.iter().any(|prop| prop.name == "is") => {
                    RootTargetRef::DynamicComponentUsage {
                        element_index,
                        usage_index,
                    }
                }
                Some((_usage_index, _usage)) => RootTargetRef::UnresolvedTarget {
                    element_index,
                    tag: tag.clone(),
                    reason: UnresolvedRootTargetReason::UnknownRootTarget,
                },
                None => RootTargetRef::UnresolvedTarget {
                    element_index,
                    tag: tag.clone(),
                    reason: UnresolvedRootTargetReason::MissingUsageLink,
                },
            };
        }

        match el.component_usage_index.and_then(|idx| {
            template
                .components
                .get(idx as usize)
                .map(|usage| (idx, usage))
        }) {
            Some((usage_index, usage)) if usage.import_source.is_some() => {
                RootTargetRef::ComponentUsage {
                    element_index,
                    usage_index,
                    name: usage.name.clone(),
                    import_source: usage.import_source.clone(),
                }
            }
            Some((_usage_index, _usage)) => RootTargetRef::UnresolvedTarget {
                element_index,
                tag: tag.clone(),
                reason: UnresolvedRootTargetReason::UnresolvedImport,
            },
            None => RootTargetRef::UnresolvedTarget {
                element_index,
                tag: tag.clone(),
                reason: UnresolvedRootTargetReason::MissingUsageLink,
            },
        }
    } else {
        RootTargetRef::NativeElement {
            element_index,
            tag: tag.clone(),
        }
    }
}

/// Extract consumed root bindings from a root element.
///
/// `class` and `style` are never consumed because Vue always merges them through.
/// `@click` and `:onClick` normalize to the same canonical listener name `"click"`.
fn extract_consumed_root_bindings(
    el: &crate::analysis::template::TemplateElement,
) -> ConsumedRootBindings {
    let mut bindings = ConsumedRootBindings::default();

    // Process static and dynamic attributes.
    // `:foo` has is_dynamic=true but a known name ("foo").
    // `:[expr]` has is_dynamic=true with a computed name that we can't statically know.
    // We detect computed names by checking if the name starts with `[`.
    for attr in &el.attributes {
        // Skip class and style — Vue always merges these
        if attr.name == "class" || attr.name == "style" {
            continue;
        }

        if attr.is_dynamic && attr.name.starts_with('[') {
            // Computed attribute name (:[expr]) — can't determine which attr is consumed
            bindings.has_dynamic_attr_name = true;
        } else {
            // Static name (both `foo="bar"` and `:foo="expr"` have known names)
            bindings.attrs.push(attr.name.clone());
        }
    }

    // Process directives
    for dir in &el.directives {
        match dir.name.as_str() {
            "on" => {
                // @event or v-on:event
                if let Some(ref arg) = dir.argument {
                    if arg.starts_with('[') {
                        // @[expr] — dynamic listener name
                        bindings.has_dynamic_listener_name = true;
                    } else {
                        // Canonical listener name = the event name (e.g., "click")
                        bindings.listeners.push(arg.clone());
                    }
                } else {
                    // v-on="obj" — dynamic listener names
                    bindings.has_dynamic_listener_name = true;
                }
            }
            "bind" => {
                if let Some(ref arg) = dir.argument {
                    // Skip class and style
                    if arg == "class" || arg == "style" {
                        continue;
                    }
                    if arg.starts_with('[') {
                        // :[expr] — dynamic attribute name
                        bindings.has_dynamic_attr_name = true;
                    } else if arg.starts_with("on")
                        && arg.len() > 2
                        && arg.as_bytes()[2].is_ascii_uppercase()
                    {
                        // :onClick → canonical listener "click"
                        let event_name = arg[2..3].to_lowercase() + &arg[3..];
                        bindings.listeners.push(event_name);
                    } else {
                        bindings.attrs.push(arg.clone());
                    }
                }
                // v-bind="obj" without argument is spread — handled by has_unknown_spread
            }
            "model" => {
                // v-model on a root element.
                // For component roots: consumes the model prop name + update:* event.
                // For native roots: consumes the Vue-facing attr/listener pair
                // based on the element type.
                if el.is_component {
                    if let Some(ref arg) = dir.argument {
                        // v-model:title → consumes "title" prop + "update:title" event
                        bindings.attrs.push(arg.clone());
                        bindings.listeners.push(format!("update:{}", arg));
                    } else {
                        // v-model → consumes "modelValue" prop + "update:modelValue" event
                        bindings.attrs.push("modelValue".to_string());
                        bindings.listeners.push("update:modelValue".to_string());
                    }
                } else {
                    // Native v-model: the consumed attr/listener pair depends on the
                    // element type. Vue's runtime behavior:
                    // - <input type="checkbox"> / <input type="radio"> → checked + change
                    // - <select> → value + change
                    // - everything else (<input>, <textarea>) → value + input
                    let tag = el.tag.as_str();
                    let is_checkbox_or_radio = tag == "input"
                        && el.attributes.iter().any(|a| {
                            a.name == "type"
                                && matches!(a.value.as_deref(), Some("checkbox" | "radio"))
                        });

                    if is_checkbox_or_radio {
                        bindings.attrs.push("checked".to_string());
                        bindings.listeners.push("change".to_string());
                    } else if tag == "select" {
                        bindings.attrs.push("value".to_string());
                        bindings.listeners.push("change".to_string());
                    } else {
                        // <input>, <textarea>, and other elements
                        bindings.attrs.push("value".to_string());
                        bindings.listeners.push("input".to_string());
                    }
                }
            }
            _ => {}
        }
    }

    // Dedup consumed names
    bindings.attrs.sort();
    bindings.attrs.dedup();
    bindings.listeners.sort();
    bindings.listeners.dedup();

    bindings
}

// ═══════════════════════════════════════════════════════════════════════════
// Extraction
// ═══════════════════════════════════════════════════════════════════════════

/// Extract component metadata from pre-resolved analysis-owned inputs.
///
/// Does NOT access host/VFS/workspace — all resolved data is pre-supplied.
/// Source order of props/events/slots/exposed is preserved.
pub fn extract_component_meta(input: ComponentMetaInput<'_>) -> ComponentMetaAnalysis {
    let options_api = input.options_api.is_some();
    let flags = extract_flags(&input);
    let evaluated_types = input.evaluated_types;

    let mut props = Vec::new();
    let mut events = Vec::new();
    let mut slots = Vec::new();
    let mut models = Vec::new();
    let mut exposed = Vec::new();

    // Collect defaults from all prop-bearing macro forms.
    let default_keys: std::collections::HashSet<&str> = input
        .macros
        .iter()
        .filter(|m| {
            matches!(
                m.kind,
                AnalyzedMacroKind::WithDefaults | AnalyzedMacroKind::DefineProps
            )
        })
        .flat_map(|m| m.default_keys.iter().map(|k| k.as_str()))
        .chain(
            input
                .resolved_macros
                .iter()
                .flat_map(|resolved| resolved.default_keys.iter().map(String::as_str)),
        )
        .collect();

    // Runtime defineProps({ ... default }) stores defaults on the DefineProps macro
    // itself, while withDefaults() stores them on the WithDefaults wrapper.
    let default_values: std::collections::HashMap<&str, &str> = input
        .macros
        .iter()
        .filter(|m| {
            matches!(
                m.kind,
                AnalyzedMacroKind::WithDefaults | AnalyzedMacroKind::DefineProps
            )
        })
        .flat_map(|m| {
            m.default_values
                .iter()
                .map(|dv| (dv.key.as_str(), dv.value.as_str()))
        })
        .collect();

    for (macro_index, mac) in input.macros.iter().enumerate() {
        let resolved_macro = merged_resolved_macro_input(input.resolved_macros, macro_index);
        match mac.kind {
            AnalyzedMacroKind::DefineProps => {
                let prop_fields = merged_prop_fields(mac, resolved_macro.as_ref());
                extract_props_from_macro(
                    macro_index,
                    &prop_fields,
                    &default_keys,
                    &default_values,
                    evaluated_types,
                    &mut props,
                );
            }
            AnalyzedMacroKind::DefineEmits => {
                let emit_fields =
                    canonical_emit_occurrences(mac, resolved_macro.as_ref(), input.file_path);
                extract_events_from_macro(&emit_fields, &mut events);
            }
            AnalyzedMacroKind::DefineSlots => {
                let slot_fields = merged_slot_fields(mac, resolved_macro.as_ref());
                extract_slots_from_macro(
                    macro_index,
                    &slot_fields.fields,
                    &slot_fields.return_publications,
                    evaluated_types,
                    &mut slots,
                );
            }
            AnalyzedMacroKind::DefineModel => {
                let prop_fields = merged_prop_fields(mac, resolved_macro.as_ref());
                extract_model_from_macro(mac, &prop_fields, &mut models);
            }
            AnalyzedMacroKind::DefineExpose => {
                extract_exposed_from_macro(
                    macro_index,
                    mac,
                    resolved_macro.as_ref(),
                    input.bindings,
                    evaluated_types,
                    &mut exposed,
                );
            }
            AnalyzedMacroKind::WithDefaults | AnalyzedMacroKind::DefineOptions => {
                // Handled above (default_keys) or flags
            }
        }
    }

    // Framework-native carriers without Vue macro syntax (for example
    // Svelte `$props`, callback events, and snippets) enter as synthetic
    // resolved inputs after the snapshot macro ordinal range. They use the
    // same typed extraction functions and publication policy as macro-backed
    // rows; no declaration or display text is inspected.
    let mut native_indices = input
        .resolved_macros
        .iter()
        .filter_map(|resolved| {
            (resolved.macro_index >= input.macros.len()).then_some(resolved.macro_index)
        })
        .collect::<Vec<_>>();
    native_indices.sort_unstable();
    native_indices.dedup();
    for macro_index in native_indices {
        let Some(resolved) = merged_resolved_macro_input(input.resolved_macros, macro_index) else {
            continue;
        };
        extract_props_from_macro(
            macro_index,
            &resolved.props,
            &default_keys,
            &default_values,
            evaluated_types,
            &mut props,
        );
        extract_events_from_macro(&resolved.emits, &mut events);
        extract_slots_from_macro(
            macro_index,
            &resolved.slots,
            &resolved.slot_return_publications,
            evaluated_types,
            &mut slots,
        );
    }

    // Merge template-discovered slots with defineSlots
    if let Some(tpl) = input.template {
        merge_template_slots(&tpl.defined_slots, &mut slots);
    }

    // Options API fallback
    if let Some(opts) = input.options_api {
        if props.is_empty() {
            extract_props_from_options(opts, evaluated_types, &mut props);
        }
        if events.is_empty() {
            extract_events_from_options(opts, &mut events);
        }
    }

    for (macro_index, mac) in input.macros.iter().enumerate() {
        if mac.kind != AnalyzedMacroKind::DefineModel {
            continue;
        }
        let resolved_macro = merged_resolved_macro_input(input.resolved_macros, macro_index);
        let prop_fields = merged_prop_fields(mac, resolved_macro.as_ref());
        synthesize_model_prop_and_event(
            mac,
            &prop_fields,
            evaluated_types,
            &mut props,
            &mut events,
        );
    }

    reconcile_update_events_with_props(&props, input.macros, &mut events);

    // Synthesize `@defaultValue` JSDoc tag from the prop's resolved default
    // value. Runs AFTER extraction so it covers every branch (type-based,
    // runtime, options API, withDefaults). Source JSDoc wins — if the field
    // already carries a `@defaultValue` tag, we do not overwrite or duplicate.
    for prop in &mut props {
        if let Some(value) = prop.default_value.as_ref() {
            if !prop.tags.iter().any(|t| t.name == "defaultValue") {
                prop.tags.push(JsdocTag {
                    name: "defaultValue".to_string(),
                    text: Some(value.clone()),
                });
            }
        }
    }

    let type_registry = input.resolved_type_registry.to_vec();
    let components = extract_components(input.template);
    let template_refs = extract_template_refs(input.template);
    let imports = extract_imports(input.imports);
    let bindings = extract_bindings(
        input.bindings,
        input.template,
        input.resolved_binding_reactivity,
    );
    let vue_api_calls = extract_vue_api_calls(input.vue_api_calls);
    let styles = extract_styles(input.styles);

    let root_reachability = extract_root_reachability(input.template, &flags);

    // Collect macro-wide expansion diagnostics (property_name == None) from
    // each macro kind. These were previously duplicated on every field.
    let macro_expansion_diagnostics =
        collect_macro_expansion_diagnostics(evaluated_types, input.macros);

    ComponentMetaAnalysis {
        props,
        events,
        slots,
        models,
        exposed,
        public_instance: None,
        ordered_sfc_structure: None,
        type_registry,
        components,
        template_refs,
        imports,
        bindings,
        vue_api_calls,
        styles,
        flags,
        root_reachability,
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness: AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface: FallthroughSurface::None {
            reason: NoFallthroughReason::NoTemplate,
        },
        macro_expansion_diagnostics,
        options_api,
        file_path: input.file_path.to_string(),
    }
}

// ── Macro-wide diagnostics ────────────────────────────────────────────────

fn collect_macro_expansion_diagnostics(
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    macros: &[AnalyzedMacro],
) -> Vec<MacroExpansionDiagnostics> {
    let Some(evaluated) = evaluated else {
        return Vec::new();
    };

    let mut out = Vec::new();

    for (macro_index, mac) in macros.iter().enumerate() {
        let (kind, result) = match mac.kind {
            AnalyzedMacroKind::DefineProps => {
                let entry = evaluated
                    .define_props
                    .iter()
                    .find(|e| e.macro_index == macro_index);
                (MacroExpansionKind::DefineProps, entry.map(|e| &e.result))
            }
            AnalyzedMacroKind::DefineEmits => {
                let entry = evaluated
                    .define_emits
                    .iter()
                    .find(|e| e.macro_index == macro_index);
                (MacroExpansionKind::DefineEmits, entry.map(|e| &e.result))
            }
            AnalyzedMacroKind::DefineSlots => {
                let entry = evaluated
                    .define_slots
                    .iter()
                    .find(|e| e.macro_index == macro_index);
                (MacroExpansionKind::DefineSlots, entry.map(|e| &e.result))
            }
            _ => continue,
        };

        let Some(result) = result else { continue };

        let global_diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.property_name.is_none())
            .cloned()
            .collect();

        if !global_diags.is_empty()
            || result.exactness != crate::analysis::type_expand::ExpansionExactness::ExactConcrete
            || result.execution_status
                != crate::analysis::type_expand::ExpansionExecutionStatus::Completed
        {
            out.push(MacroExpansionDiagnostics {
                macro_kind: kind,
                macro_index,
                diagnostics: global_diags,
                exactness: result.exactness,
                execution_status: result.execution_status,
            });
        }
    }

    out
}

// ── Props ──────────────────────────────────────────────────────────────────

fn extract_props_from_macro(
    macro_index: usize,
    prop_fields: &[ResolvedPropInput],
    default_keys: &std::collections::HashSet<&str>,
    default_values: &std::collections::HashMap<&str, &str>,
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    out: &mut Vec<PropAnalysis>,
) {
    if let Some(eval_fields) = expanded_define_props_fields(evaluated, macro_index) {
        if !eval_fields.is_empty() {
            for field in eval_fields {
                let source_row = prop_fields.iter().find(|row| row.field.name == field.name);
                let source_field = source_row.map(|row| &row.field);
                let has_default = default_keys.contains(field.name.as_str());
                let default_value = default_values
                    .get(field.name.as_str())
                    .map(|v| v.to_string());
                let type_expansion = define_props_property_expansion_metadata(
                    evaluated,
                    macro_index,
                    field.name.as_str(),
                );
                let publication = publication_from_position(
                    &field.ty,
                    type_expansion.as_ref(),
                    source_row.and_then(|row| row.authored_evidence.clone()),
                    ResolutionProvenance::SemanticEvaluator,
                );

                // JSDoc rides the span-borne supply: the source field (analyzer
                // same-file extraction or the span-sliced macro DTO surface)
                // carries the description/tags; an expansion-only member with
                // no source field publishes none.
                let (description, tags) = source_field
                    .map(|field| (field.description.clone(), field.tags.clone()))
                    .unwrap_or_default();

                out.push(PropAnalysis {
                    name: field.name.clone(),
                    callable_role: source_row
                        .map(|row| row.callable_role.clone())
                        .unwrap_or_default(),
                    publication,
                    type_expansion,
                    required: !field.optional && !has_default,
                    has_default,
                    default_value,
                    description,
                    tags,
                    // Structural-fact read from the expanded field.
                    // `ExpandedField` carries `declared_in_macro_type_arg`
                    // propagated end-to-end through the resolver stack:
                    //
                    // - Parser: `resolve_interface_with_extends`
                    //   stamps own-body interface members with `true`
                    //   and heritage-descent members (`extends`,
                    //   `Omit<...>` source, intersection non-literal
                    //   arms) with `false`.
                    // - Semantic propagation:
                    //   `expand_macro_types_impl_with_expander` and
                    //   `surface_member_to_expanded_field` thread the
                    //   fact through `AnalyzedPropField` →
                    //   `SurfaceMember` → `ExpandedField`.
                    // - Session prepared-surface walker:
                    //   `project_prepared_surface_from_expr` threads
                    //   `from_root_body` through every recursion arm
                    //   and gates `PreparedSurfaceCacheKey` /
                    //   `PreparedMemberCacheKey` on it, so two
                    //   distinct entry contexts publish two distinct
                    //   cache slots.
                    //
                    // For every reference shape — fully-local
                    // (`defineProps<LocalProps>()`), cross-file simple
                    // (`import type { Props } from './x';
                    // defineProps<Props>()`), cross-file heritage
                    // (`interface Carrier extends Omit<V,'k'> { k: T }`),
                    // and inline-literal (`defineProps<{ k: T }>()`) —
                    // `field.declared_in_macro_type_arg` is the
                    // structurally-correct value.
                    declared_in_macro_type_arg: field.declared_in_macro_type_arg,
                });
            }
            // NOTE: We intentionally do NOT fall back to prop_fields here.
            // When the evaluator runs and produces results, it is authoritative —
            // utility types like Pick/Omit may have intentionally excluded some
            // prop_fields entries. Adding them back would break filtering.
            return;
        }
    }

    for row in prop_fields {
        let field = &row.field;
        let (publication, type_expansion) = resolve_prop_type(row, evaluated);
        let has_default = default_keys.contains(field.name.as_str());
        let default_value = default_values
            .get(field.name.as_str())
            .map(|v| v.to_string());

        out.push(PropAnalysis {
            name: field.name.clone(),
            callable_role: row.callable_role.clone(),
            publication,
            type_expansion,
            required: !field.is_optional && !has_default,
            has_default,
            default_value,
            description: field.description.clone(),
            tags: field.tags.clone(),
            declared_in_macro_type_arg: field.declared_in_macro_type_arg,
        });
    }

    if let Some(eval_fields) = expanded_define_props_fields(evaluated, macro_index) {
        let mut seen: std::collections::HashSet<String> = prop_fields
            .iter()
            .map(|row| row.field.name.clone())
            .collect();
        for field in eval_fields {
            if !seen.insert(field.name.clone()) {
                continue;
            }

            let has_default = default_keys.contains(field.name.as_str());
            let default_value = default_values
                .get(field.name.as_str())
                .map(|v| v.to_string());
            let type_expansion = define_props_property_expansion_metadata(
                evaluated,
                macro_index,
                field.name.as_str(),
            );
            let publication = publication_from_position(
                &field.ty,
                type_expansion.as_ref(),
                None,
                ResolutionProvenance::SemanticEvaluator,
            );
            // Evaluator-only branch: no source AnalyzedPropField in scope, so
            // no authored symbolic fallback is available — the evaluated
            // position is kept as-is.

            // No source field reachable from this branch, so no JSDoc supply:
            // doc text rides the span-borne member supply only — an
            // evaluator-only member publishes no description and no tags. The
            // chosen raw_type came from the evaluator's textual rendering,
            // which has no typed companion.
            out.push(PropAnalysis {
                name: field.name.clone(),
                callable_role: verter_type_expr::PropCallableRole::default(),
                publication,
                type_expansion,
                required: !field.optional && !has_default,
                has_default,
                default_value,
                description: None,
                tags: Vec::new(),
                // Evaluator-only branch: name was not present in
                // `prop_fields`, so the analyzer did not observe the
                // author writing it on the macro T body. `declared = false`.
                declared_in_macro_type_arg: false,
            });
        }
    }
}

fn expanded_define_props_fields(
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    macro_index: usize,
) -> Option<&[crate::analysis::type_expand::ExpandedProperty]> {
    evaluated?
        .define_props
        .iter()
        .find(|entry| entry.macro_index == macro_index)
        .map(|entry| entry.result.value.properties.as_slice())
}

fn field_expansion_metadata(
    field: &crate::analysis::type_expand::ExpandedField,
) -> crate::analysis::type_expand::ExpansionMetadata {
    crate::analysis::type_expand::ExpansionMetadata {
        exactness: field.exactness,
        execution_status: field.execution_status,
        diagnostics: field.diagnostics.clone(),
    }
}

fn define_props_property_expansion_metadata(
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    macro_index: usize,
    prop_name: &str,
) -> Option<crate::analysis::type_expand::ExpansionMetadata> {
    let entry = evaluated?
        .define_props
        .iter()
        .find(|entry| entry.macro_index == macro_index)?;

    // Only include diagnostics that are specific to this property.
    // Macro-wide diagnostics (property_name == None) are collected separately
    // into `macro_expansion_diagnostics` to avoid duplication across every prop.
    let diagnostics = entry
        .result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.property_name.as_deref() == Some(prop_name))
        .cloned()
        .collect();

    Some(crate::analysis::type_expand::ExpansionMetadata {
        exactness: entry.result.exactness,
        execution_status: entry.result.execution_status,
        diagnostics,
    })
}

fn expanded_define_slots_shape(
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    macro_index: usize,
) -> Option<&crate::analysis::type_expand::ExpandedMacroObjectShape> {
    evaluated?
        .define_slots
        .iter()
        .find(|entry| entry.macro_index == macro_index)
}

/// Resolve prop type via priority chain:
/// 1. Evaluated type source (preferred)
/// 2. Raw annotation text (display-only fallback)
fn merged_resolved_macro_input(
    resolved_macros: &[ResolvedMacroInput],
    macro_index: usize,
) -> Option<ResolvedMacroInput> {
    let mut merged: Option<ResolvedMacroInput> = None;
    let mut seen_props = rustc_hash::FxHashSet::default();
    let mut seen_slots = rustc_hash::FxHashSet::default();
    let mut seen_exposed = rustc_hash::FxHashSet::default();

    for resolved in resolved_macros
        .iter()
        .filter(|resolved| resolved.macro_index == macro_index)
    {
        let entry = merged.get_or_insert_with(|| ResolvedMacroInput {
            macro_index,
            props: Vec::new(),
            emits: Vec::new(),
            slots: Vec::new(),
            slot_return_publications: Vec::new(),
            exposed: Vec::new(),
            default_keys: Vec::new(),
        });

        for prop in &resolved.props {
            if let Some(existing) = entry
                .props
                .iter_mut()
                .find(|row| row.field.name == prop.field.name)
            {
                merge_prop_field(&mut existing.field, &prop.field);
            } else if seen_props.insert(prop.field.name.clone()) {
                entry.props.push(prop.clone());
            }
        }
        for emit in &resolved.emits {
            entry.emits.push(emit.clone());
        }
        for (slot_index, slot) in resolved.slots.iter().enumerate() {
            if seen_slots.insert(slot.name.clone()) {
                entry.slots.push(slot.clone());
                entry.slot_return_publications.push(
                    resolved
                        .slot_return_publications
                        .get(slot_index)
                        .cloned()
                        .flatten(),
                );
            }
        }
        for exposed in &resolved.exposed {
            if seen_exposed.insert(exposed.field.name.clone()) {
                entry.exposed.push(exposed.clone());
            }
        }
        for key in &resolved.default_keys {
            if !entry.default_keys.iter().any(|existing| existing == key) {
                entry.default_keys.push(key.clone());
            }
        }
    }

    merged
}

/// Resolve the prop's `TypeExpr` per the priority chain:
///
/// 1. Evaluated `TypeExpr` post-`raise_and_reduce` (when present).
///    The graph-native projection guarantees no operator-leaf residue
///    at this layer; the evaluated type IS the authoritative resolved
///    shape.
/// 2. Raw annotation (when no evaluated entry exists for this field).
///    Parsed from the source annotation as the symbolic fallback.
/// 3. `Unknown { raw: "unknown" }` (when neither is present).
///
/// `prefer_symbolic_prop_type_expr` enforces the priority on the
/// evaluated path; the residual "fall back to raw when evaluated is
/// incomplete-placeholder" branch in
/// [`should_prefer_symbolic_prop_type_expr`] is defense-in-depth that
/// fires only when expansion stops mid-shape (incomplete + budget-
/// exceeded). When dispatch's `raise_and_reduce` substitution parity
/// closes the substitution gap documented in
/// `materialize_component_meta_type_expr_until_stable_full`, the
/// incomplete-placeholder fallback becomes structurally unreachable
/// and can be removed.
fn resolve_prop_type(
    row: &ResolvedPropInput,
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
) -> (
    TypePublication,
    Option<crate::analysis::type_expand::ExpansionMetadata>,
) {
    // A runtime-constructor position (`defineProps({ label: String })`) is
    // never authored (`field.payload` is always `None` in this branch — the
    // two are mutually exclusive at extraction, see macros.rs), so it takes
    // priority over the row's base authority (which for such a field is
    // just the PROVEN unannotated absence) whenever the binding index found
    // one or more constructor identifiers at this prop's value position.
    if let Some(position) =
        constructor_binding_source_position(&row.field.constructor_bindings, evaluated)
    {
        let metadata = evaluated.and_then(|eval| {
            eval.props
                .iter()
                .find(|f| f.name == row.field.name)
                .map(field_expansion_metadata)
        });
        let publication = publication_from_position(
            &position,
            metadata.as_ref(),
            row.authored_evidence.clone(),
            ResolutionProvenance::SemanticEvaluator,
        );
        return (publication, metadata);
    }

    // The row's own session-resolved source is the authority; the flat
    // evaluated lane contributes ONLY expansion metadata (an INCOMPLETE
    // expansion still falls back to the author's own annotation position —
    // the honest symbolic surface, never a torn partial).
    let metadata = evaluated.and_then(|eval| {
        eval.props
            .iter()
            .find(|f| f.name == row.field.name)
            .map(field_expansion_metadata)
    });
    let publication = TypePublication::new(
        row.authority.clone(),
        row.authored_evidence.clone(),
        &PublicationPolicy::exact_only(),
    );
    (publication, metadata)
}

/// Fold a runtime-constructor position's owner-aware binding-resolution
/// outcomes (`RootBindingIndex`-gated, see
/// `docs/arch/refactor/rev11/evidence/CM1/binding-index-design.md`) into a
/// [`SourcePosition`]. Shared by the macro path (`resolve_prop_type`) and
/// the Options-API path (`extract_props_from_options`) — the ONLY place
/// either path applies the runtime-constructor closed-fact fold or resolves
/// a local shadow.
///
/// Returns `None` when there is no runtime-constructor position at all
/// (`bindings` empty) or when every entry resolved `Global` to a
/// non-primitive spelling — that case keeps the EXISTING display-text-only
/// route unchanged (the caller's own `raw_type`/`type_annotation` already
/// carries the display text; no closed fact exists for it).
fn constructor_binding_source_position(
    bindings: &[verter_type_expr::ConstructorBindingEntry],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
) -> Option<SourcePosition> {
    use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact, SemanticSourceFailure};
    use verter_type_expr::{ConstructorBindingOutcome, PrimitiveName};

    if bindings.is_empty() {
        return None;
    }

    // Static resolution did not apply anywhere in this position — fails
    // closed as a genuine preparation failure, never a silent `Global`
    // fallback and never collapsed into the proven-absent `None` above.
    if bindings
        .iter()
        .any(|entry| matches!(entry.resolution, ConstructorBindingOutcome::Indeterminate))
    {
        return Some(SourcePosition::Failed(
            SemanticSourceFailure::UnrepresentableRequiredMemberValue,
        ));
    }

    // A locally-shadowed spelling is never folded as a runtime constructor —
    // it resolves through the general authored-value-reference route the
    // host already resolves defineExpose-style bindings through (the SAME
    // `ExpandedComponentTypes.bindings` lane, keyed by the shadowing
    // declaration's own name).
    if bindings
        .iter()
        .any(|entry| matches!(entry.resolution, ConstructorBindingOutcome::Local(_)))
    {
        // A constructor array mixing a `Local` entry with anything else (a
        // second `Local`, or a `Global` spelling) would need a proper union
        // of the resolved types to publish correctly — not implemented.
        // Fail closed rather than publish only one element's type and
        // silently drop the rest (`[String, LocalClass]` losing `String`,
        // or two `Local` elements losing the second, is worse than an
        // honest failure). The single-element case (the common,
        // non-array `defineProps({ label: LocalClass })` shape) still
        // resolves normally below.
        let [entry] = bindings else {
            return Some(SourcePosition::Failed(
                SemanticSourceFailure::UnrepresentableRequiredMemberValue,
            ));
        };
        let ConstructorBindingOutcome::Local(key) = &entry.resolution else {
            unreachable!("guarded by the Local-membership check above");
        };
        let Some(eval) = evaluated else {
            return Some(SourcePosition::Failed(
                SemanticSourceFailure::UnrepresentableRequiredMemberValue,
            ));
        };
        // `ExpandedComponentTypes.bindings` is the SAME lane `defineExpose`
        // resolves through, and can admit a module declaration alongside an
        // unrelated same-name instance declaration (e.g. this constructor's
        // module-owned shadow beside a `defineExpose`-requested
        // instance-owned binding of the same name). `key.owner` — the SAME
        // owner `RootBindingIndex` proved this identifier resolves under —
        // narrows the match first; a lookup that is still ambiguous after
        // matching BOTH owner and name (never expected in practice, since a
        // single owner cannot legally bind one name to two different
        // declarations) fails closed exactly like an unresolvable one,
        // never silently picking one.
        let mut matching = eval
            .bindings
            .iter()
            .filter(|f| f.name == key.name.as_ref() && f.owner == key.owner);
        let Some(field) = matching.next() else {
            return Some(SourcePosition::Failed(
                SemanticSourceFailure::UnrepresentableRequiredMemberValue,
            ));
        };
        if matching.next().is_some() {
            return Some(SourcePosition::Failed(
                SemanticSourceFailure::UnrepresentableRequiredMemberValue,
            ));
        }
        return Some(match field.authority.outcome() {
            ResolvedTypeOutcome::Present { source, .. } => {
                SourcePosition::Present(source.as_ref().clone())
            }
            ResolvedTypeOutcome::Failed { failure } => {
                let verter_type_expr::TypedResolutionFailure::SourceConstruction(failure) = failure;
                SourcePosition::Failed(*failure)
            }
            // A PROVEN `Local` binding whose evaluated authority came back
            // `Absent` is NOT "no closed-fact position, defer to the
            // caller's own route" — that route (the display-text-only
            // fallback) would treat the ORIGINAL spelling (e.g. `"String"`)
            // as if it might still be the global runtime constructor,
            // exactly the false-global fold this whole gate exists to
            // prevent. Fail closed like every other branch in this
            // function rather than returning `None` (which the caller
            // reads as "fall back to the unannotated/display-text route").
            ResolvedTypeOutcome::Absent { .. } => {
                SourcePosition::Failed(SemanticSourceFailure::UnrepresentableRequiredMemberValue)
            }
        });
    }

    // Every entry resolved `Global`: fold String/Number/Boolean/null
    // spellings to the closed primitive fact (a union of primitives for a
    // multi-element constructor array); any other spelling keeps its
    // existing display-text-only route — no closed-fact plumbing for the
    // other seven constructor spellings. `"null"` is the literal-`null`
    // array-element spelling from `resolve_runtime_constructor_array`
    // (confirmed against `@vue/runtime-core`'s own `assertType`/`getType`:
    // `[String, null]` means "String-typed value OR literal `null`").
    fn primitive_of(spelling: &str) -> Option<PrimitiveName> {
        match spelling {
            "String" => Some(PrimitiveName::String),
            "Number" => Some(PrimitiveName::Number),
            "Boolean" => Some(PrimitiveName::Boolean),
            "null" => Some(PrimitiveName::Null),
            _ => None,
        }
    }
    let primitives: Option<Vec<PrimitiveName>> = bindings
        .iter()
        .map(|entry| primitive_of(entry.spelling.as_ref()))
        .collect();
    primitives.map(|primitives| {
        let fact = if let [only] = primitives.as_slice() {
            ClosedTypeFact::Leaf(LeafTypeFact::Primitive(*only))
        } else {
            ClosedTypeFact::LeafUnion(
                primitives
                    .into_iter()
                    .map(LeafTypeFact::Primitive)
                    .collect(),
            )
        };
        SourcePosition::Present(SemanticTypeSource::Closed(fact))
    })
}

// ── Events ─────────────────────────────────────────────────────────────────

fn event_raw_signature(
    fallback_raw_type: Option<&str>,
    source_payload: Option<&str>,
) -> Option<String> {
    if let Some(source_payload) = source_payload
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Some(source_payload.to_string());
    }
    fallback_raw_type
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(preserve_or_wrap_event_payload)
}

fn preserve_or_wrap_event_payload(raw_type: &str) -> String {
    let trimmed = raw_type.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed.to_string()
    } else {
        format!("[value: {trimmed}]")
    }
}

fn extract_events_from_macro(emit_fields: &[ResolvedEmitInput], out: &mut Vec<EventAnalysis>) {
    for row in emit_fields {
        out.push(EventAnalysis {
            name: row.name.clone(),
            payload: row.payload_source.clone(),
            publication: row.payload_publication.clone(),
            return_publication: row.return_publication.clone(),
            return_publication_scope: row.return_publication_scope.clone(),
            payload_expansion: None,
            raw_signature: event_raw_signature(None, row.payload_type.as_deref()),
            description: row.description.clone(),
            tags: row.tags.clone(),
        });
    }
}

// ── Slots ──────────────────────────────────────────────────────────────────

fn extract_slots_from_macro(
    macro_index: usize,
    slot_fields: &[crate::analysis::types::AnalyzedSlotField],
    slot_return_publications: &[Option<TypePublication>],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    out: &mut Vec<SlotAnalysis>,
) {
    // The three lanes are independent partial observations:
    //
    // - `slot_fields` owns authored/resolved membership, order, docs, return
    //   source, and source optionality;
    // - `define_slots` contributes additional resolved membership plus
    //   authoritative optionality for names it actually observed;
    // - `slot_bindings` contributes resolved binding rows even when the
    //   `define_slots` shape was partial or empty.
    //
    // Never use non-emptiness of one lane as proof that it is complete. An open
    // intersection arm can leave `define_slots` as a strict subset while the
    // graph-native binding walk still resolves every callable explicit arm.
    let mut expanded_remaining = expanded_define_slot_entries(evaluated, macro_index);
    let mut seen_slots = rustc_hash::FxHashSet::default();

    for (field_index, field) in slot_fields.iter().enumerate() {
        if !seen_slots.insert(field.name.clone()) {
            continue;
        }
        let expanded = expanded_remaining
            .iter()
            .position(|slot| slot.name == field.name)
            .map(|index| expanded_remaining.remove(index));
        let (expanded_bindings, is_required) = match expanded {
            Some(slot) => (slot.bindings, slot.is_required),
            None => (
                expanded_slot_bindings(evaluated, &field.name),
                field.is_required,
            ),
        };
        let bindings = merge_slot_bindings_with_source(field, expanded_bindings);

        let return_publication = slot_return_publications
            .get(field_index)
            .cloned()
            .flatten()
            .or_else(|| slot_return_publication_from_field(field));
        out.push(SlotAnalysis {
            name: field.name.clone(),
            is_scoped: !bindings.is_empty(),
            bindings,
            is_required,
            return_type: field.return_type.clone(),
            return_publication,
            return_publication_scope: field.return_expr_scope.clone(),
            description: field.description.clone(),
            tags: field.tags.clone(),
            // Straight off the authored / resolver-projected
            // defineSlots surface.
            declared_in_macro_type_arg: true,
        });
    }

    // Expanded-only names append after the authored/resolved lane, preserving
    // the evaluator's deterministic property order. Exact name dedup prevents
    // duplicate shape rows from publishing the same slot twice.
    for slot in expanded_remaining {
        if !seen_slots.insert(slot.name.clone()) {
            continue;
        }
        out.push(SlotAnalysis {
            name: slot.name,
            is_scoped: !slot.bindings.is_empty(),
            bindings: slot.bindings,
            is_required: slot.is_required,
            return_type: None,
            return_publication: None,
            return_publication_scope: None,
            description: None,
            tags: Vec::new(),
            // No authored/resolved field declared this evaluated-only name.
            declared_in_macro_type_arg: false,
        });
    }
}

fn slot_return_publication_from_field(
    field: &crate::analysis::types::AnalyzedSlotField,
) -> Option<TypePublication> {
    let position = SourcePosition::Present(authored_payload_source(field.payload.as_ref())?);
    Some(publication_from_position(
        &position,
        None,
        authored_type_evidence(field.payload.as_ref(), field.return_type.as_deref()),
        ResolutionProvenance::FrameworkSurface,
    ))
}

#[derive(Clone)]
struct ExpandedSlotEntry {
    name: String,
    bindings: Vec<SlotBindingAnalysis>,
    is_required: bool,
}

fn expanded_define_slot_entries(
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    macro_index: usize,
) -> Vec<ExpandedSlotEntry> {
    let Some(entry) = expanded_define_slots_shape(evaluated, macro_index) else {
        return Vec::new();
    };

    entry
        .result
        .value
        .properties
        .iter()
        .map(|prop| ExpandedSlotEntry {
            name: prop.name.clone(),
            bindings: expanded_slot_bindings(evaluated, &prop.name),
            is_required: !prop.optional,
        })
        .collect()
}

fn expanded_slot_bindings(
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    slot_name: &str,
) -> Vec<SlotBindingAnalysis> {
    // The per-binding evaluation channel ("slotName.bindingName" entries) is
    // the only lower-crate source of evaluated bindings: deriving bindings by
    // walking a slot's materialized function shape is a host concern (the
    // materialized surface lives on the host's hot mirror, never here).
    let Some(evaluated) = evaluated else {
        return Vec::new();
    };
    let prefix = format!("{slot_name}.");
    let mut seen_bindings = rustc_hash::FxHashSet::default();
    evaluated
        .slot_bindings
        .iter()
        .filter_map(|field| {
            // Graph-native no-parser rows carry the exact typed pair. Other
            // rows use the established flat `slot.binding` transport key.
            let binding_name = match field.authority.source() {
                Some(SemanticTypeSource::SyntheticSlotBinding(key))
                    if key.surface_kind
                        == verter_type_expr::SyntheticCarrierSurfaceKind::SlotBinding =>
                {
                    if key.slot_name.as_deref() != Some(slot_name) {
                        return None;
                    }
                    key.binding_name.as_ref()
                }
                _ => field.name.strip_prefix(&prefix)?,
            };
            if binding_name.is_empty() || !seen_bindings.insert(binding_name.to_string()) {
                return None;
            }
            let type_expansion = field_expansion_metadata(field);
            Some(SlotBindingAnalysis {
                name: binding_name.to_string(),
                publication: TypePublication::new(
                    field.authority.clone(),
                    field.authored_evidence.clone(),
                    &PublicationPolicy::exact_only(),
                ),
                type_expansion: Some(type_expansion),
            })
        })
        .collect()
}

fn merge_slot_bindings_with_source(
    source_field: &crate::analysis::types::AnalyzedSlotField,
    expanded_bindings: Vec<SlotBindingAnalysis>,
) -> Vec<SlotBindingAnalysis> {
    // Order discipline: source-captured bindings (parser-side
    // `AnalyzedSlotField::bindings`) come first in parser order;
    // expansion-only remainder appends in the evaluator's emission order.
    // Membership sets only suppress duplicate identities; no hash iteration
    // participates in output order.
    let mut expanded_seen = rustc_hash::FxHashSet::default();
    let mut expanded_remaining: Vec<SlotBindingAnalysis> = expanded_bindings
        .into_iter()
        .filter(|binding| expanded_seen.insert(binding.name.clone()))
        .collect();
    let mut merged: Vec<SlotBindingAnalysis> =
        Vec::with_capacity(source_field.bindings.len() + expanded_remaining.len());
    let mut merged_seen = rustc_hash::FxHashSet::default();

    for source_binding in &source_field.bindings {
        if !merged_seen.insert(source_binding.name.clone()) {
            continue;
        }
        let Some(position) = expanded_remaining
            .iter()
            .position(|candidate| candidate.name == source_binding.name)
        else {
            merged.push(SlotBindingAnalysis {
                name: source_binding.name.clone(),
                publication: publication_from_position(
                    &authored_payload_position(source_binding.payload.as_ref()),
                    None,
                    authored_type_evidence(
                        source_binding.payload.as_ref(),
                        source_binding.type_annotation.as_deref(),
                    ),
                    ResolutionProvenance::SemanticEvaluator,
                ),
                type_expansion: None,
            });
            continue;
        };
        let mut binding = expanded_remaining.remove(position);
        let source_evidence = authored_type_evidence(
            source_binding.payload.as_ref(),
            source_binding.type_annotation.as_deref(),
        );
        // An incomplete per-binding evaluation falls back to the author's own
        // annotation position when one exists; an ABSENT evaluated position
        // adopts the authored annotation position.
        let evidence = source_evidence.or_else(|| binding.publication.evidence().cloned());
        binding.publication = TypePublication::new(
            binding.publication.authority().clone(),
            evidence,
            &PublicationPolicy::exact_only(),
        );
        // A typed FAILURE position is preserved — the authored fallback
        // must not paper over a failed required position unless the
        // author actually annotated it.
        merged.push(binding);
    }

    merged.extend(
        expanded_remaining
            .into_iter()
            .filter(|binding| merged_seen.insert(binding.name.clone())),
    );
    merged
}

fn reconcile_update_events_with_props(
    props: &[PropAnalysis],
    macros: &[AnalyzedMacro],
    events: &mut [EventAnalysis],
) {
    let props_by_name: rustc_hash::FxHashMap<&str, &PropAnalysis> = props
        .iter()
        .map(|prop| (prop.name.as_str(), prop))
        .collect();
    let source_backed_events: rustc_hash::FxHashSet<&str> = macros
        .iter()
        .flat_map(|mac| mac.emit_fields.iter())
        .filter(|field| field.payload_type.is_some())
        .map(|field| field.name.as_str())
        .collect();

    for event in events {
        if source_backed_events.contains(event.name.as_str()) {
            continue;
        }
        let Some(prop_name) = event.name.strip_prefix("update:") else {
            continue;
        };
        let Some(prop) = props_by_name.get(prop_name) else {
            continue;
        };
        let Some(prop_raw_type) = prop
            .publication
            .evidence()
            .map(AuthoredTypeEvidence::text)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        else {
            continue;
        };
        let should_prefer_prop = event
            .raw_signature
            .as_deref()
            .map(|raw| raw.contains(" extends ") || raw.contains("unknown"))
            .unwrap_or(true);
        if should_prefer_prop {
            event.raw_signature = Some(format!("[value: {prop_raw_type}]"));
        }
    }
}

fn merge_template_slots(
    template_slots: &[crate::analysis::template::DefinedSlot],
    out: &mut Vec<SlotAnalysis>,
) {
    for tslot in template_slots {
        if !out.iter().any(|s| s.name == tslot.name) {
            // Template-discovered slots have no AnalyzedSlotField source —
            // pair is (None, None) per the pairing invariant.
            out.push(SlotAnalysis {
                name: tslot.name.clone(),
                is_scoped: tslot.has_bindings,
                bindings: Vec::new(),
                is_required: false,
                return_type: None,
                return_publication: None,
                return_publication_scope: None,
                description: None,
                tags: Vec::new(),
                // An authored template `<slot>` element declares the name.
                declared_in_macro_type_arg: true,
            });
        }
    }
}

// ── Models ─────────────────────────────────────────────────────────────────

fn extract_model_from_macro(
    mac: &AnalyzedMacro,
    prop_fields: &[ResolvedPropInput],
    out: &mut Vec<ModelAnalysis>,
) {
    let name = mac
        .model_name
        .clone()
        .unwrap_or_else(|| "modelValue".to_string());

    // The model's TYPE source is the NORMALIZED `defineModel` surface row's
    // own source (the synthesized model prop carries its authored
    // type-argument position, or the PROVEN unannotated absence for an
    // untyped `defineModel()`) — never a same-name flat evaluated prop from
    // a sibling macro (the cross-macro shadow the sole-authority rule
    // removed).
    let type_source = prop_fields
        .iter()
        .find(|row| row.field.name == name)
        .map(|row| row.authority.source_position())
        .unwrap_or_else(SourcePosition::unannotated);

    out.push(ModelAnalysis { name, type_source });
}

// ── Exposed ────────────────────────────────────────────────────────────────

fn synthesize_model_prop_and_event(
    mac: &AnalyzedMacro,
    prop_fields: &[ResolvedPropInput],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    props: &mut Vec<PropAnalysis>,
    events: &mut Vec<EventAnalysis>,
) {
    let name = mac
        .model_name
        .clone()
        .unwrap_or_else(|| "modelValue".to_string());
    // The single authored `default` source: presence and value text both
    // derive from this one lookup, so `has_default` and `default_value`
    // cannot diverge (a defaulted model always carries its authored value
    // text).
    let default_value = mac
        .default_values
        .iter()
        .find(|entry| entry.key == name)
        .map(|entry| entry.value.clone());
    let has_default = default_value.is_some();
    let source_prop = prop_fields.iter().find(|row| row.field.name == name);
    let raw_type = source_prop.and_then(|row| row.field.type_annotation.clone());
    let is_optional = source_prop
        .map(|row| row.field.is_optional)
        .unwrap_or(false);
    // The prop's resolved SOURCE: the NORMALIZED `defineModel` surface row's
    // own source (its authored type-argument position, or the PROVEN
    // unannotated absence for an untyped `defineModel()`) — never a
    // same-name flat evaluated prop from a sibling macro (the cross-macro
    // shadow the sole-authority rule removed). The `| undefined`
    // optionality decoration is a projection-time concern (derivable from
    // `required`/`has_default`), never synthesized into the carrier.
    let prop_publication = source_prop
        .map(|row| {
            TypePublication::new(
                row.authority.clone(),
                row.authored_evidence.clone(),
                &PublicationPolicy::exact_only(),
            )
        })
        .unwrap_or_else(|| {
            publication_from_position(
                &SourcePosition::unannotated(),
                None,
                None,
                ResolutionProvenance::SemanticEvaluator,
            )
        });

    if let Some(existing_prop) = props.iter_mut().find(|prop| prop.name == name) {
        existing_prop.publication = prop_publication.clone();
        existing_prop.type_expansion = existing_prop.type_expansion.clone().or_else(|| {
            evaluated.and_then(|eval| {
                eval.props
                    .iter()
                    .find(|field| field.name == name)
                    .map(field_expansion_metadata)
            })
        });
        existing_prop.required = !has_default && !is_optional;
        existing_prop.has_default |= has_default;
        if existing_prop.default_value.is_none() {
            existing_prop.default_value = default_value;
        }
    } else {
        // `defineModel` synthesizes the prop from the model declaration —
        // there is no separate user prop annotation to lower, so the
        // typed companion is unset.
        props.push(PropAnalysis {
            name: name.clone(),
            callable_role: verter_type_expr::PropCallableRole::Other,
            publication: prop_publication.clone(),
            type_expansion: evaluated.and_then(|eval| {
                eval.props
                    .iter()
                    .find(|field| field.name == name)
                    .map(field_expansion_metadata)
            }),
            required: !has_default && !is_optional,
            has_default,
            default_value,
            description: None,
            tags: Vec::new(),
            // `defineModel` declares the prop name explicitly at the macro
            // call site (the model name is the prop name).
            declared_in_macro_type_arg: true,
        });
    }

    let event_name = format!("update:{name}");
    let evaluated_event =
        evaluated.and_then(|eval| eval.emits.iter().find(|field| field.name == event_name));
    // Only an UNDEFAULTED optional model's update event can emit
    // `undefined` — once a default exists the model ref always holds a
    // value, so the display payload stays the bare `[value: T]`.
    let source_payload_type = raw_type.as_deref().map(|raw| {
        if !has_default && is_optional {
            optionalize_model_source_type_text(raw)
        } else {
            raw.to_string()
        }
    });
    let raw_signature = event_raw_signature(
        evaluated_event
            .and_then(|field| field.authored_evidence.as_ref())
            .map(AuthoredTypeEvidence::text),
        source_payload_type
            .as_deref()
            .map(|raw| format!("[value: {raw}]"))
            .as_deref(),
    );
    // The event payload SOURCE derives from the NORMALIZED `defineModel`
    // surface: `update:X` emits the new model value, so a TYPED model
    // publishes the composed closed payload tuple `[value: <authored T>]`
    // (the element derefs the model's own authored type-argument payload on
    // demand through the one shared dispatch); an untyped `defineModel()` is
    // a PROVEN unannotated position. Never a same-name flat evaluated emit
    // from a sibling macro (the cross-macro shadow the sole-authority
    // rule removed). The flat lane contributes ONLY expansion metadata.
    let payload = source_prop
        .and_then(|row| row.field.payload.clone())
        .map(|locator| {
            use verter_type_expr::facts::{
                ClosedTypeFact, FactOrLocator, TupleElementFact, TuplePayloadFact,
            };
            SourcePosition::Present(SemanticTypeSource::Closed(ClosedTypeFact::Tuple(
                TuplePayloadFact {
                    readonly: false,
                    elements: std::sync::Arc::from(
                        vec![TupleElementFact {
                            label: Some("value".to_string()),
                            optional: false,
                            rest: false,
                            ty: FactOrLocator::MacroPayload(locator),
                        }]
                        .into_boxed_slice(),
                    ),
                },
            )))
        })
        .unwrap_or_else(SourcePosition::unannotated);

    if let Some(existing_event) = events.iter_mut().find(|event| event.name == event_name) {
        // A TYPED model's payload is authoritative for its own update event;
        // an untyped model must not DOWNGRADE an event another producer
        // already published with a present source.
        if payload.is_present() || !existing_event.publication.source_position().is_present() {
            existing_event.payload = payload.clone();
            existing_event.publication = publication_from_position(
                &payload,
                evaluated_event.map(field_expansion_metadata).as_ref(),
                source_prop.and_then(|row| {
                    authored_type_evidence(
                        row.field.payload.as_ref(),
                        source_payload_type.as_deref(),
                    )
                }),
                ResolutionProvenance::FrameworkSurface,
            );
        }
        existing_event.payload_expansion = existing_event
            .payload_expansion
            .clone()
            .or_else(|| evaluated_event.map(field_expansion_metadata));
        if existing_event.raw_signature.is_none() {
            existing_event.raw_signature = raw_signature;
        }
    } else {
        events.push(EventAnalysis {
            name: event_name.clone(),
            payload: payload.clone(),
            publication: publication_from_position(
                &payload,
                evaluated_event.map(field_expansion_metadata).as_ref(),
                source_prop.and_then(|row| {
                    authored_type_evidence(
                        row.field.payload.as_ref(),
                        source_payload_type.as_deref(),
                    )
                }),
                ResolutionProvenance::FrameworkSurface,
            ),
            return_publication: None,
            return_publication_scope: None,
            payload_expansion: evaluated_event.map(field_expansion_metadata),
            raw_signature,
            description: None,
            tags: Vec::new(),
        });
    }
}

fn optionalize_model_source_type_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || type_text_contains_undefined(trimmed) {
        trimmed.to_string()
    } else {
        format!("{trimmed} | undefined")
    }
}

/// Word-boundary-aware check for the `undefined` keyword in type text.
/// Avoids false positives on type names like `UndefinedHandler`.
fn type_text_contains_undefined(text: &str) -> bool {
    let needle = "undefined";
    let mut start = 0;
    while let Some(pos) = text[start..].find(needle) {
        let abs_pos = start + pos;
        let before_ok = abs_pos == 0
            || !text.as_bytes()[abs_pos - 1].is_ascii_alphanumeric()
                && text.as_bytes()[abs_pos - 1] != b'_';
        let after_pos = abs_pos + needle.len();
        let after_ok = after_pos >= text.len()
            || !text.as_bytes()[after_pos].is_ascii_alphanumeric()
                && text.as_bytes()[after_pos] != b'_';
        if before_ok && after_ok {
            return true;
        }
        start = abs_pos + 1;
    }
    false
}

fn extract_exposed_from_macro(
    macro_index: usize,
    mac: &AnalyzedMacro,
    resolved: Option<&ResolvedMacroInput>,
    bindings: &[AnalyzedBinding],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    out: &mut Vec<ExposedAnalysis>,
) {
    for field in &mac.expose_fields {
        // Mixed-form precedence: an authored object-literal exposure keeps
        // its own (binding-derived) source; the NORMALIZED type-argument
        // surface row only fills what the literal form does not provide (the
        // flat evaluated lane contributes metadata only).
        let resolved_field = resolved.and_then(|resolved| {
            resolved
                .exposed
                .iter()
                .find(|candidate| candidate.field.name == field.name)
        });
        // Resolve the REFERENCED LOCAL BINDING's type, never the exposed
        // property key: `defineExpose({ public: local })` must look up
        // `local` (the value expression's identifier), not `public` (the
        // published name), which may not exist as a local declaration at
        // all. `resolved_binding_key` returns `referenced_binding` ONLY
        // when the analyzer structurally captured one — a method or any
        // other non-identifier value expression (`{ public: local.foo }`)
        // has NO referenced binding at all, and must NOT fall back to
        // `field.name`: `public` is not itself a local declaration, and an
        // unrelated same-named binding elsewhere in scope must never be
        // substituted for it.
        let type_source = field
            .resolved_binding_key()
            .and_then(|key| resolve_exposed_type(key, bindings, evaluated))
            .or_else(|| resolved_field.map(|candidate| candidate.type_source.clone()))
            .unwrap_or_else(SourcePosition::unannotated);
        // Docs pair by name: the object-literal field's own leading JSDoc
        // wins; a `defineExpose<T>({ ... })` member without one inherits the
        // type-argument surface member's span-sliced JSDoc.
        let description = field
            .description
            .clone()
            .or_else(|| resolved_field.and_then(|candidate| candidate.field.description.clone()));
        let tags = if field.tags.is_empty() {
            resolved_field
                .map(|candidate| candidate.field.tags.clone())
                .unwrap_or_default()
        } else {
            field.tags.clone()
        };
        out.push(ExposedAnalysis {
            name: field.name.clone(),
            type_source,
            type_expansion: field
                .resolved_binding_key()
                .and_then(|key| {
                    evaluated.and_then(|eval| {
                        let mut matching = eval.bindings.iter().filter(|binding| {
                            binding.name == key.name.as_ref() && binding.owner == key.owner
                        });
                        let field = matching.next()?;
                        if matching.next().is_some() {
                            return None;
                        }
                        Some(field_expansion_metadata(field))
                    })
                })
                .or_else(|| {
                    exposed_lane_field(evaluated, macro_index, &field.name)
                        .map(field_expansion_metadata)
                }),
            description,
            tags,
        });
    }
    // `defineExpose<T>()` surface members with no object-literal counterpart
    // publish from the resolved type-argument surface, in surface order,
    // carrying their span-sliced docs and their NORMALIZED member-value
    // source ([`ResolvedExposeInput::type_source`] — the exposed-type
    // authority: the graph-native closed / shallow-ref / projected
    // member-path ladder, or the typed failure for a genuine miss). The
    // object-literal loop above already consumed the name-paired entries, so
    // this appends only the type-argument-only members. The flat evaluated
    // `exposed` lane contributes ONLY expansion metadata.
    let Some(resolved) = resolved else {
        return;
    };
    for candidate in &resolved.exposed {
        if mac
            .expose_fields
            .iter()
            .any(|f| f.name == candidate.field.name)
        {
            continue;
        }
        out.push(ExposedAnalysis {
            name: candidate.field.name.clone(),
            type_source: candidate.type_source.clone(),
            type_expansion: exposed_lane_field(evaluated, macro_index, &candidate.field.name)
                .map(field_expansion_metadata),
            description: candidate.field.description.clone(),
            tags: candidate.field.tags.clone(),
        });
    }
}

/// The projector's per-macro `defineExpose` lane entry for
/// `(macro_index, name)` — the join key pairing a type-argument-derived
/// exposure with its projected surface field.
fn exposed_lane_field<'a>(
    evaluated: Option<&'a crate::analysis::type_expand::ExpandedComponentTypes>,
    macro_index: usize,
    name: &str,
) -> Option<&'a crate::analysis::type_expand::ExpandedField> {
    evaluated?
        .exposed
        .iter()
        .find(|entry| entry.macro_index == macro_index)?
        .fields
        .iter()
        .find(|field| field.name == name)
}

fn resolve_exposed_type(
    key: &verter_type_expr::DeclBindingKey,
    bindings: &[AnalyzedBinding],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
) -> Option<SourcePosition> {
    if let Some(eval) = evaluated {
        // Same `(owner, name)` join constructors already use. A leftover
        // first-name match would type an instance exposure from a
        // module-owned constructor of the same spelling sitting in the
        // shared `.bindings` lane. Still-ambiguous after matching both
        // (a single owner cannot legally bind one name twice) fails
        // closed, never silently picking one.
        let mut matching = eval
            .bindings
            .iter()
            .filter(|f| f.name == key.name.as_ref() && f.owner == key.owner);
        if let Some(f) = matching.next() {
            if matching.next().is_some() {
                return Some(SourcePosition::Failed(
                    verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredMemberValue,
                ));
            }
            return match f.authority.outcome() {
                ResolvedTypeOutcome::Present { source, .. } => {
                    Some(SourcePosition::Present(source.as_ref().clone()))
                }
                // A demanded binding whose preparation FAILED is a genuine
                // typed failure, never collapsed into the same `None` an
                // unoffered binding returns below — the caller preserves
                // `Failed` through publication instead of rendering the
                // silent `Unknown` an `Absent` schema position would.
                ResolvedTypeOutcome::Failed { failure } => {
                    let verter_type_expr::TypedResolutionFailure::SourceConstruction(failure) =
                        failure;
                    Some(SourcePosition::Failed(*failure))
                }
                // A proven schema absence (an untyped binding) stays the
                // caller's own unannotated fallback — not this function's
                // concern.
                ResolvedTypeOutcome::Absent { .. } => None,
            };
        }
    }
    // `AnalyzedBinding` carries no typed payload position (its raw annotation
    // text is display-only) — an unevaluated exposed binding is an honest
    // untyped `None`, never a fabricated placeholder.
    let _ = bindings;
    None
}

// ── Options API fallback ───────────────────────────────────────────────────

/// Merge the analyzer's parse-domain prop fields with the host-resolved
/// rows into ONE per-macro row set, each carrying its member-value SOURCE.
/// An analyzer-only field's source is its authored payload position (or the
/// PROVEN unannotated absence for a payload-less runtime field); a
/// host-resolved row's `type_source` is the normalized-surface authority and
/// WINS on a same-name merge (the analysis metadata gap-fills through
/// [`merge_prop_field`]).
fn merged_prop_fields(
    mac: &AnalyzedMacro,
    resolved: Option<&ResolvedMacroInput>,
) -> Vec<ResolvedPropInput> {
    let mut rows: Vec<ResolvedPropInput> = mac
        .prop_fields
        .iter()
        .map(|field| {
            let position = authored_payload_position(field.payload.as_ref());
            ResolvedPropInput {
                authority: ResolvedTypeAuthority::from_source_position(
                    &position,
                    exactness_from_expansion(None, &position),
                    ResolutionProvenance::SemanticEvaluator,
                    std::sync::Arc::from([]),
                ),
                authored_evidence: authored_type_evidence(
                    field.payload.as_ref(),
                    field.type_annotation.as_deref(),
                ),
                callable_role: verter_type_expr::PropCallableRole::default(),
                field: field.clone(),
            }
        })
        .collect();
    if let Some(resolved) = resolved {
        for prop in &resolved.props {
            if let Some(existing) = rows
                .iter_mut()
                .find(|row| row.field.name == prop.field.name)
            {
                merge_prop_field(&mut existing.field, &prop.field);
                // The normalized-surface source is authoritative for the
                // merged row (it already prefers the PROVEN authored
                // position when one denotes the resolved member).
                existing.authority = prop.authority.clone();
                existing.authored_evidence = prop.authored_evidence.clone();
                existing.callable_role = prop.callable_role.clone();
            } else {
                rows.push(prop.clone());
            }
        }
    }
    rows
}

fn merge_prop_field(target: &mut AnalyzedPropField, candidate: &AnalyzedPropField) {
    if target.type_annotation.is_none() {
        target.type_annotation = candidate.type_annotation.clone();
    }
    if target.description.is_none() {
        target.description = candidate.description.clone();
    }
    if target.tags.is_empty() {
        target.tags = candidate.tags.clone();
    }
    if target.resolution_error.is_none() {
        target.resolution_error = candidate.resolution_error.clone();
    }
}

/// Return the canonical occurrence stream. Typed macros use only resolver
/// occurrences; runtime macros enter once from their exact authored syntax
/// entries even when the framework surface shell is present.
fn canonical_emit_occurrences(
    mac: &AnalyzedMacro,
    resolved: Option<&ResolvedMacroInput>,
    canonical_id: &str,
) -> Vec<ResolvedEmitInput> {
    if mac.is_type_based {
        return resolved
            .map(|resolved| resolved.emits.clone())
            .unwrap_or_default();
    }
    mac.emit_fields
        .iter()
        .map(|field| {
            let payload_source = authored_payload_position(field.payload.as_ref());
            ResolvedEmitInput {
                id: verter_type_expr::facts::ResolvedEmitOccurrenceId::runtime(
                    canonical_id,
                    field.span.start,
                    field.span.end,
                    field.name.as_str(),
                ),
                name: field.name.clone(),
                span: field.span,
                payload_type: field.payload_type.clone(),
                payload: field.payload.clone(),
                payload_expr_scope: field.payload_expr_scope.clone(),
                description: field.description.clone(),
                tags: field.tags.clone(),
                payload_publication: publication_from_position(
                    &payload_source,
                    None,
                    authored_type_evidence(field.payload.as_ref(), field.payload_type.as_deref()),
                    ResolutionProvenance::FrameworkSurface,
                ),
                payload_source,
                return_publication: None,
                return_publication_scope: None,
            }
        })
        .collect()
}

struct MergedSlotFields {
    fields: Vec<crate::analysis::types::AnalyzedSlotField>,
    return_publications: Vec<Option<TypePublication>>,
}

fn merged_slot_fields(
    mac: &AnalyzedMacro,
    resolved: Option<&ResolvedMacroInput>,
) -> MergedSlotFields {
    let mut fields = mac.slot_fields.clone();
    let mut return_publications = fields
        .iter()
        .map(slot_return_publication_from_field)
        .collect::<Vec<_>>();
    if let Some(resolved) = resolved {
        if fields.is_empty() {
            fields = resolved.slots.clone();
            return_publications = fields
                .iter()
                .enumerate()
                .map(|(index, field)| {
                    resolved
                        .slot_return_publications
                        .get(index)
                        .cloned()
                        .flatten()
                        .or_else(|| slot_return_publication_from_field(field))
                })
                .collect();
        } else {
            let mut seen_slots: rustc_hash::FxHashSet<String> =
                fields.iter().map(|field| field.name.clone()).collect();
            for (field_index, field) in fields.iter_mut().enumerate() {
                let Some((resolved_index, resolved_slot)) = resolved
                    .slots
                    .iter()
                    .enumerate()
                    .find(|(_, slot)| slot.name == field.name)
                else {
                    continue;
                };

                let mut seen_bindings: rustc_hash::FxHashSet<String> = field
                    .bindings
                    .iter()
                    .map(|binding| binding.name.clone())
                    .collect();
                for binding in &resolved_slot.bindings {
                    if seen_bindings.insert(binding.name.clone()) {
                        field.bindings.push(binding.clone());
                    }
                }

                if field.return_type.is_none() {
                    field.return_type = resolved_slot.return_type.clone();
                }
                if field.description.is_none() {
                    field.description = resolved_slot.description.clone();
                }
                if field.tags.is_empty() {
                    field.tags = resolved_slot.tags.clone();
                }
                field.is_required |= resolved_slot.is_required;
                if let Some(publication) = resolved
                    .slot_return_publications
                    .get(resolved_index)
                    .cloned()
                    .flatten()
                {
                    return_publications[field_index] = Some(publication);
                }
            }

            for (resolved_index, resolved_slot) in resolved.slots.iter().enumerate() {
                if seen_slots.insert(resolved_slot.name.clone()) {
                    fields.push(resolved_slot.clone());
                    return_publications.push(
                        resolved
                            .slot_return_publications
                            .get(resolved_index)
                            .cloned()
                            .flatten()
                            .or_else(|| slot_return_publication_from_field(resolved_slot)),
                    );
                }
            }
        }
    }
    MergedSlotFields {
        fields,
        return_publications,
    }
}

fn extract_props_from_options(
    opts: &AnalyzedOptionsApi,
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    out: &mut Vec<PropAnalysis>,
) {
    for prop in &opts.props {
        let raw_type = prop
            .type_annotation
            .clone()
            .or_else(|| prop.type_constructor.clone());
        // Prefer the authored `PropType<T>` payload position when available;
        // else the binding-index-gated runtime-constructor fold (closed
        // primitive fact for `Global` String/Number/Boolean, a resolved
        // local reference for `Local`, a typed failure for `Indeterminate`);
        // else the PROVEN unannotated absence (a non-primitive `Global`
        // spelling keeps its display-text-only route via `raw_type`, and a
        // genuinely annotation-less prop has no source at all).
        let position = authored_payload_source(prop.payload.as_ref())
            .map(SourcePosition::Present)
            .or_else(|| constructor_binding_source_position(&prop.constructor_bindings, evaluated))
            .unwrap_or_else(SourcePosition::unannotated);
        out.push(PropAnalysis {
            name: prop.name.clone(),
            callable_role: verter_type_expr::PropCallableRole::Other,
            publication: publication_from_position(
                &position,
                None,
                authored_type_evidence(prop.payload.as_ref(), raw_type.as_deref()),
                ResolutionProvenance::SemanticEvaluator,
            ),
            type_expansion: None,
            // Options API path: no authored companion for the raw display
            // text (the typed source is above).
            required: prop.is_required,
            has_default: prop.has_default,
            default_value: prop.default_value.clone(),
            description: prop.description.clone(),
            tags: prop.tags.clone(),
            // Options API props are explicit author declarations in the
            // `props: { ... }` object on the component's options block —
            // structurally equivalent to a runtime `defineProps({...})`
            // surface (the author wrote the name directly).
            declared_in_macro_type_arg: true,
        });
    }
}

fn extract_events_from_options(opts: &AnalyzedOptionsApi, out: &mut Vec<EventAnalysis>) {
    for field in &opts.emits {
        // Options-API emits carry no authored payload position (validator
        // payload text is display-only on `raw_signature`) — the typed
        // channel is host-raised.
        out.push(EventAnalysis {
            name: field.name.clone(),
            payload: authored_payload_position(field.payload.as_ref()),
            publication: publication_from_position(
                &authored_payload_position(field.payload.as_ref()),
                None,
                authored_type_evidence(field.payload.as_ref(), field.payload_type.as_deref()),
                ResolutionProvenance::FrameworkSurface,
            ),
            return_publication: None,
            return_publication_scope: None,
            payload_expansion: None,
            raw_signature: field.payload_type.clone(),
            description: field.description.clone(),
            tags: field.tags.clone(),
        });
    }
}

// ── Flags ──────────────────────────────────────────────────────────────────

fn extract_components(
    template: Option<&crate::analysis::template::TemplateAnalysisSnapshot>,
) -> Vec<ComponentUsageAnalysis> {
    let Some(template) = template else {
        return Vec::new();
    };

    template
        .components
        .iter()
        .map(|component| ComponentUsageAnalysis {
            name: component.name.clone(),
            import_source: component.import_source.clone(),
            is_dynamic: component.is_dynamic,
            props: component
                .props
                .iter()
                .map(|prop| ComponentPropUsageAnalysis {
                    name: prop.name.clone(),
                    is_bound: prop.is_bound,
                    constness: prop.constness,
                    expression: prop.expression.clone(),
                    referenced_bindings: prop.referenced_bindings.clone(),
                    from_spread: prop.from_spread,
                    is_shorthand: prop.is_shorthand,
                })
                .collect(),
            has_spread: component.has_spread,
            slots_used: component.slots_used.clone(),
            static_classes: component.static_classes.clone(),
            has_dynamic_class: component.has_dynamic_class,
            v_models: component
                .v_models
                .iter()
                .map(|model| model.binding_name.clone())
                .collect(),
            v_model_entries: component
                .v_models
                .iter()
                .map(|model| ComponentVModelUsageAnalysis {
                    binding_name: model.binding_name.clone(),
                })
                .collect(),
            bindings: component
                .bindings
                .iter()
                .map(|binding| ComponentBindingUsageAnalysis {
                    name: binding.name.clone(),
                    modifiers: binding.modifiers.clone(),
                })
                .collect(),
            events: component
                .events
                .iter()
                .map(|event| ComponentEventUsageAnalysis {
                    name: event.name.clone(),
                    handler_expression: event.handler_expression.clone(),
                    is_inline: event.is_inline,
                    modifiers: event.modifiers.clone(),
                })
                .collect(),
        })
        .collect()
}

fn extract_template_refs(
    template: Option<&crate::analysis::template::TemplateAnalysisSnapshot>,
) -> Vec<TemplateRefAnalysis> {
    let Some(template) = template else {
        return Vec::new();
    };

    template
        .template_refs
        .iter()
        .map(|template_ref| TemplateRefAnalysis {
            name: template_ref.name.clone(),
            is_dynamic: template_ref.is_dynamic,
            target_tag: template_ref.target_tag.clone(),
        })
        .collect()
}

fn extract_imports(imports: &[AnalyzedImport]) -> Vec<ImportAnalysis> {
    imports
        .iter()
        .map(|import| ImportAnalysis {
            source: import.source.clone(),
            is_type_only: import.is_type_only,
            bindings: import
                .bindings
                .iter()
                .map(|binding| ImportBindingAnalysis {
                    name: binding.name.clone(),
                    kind: binding.kind,
                    imported_name: binding.imported_name.clone(),
                    is_type_only: binding.is_type_only,
                })
                .collect(),
        })
        .collect()
}

/// Refine a binding's collapsed decoration kind from an EXACT resolved
/// whole-return wrapper role — MONOTONE: it never downgrades a value-space
/// classification.
///
/// - An exact wrapper family maps onto its decoration kind.
/// - [`ReactiveWrapperRole::None`] is a proof about the WRAPPER FAMILY of the
///   whole return type, not a proof of non-reactivity: Vue's `reactive<T>()`
///   returns `UnwrapNestedRefs<T>`, so a non-wrapper return type does not
///   contradict a value-space `reactive` fact. It refines nothing.
/// - A typed degradation refines nothing; the reason is published on the
///   binding's `return_wrapper_role` sidecar instead.
fn refined_reactivity_kind(
    current: crate::analysis::types::ReactivityKind,
    role: &verter_type_expr::ReactiveWrapperRole,
) -> crate::analysis::types::ReactivityKind {
    use crate::analysis::types::ReactivityKind;
    use verter_type_expr::ReactiveWrapperRole;
    match role {
        ReactiveWrapperRole::Ref
        | ReactiveWrapperRole::ShallowRef
        | ReactiveWrapperRole::ModelRef => ReactivityKind::Ref,
        ReactiveWrapperRole::ComputedRef => ReactivityKind::Computed,
        ReactiveWrapperRole::Reactive | ReactiveWrapperRole::ShallowReactive => {
            ReactivityKind::Reactive
        }
        ReactiveWrapperRole::None | ReactiveWrapperRole::Unresolved { .. } => current,
    }
}

fn extract_bindings(
    bindings: &[AnalyzedBinding],
    template: Option<&crate::analysis::template::TemplateAnalysisSnapshot>,
    resolved_binding_reactivity: &[ResolvedBindingReactivityInput],
) -> Vec<BindingAnalysis> {
    let template_bindings: std::collections::HashSet<&str> = template
        .map(|template| {
            template
                .binding_occurrences
                .iter()
                .map(|occurrence| occurrence.name.as_str())
                .collect()
        })
        .unwrap_or_default();
    let resolved_roles: rustc_hash::FxHashMap<usize, &verter_type_expr::ReactiveWrapperRole> =
        resolved_binding_reactivity
            .iter()
            .map(|row| (row.binding_index, &row.return_wrapper_role))
            .collect();

    bindings
        .iter()
        .enumerate()
        .map(|(binding_index, binding)| BindingAnalysis {
            name: binding.name.clone(),
            kind: match binding.kind {
                crate::analysis::types::AnalyzedBindingKind::Const => BindingKindAnalysis::Const,
                crate::analysis::types::AnalyzedBindingKind::Let => BindingKindAnalysis::Let,
                crate::analysis::types::AnalyzedBindingKind::Var => BindingKindAnalysis::Var,
                crate::analysis::types::AnalyzedBindingKind::Function => {
                    BindingKindAnalysis::Function
                }
                crate::analysis::types::AnalyzedBindingKind::AsyncFunction => {
                    BindingKindAnalysis::AsyncFunction
                }
                crate::analysis::types::AnalyzedBindingKind::Class => BindingKindAnalysis::Class,
            },
            reactivity_kind: match resolved_roles.get(&binding_index) {
                Some(role) => refined_reactivity_kind(binding.reactivity_kind, role),
                None => binding.reactivity_kind,
            },
            return_wrapper_role: resolved_roles
                .get(&binding_index)
                .map(|role| (*role).clone()),
            type_annotation: binding.type_annotation.clone(),
            used_in_template: template_bindings.contains(binding.name.as_str()),
            used_in_style: binding.used_in_style,
        })
        .collect()
}

fn extract_vue_api_calls(calls: &[VueApiCallSite]) -> Vec<VueApiCallAnalysis> {
    calls
        .iter()
        .map(|call| VueApiCallAnalysis {
            api: call.api,
            arg_value: call.arg_value.clone(),
        })
        .collect()
}

fn extract_styles(styles: &[crate::analysis::style::StyleBlockAnalysis]) -> Vec<StyleAnalysis> {
    styles
        .iter()
        .map(|style| {
            let css = style.css.as_ref();

            StyleAnalysis {
                lang: style.lang,
                scoped: style.scoped,
                is_module: style.is_module,
                module_name: style.module_name.clone(),
                block_ref: style.block_ref.clone(),
                classes: css
                    .map(|css| css.classes.iter().map(|class| class.name.clone()).collect())
                    .unwrap_or_default(),
                ids: css
                    .map(|css| css.ids.iter().map(|id| id.name.clone()).collect())
                    .unwrap_or_default(),
                custom_properties: css
                    .map(|css| {
                        css.custom_properties
                            .iter()
                            .map(|property| property.name.clone())
                            .collect()
                    })
                    .unwrap_or_default(),
                v_binds: style
                    .v_binds
                    .iter()
                    .map(|v_bind| v_bind.expression.clone())
                    .collect(),
                selectors: css
                    .map(|css| {
                        css.selectors
                            .iter()
                            .map(|selector| SelectorAnalysis {
                                text: selector.text.clone(),
                                specificity: selector.specificity,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect()
}

fn extract_flags(input: &ComponentMetaInput<'_>) -> ComponentMetaFlags {
    let has_inherit_attrs_false = input
        .macros
        .iter()
        .any(|m| m.kind == AnalyzedMacroKind::DefineOptions && m.has_inherit_attrs_false);

    let flags = input.analysis_flags;

    ComponentMetaFlags {
        async_setup: flags.contains(AnalysisFlags::ASYNC_SETUP),
        has_reactive_state: flags.contains(AnalysisFlags::HAS_REACTIVE_STATE),
        has_computed: flags.contains(AnalysisFlags::HAS_COMPUTED),
        has_watchers: flags.contains(AnalysisFlags::HAS_WATCHERS),
        has_lifecycle_hooks: flags.contains(AnalysisFlags::HAS_LIFECYCLE_HOOKS),
        has_provide: flags.contains(AnalysisFlags::HAS_PROVIDE),
        has_inject: flags.contains(AnalysisFlags::HAS_INJECT),
        has_inherit_attrs_false: flags.contains(AnalysisFlags::HAS_INHERIT_ATTRS_FALSE)
            || has_inherit_attrs_false,
        has_store_usage: !input.store_usages.is_empty(),
        // D123 — analysis itself has no view of lowering failures, so
        // the field is emitted `false` here. No downstream production
        // path flips it today; macro-impacting lowering failures are
        // surfaced through the structured `macro_expansion_diagnostics`
        // entries rather than this flag.
        has_macro_failure: false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[path = "component_meta_tests.rs"]
mod component_meta_tests;
