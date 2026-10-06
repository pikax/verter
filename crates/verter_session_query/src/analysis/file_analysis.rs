//! The serializable per-file analysis snapshot and the content identity of
//! the source bytes it observed.

use std::sync::Arc;

use crate::analysis::types::Hash16;

/// Content identity of the exact source bytes an analysis observed.
///
/// A consumer applying an analyzer-minted edit compares this to
/// [`Self::of_source`] of the live buffer; mismatch must produce no edit
/// (an in-bounds offset from another revision lands in the wrong place).
/// Not `RevisionMarker` (query revision, not content) and not LSP
/// `version` alone (a host-served analysis has none).
///
/// `Default` is the unstamped sentinel — equals no realistic identity,
/// so an unstamped analysis fails closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize)]
#[serde(transparent)]
pub struct AnalysisSourceRevision(Hash16);

impl AnalysisSourceRevision {
    /// Mint from these exact source bytes. Producer and consumer share
    /// this so the hash algorithm cannot drift.
    pub fn of_source(source: &str) -> Self {
        verter_audit::attribute_n!(ContentHash, source.len());
        Self(xxhash_rust::xxh3::xxh3_128(source.as_bytes()).to_le_bytes())
    }

    /// Adopt a `ParseSnapshot::whole_hash`, which is already
    /// `hash_16(source.as_bytes())` over the whole file — identical to
    /// [`Self::of_source`] on the same bytes, and free at a producer that
    /// already holds it.
    pub fn from_whole_hash(whole_hash: Hash16) -> Self {
        Self(whole_hash)
    }

    /// Whether this revision was never stamped (the `Default` sentinel).
    pub fn is_unstamped(&self) -> bool {
        *self == Self::default()
    }
}

/// Serializable snapshot of file analysis data, suitable for WASM export.
///
/// Returned by `VerterHost::get_analysis`.
/// Contains the combined script, style, and template analysis for an SFC.
///
/// Most fields are `Arc`-wrapped for cheap cloning — the underlying data is
/// shared between all snapshots of the same file version. Only `imports` and
/// `bindings` are owned `Vec`s because `VerterHost::get_analysis` mutates
/// them (import resolution and destructured binding enrichment).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAnalysisSnapshot {
    /// Import statements found in script blocks.
    /// Owned because `resolve_snapshot_imports` mutates `resolved_canonical_id`.
    pub imports: Vec<crate::analysis::types::AnalyzedImport>,
    /// Module reference sites found in script blocks.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub module_references: Arc<Vec<crate::analysis::types::AnalyzedModuleReference>>,
    /// Variable/function bindings declared in script blocks.
    /// Owned because `enrich_destructured_bindings` mutates `reactivity_kind`.
    pub bindings: Vec<crate::analysis::types::AnalyzedBinding>,
    /// Vue compiler macros used (defineProps, defineEmits, etc.).
    pub macros: Arc<Vec<crate::analysis::types::AnalyzedMacro>>,
    /// Type dependencies from macros that reference external files.
    pub macro_type_deps: Arc<Vec<crate::analysis::types::MacroTypeDep>>,
    /// Bitflags representing script characteristics (see `verter_semantic::analysis::ScriptFlags`).
    pub script_flags: u32,
    /// Per-style-block analysis (scoped, modules, v-bind usage).
    pub styles: Arc<Vec<crate::analysis::style::StyleBlockAnalysis>>,
    /// Template analysis (components, bindings, slots, refs, events).
    /// Present after compilation when template analysis scope flags are active.
    pub template: Option<Arc<crate::analysis::template::TemplateAnalysisSnapshot>>,
    /// Vue API call sites (lifecycle hooks, watchers, provide/inject, etc.).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub vue_api_calls: Arc<Vec<crate::analysis::types::VueApiCallSite>>,
    /// DOM query call sites (querySelector, getElementById, etc.).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub dom_query_calls: Arc<Vec<crate::analysis::script_snapshot::DomQueryCallSite>>,

    /// CSS variable manipulations via DOM style APIs.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub css_var_manipulations: Arc<Vec<crate::analysis::types::CssVarManipulation>>,

    /// Script-side binding usage occurrences with exact spans.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub script_binding_occurrences: Arc<Vec<crate::analysis::types::ScriptBindingOccurrence>>,

    /// Script-side usage facts for macro-declared members (unused-declaration
    /// diagnostics). `None` for files without Vue macros.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macro_usage: Option<crate::analysis::macro_usage::MacroUsageFacts>,

    /// Root identifiers referenced by `<style>` `v-bind()` expressions —
    /// style `v-bind()` resolves PROPS by bare name, so prop-member liveness
    /// consumes this set (see `ScriptAnalysisSnapshot::style_vbind_roots`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub style_vbind_roots: Vec<String>,

    /// Resolvable class-name tokens in carrier markup, for carriers WITHOUT a
    /// template element IR (Svelte). Empty for Vue.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub markup_class_tokens: Arc<Vec<crate::analysis::template::MarkupClassToken>>,

    /// Export signatures extracted from the file's script block.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub export_signatures: Arc<Vec<crate::analysis::types::ExportSignature>>,

    /// Options API analysis (`export default { ... }` or `export default defineComponent({ ... })`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_api: Option<crate::analysis::types::AnalyzedOptionsApi>,

    /// Store usage sites (Pinia, Vuex, convention-based composables).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub store_usages: Arc<Vec<crate::analysis::types::StoreUsage>>,
    /// Store definitions (defineStore, createStore, etc.).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub store_definitions: Arc<Vec<crate::analysis::types::StoreDefinition>>,

    /// Whether the script block uses TypeScript (`lang="ts"`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_typescript: bool,

    /// Content identity of the exact source bytes this analysis observed.
    ///
    /// A per-FILE identity, so two macros in one snapshot can never disagree
    /// about which buffer they address. Consumers that apply analyzer-minted
    /// edit anchors to a live buffer compare this against
    /// [`AnalysisSourceRevision::of_source`] of that buffer and fail closed on
    /// mismatch. `Default` (unstamped) never matches, so an unstamped snapshot
    /// also fails closed.
    #[serde(default, skip_serializing_if = "AnalysisSourceRevision::is_unstamped")]
    pub anchor_revision: AnalysisSourceRevision,
}

/// Helper for `skip_serializing_if` on `Arc<Vec<T>>`.
fn arc_vec_is_empty<T>(v: &Arc<Vec<T>>) -> bool {
    v.is_empty()
}
