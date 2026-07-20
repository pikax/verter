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
    AnalysisFlags, AnalyzedBinding, AnalyzedEmitField, AnalyzedExposeField, AnalyzedImport,
    AnalyzedMacro, AnalyzedMacroKind, AnalyzedOptionsApi, AnalyzedPropField, AnalyzedSlotField,
    ImportBindingKind, JsdocTag, StoreUsage, VueApiCallSite,
};
use verter_type_expr::facts::{SemanticTypeSource, SourcePosition};
use verter_type_expr::locators::{AuthoredBodyLocator, MacroPayloadLocator};
use verter_type_expr::TypeExprScope;
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

/// A present source, else the PROVEN unannotated schema absence. Used at
/// the extraction fallbacks whose `None` structurally means "this position
/// carries no semantic annotation" (untyped bindings, unevaluated exposures,
/// display-only positions) — REQUIRED-position failures never flow through
/// these fallbacks (they arrive as already-classified `Failed` positions on
/// the evaluated lanes).
fn present_or_unannotated(source: Option<SemanticTypeSource>) -> SourcePosition {
    source
        .map(SourcePosition::Present)
        .unwrap_or_else(SourcePosition::unannotated)
}

/// Choose the resolved source POSITION for a field: the evaluated position,
/// unless the expansion is INCOMPLETE and an authored payload exists — an
/// incomplete evaluation's honest surface is the author's own annotation
/// position (the symbolic fallback), never a torn partial.
///
/// A `Failed` evaluated position is FINAL and never falls back: it is the
/// producer's TYPED fail-closed decision (e.g. a merged same-name member
/// with an unresolvable contributor), not a torn partial — degrading it to
/// one contributor's authored annotation would mask the failure with that
/// contributor's value (a wrong concrete success). The authored fallback
/// applies only to the incomplete evaluation's non-failed positions.
fn prefer_authored_on_incomplete(
    evaluated: &SourcePosition,
    authored: Option<&MacroPayloadLocator>,
    metadata: Option<&crate::analysis::type_expand::ExpansionMetadata>,
) -> SourcePosition {
    let incomplete = metadata.is_some_and(|m| {
        m.exactness == crate::analysis::type_expand::ExpansionExactness::Incomplete
    });
    if incomplete && !matches!(evaluated, SourcePosition::Failed(_)) {
        if let Some(source) = authored_payload_source(authored) {
            return SourcePosition::Present(source);
        }
    }
    evaluated.clone()
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
    pub type_source: SourcePosition,
}

/// One host-resolved emit row: the emit analysis field plus the payload's
/// session-resolved SOURCE POSITION — the emit-payload authority the
/// extraction publishes (the flat evaluated lanes contribute metadata only).
#[derive(Debug, Clone)]
pub struct ResolvedEmitInput {
    /// The emit analysis field.
    pub field: AnalyzedEmitField,
    /// The payload's published source position.
    pub payload_source: SourcePosition,
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
    /// Host-resolved exposed rows for the macro (`defineExpose<T>()`
    /// type-argument surface members with their span-sliced JSDoc), each
    /// with its member-value source.
    pub exposed: Vec<ResolvedExposeInput>,
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
    /// Host-populated SFC block metadata sidecar derived from parsed root blocks.
    pub sfc_blocks: Option<SfcBlocksAnalysis>,
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
    /// The resolved type SOURCE POSITION: the evaluated source unless the
    /// expansion is incomplete and an authored payload exists (the symbolic
    /// fallback); a PROVEN schema absence when the position carries no
    /// annotation; a typed failure when the REQUIRED value position's source
    /// could not be constructed (fails output materialization).
    pub type_source: SourcePosition,
    /// Completeness and diagnostics from native expansion when available.
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    /// Original annotation text from the source.
    pub raw_type: Option<String>,
    /// Authored-source companion of `raw_type` — the author's own annotation
    /// position when the chosen `raw_type` text equals the source annotation.
    /// `None` when the chosen `raw_type` came from a backend's textual
    /// rendering or the source had no typed payload.
    pub raw_type_source: Option<SemanticTypeSource>,
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
    /// The resolved payload SOURCE POSITION: present, PROVEN schema-absent
    /// (an unannotated runtime emit), or a typed failure at the REQUIRED
    /// payload-tuple position (fails output materialization).
    pub payload: SourcePosition,
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
    /// The slot return's authored SOURCE, propagated from
    /// [`AnalyzedSlotField::payload`] — `return_type` is display-only.
    pub return_source: Option<SemanticTypeSource>,
    /// Scope of `return_source`: canonical_id of the file whose parse produced
    /// the authored return type. Pairing invariant:
    /// `return_source.is_some() <=> return_source_scope.is_some()`.
    pub return_source_scope: Option<TypeExprScope>,
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
    pub type_source: SourcePosition,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    pub raw_type: Option<String>,
    /// Authored-source companion of `raw_type`. See
    /// [`PropAnalysis::raw_type_source`] for the contract.
    pub raw_type_source: Option<SemanticTypeSource>,
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

/// Host-populated SFC root block metadata exposed by the official API.
#[derive(Debug, Clone)]
pub struct SfcBlocksAnalysis {
    pub template: Option<TemplateBlockAnalysis>,
    pub script: Option<ScriptBlockAnalysis>,
    pub script_setup: Option<ScriptBlockAnalysis>,
    pub styles: Vec<StyleBlockInfoAnalysis>,
    pub custom: Vec<CustomBlockAnalysis>,
}

/// A single raw SFC root-block attribute.
#[derive(Debug, Clone)]
pub struct SfcAttributeAnalysis {
    pub name: String,
    pub value: Option<String>,
}

/// Metadata for the `<template>` block.
#[derive(Debug, Clone)]
pub struct TemplateBlockAnalysis {
    pub lang: Option<String>,
    pub src: Option<String>,
    pub attributes: Vec<SfcAttributeAnalysis>,
}

/// Metadata for a `<script>` or `<script setup>` block.
#[derive(Debug, Clone)]
pub struct ScriptBlockAnalysis {
    pub lang: Option<String>,
    pub src: Option<String>,
    pub generic: Option<String>,
    pub attrs_type: Option<String>,
    pub attributes: Vec<SfcAttributeAnalysis>,
}

/// Metadata for a `<style>` block.
#[derive(Debug, Clone)]
pub struct StyleBlockInfoAnalysis {
    pub index: usize,
    pub lang: Option<String>,
    pub src: Option<String>,
    pub scoped: bool,
    pub is_module: bool,
    pub module_name: Option<String>,
    pub attributes: Vec<SfcAttributeAnalysis>,
}

/// Metadata for a custom root block such as `<i18n>`.
#[derive(Debug, Clone)]
pub struct CustomBlockAnalysis {
    pub index: usize,
    pub block_type: String,
    pub lang: Option<String>,
    pub src: Option<String>,
    pub attributes: Vec<SfcAttributeAnalysis>,
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
    /// The accepted prop's resolved type SOURCE POSITION (`Absent` =
    /// untyped / cross-branch divergent).
    pub type_source: SourcePosition,
    /// The canonical file scope `type_source`'s SCOPE-RELATIVE names (bare
    /// `Ref` leaf spellings, producer-local anchors at any nesting depth)
    /// resolve under — the PRODUCING owner of an inherited source, carried
    /// positionally per the cross-owner effective-scope invariant. `None` =
    /// the analysis owner itself (own/declared rows, intrinsic attr rows).
    pub type_source_scope: Option<String>,
    pub raw_type: Option<String>,
    /// Authored-source companion of `raw_type`. See
    /// [`PropAnalysis::raw_type_source`] for the contract.
    pub raw_type_source: Option<SemanticTypeSource>,
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
    /// The inherited prop's resolved type SOURCE POSITION (`Absent` =
    /// untyped).
    pub type_source: SourcePosition,
    /// The canonical file scope `type_source`'s SCOPE-RELATIVE names resolve
    /// under — the PRODUCING owner (the terminal origin of a multi-hop
    /// inheritance chain), carried positionally per the cross-owner
    /// effective-scope invariant. `None` = the intrinsic/native case with no
    /// producing file (the branch owner's scope applies).
    pub type_source_scope: Option<String>,
    pub raw_type: Option<String>,
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
                let emit_fields = merged_emit_fields(mac, resolved_macro.as_ref());
                extract_events_from_macro(macro_index, &emit_fields, evaluated_types, &mut events);
            }
            AnalyzedMacroKind::DefineSlots => {
                let slot_fields = merged_slot_fields(mac, resolved_macro.as_ref());
                extract_slots_from_macro(macro_index, &slot_fields, evaluated_types, &mut slots);
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

    // Merge template-discovered slots with defineSlots
    if let Some(tpl) = input.template {
        merge_template_slots(&tpl.defined_slots, &mut slots);
    }

    // Options API fallback
    if let Some(opts) = input.options_api {
        if props.is_empty() {
            extract_props_from_options(opts, &mut props);
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
    let bindings = extract_bindings(input.bindings, input.template);
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
        sfc_blocks: None,
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
                let source_field = prop_fields
                    .iter()
                    .find(|row| row.field.name == field.name)
                    .map(|row| &row.field);
                let evaluated_field = evaluated.and_then(|eval| {
                    eval.props
                        .iter()
                        .find(|candidate| candidate.name == field.name)
                });
                let has_default = default_keys.contains(field.name.as_str());
                let default_value = default_values
                    .get(field.name.as_str())
                    .map(|v| v.to_string());
                let type_expansion = define_props_property_expansion_metadata(
                    evaluated,
                    macro_index,
                    field.name.as_str(),
                );
                let raw_type = prop_raw_type_from_evaluated_and_source(
                    evaluated_field.and_then(|candidate| candidate.raw_type.as_deref()),
                    source_field.and_then(|prop| prop.type_annotation.as_deref()),
                    field.optional,
                );
                let raw_type_source = raw_type_source_from_source_annotation(
                    raw_type.as_deref(),
                    source_field.and_then(|prop| prop.type_annotation.as_deref()),
                    source_field.and_then(|prop| prop.payload.as_ref()),
                );
                let type_source = prefer_authored_on_incomplete(
                    &field.ty,
                    source_field.and_then(|prop| prop.payload.as_ref()),
                    type_expansion.as_ref(),
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
                    type_source,
                    type_expansion,
                    raw_type,
                    raw_type_source,
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
        let (type_source, type_expansion) = resolve_prop_type(row, evaluated);
        let has_default = default_keys.contains(field.name.as_str());
        let default_value = default_values
            .get(field.name.as_str())
            .map(|v| v.to_string());

        let raw_type = prop_raw_type_from_evaluated_and_source(
            evaluated.and_then(|eval| {
                eval.props
                    .iter()
                    .find(|candidate| candidate.name == field.name)
                    .and_then(|candidate| candidate.raw_type.as_deref())
            }),
            field.type_annotation.as_deref(),
            field.is_optional,
        );
        let raw_type_source = raw_type_source_from_source_annotation(
            raw_type.as_deref(),
            field.type_annotation.as_deref(),
            field.payload.as_ref(),
        );

        out.push(PropAnalysis {
            name: field.name.clone(),
            type_source,
            type_expansion,
            raw_type,
            raw_type_source,
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
            let raw_type = evaluated.and_then(|eval| {
                eval.props
                    .iter()
                    .find(|candidate| candidate.name == field.name)
                    .and_then(|candidate| candidate.raw_type.clone())
            });
            // Evaluator-only branch: no source AnalyzedPropField in scope, so
            // no authored symbolic fallback is available — the evaluated
            // position is kept as-is.
            let type_source = field.ty.clone();

            // No source field reachable from this branch, so no JSDoc supply:
            // doc text rides the span-borne member supply only — an
            // evaluator-only member publishes no description and no tags. The
            // chosen raw_type came from the evaluator's textual rendering,
            // which has no typed companion.
            out.push(PropAnalysis {
                name: field.name.clone(),
                type_source,
                type_expansion,
                raw_type,
                raw_type_source: None,
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

fn expanded_define_emits_shape(
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    macro_index: usize,
) -> Option<&crate::analysis::type_expand::ExpandedMacroObjectShape> {
    evaluated?
        .define_emits
        .iter()
        .find(|entry| entry.macro_index == macro_index)
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

fn macro_object_property_expansion_metadata(
    entry: &crate::analysis::type_expand::ExpandedMacroObjectShape,
    property_name: &str,
) -> crate::analysis::type_expand::ExpansionMetadata {
    // Only include diagnostics specific to this property.
    // Macro-wide diagnostics (property_name == None) are in `macro_expansion_diagnostics`.
    let diagnostics = entry
        .result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.property_name.as_deref() == Some(property_name))
        .cloned()
        .collect();

    crate::analysis::type_expand::ExpansionMetadata {
        exactness: entry.result.exactness,
        execution_status: entry.result.execution_status,
        diagnostics,
    }
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
    let mut seen_emits = rustc_hash::FxHashSet::default();
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
            exposed: Vec::new(),
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
            if seen_emits.insert(emit.field.name.clone()) {
                entry.emits.push(emit.clone());
            }
        }
        for slot in &resolved.slots {
            if seen_slots.insert(slot.name.clone()) {
                entry.slots.push(slot.clone());
            }
        }
        for exposed in &resolved.exposed {
            if seen_exposed.insert(exposed.field.name.clone()) {
                entry.exposed.push(exposed.clone());
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
    SourcePosition,
    Option<crate::analysis::type_expand::ExpansionMetadata>,
) {
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
    let type_source = prefer_authored_on_incomplete(
        &row.type_source,
        row.field.payload.as_ref(),
        metadata.as_ref(),
    );
    (type_source, metadata)
}

// ── Events ─────────────────────────────────────────────────────────────────

fn prop_raw_type_from_evaluated_and_source(
    evaluated_raw_type: Option<&str>,
    source_annotation: Option<&str>,
    _is_optional: bool,
) -> Option<String> {
    symbolic_type_from_evaluated_and_source(evaluated_raw_type, source_annotation)
}

/// Compute the authored-source companion `raw_type_source` for a
/// `PropAnalysis` / `SlotBindingAnalysis` when the chosen `raw_type` text
/// equals the user's source annotation text: the author's own payload
/// position, demanded through the shared dispatch on read.
///
/// Returns `None` when the chosen `raw_type` came from a backend's textual
/// rendering (no authored companion) or when the source had no payload
/// position.
fn raw_type_source_from_source_annotation(
    chosen_raw_type: Option<&str>,
    source_annotation: Option<&str>,
    source_payload: Option<&MacroPayloadLocator>,
) -> Option<SemanticTypeSource> {
    let chosen = chosen_raw_type?.trim();
    let source = source_annotation?.trim();
    if chosen.is_empty() || source.is_empty() || chosen != source {
        return None;
    }
    authored_payload_source(source_payload)
}

fn symbolic_type_from_evaluated_and_source(
    evaluated_raw_type: Option<&str>,
    source_annotation: Option<&str>,
) -> Option<String> {
    let evaluated_raw_type = evaluated_raw_type
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let source_annotation = source_annotation
        .map(str::trim)
        .filter(|text| !text.is_empty());

    match (evaluated_raw_type, source_annotation) {
        (Some(raw_type), Some(source_annotation))
            if backend_raw_type_is_suspicious_identifier(raw_type)
                && normalize_type_text_for_compare(raw_type)
                    != normalize_type_text_for_compare(source_annotation) =>
        {
            Some(source_annotation.to_string())
        }
        (Some(raw_type), Some(source_annotation))
            if source_annotation_beats_backend_prop_display(raw_type, source_annotation) =>
        {
            Some(source_annotation.to_string())
        }
        (Some(raw_type), _) => Some(raw_type.to_string()),
        (None, Some(source_annotation)) => Some(source_annotation.to_string()),
        (None, None) => None,
    }
}

fn prop_raw_type_is_placeholder(raw_type: &str) -> bool {
    matches!(raw_type.trim(), "" | "any" | "unknown")
}

fn backend_raw_type_is_suspicious_identifier(raw_type: &str) -> bool {
    let trimmed = raw_type.trim();
    !trimmed.is_empty()
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
        && trimmed
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase() || ch == '_' || ch == '$')
        && !matches!(
            trimmed,
            "any"
                | "unknown"
                | "string"
                | "number"
                | "boolean"
                | "symbol"
                | "bigint"
                | "void"
                | "never"
                | "null"
                | "undefined"
                | "object"
                | "true"
                | "false"
        )
}

fn source_annotation_beats_placeholder_backend_type(annotation: &str) -> bool {
    let annotation = annotation.trim();
    !annotation.is_empty() && !matches!(annotation, "any" | "unknown")
}

fn source_annotation_beats_backend_prop_display(raw_type: &str, source_annotation: &str) -> bool {
    if !source_annotation_beats_placeholder_backend_type(source_annotation) {
        return false;
    }

    prop_raw_type_is_placeholder(raw_type)
        || backend_optionalizes_source_annotation(raw_type, source_annotation)
        || (source_annotation_contains_conditional(source_annotation)
            && !source_annotation_contains_conditional(raw_type))
        || (source_annotation_contains_indexed_access(source_annotation)
            && backend_raw_type_is_expanded_display(raw_type))
}

fn backend_optionalizes_source_annotation(raw_type: &str, source_annotation: &str) -> bool {
    let Some(stripped) = strip_top_level_undefined_from_union(raw_type) else {
        return false;
    };
    normalize_type_text_for_compare(&stripped) == normalize_type_text_for_compare(source_annotation)
}

fn strip_top_level_undefined_from_union(text: &str) -> Option<String> {
    let mut parts = split_top_level_union(text);
    let original_len = parts.len();
    parts.retain(|part| normalize_type_text_for_compare(part) != "undefined");
    if parts.len() == original_len {
        return None;
    }
    Some(parts.join(" | "))
}

fn split_top_level_union(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut brace_depth = 0i32;
    let chars: Vec<_> = text.char_indices().collect();

    for (index, ch) in chars {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            '|' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                let part = text[start..index].trim();
                if !part.is_empty() {
                    parts.push(part.to_string());
                }
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    let tail = text[start..].trim();
    if !tail.is_empty() {
        parts.push(tail.to_string());
    }
    parts
}

fn normalize_type_text_for_compare(text: &str) -> String {
    text.split_whitespace().collect()
}

fn source_annotation_contains_conditional(text: &str) -> bool {
    text.contains(" extends ")
}

fn source_annotation_contains_indexed_access(text: &str) -> bool {
    text.contains('[') && text.contains(']')
}

fn backend_raw_type_is_expanded_display(text: &str) -> bool {
    let text = text.trim();
    text.starts_with('{') || text.contains('\n')
}

fn event_raw_signature_from_evaluated_and_source(
    evaluated_raw_type: Option<&str>,
    source_payload: Option<&str>,
) -> Option<String> {
    let evaluated_raw_type = evaluated_raw_type
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let source_payload = source_payload
        .map(str::trim)
        .filter(|text| !text.is_empty());

    match (evaluated_raw_type, source_payload) {
        (Some(raw_type), Some(source_payload))
            if source_payload_beats_backend_event_display(raw_type, source_payload) =>
        {
            Some(source_payload.to_string())
        }
        (Some(raw_type), _) => Some(preserve_or_wrap_event_payload(raw_type)),
        (None, Some(source_payload)) => Some(source_payload.to_string()),
        (None, None) => None,
    }
}

fn preserve_or_wrap_event_payload(raw_type: &str) -> String {
    let trimmed = raw_type.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed.to_string()
    } else {
        format!("[value: {trimmed}]")
    }
}

fn source_payload_beats_backend_event_display(raw_type: &str, source_payload: &str) -> bool {
    let raw_inner = strip_event_tuple_wrapper(raw_type).unwrap_or(raw_type);
    let source_inner = strip_event_tuple_wrapper(source_payload).unwrap_or(source_payload);
    source_annotation_beats_placeholder_backend_type(source_inner)
        && (prop_raw_type_is_placeholder(raw_type)
            || (backend_raw_type_is_suspicious_identifier(raw_inner)
                && normalize_type_text_for_compare(raw_inner)
                    != normalize_type_text_for_compare(source_inner))
            || backend_optionalizes_source_annotation(source_inner, raw_inner)
            || (source_annotation_contains_conditional(source_inner)
                && !source_annotation_contains_conditional(raw_type))
            || (source_annotation_contains_indexed_access(source_inner)
                && backend_raw_type_is_expanded_display(raw_type)))
}

fn strip_event_tuple_wrapper(source_payload: &str) -> Option<&str> {
    let payload = source_payload.trim();
    let payload = payload.strip_prefix("[value:")?;
    let payload = payload.strip_suffix(']')?;
    Some(payload.trim())
}

fn extract_events_from_macro(
    macro_index: usize,
    emit_fields: &[ResolvedEmitInput],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    out: &mut Vec<EventAnalysis>,
) {
    let expanded_events = expanded_define_emit_events(evaluated, macro_index);
    if !expanded_events.is_empty() {
        let expanded_by_name: rustc_hash::FxHashMap<_, _> = expanded_events
            .iter()
            .cloned()
            .map(|event| (event.name.clone(), event))
            .collect();

        if emit_fields.is_empty() {
            for event in expanded_events {
                let evaluated_field = evaluated.and_then(|eval| {
                    eval.emits
                        .iter()
                        .find(|candidate| candidate.name == event.name)
                });
                out.push(EventAnalysis {
                    name: event.name,
                    payload: event.payload,
                    payload_expansion: event.payload_expansion,
                    raw_signature: event_raw_signature_from_evaluated_and_source(
                        evaluated_field.and_then(|candidate| candidate.raw_type.as_deref()),
                        None,
                    ),
                    description: None,
                    tags: Vec::new(),
                });
            }
            return;
        }

        for row in emit_fields {
            let field = &row.field;
            // The `define_emits` SHAPE lane row (built from the normalized
            // payload source) is the payload authority when present; the
            // row's own session-resolved `payload_source` covers the rest.
            // The flat evaluated lane contributes ONLY expansion metadata —
            // never the payload source.
            let (payload, payload_expansion) = expanded_by_name
                .get(&field.name)
                .map(|event| (event.payload.clone(), event.payload_expansion.clone()))
                .unwrap_or_else(|| {
                    let metadata = evaluated.and_then(|eval| {
                        eval.emits
                            .iter()
                            .find(|f| f.name == field.name)
                            .map(field_expansion_metadata)
                    });
                    (row.payload_source.clone(), metadata)
                });

            out.push(EventAnalysis {
                name: field.name.clone(),
                payload,
                payload_expansion,
                raw_signature: event_raw_signature_from_evaluated_and_source(
                    evaluated.and_then(|eval| {
                        eval.emits
                            .iter()
                            .find(|candidate| candidate.name == field.name)
                            .and_then(|candidate| candidate.raw_type.as_deref())
                    }),
                    field.payload_type.as_deref(),
                ),
                description: field.description.clone(),
                tags: field.tags.clone(),
            });
        }
        return;
    }

    for row in emit_fields {
        let field = &row.field;
        // The row's session-resolved payload source is the authority; the
        // flat evaluated lane contributes ONLY expansion metadata.
        let payload_expansion = evaluated.and_then(|eval| {
            eval.emits
                .iter()
                .find(|f| f.name == field.name)
                .map(field_expansion_metadata)
        });

        out.push(EventAnalysis {
            name: field.name.clone(),
            payload: row.payload_source.clone(),
            payload_expansion,
            raw_signature: event_raw_signature_from_evaluated_and_source(
                evaluated.and_then(|eval| {
                    eval.emits
                        .iter()
                        .find(|candidate| candidate.name == field.name)
                        .and_then(|candidate| candidate.raw_type.as_deref())
                }),
                field.payload_type.as_deref(),
            ),
            description: field.description.clone(),
            tags: field.tags.clone(),
        });
    }
}

// ── Slots ──────────────────────────────────────────────────────────────────

fn extract_slots_from_macro(
    macro_index: usize,
    slot_fields: &[crate::analysis::types::AnalyzedSlotField],
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

    for field in slot_fields {
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

        let return_source = authored_payload_source(field.payload.as_ref());
        let return_source_scope = field.return_expr_scope.clone();
        out.push(SlotAnalysis {
            name: field.name.clone(),
            is_scoped: !bindings.is_empty(),
            bindings,
            is_required,
            return_type: field.return_type.clone(),
            return_source,
            return_source_scope,
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
            return_source: None,
            return_source_scope: None,
            description: None,
            tags: Vec::new(),
            // No authored/resolved field declared this evaluated-only name.
            declared_in_macro_type_arg: false,
        });
    }
}

#[derive(Clone)]
struct ExpandedEventEntry {
    name: String,
    payload: SourcePosition,
    payload_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
}

#[derive(Clone)]
struct ExpandedSlotEntry {
    name: String,
    bindings: Vec<SlotBindingAnalysis>,
    is_required: bool,
}

fn expanded_define_emit_events(
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    macro_index: usize,
) -> Vec<ExpandedEventEntry> {
    use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact};

    let Some(entry) = expanded_define_emits_shape(evaluated, macro_index) else {
        return Vec::new();
    };

    let mut seen = rustc_hash::FxHashSet::default();
    let mut events = Vec::new();

    for prop in &entry.result.value.properties {
        if !seen.insert(prop.name.clone()) {
            continue;
        }
        events.push(ExpandedEventEntry {
            name: prop.name.clone(),
            payload: prop.ty.clone(),
            payload_expansion: Some(macro_object_property_expansion_metadata(entry, &prop.name)),
        });
    }

    for sig in &entry.result.value.call_signatures {
        let Some(first) = sig.parameters.first() else {
            continue;
        };
        // Call-signature events: the event NAME is derivable from a CLOSED
        // string-literal first-parameter fact. The realized signature's
        // payload-tuple position is REQUIRED: zero post-event-name params
        // are the PRESENT empty closed tuple; params richer than the closed
        // element vocabulary have no faithful lower-crate CLOSED source, so
        // the position is the typed source-construction FAILURE — never a
        // fabricated unknown success. (This arm covers a shape-carried
        // call-signature fact only; the session-side normalized emit rows
        // publish richer payloads through the projected callable-params
        // replay route and reach `EventAnalysis.payload` via the property
        // rows above.)
        // Call-signature events have no per-field diagnostics. Macro-wide
        // diagnostics are collected separately into `macro_expansion_diagnostics`.
        let payload_expansion = Some(crate::analysis::type_expand::ExpansionMetadata {
            exactness: entry.result.exactness,
            execution_status: entry.result.execution_status,
            diagnostics: vec![],
        });

        if let SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::StringLiteral(name))) =
            &first.ty
        {
            if seen.insert(name.clone()) {
                let payload = if sig.parameters.len() == 1 {
                    SourcePosition::Present(SemanticTypeSource::Closed(ClosedTypeFact::Tuple(
                        verter_type_expr::facts::TuplePayloadFact {
                            readonly: false,
                            elements: std::sync::Arc::from(Vec::new().into_boxed_slice()),
                        },
                    )))
                } else {
                    SourcePosition::Failed(
                        verter_type_expr::facts::SemanticSourceFailure::UnrepresentableRequiredPayload,
                    )
                };
                events.push(ExpandedEventEntry {
                    name: name.clone(),
                    payload,
                    payload_expansion: payload_expansion.clone(),
                });
            }
        }
    }

    events
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
            let binding_name = match &field.r#type {
                SourcePosition::Present(SemanticTypeSource::SyntheticSlotBinding(key))
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
                type_source: field.r#type.clone(),
                type_expansion: Some(type_expansion),
                raw_type: field.raw_type.clone(),
                raw_type_source: None,
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
                type_source: authored_payload_position(source_binding.payload.as_ref()),
                type_expansion: None,
                raw_type: source_binding.type_annotation.clone(),
                raw_type_source: authored_payload_source(source_binding.payload.as_ref()),
            });
            continue;
        };
        let mut binding = expanded_remaining.remove(position);
        let raw_type = symbolic_type_from_evaluated_and_source(
            binding.raw_type.as_deref(),
            source_binding.type_annotation.as_deref(),
        );
        // An incomplete per-binding evaluation falls back to the author's own
        // annotation position when one exists; an ABSENT evaluated position
        // adopts the authored annotation position.
        binding.type_source = if binding.type_source.is_present() {
            prefer_authored_on_incomplete(
                &binding.type_source,
                source_binding.payload.as_ref(),
                binding.type_expansion.as_ref(),
            )
        } else if matches!(binding.type_source, SourcePosition::Absent(_)) {
            authored_payload_position(source_binding.payload.as_ref())
        } else {
            // A typed FAILURE position is preserved — the authored fallback
            // must not paper over a failed required position unless the
            // author actually annotated it.
            authored_payload_source(source_binding.payload.as_ref())
                .map(SourcePosition::Present)
                .unwrap_or_else(|| binding.type_source.clone())
        };
        binding.raw_type = raw_type;
        binding.raw_type_source = raw_type_source_from_source_annotation(
            binding.raw_type.as_deref(),
            source_binding.type_annotation.as_deref(),
            source_binding.payload.as_ref(),
        );
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
            .raw_type
            .as_deref()
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
                return_source: None,
                return_source_scope: None,
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
        .map(|row| row.type_source.clone())
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
    let prop_type_source = source_prop
        .map(|row| row.type_source.clone())
        .unwrap_or_else(SourcePosition::unannotated);

    if let Some(existing_prop) = props.iter_mut().find(|prop| prop.name == name) {
        existing_prop.type_source = prop_type_source.clone();
        existing_prop.type_expansion = existing_prop.type_expansion.clone().or_else(|| {
            evaluated.and_then(|eval| {
                eval.props
                    .iter()
                    .find(|field| field.name == name)
                    .map(field_expansion_metadata)
            })
        });
        if existing_prop.raw_type.is_none() {
            existing_prop.raw_type = raw_type.clone();
        }
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
            type_source: prop_type_source.clone(),
            type_expansion: evaluated.and_then(|eval| {
                eval.props
                    .iter()
                    .find(|field| field.name == name)
                    .map(field_expansion_metadata)
            }),
            raw_type: raw_type.clone(),
            raw_type_source: None,
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
    let raw_signature = event_raw_signature_from_evaluated_and_source(
        evaluated_event.and_then(|field| field.raw_type.as_deref()),
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
        if payload.is_present() || !existing_event.payload.is_present() {
            existing_event.payload = payload;
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
            payload,
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
        let type_source = resolve_exposed_type(&field.name, bindings, evaluated)
            .map(SourcePosition::Present)
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
            type_expansion: evaluated
                .and_then(|eval| {
                    eval.bindings
                        .iter()
                        .find(|binding| binding.name == field.name)
                        .map(field_expansion_metadata)
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
            type_expansion: evaluated
                .and_then(|eval| {
                    eval.bindings
                        .iter()
                        .find(|binding| binding.name == candidate.field.name)
                        .map(field_expansion_metadata)
                })
                .or_else(|| {
                    exposed_lane_field(evaluated, macro_index, &candidate.field.name)
                        .map(field_expansion_metadata)
                }),
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
    name: &str,
    bindings: &[AnalyzedBinding],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
) -> Option<SemanticTypeSource> {
    if let Some(eval) = evaluated {
        if let Some(f) = eval.bindings.iter().find(|f| f.name == name) {
            return f.r#type.present().cloned();
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
        .map(|field| ResolvedPropInput {
            type_source: authored_payload_position(field.payload.as_ref()),
            field: field.clone(),
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
                existing.type_source = prop.type_source.clone();
            } else {
                rows.push(prop.clone());
            }
        }
    }
    rows
}

fn merge_prop_field(target: &mut AnalyzedPropField, candidate: &AnalyzedPropField) {
    if should_replace_merged_prop_type_annotation(
        target.type_annotation.as_deref(),
        candidate.type_annotation.as_deref(),
    ) {
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

fn should_replace_merged_prop_type_annotation(
    current: Option<&str>,
    candidate: Option<&str>,
) -> bool {
    let Some(candidate) = candidate.map(str::trim).filter(|text| !text.is_empty()) else {
        return false;
    };
    let Some(current) = current.map(str::trim).filter(|text| !text.is_empty()) else {
        return true;
    };

    prop_raw_type_is_placeholder(current)
        || (backend_raw_type_is_suspicious_identifier(current)
            && !backend_raw_type_is_suspicious_identifier(candidate))
        || (source_annotation_contains_indexed_access(candidate)
            && !source_annotation_contains_indexed_access(current))
}

/// Merge the analyzer's parse-domain emit fields with the host-resolved
/// rows into ONE per-macro row set, each carrying its payload SOURCE. An
/// analyzer-only field's source is its authored payload position; a
/// host-resolved row carries the normalized `payload_source` authority.
/// First-writer-wins by event name (analyzer rows first — a local authored
/// event keeps its authored position).
fn merged_emit_fields(
    mac: &AnalyzedMacro,
    resolved: Option<&ResolvedMacroInput>,
) -> Vec<ResolvedEmitInput> {
    let mut rows: Vec<ResolvedEmitInput> = mac
        .emit_fields
        .iter()
        .map(|field| ResolvedEmitInput {
            payload_source: authored_payload_position(field.payload.as_ref()),
            field: field.clone(),
        })
        .collect();
    if let Some(resolved) = resolved {
        for emit in &resolved.emits {
            if let Some(existing) = rows
                .iter_mut()
                .find(|row| row.field.name == emit.field.name)
            {
                // The normalized-surface payload source is authoritative for
                // the merged row (it already prefers the PROVEN authored
                // position when one denotes the resolved member); the
                // analyzer row keeps its local analysis metadata.
                existing.payload_source = emit.payload_source.clone();
            } else {
                rows.push(emit.clone());
            }
        }
    }
    rows
}

fn merged_slot_fields(
    mac: &AnalyzedMacro,
    resolved: Option<&ResolvedMacroInput>,
) -> Vec<crate::analysis::types::AnalyzedSlotField> {
    let mut fields = mac.slot_fields.clone();
    if let Some(resolved) = resolved {
        if fields.is_empty() {
            fields = resolved.slots.clone();
        } else {
            let mut seen_slots: rustc_hash::FxHashSet<String> =
                fields.iter().map(|field| field.name.clone()).collect();
            for field in &mut fields {
                let Some(resolved_slot) =
                    resolved.slots.iter().find(|slot| slot.name == field.name)
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
            }

            for resolved_slot in &resolved.slots {
                if seen_slots.insert(resolved_slot.name.clone()) {
                    fields.push(resolved_slot.clone());
                }
            }
        }
    }
    fields
}

fn extract_props_from_options(opts: &AnalyzedOptionsApi, out: &mut Vec<PropAnalysis>) {
    for prop in &opts.props {
        let raw_type = prop
            .type_annotation
            .clone()
            .or_else(|| prop.type_constructor.clone());
        // Prefer the authored `PropType<T>` payload position when available;
        // else fold the primitive runtime constructor to a closed leaf fact
        // (non-primitive constructors carry display text only — `raw_type`).
        let type_source = authored_payload_source(prop.payload.as_ref()).or_else(|| {
            prop.type_constructor.as_ref().and_then(|rt| {
                use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact};
                use verter_type_expr::PrimitiveName;
                let primitive = match rt.as_str() {
                    "String" => PrimitiveName::String,
                    "Number" => PrimitiveName::Number,
                    "Boolean" => PrimitiveName::Boolean,
                    _ => return None,
                };
                Some(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(
                    LeafTypeFact::Primitive(primitive),
                )))
            })
        });
        out.push(PropAnalysis {
            name: prop.name.clone(),
            type_source: present_or_unannotated(type_source),
            type_expansion: None,
            raw_type,
            // Options API path: no authored companion for the raw display
            // text (the typed source is above).
            raw_type_source: None,
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

fn extract_bindings(
    bindings: &[AnalyzedBinding],
    template: Option<&crate::analysis::template::TemplateAnalysisSnapshot>,
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

    bindings
        .iter()
        .map(|binding| BindingAnalysis {
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
            reactivity_kind: binding.reactivity_kind,
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
