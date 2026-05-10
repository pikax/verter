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
    AnalysisFlags, AnalyzedBinding, AnalyzedEmitField, AnalyzedImport, AnalyzedMacro,
    AnalyzedMacroKind, AnalyzedOptionsApi, AnalyzedPropField, AnalyzedSlotField, ImportBindingKind,
    JsdocTag, StoreUsage, VueApiCallSite,
};
use verter_type_expr::{PrimitiveName, TypeExpr};

/// Convenience: build `TypeExpr::Unknown { raw }` from a string.
fn unknown_type(raw: impl Into<String>) -> TypeExpr {
    TypeExpr::Unknown { raw: raw.into() }
}

/// Convert a typed expression option into a concrete `TypeExpr`, falling back
/// to a raw-preserving `Unknown` sentinel when no typed form is available.
///
/// Producers (analyzer in `macros.rs`, cross-file external resolver in
/// `verter_parser`) populate `*_expr` whenever they have an OXC `TSType<'_>`
/// in scope. Consumers therefore prefer the typed form and only fall back to
/// `Unknown { raw }` when the producer could not lower (e.g., raw text from
/// an Options API path whose lowering was not part of W0.1 schema work).
fn typed_or_unknown(typed: Option<&TypeExpr>, raw: &str) -> TypeExpr {
    match typed {
        Some(expr) => expr.clone(),
        None => unknown_type(raw.to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Input view
// ═══════════════════════════════════════════════════════════════════════════

/// Input view for component-meta extraction.
///
/// All fields reference existing `verter_semantic::analysis` types.
/// The host constructs this by projecting from its internal snapshot.
#[derive(Debug, Clone, Default)]
pub struct ResolvedMacroInput {
    /// Index of the macro in `ComponentMetaInput.macros`.
    pub macro_index: usize,
    /// Host-resolved props for the macro.
    pub props: Vec<AnalyzedPropField>,
    /// Host-resolved emits for the macro.
    pub emits: Vec<AnalyzedEmitField>,
    /// Host-resolved slots for the macro.
    pub slots: Vec<AnalyzedSlotField>,
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
    /// Source text of `file_path` used as a name-based JSDoc fallback for
    /// expanded-only props (those produced by type expansion that do not
    /// appear in `prop_fields` or any `resolved_macros` projection). The
    /// extractor stays host-independent: it never reads files itself.
    pub canonical_source: Option<&'a str>,
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
    /// Resolved via priority chain: evaluated TypeExpr > raw annotation > Unknown.
    pub type_expr: TypeExpr,
    /// Completeness and diagnostics from native expansion when available.
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    /// Original annotation text from the source.
    pub raw_type: Option<String>,
    pub required: bool,
    pub has_default: bool,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<JsdocTag>,
}

/// Analyzed event from `defineEmits`.
#[derive(Debug, Clone)]
pub struct EventAnalysis {
    pub name: String,
    pub payload: TypeExpr,
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
    pub description: Option<String>,
    pub tags: Vec<JsdocTag>,
}

/// A single binding property on a scoped slot.
#[derive(Debug, Clone)]
pub struct SlotBindingAnalysis {
    pub name: String,
    pub type_expr: TypeExpr,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    pub raw_type: Option<String>,
}

/// Analyzed model from `defineModel`.
#[derive(Debug, Clone)]
pub struct ModelAnalysis {
    pub name: String,
    pub type_expr: TypeExpr,
}

/// Analyzed exposed member from `defineExpose`.
#[derive(Debug, Clone)]
pub struct ExposedAnalysis {
    pub name: String,
    pub type_expr: TypeExpr,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    pub description: Option<String>,
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
    pub type_expr: TypeExpr,
    pub type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
    pub raw_type: Option<String>,
    pub description: Option<String>,
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
    pub type_expr: TypeExpr,
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
    /// D123 (Tier 1A) — set to `true` when lowering produces a
    /// `verter_session::owned_artifacts::eval_program::LoweringError`.
    /// Consumers detect macro-impacting failures via this flag plus
    /// the structured `macro_expansion_diagnostics` entry. Per D117,
    /// the public `getComponentMeta` API still returns
    /// `Option<ComponentMetaPayload>` (NOT `Result`); macro-impacting
    /// lowering failures surface as a populated payload with this
    /// flag set, so NAPI does not throw exceptions.
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
    pub type_expr: TypeExpr,
    pub raw_type: Option<String>,
    pub required: bool,
    pub provenance: MemberProvenance,
    pub availability: MemberAvailability,
    pub kind: AcceptedPropKind,
}

/// An accepted event on the computed call-site surface.
#[derive(Debug, Clone)]
pub struct AcceptedEventAnalysis {
    pub name: String,
    pub payload: TypeExpr,
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
    pub type_expr: TypeExpr,
    pub raw_type: Option<String>,
    pub sources: Vec<InheritedSource>,
}

/// An inherited event entry in a fallthrough branch.
#[derive(Debug, Clone, PartialEq)]
pub struct FallthroughEventEntry {
    pub name: String,
    pub payload: TypeExpr,
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
                    input.canonical_source,
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
                extract_model_from_macro(mac, &prop_fields, evaluated_types, &mut models);
            }
            AnalyzedMacroKind::DefineExpose => {
                extract_exposed_from_macro(mac, input.bindings, evaluated_types, &mut exposed);
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
    prop_fields: &[AnalyzedPropField],
    default_keys: &std::collections::HashSet<&str>,
    default_values: &std::collections::HashMap<&str, &str>,
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    canonical_source: Option<&str>,
    out: &mut Vec<PropAnalysis>,
) {
    if let Some(eval_fields) = expanded_define_props_fields(evaluated, macro_index) {
        if !eval_fields.is_empty() {
            for field in eval_fields {
                let source_field = prop_fields.iter().find(|prop| prop.name == field.name);
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
                let type_expr = prefer_symbolic_prop_type_expr(
                    &field.ty,
                    source_field.and_then(|prop| prop.type_expr.as_ref()),
                    raw_type.as_deref(),
                    type_expansion.as_ref(),
                );

                let (description, tags) =
                    jsdoc_for_expanded_prop(source_field, canonical_source, field.name.as_str());

                out.push(PropAnalysis {
                    name: field.name.clone(),
                    type_expr,
                    type_expansion,
                    raw_type,
                    required: !field.optional && !has_default,
                    has_default,
                    default_value,
                    description,
                    tags,
                });
            }
            // NOTE: We intentionally do NOT fall back to prop_fields here.
            // When the evaluator runs and produces results, it is authoritative —
            // utility types like Pick/Omit may have intentionally excluded some
            // prop_fields entries. Adding them back would break filtering.
            return;
        }
    }

    for field in prop_fields {
        let (type_expr, type_expansion) = resolve_prop_type(field, evaluated);
        let has_default = default_keys.contains(field.name.as_str());
        let default_value = default_values
            .get(field.name.as_str())
            .map(|v| v.to_string());

        out.push(PropAnalysis {
            name: field.name.clone(),
            type_expr,
            type_expansion,
            raw_type: prop_raw_type_from_evaluated_and_source(
                evaluated.and_then(|eval| {
                    eval.props
                        .iter()
                        .find(|candidate| candidate.name == field.name)
                        .and_then(|candidate| candidate.raw_type.as_deref())
                }),
                field.type_annotation.as_deref(),
                field.is_optional,
            ),
            required: !field.is_optional && !has_default,
            has_default,
            default_value,
            description: field.description.clone(),
            tags: field.tags.clone(),
        });
    }

    if let Some(eval_fields) = expanded_define_props_fields(evaluated, macro_index) {
        let mut seen: std::collections::HashSet<String> =
            prop_fields.iter().map(|field| field.name.clone()).collect();
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
            // no `*_expr` symbolic fallback is available. The heuristic still
            // runs; when it prefers symbolic, the `evaluated_type` is kept.
            let type_expr = prefer_symbolic_prop_type_expr(
                &field.ty,
                None,
                raw_type.as_deref(),
                type_expansion.as_ref(),
            );

            let (description, tags) =
                jsdoc_for_expanded_prop(None, canonical_source, field.name.as_str());

            out.push(PropAnalysis {
                name: field.name.clone(),
                type_expr,
                type_expansion,
                raw_type,
                required: !field.optional && !has_default,
                has_default,
                default_value,
                description,
                tags,
            });
        }
    }
}

/// JSDoc lookup for an expanded prop. Uses the source field's JSDoc when the
/// expansion mirrors a declared prop, otherwise falls back to a name-based
/// lookup in the canonical source. When neither is available, returns empty.
fn jsdoc_for_expanded_prop(
    source_field: Option<&AnalyzedPropField>,
    canonical_source: Option<&str>,
    prop_name: &str,
) -> (Option<String>, Vec<JsdocTag>) {
    if let Some(field) = source_field {
        if field.description.is_some() || !field.tags.is_empty() {
            return (field.description.clone(), field.tags.clone());
        }
    }
    if let Some(source) = canonical_source {
        return crate::analysis::jsdoc::extract_jsdoc_for_property_name(source, prop_name);
    }
    (None, Vec::new())
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
/// 1. Evaluated TypeExpr (preferred)
/// 2. Raw annotation text → TypeExpr::Unknown
fn merged_resolved_macro_input(
    resolved_macros: &[ResolvedMacroInput],
    macro_index: usize,
) -> Option<ResolvedMacroInput> {
    let mut merged: Option<ResolvedMacroInput> = None;
    let mut seen_props = rustc_hash::FxHashSet::default();
    let mut seen_emits = rustc_hash::FxHashSet::default();
    let mut seen_slots = rustc_hash::FxHashSet::default();

    for resolved in resolved_macros
        .iter()
        .filter(|resolved| resolved.macro_index == macro_index)
    {
        let entry = merged.get_or_insert_with(|| ResolvedMacroInput {
            macro_index,
            props: Vec::new(),
            emits: Vec::new(),
            slots: Vec::new(),
        });

        for prop in &resolved.props {
            if let Some(existing) = entry.props.iter_mut().find(|field| field.name == prop.name) {
                merge_prop_field(existing, prop);
            } else if seen_props.insert(prop.name.clone()) {
                entry.props.push(prop.clone());
            }
        }
        for emit in &resolved.emits {
            if seen_emits.insert(emit.name.clone()) {
                entry.emits.push(emit.clone());
            }
        }
        for slot in &resolved.slots {
            if seen_slots.insert(slot.name.clone()) {
                entry.slots.push(slot.clone());
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
    field: &AnalyzedPropField,
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
) -> (
    TypeExpr,
    Option<crate::analysis::type_expand::ExpansionMetadata>,
) {
    if let Some(eval) = evaluated {
        if let Some(ef) = eval.props.iter().find(|f| f.name == field.name) {
            let metadata = field_expansion_metadata(ef);
            let preferred_raw_type = symbolic_type_from_evaluated_and_source(
                ef.raw_type.as_deref(),
                field.type_annotation.as_deref(),
            );
            let type_expr = prefer_symbolic_prop_type_expr(
                &ef.r#type,
                field.type_expr.as_ref(),
                preferred_raw_type.as_deref(),
                Some(&metadata),
            );
            return (type_expr, Some(metadata));
        }
    }
    let raw = field.type_annotation.as_deref().unwrap_or("unknown");
    (typed_or_unknown(field.type_expr.as_ref(), raw), None)
}

// ── Events ─────────────────────────────────────────────────────────────────

fn prop_raw_type_from_evaluated_and_source(
    evaluated_raw_type: Option<&str>,
    source_annotation: Option<&str>,
    _is_optional: bool,
) -> Option<String> {
    symbolic_type_from_evaluated_and_source(evaluated_raw_type, source_annotation)
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

const LARGE_PARTIAL_PROP_TYPE_NODE_LIMIT: usize = 256;

/// Choose between the evaluator-produced `evaluated_type` and a symbolic
/// fallback supplied by the caller.
///
/// The decision (`should_prefer_symbolic_prop_type_expr`) reads display
/// strings (`preferred_raw_type`) for shape heuristics — that is purely a
/// classification of incomplete / placeholder evaluator output. The returned
/// `TypeExpr` itself comes from typed inputs only:
///
/// - When the heuristic falls through, the `evaluated_type` clone is
///   returned unchanged.
/// - When the heuristic prefers the symbolic alternative, `symbolic_type`
///   (the caller's pre-lowered typed form, populated by the analyzer at the
///   producer site) is returned. If the caller has no typed form available,
///   the function falls back to `evaluated_type` so no string parsing is
///   reintroduced.
fn prefer_symbolic_prop_type_expr(
    evaluated_type: &TypeExpr,
    symbolic_type: Option<&TypeExpr>,
    preferred_raw_type: Option<&str>,
    metadata: Option<&crate::analysis::type_expand::ExpansionMetadata>,
) -> TypeExpr {
    if !should_prefer_symbolic_prop_type_expr(evaluated_type, preferred_raw_type, metadata) {
        return evaluated_type.clone();
    }

    symbolic_type
        .cloned()
        .unwrap_or_else(|| evaluated_type.clone())
}

fn should_prefer_symbolic_prop_type_expr(
    evaluated_type: &TypeExpr,
    preferred_raw_type: Option<&str>,
    metadata: Option<&crate::analysis::type_expand::ExpansionMetadata>,
) -> bool {
    let Some(raw_type) = preferred_raw_type
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return false;
    };

    let Some(metadata) = metadata else {
        return false;
    };

    let raw_type_supports_symbolic_fallback =
        source_annotation_beats_placeholder_backend_type(raw_type)
            || (matches!(raw_type, "any" | "unknown")
                && type_expr_is_suspicious_identifier_ref(evaluated_type));

    metadata.exactness == crate::analysis::type_expand::ExpansionExactness::Incomplete
        && raw_type_supports_symbolic_fallback
        && (type_expr_exceeds_node_limit(evaluated_type, LARGE_PARTIAL_PROP_TYPE_NODE_LIMIT)
            || type_expr_is_placeholder_for_symbolic_fallback(evaluated_type))
}

fn type_expr_exceeds_node_limit(type_expr: &TypeExpr, limit: usize) -> bool {
    fn visit(type_expr: &TypeExpr, seen: &mut usize, limit: usize) -> bool {
        *seen += 1;
        if *seen > limit {
            return true;
        }

        match type_expr {
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::TypeOf(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::TypeParameter(_) => false,
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                types.iter().any(|inner| visit(inner, seen, limit))
            }
            TypeExpr::Array { element, .. }
            | TypeExpr::KeyOf(element)
            | TypeExpr::Rest(element)
            | TypeExpr::Parenthesized(element) => visit(element, seen, limit),
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|element| visit(&element.ty, seen, limit)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                verter_type_expr::ObjectMember::Property(property) => {
                    visit(&property.ty, seen, limit)
                }
                verter_type_expr::ObjectMember::IndexSignature(signature) => {
                    visit(&signature.key_type, seen, limit)
                        || visit(&signature.value_type, seen, limit)
                }
                verter_type_expr::ObjectMember::CallSignature(function)
                | verter_type_expr::ObjectMember::ConstructSignature(function) => {
                    type_expr_function_exceeds_node_limit(function, seen, limit)
                }
                verter_type_expr::ObjectMember::Method(method) => {
                    type_expr_function_exceeds_node_limit(&method.function, seen, limit)
                }
            }),
            TypeExpr::Function(function) => {
                type_expr_function_exceeds_node_limit(function, seen, limit)
            }
            TypeExpr::Ref { type_arguments, .. } => {
                type_arguments.iter().any(|inner| visit(inner, seen, limit))
            }
            TypeExpr::IndexedAccess { object, index } => {
                visit(object, seen, limit) || visit(index, seen, limit)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                visit(check, seen, limit)
                    || visit(extends, seen, limit)
                    || visit(true_type, seen, limit)
                    || visit(false_type, seen, limit)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                visit(source, seen, limit)
                    || visit(value, seen, limit)
                    || name_type
                        .as_deref()
                        .is_some_and(|inner| visit(inner, seen, limit))
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                expressions.iter().any(|inner| visit(inner, seen, limit))
            }
        }
    }

    fn type_expr_function_exceeds_node_limit(
        function: &verter_type_expr::FunctionExpr,
        seen: &mut usize,
        limit: usize,
    ) -> bool {
        function
            .parameters
            .iter()
            .any(|param| visit(&param.ty, seen, limit))
            || function
                .return_type
                .as_deref()
                .is_some_and(|return_type| visit(return_type, seen, limit))
            || function.type_parameters.iter().any(|type_param| {
                type_param
                    .constraint
                    .as_deref()
                    .is_some_and(|constraint| visit(constraint, seen, limit))
                    || type_param
                        .default
                        .as_deref()
                        .is_some_and(|default| visit(default, seen, limit))
            })
    }

    let mut seen = 0;
    visit(type_expr, &mut seen, limit)
}

fn type_expr_is_placeholder_for_symbolic_fallback(type_expr: &TypeExpr) -> bool {
    match type_expr {
        TypeExpr::Primitive(
            PrimitiveName::Any | PrimitiveName::Unknown | PrimitiveName::Undefined,
        ) => true,
        TypeExpr::Unknown { .. } => true,
        TypeExpr::Ref { .. }
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. } => true,
        TypeExpr::Parenthesized(inner) => type_expr_is_placeholder_for_symbolic_fallback(inner),
        TypeExpr::Object(object) => {
            object.properties.is_empty()
                || object.properties.iter().all(|member| match member {
                    verter_type_expr::ObjectMember::Property(property) => {
                        type_expr_is_placeholder_for_symbolic_fallback(&property.ty)
                    }
                    verter_type_expr::ObjectMember::IndexSignature(signature) => {
                        type_expr_is_placeholder_for_symbolic_fallback(&signature.value_type)
                    }
                    verter_type_expr::ObjectMember::CallSignature(_)
                    | verter_type_expr::ObjectMember::ConstructSignature(_)
                    | verter_type_expr::ObjectMember::Method(_) => false,
                })
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            !types.is_empty()
                && types
                    .iter()
                    .all(type_expr_is_placeholder_for_symbolic_fallback)
        }
        _ => false,
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

fn type_expr_is_suspicious_identifier_ref(type_expr: &TypeExpr) -> bool {
    match type_expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => type_arguments.is_empty() && backend_raw_type_is_suspicious_identifier(name),
        TypeExpr::Parenthesized(inner) => type_expr_is_suspicious_identifier_ref(inner),
        _ => false,
    }
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

fn event_payload_raw_signature_from_type_expr(payload: &TypeExpr) -> Option<String> {
    use verter_type_expr::LiteralValue;

    fn render_type_expr(expr: &TypeExpr) -> Option<String> {
        match expr {
            TypeExpr::Primitive(name) => Some(match name {
                verter_type_expr::PrimitiveName::String => "string".to_string(),
                verter_type_expr::PrimitiveName::Number => "number".to_string(),
                verter_type_expr::PrimitiveName::Boolean => "boolean".to_string(),
                verter_type_expr::PrimitiveName::BigInt => "bigint".to_string(),
                verter_type_expr::PrimitiveName::Symbol => "symbol".to_string(),
                verter_type_expr::PrimitiveName::Null => "null".to_string(),
                verter_type_expr::PrimitiveName::Undefined => "undefined".to_string(),
                verter_type_expr::PrimitiveName::Void => "void".to_string(),
                verter_type_expr::PrimitiveName::Any => "any".to_string(),
                verter_type_expr::PrimitiveName::Unknown => "unknown".to_string(),
                verter_type_expr::PrimitiveName::Never => "never".to_string(),
                verter_type_expr::PrimitiveName::Object => "object".to_string(),
            }),
            TypeExpr::Literal(LiteralValue::String(value)) => Some(format!("{value:?}")),
            TypeExpr::Literal(LiteralValue::Number(value)) => Some(value.to_string()),
            TypeExpr::Literal(LiteralValue::Boolean(value)) => Some(value.to_string()),
            TypeExpr::Literal(LiteralValue::BigInt(value)) => Some(value.clone()),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if type_arguments.is_empty() {
                    Some(name.to_string())
                } else {
                    let args = type_arguments
                        .iter()
                        .map(render_type_expr)
                        .collect::<Option<Vec<_>>>()?;
                    Some(format!("{}<{}>", name, args.join(", ")))
                }
            }
            TypeExpr::Union(types) => Some(
                types
                    .iter()
                    .map(render_type_expr)
                    .collect::<Option<Vec<_>>>()?
                    .join(" | "),
            ),
            TypeExpr::Intersection(types) => Some(
                types
                    .iter()
                    .map(render_type_expr)
                    .collect::<Option<Vec<_>>>()?
                    .join(" & "),
            ),
            TypeExpr::Tuple { elements, .. } => Some(format!(
                "[{}]",
                elements
                    .iter()
                    .map(|element| {
                        let mut rendered = String::new();
                        if let Some(label) = &element.label {
                            rendered.push_str(label);
                            if element.optional {
                                rendered.push('?');
                            }
                            rendered.push_str(": ");
                        }
                        if element.rest {
                            rendered.push_str("...");
                        }
                        rendered.push_str(&render_type_expr(&element.ty)?);
                        Some(rendered)
                    })
                    .collect::<Option<Vec<_>>>()?
                    .join(", ")
            )),
            _ => None,
        }
    }

    render_type_expr(payload).filter(|rendered| rendered.starts_with('['))
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
    emit_fields: &[crate::analysis::types::AnalyzedEmitField],
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
                let expanded_raw_signature =
                    event_payload_raw_signature_from_type_expr(&event.payload);
                out.push(EventAnalysis {
                    name: event.name,
                    payload: event.payload,
                    payload_expansion: event.payload_expansion,
                    raw_signature: event_raw_signature_from_evaluated_and_source(
                        evaluated_field.and_then(|candidate| candidate.raw_type.as_deref()),
                        expanded_raw_signature.as_deref(),
                    ),
                    description: None,
                    tags: Vec::new(),
                });
            }
            return;
        }

        for field in emit_fields {
            let (payload, payload_expansion) = expanded_by_name
                .get(&field.name)
                .map(|event| (event.payload.clone(), event.payload_expansion.clone()))
                .or_else(|| {
                    evaluated.and_then(|eval| {
                        eval.emits
                            .iter()
                            .find(|f| f.name == field.name)
                            .map(|f| (f.r#type.clone(), Some(field_expansion_metadata(f))))
                    })
                })
                .unwrap_or_else(|| {
                    let raw = field.payload_type.as_deref().unwrap_or("unknown");
                    (typed_or_unknown(field.payload_expr.as_ref(), raw), None)
                });
            let expanded_raw_signature = event_payload_raw_signature_from_type_expr(&payload);

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
                    expanded_raw_signature
                        .as_deref()
                        .or(field.payload_type.as_deref()),
                ),
                description: field.description.clone(),
                tags: field.tags.clone(),
            });
        }
        return;
    }

    for field in emit_fields {
        let (payload, payload_expansion) = if let Some(eval) = evaluated {
            eval.emits
                .iter()
                .find(|f| f.name == field.name)
                .map(|f| (f.r#type.clone(), Some(field_expansion_metadata(f))))
                .unwrap_or_else(|| {
                    let raw = field.payload_type.as_deref().unwrap_or("unknown");
                    (typed_or_unknown(field.payload_expr.as_ref(), raw), None)
                })
        } else {
            let raw = field.payload_type.as_deref().unwrap_or("unknown");
            (typed_or_unknown(field.payload_expr.as_ref(), raw), None)
        };

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
}

// ── Slots ──────────────────────────────────────────────────────────────────

fn extract_slots_from_macro(
    macro_index: usize,
    slot_fields: &[crate::analysis::types::AnalyzedSlotField],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    out: &mut Vec<SlotAnalysis>,
) {
    let expanded_slots = expanded_define_slot_entries(evaluated, macro_index);
    if !expanded_slots.is_empty() {
        let mut remaining = expanded_slots;

        for field in slot_fields {
            let Some(slot_index) = remaining.iter().position(|slot| slot.name == field.name) else {
                continue;
            };
            let mut slot = remaining.remove(slot_index);
            slot.bindings = merge_slot_bindings_with_source(field, slot.bindings);
            out.push(SlotAnalysis {
                name: slot.name,
                is_scoped: !slot.bindings.is_empty(),
                bindings: slot.bindings,
                is_required: slot.is_required,
                return_type: field.return_type.clone(),
                description: field.description.clone(),
                tags: field.tags.clone(),
            });
        }

        for slot in remaining {
            let source_field = slot_fields.iter().find(|field| field.name == slot.name);
            out.push(SlotAnalysis {
                name: slot.name,
                is_scoped: !slot.bindings.is_empty(),
                bindings: slot.bindings,
                is_required: slot.is_required,
                return_type: source_field.and_then(|field| field.return_type.clone()),
                description: source_field.and_then(|field| field.description.clone()),
                tags: source_field
                    .map(|field| field.tags.clone())
                    .unwrap_or_default(),
            });
        }
        return;
    }

    for field in slot_fields {
        let bindings: Vec<SlotBindingAnalysis> = field
            .bindings
            .iter()
            .map(|b| {
                let (type_expr, type_expansion) = if let Some(eval) = evaluated {
                    // Slot bindings are keyed as "slotName.bindingName" in ExpandedComponentTypes
                    let key = format!("{}.{}", field.name, b.name);
                    eval.slot_bindings
                        .iter()
                        .find(|f| f.name == key)
                        .map(|f| {
                            let type_expansion = field_expansion_metadata(f);
                            let raw_type = symbolic_type_from_evaluated_and_source(
                                f.raw_type.as_deref(),
                                b.type_annotation.as_deref(),
                            );
                            (
                                prefer_symbolic_prop_type_expr(
                                    &f.r#type,
                                    b.binding_expr.as_ref(),
                                    raw_type.as_deref(),
                                    Some(&type_expansion),
                                ),
                                Some(type_expansion),
                            )
                        })
                        .unwrap_or_else(|| {
                            let raw = b.type_annotation.as_deref().unwrap_or("unknown");
                            (typed_or_unknown(b.binding_expr.as_ref(), raw), None)
                        })
                } else {
                    let raw = b.type_annotation.as_deref().unwrap_or("unknown");
                    (typed_or_unknown(b.binding_expr.as_ref(), raw), None)
                };
                SlotBindingAnalysis {
                    name: b.name.clone(),
                    type_expr,
                    type_expansion,
                    raw_type: evaluated
                        .and_then(|eval| {
                            let key = format!("{}.{}", field.name, b.name);
                            eval.slot_bindings
                                .iter()
                                .find(|candidate| candidate.name == key)
                                .and_then(|candidate| candidate.raw_type.clone())
                        })
                        .or_else(|| b.type_annotation.clone()),
                }
            })
            .collect();

        out.push(SlotAnalysis {
            name: field.name.clone(),
            is_scoped: !field.bindings.is_empty(),
            bindings,
            is_required: field.is_required,
            return_type: field.return_type.clone(),
            description: field.description.clone(),
            tags: field.tags.clone(),
        });
    }
}

#[derive(Clone)]
struct ExpandedEventEntry {
    name: String,
    payload: TypeExpr,
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
    use verter_type_expr::{LiteralValue, TupleElement, TypeExpr};

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
        let payload = TypeExpr::Tuple {
            elements: std::sync::Arc::from(
                sig.parameters
                    .iter()
                    .skip(1)
                    .map(|param| TupleElement {
                        label: (!param.name.is_empty()).then(|| param.name.clone()),
                        ty: param.ty.clone(),
                        optional: param.optional,
                        rest: param.rest,
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: false,
        };
        // Call-signature events have no per-field diagnostics. Macro-wide
        // diagnostics are collected separately into `macro_expansion_diagnostics`.
        let payload_expansion = Some(crate::analysis::type_expand::ExpansionMetadata {
            exactness: entry.result.exactness,
            execution_status: entry.result.execution_status,
            diagnostics: vec![],
        });

        match &first.ty {
            TypeExpr::Literal(LiteralValue::String(name)) => {
                if seen.insert(name.clone()) {
                    events.push(ExpandedEventEntry {
                        name: name.clone(),
                        payload: payload.clone(),
                        payload_expansion: payload_expansion.clone(),
                    });
                }
            }
            TypeExpr::Union(types) => {
                for ty in types.iter() {
                    let TypeExpr::Literal(LiteralValue::String(name)) = ty else {
                        continue;
                    };
                    if seen.insert(name.clone()) {
                        events.push(ExpandedEventEntry {
                            name: name.clone(),
                            payload: payload.clone(),
                            payload_expansion: payload_expansion.clone(),
                        });
                    }
                }
            }
            _ => {}
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
            bindings: expanded_slot_bindings(
                evaluated,
                &prop.name,
                &prop.ty,
                Some(macro_object_property_expansion_metadata(entry, &prop.name)),
            ),
            is_required: !prop.optional,
        })
        .collect()
}

fn expanded_slot_bindings(
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    slot_name: &str,
    ty: &TypeExpr,
    type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
) -> Vec<SlotBindingAnalysis> {
    fn evaluated_slot_bindings_for_slot(
        evaluated: &crate::analysis::type_expand::ExpandedComponentTypes,
        slot_name: &str,
    ) -> Vec<SlotBindingAnalysis> {
        evaluated
            .slot_bindings
            .iter()
            .filter(|field| field.name.starts_with(&format!("{slot_name}.")))
            .map(|field| {
                let type_expansion = field_expansion_metadata(field);
                SlotBindingAnalysis {
                    name: field
                        .name
                        .split_once('.')
                        .map(|(_, binding)| binding.to_string())
                        .unwrap_or_else(|| field.name.clone()),
                    // Evaluator-only branch: source `AnalyzedSlotFieldBinding`
                    // is not in scope here, so no `binding_expr` symbolic
                    // fallback is available. The heuristic still runs;
                    // when it would prefer symbolic, the evaluated type is
                    // kept.
                    type_expr: prefer_symbolic_prop_type_expr(
                        &field.r#type,
                        None,
                        field.raw_type.as_deref(),
                        Some(&type_expansion),
                    ),
                    type_expansion: Some(type_expansion),
                    raw_type: field.raw_type.clone(),
                }
            })
            .collect()
    }

    fn should_prefer_evaluated_slot_binding(
        current: &SlotBindingAnalysis,
        evaluated: &SlotBindingAnalysis,
    ) -> bool {
        type_expr_is_placeholder_for_symbolic_fallback(&current.type_expr)
            && !type_expr_is_placeholder_for_symbolic_fallback(&evaluated.type_expr)
    }

    let mut direct_bindings = slot_bindings_from_type_expr(ty, type_expansion.clone());
    let Some(evaluated) = evaluated else {
        return direct_bindings;
    };

    let evaluated_bindings = evaluated_slot_bindings_for_slot(evaluated, slot_name);
    if direct_bindings.is_empty() {
        return evaluated_bindings;
    }

    let mut evaluated_by_name: rustc_hash::FxHashMap<String, SlotBindingAnalysis> =
        evaluated_bindings
            .into_iter()
            .map(|binding| (binding.name.clone(), binding))
            .collect();
    for binding in &mut direct_bindings {
        let Some(evaluated) = evaluated_by_name.remove(&binding.name) else {
            continue;
        };
        if should_prefer_evaluated_slot_binding(binding, &evaluated) {
            binding.type_expr = evaluated.type_expr;
            binding.type_expansion = evaluated.type_expansion;
        }
        if binding.raw_type.is_none() {
            binding.raw_type = evaluated.raw_type;
        }
    }
    direct_bindings.extend(evaluated_by_name.into_values());
    direct_bindings
}

fn merge_slot_bindings_with_source(
    source_field: &crate::analysis::types::AnalyzedSlotField,
    expanded_bindings: Vec<SlotBindingAnalysis>,
) -> Vec<SlotBindingAnalysis> {
    if source_field.bindings.is_empty() {
        return expanded_bindings;
    }
    if expanded_bindings.is_empty() {
        return source_field
            .bindings
            .iter()
            .map(|binding| {
                let raw = binding.type_annotation.as_deref().unwrap_or("unknown");
                SlotBindingAnalysis {
                    name: binding.name.clone(),
                    type_expr: typed_or_unknown(binding.binding_expr.as_ref(), raw),
                    type_expansion: None,
                    raw_type: binding.type_annotation.clone(),
                }
            })
            .collect();
    }

    let mut expanded_by_name: rustc_hash::FxHashMap<String, SlotBindingAnalysis> =
        expanded_bindings
            .into_iter()
            .map(|binding| (binding.name.clone(), binding))
            .collect();
    let mut merged = Vec::new();

    for source_binding in &source_field.bindings {
        if let Some(mut binding) = expanded_by_name.remove(&source_binding.name) {
            let raw_type = symbolic_type_from_evaluated_and_source(
                binding.raw_type.as_deref(),
                source_binding.type_annotation.as_deref(),
            );
            binding.type_expr = prefer_symbolic_prop_type_expr(
                &binding.type_expr,
                source_binding.binding_expr.as_ref(),
                raw_type.as_deref(),
                binding.type_expansion.as_ref(),
            );
            binding.raw_type = raw_type;
            merged.push(binding);
        }
    }

    merged.extend(expanded_by_name.into_values());
    merged
}

fn slot_bindings_from_type_expr(
    ty: &TypeExpr,
    type_expansion: Option<crate::analysis::type_expand::ExpansionMetadata>,
) -> Vec<SlotBindingAnalysis> {
    let mut binding_param_types = Vec::new();
    collect_slot_binding_param_types(ty, &mut binding_param_types);
    if binding_param_types.is_empty() {
        return Vec::new();
    }

    let mut seen = rustc_hash::FxHashSet::default();
    let mut bindings = Vec::new();
    for binding_param_ty in binding_param_types {
        collect_slot_bindings_from_object_type(
            binding_param_ty,
            &type_expansion,
            &mut seen,
            &mut bindings,
        );
    }
    bindings
}

fn collect_slot_binding_param_types<'a>(ty: &'a TypeExpr, out: &mut Vec<&'a TypeExpr>) {
    match ty {
        TypeExpr::Parenthesized(inner) => collect_slot_binding_param_types(inner, out),
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
            for inner in types.iter() {
                collect_slot_binding_param_types(inner, out);
            }
        }
        TypeExpr::Function(func) => {
            if let Some(first) = func.parameters.first() {
                out.push(&first.ty);
            }
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    verter_type_expr::ObjectMember::CallSignature(function)
                    | verter_type_expr::ObjectMember::ConstructSignature(function) => {
                        if let Some(first) = function.parameters.first() {
                            out.push(&first.ty);
                        }
                    }
                    verter_type_expr::ObjectMember::Method(method) => {
                        if let Some(first) = method.function.parameters.first() {
                            out.push(&first.ty);
                        }
                    }
                    verter_type_expr::ObjectMember::Property(_)
                    | verter_type_expr::ObjectMember::IndexSignature(_) => {}
                }
            }
        }
        _ => {}
    }
}

fn collect_slot_bindings_from_object_type(
    ty: &TypeExpr,
    type_expansion: &Option<crate::analysis::type_expand::ExpansionMetadata>,
    seen: &mut rustc_hash::FxHashSet<String>,
    out: &mut Vec<SlotBindingAnalysis>,
) {
    use verter_type_expr::{ObjectMember, TypeExpr};

    match ty {
        TypeExpr::Parenthesized(inner) => {
            collect_slot_bindings_from_object_type(inner, type_expansion, seen, out);
        }
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
            for inner in types.iter() {
                collect_slot_bindings_from_object_type(inner, type_expansion, seen, out);
            }
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                let ObjectMember::Property(prop) = member else {
                    continue;
                };
                if !seen.insert(prop.name.clone()) {
                    continue;
                }
                out.push(SlotBindingAnalysis {
                    name: prop.name.clone(),
                    type_expr: prop.ty.clone(),
                    type_expansion: type_expansion.clone(),
                    raw_type: None,
                });
            }
        }
        _ => {}
    }
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
            out.push(SlotAnalysis {
                name: tslot.name.clone(),
                is_scoped: tslot.has_bindings,
                bindings: Vec::new(),
                is_required: false,
                return_type: None,
                description: None,
                tags: Vec::new(),
            });
        }
    }
}

// ── Models ─────────────────────────────────────────────────────────────────

fn extract_model_from_macro(
    mac: &AnalyzedMacro,
    prop_fields: &[AnalyzedPropField],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    out: &mut Vec<ModelAnalysis>,
) {
    let name = mac
        .model_name
        .clone()
        .unwrap_or_else(|| "modelValue".to_string());

    // Try evaluated type from props (model generates a prop with the model name)
    let type_expr = if let Some(eval) = evaluated {
        eval.props
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.r#type.clone())
            .unwrap_or_else(|| unknown_type("unknown".to_string()))
    } else {
        // Fall back to prop_fields on the macro itself.
        // Prefer the analyzer-populated `type_expr`; fall back to a
        // raw-preserving Unknown sentinel when the producer could not
        // lower (no OXC TSType node was in scope).
        prop_fields
            .iter()
            .find(|f| f.name == name)
            .map(|f| {
                let raw = f.type_annotation.as_deref().unwrap_or("unknown");
                typed_or_unknown(f.type_expr.as_ref(), raw)
            })
            .unwrap_or_else(|| unknown_type("unknown".to_string()))
    };

    out.push(ModelAnalysis { name, type_expr });
}

// ── Exposed ────────────────────────────────────────────────────────────────

fn synthesize_model_prop_and_event(
    mac: &AnalyzedMacro,
    prop_fields: &[AnalyzedPropField],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    props: &mut Vec<PropAnalysis>,
    events: &mut Vec<EventAnalysis>,
) {
    let name = mac
        .model_name
        .clone()
        .unwrap_or_else(|| "modelValue".to_string());
    let has_default = mac.default_keys.iter().any(|key| key == &name);
    let source_prop = prop_fields.iter().find(|field| field.name == name);
    let raw_type = source_prop.and_then(|field| field.type_annotation.clone());
    let evaluated_prop =
        evaluated.and_then(|eval| eval.props.iter().find(|field| field.name == name));
    let is_optional = source_prop
        .map(|field| field.is_optional)
        .or_else(|| evaluated_prop.map(|field| field.optional))
        .unwrap_or(false);
    let mut prop_type_expr = evaluated_prop
        .map(|field| field.r#type.clone())
        .or_else(|| {
            // Prefer the analyzer-populated typed form. Falls back to a
            // raw-preserving `Unknown` when the producer left `type_expr`
            // unset (e.g., no OXC TSType in scope).
            source_prop.map(|field| {
                let raw = field.type_annotation.as_deref().unwrap_or("unknown");
                typed_or_unknown(field.type_expr.as_ref(), raw)
            })
        })
        .unwrap_or_else(|| unknown_type("unknown".to_string()));

    if has_default || is_optional {
        prop_type_expr = add_top_level_undefined_to_type_expr(prop_type_expr);
    }

    if let Some(existing_prop) = props.iter_mut().find(|prop| prop.name == name) {
        existing_prop.type_expr = prop_type_expr.clone();
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
    } else {
        props.push(PropAnalysis {
            name: name.clone(),
            type_expr: prop_type_expr.clone(),
            type_expansion: evaluated.and_then(|eval| {
                eval.props
                    .iter()
                    .find(|field| field.name == name)
                    .map(field_expansion_metadata)
            }),
            raw_type: raw_type.clone(),
            required: !has_default && !is_optional,
            has_default,
            default_value: None,
            description: None,
            tags: Vec::new(),
        });
    }

    let event_name = format!("update:{name}");
    let evaluated_event =
        evaluated.and_then(|eval| eval.emits.iter().find(|field| field.name == event_name));
    let source_payload_type = raw_type.as_deref().map(|raw| {
        if has_default || is_optional {
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
    let payload = {
        let payload = evaluated_event
            .map(|field| field.r#type.clone())
            .or_else(|| raw_signature.as_ref().map(|raw| unknown_type(raw.clone())))
            .unwrap_or_else(|| unknown_type("unknown".to_string()));
        if has_default || is_optional {
            add_top_level_undefined_to_model_event_payload(payload)
        } else {
            payload
        }
    };

    if let Some(existing_event) = events.iter_mut().find(|event| event.name == event_name) {
        existing_event.payload = payload;
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

fn add_top_level_undefined_to_type_expr(type_expr: TypeExpr) -> TypeExpr {
    if matches!(
        type_expr,
        TypeExpr::Primitive(verter_type_expr::PrimitiveName::Undefined)
    ) {
        return type_expr;
    }

    if let TypeExpr::Union(members) = &type_expr {
        if members.iter().any(|member| {
            matches!(
                member,
                TypeExpr::Primitive(verter_type_expr::PrimitiveName::Undefined)
            )
        }) {
            return type_expr;
        }
    }

    match type_expr {
        TypeExpr::Unknown { .. } => type_expr,
        other => TypeExpr::union(vec![
            other,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Undefined),
        ]),
    }
}

fn add_top_level_undefined_to_model_event_payload(payload: TypeExpr) -> TypeExpr {
    match payload {
        TypeExpr::Tuple { elements, readonly } => {
            let updated = elements
                .iter()
                .enumerate()
                .map(|(index, element)| verter_type_expr::TupleElement {
                    label: element.label.clone(),
                    ty: if index == 0 {
                        add_top_level_undefined_to_type_expr(element.ty.clone())
                    } else {
                        element.ty.clone()
                    },
                    optional: element.optional,
                    rest: element.rest,
                })
                .collect::<Vec<_>>();
            TypeExpr::Tuple {
                elements: std::sync::Arc::from(updated),
                readonly,
            }
        }
        other => add_top_level_undefined_to_type_expr(other),
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
    mac: &AnalyzedMacro,
    bindings: &[AnalyzedBinding],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
    out: &mut Vec<ExposedAnalysis>,
) {
    for field in &mac.expose_fields {
        let type_expr = resolve_exposed_type(&field.name, bindings, evaluated);
        out.push(ExposedAnalysis {
            name: field.name.clone(),
            type_expr,
            type_expansion: evaluated.and_then(|eval| {
                eval.bindings
                    .iter()
                    .find(|binding| binding.name == field.name)
                    .map(field_expansion_metadata)
            }),
            description: None,
        });
    }
}

fn resolve_exposed_type(
    name: &str,
    bindings: &[AnalyzedBinding],
    evaluated: Option<&crate::analysis::type_expand::ExpandedComponentTypes>,
) -> TypeExpr {
    if let Some(eval) = evaluated {
        if let Some(f) = eval.bindings.iter().find(|f| f.name == name) {
            return f.r#type.clone();
        }
    }
    // Fall back to binding type annotation if available.
    //
    // `AnalyzedBinding` does not currently carry a typed `type_expr`
    // companion (W0.1 scope covered macro-side surfaces only — see
    // feedback file for the follow-up debt). The raw annotation is
    // preserved on the published `TypeExpr::Unknown { raw }` sentinel so
    // downstream display passthroughs keep the original text.
    if let Some(binding) = bindings.iter().find(|b| b.name == name) {
        if let Some(ref ann) = binding.type_annotation {
            return unknown_type(ann.clone());
        }
    }
    unknown_type("unknown".to_string())
}

// ── Options API fallback ───────────────────────────────────────────────────

fn merged_prop_fields(
    mac: &AnalyzedMacro,
    resolved: Option<&ResolvedMacroInput>,
) -> Vec<AnalyzedPropField> {
    let mut fields = mac.prop_fields.clone();
    if let Some(resolved) = resolved {
        for prop in &resolved.props {
            if let Some(existing) = fields.iter_mut().find(|field| field.name == prop.name) {
                merge_prop_field(existing, prop);
            } else {
                fields.push(prop.clone());
            }
        }
    }
    fields
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

fn merged_emit_fields(
    mac: &AnalyzedMacro,
    resolved: Option<&ResolvedMacroInput>,
) -> Vec<crate::analysis::types::AnalyzedEmitField> {
    let mut fields = mac.emit_fields.clone();
    let mut seen: rustc_hash::FxHashSet<String> =
        fields.iter().map(|field| field.name.clone()).collect();
    if let Some(resolved) = resolved {
        for emit in &resolved.emits {
            if seen.insert(emit.name.clone()) {
                fields.push(emit.clone());
            }
        }
    }
    fields
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
        out.push(PropAnalysis {
            name: prop.name.clone(),
            // `AnalyzedOptionsProp` does not currently carry a typed
            // `type_expr` companion (W0.1 scope covered macro-side surfaces
            // only — see feedback file for the follow-up debt). The raw
            // annotation is preserved on `TypeExpr::Unknown { raw }`; the
            // constructor branch maps Vue runtime constructor names
            // structurally (no string parsing — these are nominal
            // identifiers, not TS type annotations).
            type_expr: prop
                .type_annotation
                .as_ref()
                .map(|raw| unknown_type(raw.clone()))
                .or_else(|| {
                    prop.type_constructor.as_ref().map(|rt| match rt.as_str() {
                        "String" => TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
                        "Number" => TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
                        "Boolean" => TypeExpr::Primitive(verter_type_expr::PrimitiveName::Boolean),
                        "Function" => unknown_type("Function".to_string()),
                        "Array" => unknown_type("Array".to_string()),
                        "Object" => unknown_type("Object".to_string()),
                        other => unknown_type(other.to_string()),
                    })
                })
                .unwrap_or_else(|| unknown_type("unknown".to_string())),
            type_expansion: None,
            raw_type,
            required: prop.is_required,
            has_default: prop.has_default,
            default_value: prop.default_value.clone(),
            description: prop.description.clone(),
            tags: prop.tags.clone(),
        });
    }
}

fn extract_events_from_options(opts: &AnalyzedOptionsApi, out: &mut Vec<EventAnalysis>) {
    for field in &opts.emits {
        // The Options API analyzer (`extract_options_emits`) does not
        // currently lower OXC TS types, so `payload_expr` is `None` here
        // — see feedback file for the follow-up debt. We preserve the
        // raw payload string on `TypeExpr::Unknown { raw }` and avoid
        // string-reparsing inside the resolver pipeline.
        let raw = field.payload_type.as_deref().unwrap_or("unknown");
        out.push(EventAnalysis {
            name: field.name.clone(),
            payload: typed_or_unknown(field.payload_expr.as_ref(), raw),
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
        // D123 (Tier 1A) — `has_macro_failure` is set downstream of
        // analysis (at lowering / parse stage) when a `LoweringError`
        // is produced for the SFC. Analysis itself has no view of
        // lowering failures, so the field defaults to `false` here;
        // the parse-stage diagnostic populator (Tier 1C-α) flips it
        // to `true` on macro-impacting failure.
        has_macro_failure: false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[path = "component_meta_tests.rs"]
mod component_meta_tests;
