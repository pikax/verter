use sha2::{Digest, Sha256};
use verter_span::Span;
use verter_type_expr::{TypeExpr, TypeExprScope};

/// Truncated SHA-256 hash (first 16 bytes). Used for content-based change detection.
pub type Hash16 = [u8; 16];

// ---------------------------------------------------------------------------
// Stable declaration identifiers
// ---------------------------------------------------------------------------

/// A globally unique, deterministic identifier for a type or value declaration.
///
/// Composed of a canonical file ID and a declaration name, making it stable
/// across re-analyses of the same source and unique across the workspace.
///
/// Used as `symbol_id` in `ResolutionNodeKey` for the resolver's node cache.
///
/// # Format
///
/// String representation: `{canonical_id}#{name}`
///
/// For file-level references (e.g., default export of a Vue SFC):
/// `{canonical_id}#*`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StableDeclarationId {
    canonical_id: String,
    name: String,
}

impl StableDeclarationId {
    /// Create a stable declaration ID for a named declaration in a file.
    pub fn new(canonical_id: &str, name: &str) -> Self {
        Self {
            canonical_id: canonical_id.to_string(),
            name: name.to_string(),
        }
    }

    /// Create a stable declaration ID for a file-level reference (e.g., default export).
    pub fn for_file(canonical_id: &str) -> Self {
        Self {
            canonical_id: canonical_id.to_string(),
            name: "*".to_string(),
        }
    }

    /// The canonical file path.
    pub fn canonical_id(&self) -> &str {
        &self.canonical_id
    }

    /// The declaration name within the file (`"*"` for file-level references).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether this is a file-level reference rather than a named declaration.
    pub fn is_file_level(&self) -> bool {
        self.name == "*"
    }

    /// Convert to the string representation used as `ResolutionNodeKey.symbol_id`.
    pub fn to_symbol_id(&self) -> String {
        format!("{}#{}", self.canonical_id, self.name)
    }

    /// Parse a `symbol_id` string back into a `StableDeclarationId`.
    ///
    /// Returns `None` if the string doesn't contain `#`.
    pub fn from_symbol_id(symbol_id: &str) -> Option<Self> {
        let (canonical_id, name) = symbol_id.rsplit_once('#')?;
        Some(Self {
            canonical_id: canonical_id.to_string(),
            name: name.to_string(),
        })
    }
}

impl std::fmt::Display for StableDeclarationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.canonical_id, self.name)
    }
}

/// Kind of declaration in the local namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalDeclarationKind {
    /// Type-only: `interface Foo`, `type Foo = ...`
    Type,
    /// Value-only: `const foo`, `function foo()`, `let foo`
    Value,
    /// Both type and value: `class Foo {}`, `enum Foo {}`
    TypeAndValue,
}

/// A local declaration entry in the analysis snapshot.
///
/// Represents a single type or value declaration in a file's top-level scope.
/// Used by the resolver to construct stable cross-file declaration identifiers
/// and to detect when a declaration's content has changed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDeclarationEntry {
    /// Declaration name.
    pub name: String,
    /// Whether this is a type, value, or both.
    pub kind: LocalDeclarationKind,
    /// Content hash of the declaration body. Changes when the declaration text changes.
    pub content_hash: Hash16,
    /// SFC-absolute span of the full declaration.
    pub span: Span,
}

/// Compute a truncated SHA-256 hash (first 16 bytes).
///
/// Used for export signature fingerprinting (cross-file change detection).
/// SHA-256 provides deterministic, collision-resistant hashes across builds.
pub fn hash_16(data: &[u8]) -> Hash16 {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}

bitflags::bitflags! {
    /// Bitwise flags for O(1) queries on analysis results.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct AnalysisFlags: u32 {
        const ASYNC_SETUP             = 1 << 0;
        const HAS_DEFINE_PROPS        = 1 << 1;
        const HAS_DEFINE_EMITS        = 1 << 2;
        const HAS_DEFINE_MODEL        = 1 << 3;
        const HAS_DEFINE_EXPOSE       = 1 << 4;
        const HAS_DEFINE_OPTIONS      = 1 << 5;
        const HAS_DEFINE_SLOTS        = 1 << 6;
        const HAS_WITH_DEFAULTS       = 1 << 7;
        const HAS_TYPE_BASED_PROPS    = 1 << 8;
        const HAS_TYPE_BASED_EMITS    = 1 << 9;
        const HAS_TYPE_BASED_MODEL    = 1 << 10;
        const HAS_REACTIVE_STATE      = 1 << 11;
        const HAS_COMPUTED            = 1 << 12;
        const HAS_WATCHERS            = 1 << 13;
        const HAS_LIFECYCLE_HOOKS     = 1 << 14;
        const HAS_PROVIDE             = 1 << 15;
        const HAS_INJECT              = 1 << 16;
        const HAS_EXTERNAL_TYPE_DEPS  = 1 << 17;
        const HAS_INHERIT_ATTRS_FALSE = 1 << 18;
        const HAS_OPTIONS_API         = 1 << 19;
        const HAS_STORE_USAGE         = 1 << 20;
        const HAS_STORE_DEFINITION    = 1 << 21;
        /// Set when the analyzed file declares a `TSInterfaceDeclaration`
        /// named exactly `AppConfig`. Covers top-level, exported,
        /// default-exported, and nested-inside-`declare module`/`declare
        /// global` cases. Used by the `AppConfigNoOverrideProofDb`
        /// production producer (Track 2.4) to short-circuit the proof
        /// for files that cannot contribute an override.
        const DECLARES_INTERFACE_APP_CONFIG = 1 << 22;
    }
}

/// Comprehensive script analysis captured during SFC parsing.
/// Powers dependency tracking, type-aware linting, and codegen optimization.
///
/// This is an immutable post-parse artifact: once produced it is never
/// mutated, and the host shares it across readers via
/// `Arc<ScriptAnalysisSnapshot>` rather than deep-copying ~18 owned vectors
/// per read. `Clone` (derived) performs a genuine field-by-field deep copy and
/// is reserved for the rare caller that needs an owned copy; the hot read path
/// `Arc::clone`s the shared handle instead, which is a refcount bump.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptAnalysisSnapshot {
    /// All import declarations found in the script block(s).
    pub imports: Vec<AnalyzedImport>,

    /// All module reference sites found in the script block(s).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub module_references: Vec<AnalyzedModuleReference>,

    /// Top-level bindings (variables, functions, classes).
    pub bindings: Vec<AnalyzedBinding>,

    /// Vue macro calls detected in script setup.
    pub macros: Vec<AnalyzedMacro>,

    /// Which imported types are used by which macros (derived from macros + imports).
    pub macro_type_deps: Vec<MacroTypeDep>,

    /// Bitwise flags for quick queries (serialized as raw u32 bits).
    #[serde(
        serialize_with = "serialize_analysis_flags",
        deserialize_with = "deserialize_analysis_flags"
    )]
    pub flags: AnalysisFlags,

    /// Exported functions with return type analysis (when FUNC_RETURNS scope is active).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exported_functions: Vec<AnalyzedExportedFunction>,

    /// Vue API function call sites (lifecycle hooks, watchers, provide/inject, etc.).
    /// Tracks calls whose return values are typically discarded (e.g., `onMounted(cb)`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vue_api_calls: Vec<VueApiCallSite>,

    /// DOM query call sites (querySelector, getElementById, etc.).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dom_query_calls: Vec<DomQueryCallSite>,

    /// CSS variable manipulations via DOM style APIs (setProperty, getPropertyValue, removeProperty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub css_var_manipulations: Vec<CssVarManipulation>,

    /// Script-side binding usage occurrences with exact spans.
    /// Populated when `AnalysisScope::SCRIPT_USAGES` is active.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub script_binding_occurrences: Vec<ScriptBindingOccurrence>,

    /// SFC-absolute byte offset of the first top-level `await` expression (if any).
    /// Used by lint rules to detect lifecycle hooks/watchers called after await.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_await_offset: Option<u32>,

    /// TODO(type-provider): Enhanced type info populated by TSGO when connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_enhancements: Option<ScriptTypeEnhancements>,

    /// Options API analysis extracted from `export default { ... }` or
    /// `export default defineComponent({ ... })`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_api: Option<AnalyzedOptionsApi>,

    /// Vue compiler macro calls found inside nested scopes (functions, conditionals, loops).
    /// These macros must be at root level in `<script setup>` — nested calls are invalid.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nested_macro_calls: Vec<NestedMacroCall>,

    /// Store usage sites (Pinia, Vuex, convention-based composable stores).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_usages: Vec<StoreUsage>,

    /// Store definitions (`defineStore`, `createStore`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_definitions: Vec<StoreDefinition>,

    /// Whether the script block uses TypeScript (`lang="ts"`).
    ///
    /// Used by lint rules that only apply to TypeScript SFCs (e.g., `define-props-declaration`,
    /// `define-emits-declaration`, `define-model-type-required`, `require-typed-ref`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_typescript: bool,

    /// Top-level declaration entries: types, values, and class/enum (both).
    ///
    /// Provides a unified view of all declarations in the file for the resolver.
    /// Each entry has a content hash for change detection and a span for position.
    /// Populated during `build_script_analysis`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declaration_entries: Vec<LocalDeclarationEntry>,
}

impl ScriptAnalysisSnapshot {
    /// Mark bindings that are referenced by CSS `v-bind()` expressions in style blocks.
    ///
    /// For each `v-bind(expr)` found in style analysis, extracts the root identifier
    /// (e.g., `"color"` from `v-bind(color)`, `"theme"` from `v-bind(theme.color)`)
    /// and sets `used_in_style = true` on the matching binding.
    pub fn mark_bindings_used_in_style(
        &mut self,
        style_analyses: &[crate::analysis::style::StyleBlockAnalysis],
    ) {
        // Collect all root identifiers from v-bind() expressions across all style blocks.
        let referenced: rustc_hash::FxHashSet<&str> = style_analyses
            .iter()
            .flat_map(|s| &s.v_binds)
            .map(|vb| {
                // Extract root identifier: "theme.color" → "theme", "color" → "color"
                vb.expression
                    .split_once('.')
                    .map_or(vb.expression.as_str(), |(root, _)| root)
            })
            // Also handle bracket access: "obj['key']" → "obj"
            .map(|root| {
                root.split_once('[')
                    .map_or(root, |(before_bracket, _)| before_bracket)
            })
            .filter(|name| !name.is_empty())
            .collect();

        if referenced.is_empty() {
            return;
        }

        for binding in &mut self.bindings {
            if referenced.contains(binding.name.as_str()) {
                binding.used_in_style = true;
            }
        }
    }
}

/// A Vue compiler macro call found inside a nested scope (not at the root level of `<script setup>`).
/// These are invalid — macros like `defineProps`, `defineEmits`, etc. must be at the top level.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NestedMacroCall {
    /// The macro name as written in source (e.g., `"defineProps"`).
    pub name: String,
    /// SFC-absolute byte span of the call expression.
    pub span: Span,
}

/// A single import declaration extracted from a script block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedImport {
    /// The import source specifier (e.g., "./types", "vue", "@/utils").
    pub source: String,
    /// Whether this is `import type { ... }` (declaration-level type-only).
    pub is_type_only: bool,
    /// Individual bindings imported.
    pub bindings: Vec<AnalyzedImportBinding>,
    /// SFC-absolute byte span of the import declaration.
    pub span: Span,
    /// Canonical file ID resolved by the host (None during standalone analysis).
    /// Populated by verter_session after path resolution for cross-file go-to-definition.
    pub resolved_canonical_id: Option<String>,
}

/// Syntax form that introduced a module reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ModuleReferenceSyntax {
    StaticImport,
    ExportFrom,
    DynamicImport,
    RequireCall,
}

/// Runtime resolution semantics for a module reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ModuleReferenceSemantics {
    Import,
    Require,
}

/// Whether the module reference can be resolved statically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ModuleReferenceAnalyzability {
    Exact,
    FiniteSet,
    UnknownDynamic,
}

/// A module reference extracted from script content.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedModuleReference {
    /// Syntax form that introduced the reference.
    pub syntax: ModuleReferenceSyntax,
    /// Runtime resolution semantics (`import` vs `require`).
    pub semantics: ModuleReferenceSemantics,
    /// Whether the site was declaration-level type-only.
    pub is_type_only: bool,
    /// SFC-absolute span of the containing statement or call expression.
    pub span: Span,
    /// SFC-absolute span of the module specifier expression.
    pub expr_span: Span,
    /// Raw source text for the specifier expression.
    pub raw_text: String,
    /// Exact resolved literal when statically known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literal_specifier: Option<String>,
    /// Finite set of candidate literals when the site can be narrowed to a union.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finite_specifiers: Vec<String>,
    /// Static prefix, if any, for dynamic sites such as ``import(`foo-${bar}`)``.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_prefix: Option<String>,
    /// Static analyzability classification.
    pub analyzability: ModuleReferenceAnalyzability,
}

impl serde::Serialize for AnalyzedImport {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 5 + usize::from(self.resolved_canonical_id.is_some());
        let mut s = serializer.serialize_struct("AnalyzedImport", count)?;
        s.serialize_field("source", &self.source)?;
        s.serialize_field("isTypeOnly", &self.is_type_only)?;
        s.serialize_field("bindings", &self.bindings)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        if self.resolved_canonical_id.is_some() {
            s.serialize_field("resolvedCanonicalId", &self.resolved_canonical_id)?;
        }
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnalyzedImport {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            source: String,
            is_type_only: bool,
            bindings: Vec<AnalyzedImportBinding>,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
            #[serde(default)]
            resolved_canonical_id: Option<String>,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            source: w.source,
            is_type_only: w.is_type_only,
            bindings: w.bindings,
            span: Span::new(w.span_start, w.span_end),
            resolved_canonical_id: w.resolved_canonical_id,
        })
    }
}

/// A single specifier within an import declaration (e.g., `Foo` in `import { Foo } from "bar"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedImportBinding {
    /// Local binding name as used in the script.
    pub name: String,
    /// Import syntax kind (`named`, `default`, `namespace`).
    pub kind: ImportBindingKind,
    /// Imported/exported name for this binding when applicable.
    /// For default imports this is `"default"`.
    pub imported_name: Option<String>,
    /// Whether this specifier is type-only (`import { type Foo }`).
    pub is_type_only: bool,
    /// Vue API classification if the import source is 'vue'.
    pub vue_api: Option<VueApiClassification>,
    /// SFC-absolute byte span of the specifier name.
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportBindingKind {
    Named,
    Default,
    Namespace,
}

impl serde::Serialize for AnalyzedImportBinding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AnalyzedImportBinding", 7)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("kind", &self.kind)?;
        s.serialize_field("importedName", &self.imported_name)?;
        s.serialize_field("isTypeOnly", &self.is_type_only)?;
        s.serialize_field("vueApi", &self.vue_api)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnalyzedImportBinding {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            #[serde(default)]
            kind: Option<ImportBindingKind>,
            #[serde(default)]
            imported_name: Option<String>,
            is_type_only: bool,
            vue_api: Option<VueApiClassification>,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            kind: w.kind.unwrap_or(ImportBindingKind::Named),
            imported_name: w.imported_name,
            is_type_only: w.is_type_only,
            vue_api: w.vue_api,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

/// Rich classification of Vue imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VueApiClassification {
    // Reactivity
    Ref,
    ShallowRef,
    Reactive,
    ShallowReactive,
    Computed,
    ToRef,
    ToRefs,
    CustomRef,
    TriggerRef,
    Readonly,
    ShallowReadonly,
    // Lifecycle
    OnMounted,
    OnUnmounted,
    OnBeforeMount,
    OnBeforeUnmount,
    OnUpdated,
    OnBeforeUpdate,
    OnActivated,
    OnDeactivated,
    OnErrorCaptured,
    OnRenderTracked,
    OnRenderTriggered,
    OnServerPrefetch,
    // Watchers
    Watch,
    WatchEffect,
    WatchPostEffect,
    WatchSyncEffect,
    // Dependency injection
    Provide,
    Inject,
    // Template utils
    UseSlots,
    UseAttrs,
    UseTemplateRef,
    UseId,
    // Instance
    GetCurrentInstance,
    NextTick,
    // Model helper (Vue 3.4+)
    UseModel,
    // Watcher cleanup (Vue 3.5+)
    OnWatcherCleanup,
    // DI utility (Vue 3.3+)
    HasInjectionContext,
    // Macros
    DefineProps,
    DefineEmits,
    DefineModel,
    DefineExpose,
    DefineOptions,
    DefineSlots,
    WithDefaults,
    // Component helpers
    DefineComponent,
    DefineAsyncComponent,
    // Other known APIs
    H,
    CreateApp,
    CreateSSRApp,
    // Unknown Vue export
    Other,
}

/// A Vue API function call site found in the script.
/// Tracks calls like `onMounted(cb)`, `watch(source, cb)`, `provide(key, val)`, etc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueApiCallSite {
    /// Which Vue API was called.
    pub api: VueApiClassification,
    /// SFC-absolute byte span of the call expression.
    pub span: Span,
    /// First string argument value, if available (e.g., for `useTemplateRef('foo')`).
    pub arg_value: Option<String>,
    /// Whether the call has type parameters (e.g., `ref<string>()`).
    pub has_type_params: bool,
    /// Whether the first function argument is an async function/arrow.
    /// Used by `no-async-in-computed` rule.
    pub is_async_callback: bool,
    /// Callback parameters with inferred types (e.g., `watch(x, (val) => ...)` → `val: number`).
    pub callback_params: Vec<VueApiCallbackParam>,
}

/// A parameter from a Vue API callback function, with an optionally inferred type.
///
/// Examples:
/// - `watch(countRef, (val, old) => ...)` → `val: number`, `old: number`
/// - `onMounted(() => ...)` → no params
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VueApiCallbackParam {
    /// Parameter name.
    pub name: String,
    /// SFC-absolute byte span of the parameter name.
    pub span: Span,
    /// Inferred type string (e.g., "number" from unwrapping `Ref<number>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred_type: Option<String>,
}

impl serde::Serialize for VueApiCallSite {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 3
            + usize::from(self.arg_value.is_some())
            + usize::from(self.has_type_params)
            + usize::from(self.is_async_callback)
            + usize::from(!self.callback_params.is_empty());
        let mut s = serializer.serialize_struct("VueApiCallSite", count)?;
        s.serialize_field("api", &self.api)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        if self.arg_value.is_some() {
            s.serialize_field("argValue", &self.arg_value)?;
        }
        if self.has_type_params {
            s.serialize_field("hasTypeParams", &self.has_type_params)?;
        }
        if self.is_async_callback {
            s.serialize_field("isAsyncCallback", &self.is_async_callback)?;
        }
        if !self.callback_params.is_empty() {
            s.serialize_field("callbackParams", &self.callback_params)?;
        }
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for VueApiCallSite {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            api: VueApiClassification,
            span_start: u32,
            span_end: u32,
            #[serde(default)]
            arg_value: Option<String>,
            #[serde(default)]
            has_type_params: bool,
            #[serde(default)]
            is_async_callback: bool,
            #[serde(default)]
            callback_params: Vec<VueApiCallbackParam>,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            api: w.api,
            span: Span::new(w.span_start, w.span_end),
            arg_value: w.arg_value,
            has_type_params: w.has_type_params,
            is_async_callback: w.is_async_callback,
            callback_params: w.callback_params,
        })
    }
}

/// A DOM query call site found in the script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomQueryCallSite {
    /// Which DOM query API was called.
    pub kind: DomQueryKind,
    /// The raw string argument (selector text or ID).
    pub selector_text: String,
    /// Parsed selector structure (reuses the CSS selector parser).
    pub parsed: Option<crate::analysis::style::StructuredSelector>,
    /// SFC-absolute byte span of the call expression.
    pub span: Span,
    /// SFC-absolute byte span of just the string argument.
    pub arg_span: Span,
}

impl serde::Serialize for DomQueryCallSite {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 6 + usize::from(self.parsed.is_some());
        let mut s = serializer.serialize_struct("DomQueryCallSite", count)?;
        s.serialize_field("kind", &self.kind)?;
        s.serialize_field("selectorText", &self.selector_text)?;
        if self.parsed.is_some() {
            s.serialize_field("parsed", &self.parsed)?;
        }
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.serialize_field("argSpanStart", &self.arg_span.start)?;
        s.serialize_field("argSpanEnd", &self.arg_span.end)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for DomQueryCallSite {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            kind: DomQueryKind,
            selector_text: String,
            #[serde(default)]
            parsed: Option<crate::analysis::style::StructuredSelector>,
            span_start: u32,
            span_end: u32,
            arg_span_start: u32,
            arg_span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            kind: w.kind,
            selector_text: w.selector_text,
            parsed: w.parsed,
            span: Span::new(w.span_start, w.span_end),
            arg_span: Span::new(w.arg_span_start, w.arg_span_end),
        })
    }
}

/// Discriminant for DOM query API types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DomQueryKind {
    QuerySelector,
    QuerySelectorAll,
    GetElementById,
    GetElementsByClassName,
}

impl DomQueryKind {
    /// Get the JavaScript method name.
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::QuerySelector => "querySelector",
            Self::QuerySelectorAll => "querySelectorAll",
            Self::GetElementById => "getElementById",
            Self::GetElementsByClassName => "getElementsByClassName",
        }
    }
}

impl VueApiClassification {
    /// Check if this API is a lifecycle hook.
    pub const fn is_lifecycle(&self) -> bool {
        matches!(
            self,
            Self::OnMounted
                | Self::OnUnmounted
                | Self::OnBeforeMount
                | Self::OnBeforeUnmount
                | Self::OnUpdated
                | Self::OnBeforeUpdate
                | Self::OnActivated
                | Self::OnDeactivated
                | Self::OnErrorCaptured
                | Self::OnRenderTracked
                | Self::OnRenderTriggered
                | Self::OnServerPrefetch
        )
    }

    /// Check if this API is a watcher.
    pub const fn is_watcher(&self) -> bool {
        matches!(
            self,
            Self::Watch | Self::WatchEffect | Self::WatchPostEffect | Self::WatchSyncEffect
        )
    }

    /// Check if this API requires synchronous setup context.
    pub const fn requires_sync_context(&self) -> bool {
        self.is_lifecycle() || self.is_watcher() || matches!(self, Self::Provide | Self::Inject)
    }

    /// Get the Vue API function name as it appears in source code.
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Ref => "ref",
            Self::ShallowRef => "shallowRef",
            Self::Reactive => "reactive",
            Self::ShallowReactive => "shallowReactive",
            Self::Computed => "computed",
            Self::ToRef => "toRef",
            Self::ToRefs => "toRefs",
            Self::CustomRef => "customRef",
            Self::TriggerRef => "triggerRef",
            Self::Readonly => "readonly",
            Self::ShallowReadonly => "shallowReadonly",
            Self::OnMounted => "onMounted",
            Self::OnUnmounted => "onUnmounted",
            Self::OnBeforeMount => "onBeforeMount",
            Self::OnBeforeUnmount => "onBeforeUnmount",
            Self::OnUpdated => "onUpdated",
            Self::OnBeforeUpdate => "onBeforeUpdate",
            Self::OnActivated => "onActivated",
            Self::OnDeactivated => "onDeactivated",
            Self::OnErrorCaptured => "onErrorCaptured",
            Self::OnRenderTracked => "onRenderTracked",
            Self::OnRenderTriggered => "onRenderTriggered",
            Self::OnServerPrefetch => "onServerPrefetch",
            Self::Watch => "watch",
            Self::WatchEffect => "watchEffect",
            Self::WatchPostEffect => "watchPostEffect",
            Self::WatchSyncEffect => "watchSyncEffect",
            Self::Provide => "provide",
            Self::Inject => "inject",
            Self::UseSlots => "useSlots",
            Self::UseAttrs => "useAttrs",
            Self::UseTemplateRef => "useTemplateRef",
            Self::UseId => "useId",
            Self::GetCurrentInstance => "getCurrentInstance",
            Self::NextTick => "nextTick",
            Self::UseModel => "useModel",
            Self::OnWatcherCleanup => "onWatcherCleanup",
            Self::HasInjectionContext => "hasInjectionContext",
            Self::DefineProps => "defineProps",
            Self::DefineEmits => "defineEmits",
            Self::DefineModel => "defineModel",
            Self::DefineExpose => "defineExpose",
            Self::DefineOptions => "defineOptions",
            Self::DefineSlots => "defineSlots",
            Self::WithDefaults => "withDefaults",
            Self::DefineComponent => "defineComponent",
            Self::DefineAsyncComponent => "defineAsyncComponent",
            Self::H => "h",
            Self::CreateApp => "createApp",
            Self::CreateSSRApp => "createSSRApp",
            Self::Other => "unknown",
        }
    }
}

/// Granular reactivity classification of a binding.
/// Distinguishes ref-like (needs `.value`) from reactive-like (direct property access).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum ReactivityKind {
    /// Not reactive: const literal, function, class, plain const.
    #[default]
    None,
    /// Ref-like: `ref()`, `shallowRef()`, `customRef()`, `toRef()` — needs `.value` unwrap.
    Ref,
    /// Computed: `computed()` — needs `.value` unwrap, read-only.
    Computed,
    /// Reactive-like: `reactive()`, `shallowReactive()` — direct property access.
    Reactive,
    /// Composable return: `useSomething()` — may or may not be ref.
    MaybeRef,
    /// Mutable: `let` binding — reassignable.
    Mutable,
}

/// A top-level variable, function, or class binding declared in the script block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedBinding {
    /// Binding name as declared in source.
    pub name: String,
    /// Declaration kind (`const`, `let`, `function`, etc.).
    pub kind: AnalyzedBindingKind,
    /// Whether the binding holds reactive state (e.g., initialized via `ref()` or `reactive()`).
    pub is_reactive: bool,
    /// Granular reactivity classification (replaces `is_reactive` semantically).
    pub reactivity_kind: ReactivityKind,
    /// TypeScript type annotation from the AST (e.g., `"Ref<number>"`).
    pub type_annotation: Option<String>,
    /// What expression created this binding, if classifiable.
    pub initializer: Option<BindingInitializer>,
    /// SFC-absolute byte span of the binding name.
    pub span: Span,
    /// Whether this binding is referenced elsewhere in the script body
    /// (not counting its own declaration).
    pub used_in_script: bool,
    /// Whether this binding is referenced in a style block's `v-bind()`.
    pub used_in_style: bool,
}

impl serde::Serialize for AnalyzedBinding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 9 + usize::from(self.type_annotation.is_some());
        let mut s = serializer.serialize_struct("AnalyzedBinding", count)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("kind", &self.kind)?;
        s.serialize_field("isReactive", &self.is_reactive)?;
        s.serialize_field("reactivityKind", &self.reactivity_kind)?;
        if self.type_annotation.is_some() {
            s.serialize_field("typeAnnotation", &self.type_annotation)?;
        }
        s.serialize_field("initializer", &self.initializer)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.serialize_field("usedInScript", &self.used_in_script)?;
        s.serialize_field("usedInStyle", &self.used_in_style)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnalyzedBinding {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            name: String,
            kind: AnalyzedBindingKind,
            is_reactive: bool,
            #[serde(default)]
            reactivity_kind: ReactivityKind,
            #[serde(default)]
            type_annotation: Option<String>,
            initializer: Option<BindingInitializer>,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
            #[serde(default)]
            used_in_script: bool,
            #[serde(default)]
            used_in_style: bool,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            name: w.name,
            kind: w.kind,
            is_reactive: w.is_reactive,
            reactivity_kind: w.reactivity_kind,
            type_annotation: w.type_annotation,
            initializer: w.initializer,
            span: Span::new(w.span_start, w.span_end),
            used_in_script: w.used_in_script,
            used_in_style: w.used_in_style,
        })
    }
}

/// Tracks what function/expression created a binding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BindingInitializer {
    /// Created by a function call: `const x = foo()`
    FunctionCall {
        callee: String,
        callee_import_source: Option<String>,
        vue_api: Option<VueApiClassification>,
        /// When the call is `defineAsyncComponent(() => import('./X.vue'))`
        /// (`vue_api == DefineAsyncComponent`), the statically-resolvable
        /// dynamic-import specifier of the wrapped component carrier
        /// (`"./X.vue"`). `None` for every other call, and for an async
        /// component whose import target is not a single static literal.
        /// Carries the carrier-linkage source so the template converter can
        /// link a `<X>` tag bound to this binding to its `.vue` carrier.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        async_component_source: Option<String>,
    },
    /// Created by a literal: `const x = 42`
    Literal { kind: LiteralKind },
    /// Created by an identifier reference: `const x = importedValue`
    Reference { name: String },
    /// Created by something we can't classify
    Other,
}

/// The kind of literal value used to initialize a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LiteralKind {
    String,
    Number,
    Boolean,
    Null,
    Undefined,
    Array,
    Object,
    Template,
}

/// Declaration kind for a top-level binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AnalyzedBindingKind {
    Const,
    Let,
    Var,
    Function,
    AsyncFunction,
    Class,
}

/// A single JSDoc tag extracted from a `/** ... */` comment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsdocTag {
    /// Tag name without the `@` prefix (e.g., `"param"`, `"deprecated"`, `"default"`).
    pub name: String,
    /// Tag text after the tag name, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// An individual prop field from `defineProps`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedPropField {
    /// Prop name as declared (e.g., `"count"` from `defineProps<{ count: number }>()`).
    pub name: String,
    /// Whether the prop is optional. For type-based props: `true` when declared with `?`.
    /// For runtime props: `true` unless `required: true` is explicitly set (Vue default).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_optional: bool,
    /// SFC-absolute byte span of the prop name in the declaration.
    pub span: Span,
    /// TypeScript type annotation text (e.g., `"'primary' | 'secondary'"` from
    /// `defineProps<{ variant: 'primary' | 'secondary' }>()`).
    /// Only populated for type-based `defineProps` with inline type literals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
    /// Lowered typed form of the prop's type annotation. Populated by the
    /// producer that has the OXC `TSType<'_>` AST node in scope (analyzer or
    /// cross-file external resolver). Authoritative for resolver / projector /
    /// registry / policy / materialiser consumers — `type_annotation` is
    /// display-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_expr: Option<TypeExpr>,
    /// Scope of `type_expr`: canonical_id of the file whose OXC parse produced
    /// the typed expression. Required so consumers walking nested
    /// `TypeExpr::Ref` nodes resolve them in the file where the annotation was
    /// written. Pairing invariant: `type_expr.is_some() <=> type_expr_scope.is_some()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_expr_scope: Option<TypeExprScope>,
    /// JSDoc description extracted from the leading `/** ... */` comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSDoc tags (e.g., `@default`, `@deprecated`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<JsdocTag>,
    /// How this prop's type was resolved.
    #[serde(default, skip_serializing_if = "is_rust_resolution")]
    pub resolution_source: TypeResolutionSource,
    /// If `resolution_source` is `Unresolved`, explains why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_error: Option<String>,
    /// True iff the SFC author explicitly wrote this prop name as a member of
    /// the `defineProps<T>()` type argument's own body (inline `TSTypeLiteral`,
    /// the directly-referenced interface's own body, an explicit Object arm of
    /// an intersection literal, or the runtime object / array form). False
    /// when the prop only reaches the surface via heritage (`extends`),
    /// utility-type expansion (`Omit`, `Pick`, etc.), or intersection arms
    /// resolved through external references. Consumed by
    /// `verter_audit::PublishedSurfacePolicy::Refined` to distinguish "author
    /// asked for this name" from "this name arrived via inheritance".
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub declared_in_macro_type_arg: bool,
}

fn is_rust_resolution(src: &TypeResolutionSource) -> bool {
    matches!(src, TypeResolutionSource::Rust)
}

fn is_false(v: &bool) -> bool {
    !v
}

/// An individual emit field from `defineEmits`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedEmitField {
    /// Event name as declared (e.g., `"custom"` from `defineEmits<{ custom: [payload: string] }>()`).
    pub name: String,
    /// SFC-absolute byte span of the event name in the declaration.
    pub span: Span,
    /// Payload type text extracted from the type declaration.
    /// For property signatures: the value type (e.g., `"[id: number]"`).
    /// For call signatures: params after event name as tuple (e.g., `"[id: number]"`).
    /// `None` for runtime emits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_type: Option<String>,
    /// Lowered typed form of the emit's payload type. Populated by the
    /// producer that has the OXC `TSType<'_>` AST node in scope (analyzer or
    /// cross-file external resolver). Authoritative for resolver / projector /
    /// registry / policy / materialiser consumers — `payload_type` is display-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_expr: Option<TypeExpr>,
    /// Scope of `payload_expr`: canonical_id of the file whose OXC parse
    /// produced the typed expression. Pairing invariant:
    /// `payload_expr.is_some() <=> payload_expr_scope.is_some()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_expr_scope: Option<TypeExprScope>,
    /// JSDoc description extracted from the leading `/** ... */` comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSDoc tags (e.g., `@deprecated`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<JsdocTag>,
}

/// An individual slot field from `defineSlots`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedSlotField {
    /// Slot name (e.g., `"default"`, `"header"`).
    pub name: String,
    /// Whether the slot is required (no `?` in type param).
    pub is_required: bool,
    /// SFC-absolute byte span of the slot name in the declaration.
    pub span: Span,
    /// Binding properties from the slot function's first parameter type.
    /// E.g., `default(props: { item: string })` → `[{name: "item", type_annotation: Some("string")}]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<AnalyzedSlotFieldBinding>,
    /// Return type text of the slot function (e.g., `"VNode[]"`, `"any"`).
    /// Used by strict slots to validate slot children types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_type: Option<String>,
    /// Lowered typed form of the slot's return type. Populated by the
    /// producer that has the OXC `TSType<'_>` AST node in scope. Authoritative
    /// for resolver / projector / registry / policy / materialiser consumers —
    /// `return_type` is display-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_expr: Option<TypeExpr>,
    /// Scope of `return_expr`: canonical_id of the file whose OXC parse
    /// produced the typed expression. Pairing invariant:
    /// `return_expr.is_some() <=> return_expr_scope.is_some()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_expr_scope: Option<TypeExprScope>,
    /// JSDoc description extracted from the leading `/** ... */` comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSDoc tags (e.g., `@deprecated`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<JsdocTag>,
}

/// A single binding property from a slot function parameter type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedSlotFieldBinding {
    /// Binding name (e.g., `"item"`, `"index"`).
    pub name: String,
    /// Type annotation text extracted from source (e.g., `"string"`, `"MyItem"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
    /// Lowered typed form of the binding's type. Populated by the producer
    /// that has the OXC `TSType<'_>` AST node in scope (typically
    /// `TypeExpr::IndexedAccess` against the slot parameter object).
    /// Authoritative for resolver / projector / registry / policy /
    /// materialiser consumers — `type_annotation` is display-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_expr: Option<TypeExpr>,
    /// Scope of `binding_expr`: canonical_id of the file whose OXC parse
    /// produced the typed expression. Pairing invariant:
    /// `binding_expr.is_some() <=> binding_expr_scope.is_some()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_expr_scope: Option<TypeExprScope>,
    /// SFC-absolute byte span of the binding key in `defineSlots` type.
    /// Zero-span fallback for backward compat with older JSON.
    #[serde(default)]
    pub span: Span,
}

// ── Options API Analysis Types ──

/// Full analysis of an Options API component (`export default { ... }` or
/// `export default defineComponent({ ... })`).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedOptionsApi {
    /// Whether the export is wrapped in `defineComponent()`.
    pub is_define_component: bool,
    /// SFC-absolute byte span of the options object.
    pub object_span: Span,
    /// Props declared via `props: [...]` or `props: { ... }`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<AnalyzedOptionsProp>,
    /// Emits declared via `emits: [...]` or `emits: { ... }`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emits: Vec<AnalyzedEmitField>,
    /// Fields returned from `data()`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_fields: Vec<AnalyzedOptionsField>,
    /// Computed properties from `computed: { ... }`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub computed_fields: Vec<AnalyzedOptionsField>,
    /// Methods from `methods: { ... }`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<AnalyzedOptionsField>,
    /// Fields from `expose: [...]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expose: Vec<AnalyzedOptionsField>,
    /// Keys from `provide: { ... }` or `provide()`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provide_keys: Vec<AnalyzedOptionsField>,
    /// Keys from `inject: [...]` or `inject: { ... }`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inject_keys: Vec<AnalyzedOptionsField>,
    /// Locally registered components from `components: { ... }`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<AnalyzedOptionsComponent>,
    /// Whether `inheritAttrs: false` is set.
    pub has_inherit_attrs_false: bool,
}

/// A single prop from the Options API `props` option.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedOptionsProp {
    /// Prop name as declared.
    pub name: String,
    /// SFC-absolute byte span of the prop key.
    pub span: Span,
    /// Vue constructor name if provided (e.g., `"String"`, `"Number"`, `"Array"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_constructor: Option<String>,
    /// Whether `required: true` is set.
    pub is_required: bool,
    /// Whether a `default` value is provided.
    pub has_default: bool,
    /// Default value source text (e.g., `"'Hello'"` or `"42"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    /// Type annotation from `PropType<T>` (e.g., `"HTMLCanvasElement"`).
    /// Display-only — typed consumers MUST read `type_expr` (typed sidecar).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
    /// Lowered typed form of the `PropType<T>` annotation. Populated by the
    /// analyzer when an OXC `TSType<'_>` for `T` is in scope (i.e., the prop
    /// is defined as `{ type: <ctor> as PropType<T> }`). Authoritative for
    /// resolver / projector / registry / policy / materialiser consumers —
    /// `type_annotation` is display-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_expr: Option<TypeExpr>,
    /// Scope of `type_expr`: canonical_id of the file whose OXC parse
    /// produced the typed expression. Pairing invariant:
    /// `type_expr.is_some() <=> type_expr_scope.is_some()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_expr_scope: Option<TypeExprScope>,
    /// JSDoc description (e.g., `"The display label"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSDoc tags (e.g., `@default`, `@deprecated`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<JsdocTag>,
}

/// A named field from an Options API option (data, computed, methods, etc.).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedOptionsField {
    /// Field name.
    pub name: String,
    /// SFC-absolute byte span of the field key.
    pub span: Span,
}

/// A locally registered component from the `components` option.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedOptionsComponent {
    /// Component name as registered (e.g., `"MyComp"` or `"Alias"`).
    pub name: String,
    /// SFC-absolute byte span of the component key.
    pub span: Span,
    /// Import source if the value is a known imported binding (e.g., `"./MyComp.vue"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_source: Option<String>,
}

/// An individual exposed field from `defineExpose`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedExposeField {
    /// Field name as declared (e.g., `"foo"` from `defineExpose({ foo })`).
    pub name: String,
    /// SFC-absolute byte span of the field key. `Some` only for fields the
    /// analyzer extracted from the SFC's own object-literal argument; `None`
    /// for fields normalized from a `defineExpose<T>()` type-argument
    /// surface, whose members live in their declaration file (their JSDoc
    /// arrives pre-sliced on `description`/`tags`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    /// Lowered typed form of the exposed member's type. Populated by the
    /// producer that resolved the member's type surface (the typeinfo macro
    /// DTO normalizer raising the `defineExpose<T>()` surface member value);
    /// `None` for analyzer object-literal fields, whose type comes from
    /// binding / evaluation lookups downstream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_expr: Option<TypeExpr>,
    /// Scope of `type_expr`: canonical_id of the file whose parse produced
    /// the typed expression. Pairing invariant:
    /// `type_expr.is_some() <=> type_expr_scope.is_some()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_expr_scope: Option<TypeExprScope>,
    /// JSDoc description from the leading `/** ... */` block on the field
    /// key, captured at extraction exactly like runtime prop fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSDoc tags from the same leading block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<JsdocTag>,
}

/// How a prop type was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TypeResolutionSource {
    /// Resolved entirely by the Rust analyzer (inline types, simple references).
    #[default]
    Rust,
    /// Resolved with help from the TypeScript type checker.
    TypeScript,
    /// Could not be resolved; see `resolution_error` for details.
    Unresolved,
}

/// A default value extracted from `withDefaults()` second argument.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedDefaultValue {
    /// The prop name this default applies to.
    pub key: String,
    /// The source text of the default value expression.
    pub value: String,
    /// SFC-absolute byte span of the value expression.
    pub span: Span,
}

/// A locally resolved type expansion referenced by macro type parameters.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLocalType {
    /// The type name as referenced in the macro (e.g., `"Props"`).
    pub name: String,
    /// The expanded type text (e.g., `"{ count: number; label?: string }"`).
    pub expanded: String,
    /// Structured expanded object form retained for consumers that need
    /// canonical IR instead of reparsing `expanded`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_expr: Option<verter_type_expr::TypeExpr>,
    /// SFC-absolute byte span of the type declaration.
    pub span: Span,
}

impl PartialEq for ResolvedLocalType {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.expanded == other.expanded
            && self.type_expr == other.type_expr
            && self.span == other.span
    }
}

impl Eq for ResolvedLocalType {}

/// A Vue compiler macro call found in `<script setup>` (e.g., `defineProps`, `defineEmits`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedMacro {
    /// Which macro was called.
    pub kind: AnalyzedMacroKind,
    /// Whether the macro uses type params `<...>`.
    pub is_type_based: bool,
    /// Type names referenced in the type params.
    pub type_references: Vec<String>,
    /// The binding name if `const X = defineProps<...>()`.
    pub binding_name: Option<String>,
    /// For `defineModel('name')`: the model property name. `None` means default (`"modelValue"`).
    pub model_name: Option<String>,
    /// For `defineOptions({ inheritAttrs: false })`: whether `inheritAttrs` is set to `false`.
    pub has_inherit_attrs_false: bool,
    /// Individual prop fields from `defineProps` (type-based and runtime).
    /// Each field has the prop name and the SFC-absolute span of the property key.
    pub prop_fields: Vec<AnalyzedPropField>,
    /// Individual emit fields from `defineEmits` (type-based and runtime).
    /// Each field has the event name and the SFC-absolute span of the event name key.
    pub emit_fields: Vec<AnalyzedEmitField>,
    /// Individual slot fields from `defineSlots` (type-based).
    /// Each field has the slot name, whether it's required, and the SFC-absolute span.
    pub slot_fields: Vec<AnalyzedSlotField>,
    /// Object property key names from the `withDefaults()` second argument.
    /// Only populated for `WithDefaults` macros.
    pub default_keys: Vec<String>,
    /// Default values from `withDefaults()` second argument, with key-value pairs.
    pub default_values: Vec<AnalyzedDefaultValue>,
    /// Individual exposed fields from `defineExpose({ ... })`.
    /// Only populated for `DefineExpose` macros with an object literal argument.
    pub expose_fields: Vec<AnalyzedExposeField>,
    /// Locally resolved type expansions referenced by macro type parameters.
    pub resolved_local_types: Vec<ResolvedLocalType>,
    /// First type argument of the macro call (the parent shell), parsed
    /// once during shallow analysis (plan Step 1 / D1.2). For
    /// `defineProps<Props<T>>()` this is `Props<T>`. For
    /// `defineEmits<Emits>()` this is `Emits`. `None` when the macro
    /// has no type arguments or when parsing the source slice fails.
    ///
    /// Cache-owned per the Shallow File Processing Core Invariant
    /// (rule 1: capture once during single read/parse, never re-parse
    /// per call). Used by the host-side closure
    /// (`compute_evaluated_types_from_owner_context`) to drive
    /// dispatch-mediated projection of macro fields.
    ///
    /// `Arc<TypeExpr>` rather than `Box<TypeExpr>` so the closure +
    /// dispatch lower call clone a single refcount instead of deep-copying
    /// the full expression tree (R6).
    pub parsed_type_argument: Option<std::sync::Arc<verter_type_expr::TypeExpr>>,
    /// Scope of `parsed_type_argument`: canonical_id of the file whose
    /// OXC parse produced the typed expression. Pairing invariant:
    /// `parsed_type_argument.is_some() <=> parsed_type_argument_scope.is_some()`.
    /// Populated with the local SFC's canonical_id by the analyzer (the
    /// macro is always parsed in the local SFC's scope, but explicit
    /// pairing is required for the §3.1 invariant).
    pub parsed_type_argument_scope: Option<TypeExprScope>,
    /// SFC-absolute byte span of the macro call.
    pub span: Span,
}

impl serde::Serialize for AnalyzedMacro {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 7
            + usize::from(self.model_name.is_some())
            + usize::from(!self.prop_fields.is_empty())
            + usize::from(!self.emit_fields.is_empty())
            + usize::from(!self.slot_fields.is_empty())
            + usize::from(!self.default_keys.is_empty())
            + usize::from(!self.default_values.is_empty())
            + usize::from(!self.expose_fields.is_empty())
            + usize::from(!self.resolved_local_types.is_empty())
            + usize::from(self.parsed_type_argument.is_some())
            + usize::from(self.parsed_type_argument_scope.is_some());
        let mut s = serializer.serialize_struct("AnalyzedMacro", count)?;
        s.serialize_field("kind", &self.kind)?;
        s.serialize_field("isTypeBased", &self.is_type_based)?;
        s.serialize_field("typeReferences", &self.type_references)?;
        s.serialize_field("bindingName", &self.binding_name)?;
        if self.model_name.is_some() {
            s.serialize_field("modelName", &self.model_name)?;
        }
        s.serialize_field("hasInheritAttrsFalse", &self.has_inherit_attrs_false)?;
        if !self.prop_fields.is_empty() {
            s.serialize_field("propFields", &self.prop_fields)?;
        }
        if !self.emit_fields.is_empty() {
            s.serialize_field("emitFields", &self.emit_fields)?;
        }
        if !self.slot_fields.is_empty() {
            s.serialize_field("slotFields", &self.slot_fields)?;
        }
        if !self.default_keys.is_empty() {
            s.serialize_field("defaultKeys", &self.default_keys)?;
        }
        if !self.default_values.is_empty() {
            s.serialize_field("defaultValues", &self.default_values)?;
        }
        if !self.expose_fields.is_empty() {
            s.serialize_field("exposeFields", &self.expose_fields)?;
        }
        if !self.resolved_local_types.is_empty() {
            s.serialize_field("resolvedLocalTypes", &self.resolved_local_types)?;
        }
        // D1.2: opt-in field — only serialised when populated (mirrors
        // the convention used for prop_fields / emit_fields above and
        // keeps wire payloads compact when the macro has no type
        // argument). Field name is camelCase to match the Wire struct's
        // `#[serde(rename_all = "camelCase")]` deserialization.
        if let Some(arg) = self.parsed_type_argument.as_ref() {
            let inner: &verter_type_expr::TypeExpr = arg.as_ref();
            s.serialize_field("parsedTypeArgument", inner)?;
        }
        if let Some(scope) = self.parsed_type_argument_scope.as_ref() {
            s.serialize_field("parsedTypeArgumentScope", scope)?;
        }
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnalyzedMacro {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            kind: AnalyzedMacroKind,
            is_type_based: bool,
            type_references: Vec<String>,
            binding_name: Option<String>,
            #[serde(default)]
            model_name: Option<String>,
            #[serde(default)]
            has_inherit_attrs_false: bool,
            #[serde(default)]
            prop_fields: Vec<AnalyzedPropField>,
            #[serde(default)]
            emit_fields: Vec<AnalyzedEmitField>,
            #[serde(default)]
            slot_fields: Vec<AnalyzedSlotField>,
            #[serde(default)]
            default_keys: Vec<String>,
            #[serde(default)]
            default_values: Vec<AnalyzedDefaultValue>,
            #[serde(default)]
            expose_fields: Vec<AnalyzedExposeField>,
            #[serde(default)]
            resolved_local_types: Vec<ResolvedLocalType>,
            // D1.2 back-compat: old payloads (no parsedTypeArgument
            // key) deserialize with `None`; manual-serde edits don't
            // pick up `#[serde(default)]` on the outer struct so the
            // attribute lives on the Wire deserialization helper.
            #[serde(default)]
            parsed_type_argument: Option<verter_type_expr::TypeExpr>,
            #[serde(default)]
            parsed_type_argument_scope: Option<TypeExprScope>,
            #[serde(default)]
            span_start: u32,
            #[serde(default)]
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            kind: w.kind,
            is_type_based: w.is_type_based,
            type_references: w.type_references,
            binding_name: w.binding_name,
            model_name: w.model_name,
            has_inherit_attrs_false: w.has_inherit_attrs_false,
            prop_fields: w.prop_fields,
            emit_fields: w.emit_fields,
            slot_fields: w.slot_fields,
            default_keys: w.default_keys,
            default_values: w.default_values,
            expose_fields: w.expose_fields,
            resolved_local_types: w.resolved_local_types,
            parsed_type_argument: w.parsed_type_argument.map(std::sync::Arc::new),
            parsed_type_argument_scope: w.parsed_type_argument_scope,
            span: Span::new(w.span_start, w.span_end),
        })
    }
}

/// Identifies which Vue compiler macro was called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AnalyzedMacroKind {
    DefineProps,
    DefineEmits,
    DefineModel,
    DefineExpose,
    DefineOptions,
    DefineSlots,
    WithDefaults,
}

/// Which imported types are used by which macros.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroTypeDep {
    /// The type name referenced in the macro's type parameter (e.g., `"BadgeProps"`).
    pub type_name: String,
    /// The import source where the type is declared (e.g., `"./types"`).
    pub import_source: String,
    /// Which macro references this type.
    pub macro_kind: AnalyzedMacroKind,
    /// Stable index of the originating macro in the raw snapshot.
    pub macro_index: usize,
    /// Stable identity of the originating macro in the raw snapshot.
    pub macro_span: verter_span::Span,
}

/// Per-export signature for dependency files.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSignature {
    /// Exported name (e.g., `"MyType"`, `"default"`).
    pub name: String,
    /// Content hash of the declaration body. Changes when the declaration's text changes.
    pub declaration_hash: Hash16,
    /// Whether this is a type-only export (`export type` or `export interface`).
    pub is_type: bool,
    /// SFC-absolute byte span of the export's identifier.
    /// Points to the identifier name (e.g., `foo` in `export function foo()`),
    /// or `default` keyword for anonymous default exports.
    pub span: Span,
    /// Source module for re-exports (e.g., `"./Popup.vue"`). None for local exports.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reexport_source: Option<String>,
    /// Original name in source module (e.g., `"default"`). None for local exports.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reexport_local: Option<String>,
    /// Byte span of the local name in aliased re-exports (e.g., `foo` in `export { foo as bar }`).
    /// `None` when local == exported (no alias) or for local/declaration exports.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub local_span: Option<Span>,
}

fn serialize_analysis_flags<S: serde::Serializer>(
    flags: &AnalysisFlags,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_u32(flags.bits())
}

fn deserialize_analysis_flags<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<AnalysisFlags, D::Error> {
    let bits = <u32 as serde::Deserialize>::deserialize(deserializer)?;
    Ok(AnalysisFlags::from_bits_truncate(bits))
}

/// Lightweight import info returned to callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSourceInfo {
    /// The import specifier (e.g., "./types", "vue").
    pub source: String,
    /// Whether this is a type-only import.
    pub is_type_only: bool,
    /// Names imported from this source.
    pub bindings: Vec<String>,
}

// ── Non-SFC Deep Analysis ──

/// Analyzed exported function from a non-SFC file.
/// Used for composable return type analysis and function parameter info.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedExportedFunction {
    /// Function name.
    pub name: String,
    /// Whether this is a default export.
    pub is_default: bool,
    /// Parameters (name + optional type annotation string).
    pub params: Vec<FunctionParam>,
    /// TypeScript return type annotation extracted directly from the AST.
    /// e.g., `"Ref<number>"`, `"{ count: Ref<number>, increment: () => void }"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_type_annotation: Option<String>,
    /// Inferred return reactivity from body analysis (heuristic).
    pub return_reactivity: ReturnReactivity,
    /// Whether this is async.
    pub is_async: bool,
    /// Composable info (None if not a composable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composable: Option<ComposableInfo>,
}

/// First-class composable info (functions following the `useXxx` convention).
/// Composables are Vue's primary code reuse pattern and warrant dedicated tracking.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposableInfo {
    /// Composable name (e.g., `"useCounter"`, `"useFetch"`).
    pub name: String,
    /// Vue lifecycle hooks called inside (onMounted, onUnmounted, etc.).
    pub lifecycle_hooks: Vec<VueApiClassification>,
    /// Whether it calls `provide()`.
    pub has_provide: bool,
    /// Whether it calls `inject()`.
    pub has_inject: bool,
    /// Whether it calls `watch`/`watchEffect`.
    pub has_watchers: bool,
    /// Reactive state created inside (ref, reactive, computed).
    pub internal_reactive_state: Vec<(String, ReactivityKind)>,
    /// What it returns (structured).
    pub return_shape: ComposableReturn,
}

/// What a composable returns.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ComposableReturn {
    /// Returns a single ref/reactive value.
    Single(ReactivityKind),
    /// Returns a destructurable object: `{ count, increment, reset }`.
    Object(Vec<ComposableReturnField>),
    /// Returns a tuple-like array: `[value, setValue]`.
    Tuple(Vec<ReactivityKind>),
    /// Cannot determine.
    Unknown,
}

/// A field in a composable return object.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposableReturnField {
    /// Field name.
    pub name: String,
    /// Reactivity classification.
    pub reactivity: ReactivityKind,
    /// Whether this field is a function (method).
    pub is_function: bool,
}

/// A function parameter with optional type annotation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionParam {
    /// Parameter name.
    pub name: String,
    /// TypeScript type annotation extracted directly from the AST.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
    /// Whether this parameter is optional.
    pub is_optional: bool,
    /// Whether this parameter has a default value.
    pub has_default: bool,
    /// Script-relative byte span of the parameter name identifier.
    #[serde(default)]
    pub span: Span,
}

/// What a function returns in terms of reactivity.
///
/// Determined via two levels:
/// 1. **AST-level** (immediate, from OXC): explicit TS return type annotations
/// 2. **Body-level** (heuristic): walk return statements to detect patterns
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReturnReactivity {
    /// Returns a ref-like value (detected via return type annotation or `return ref(...)`).
    Ref,
    /// Returns a reactive object (detected via `return reactive(...)`).
    Reactive,
    /// Returns a plain object with known reactive properties.
    ObjectWithReactiveFields(Vec<(String, ReactivityKind)>),
    /// Returns a plain non-reactive value.
    Plain,
    /// Cannot determine (complex control flow, dynamic returns).
    #[default]
    Unknown,
}

// ── Type Enhancement Placeholders ──

/// TODO(type-provider): Script-level type enhancements.
/// Populated by external type providers (TS language service, TSGO, etc.).
/// Enhances `ScriptAnalysisSnapshot` with resolved type info.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptTypeEnhancements {
    /// Resolved return types for functions (keyed by function name).
    pub function_return_types: rustc_hash::FxHashMap<String, ResolvedTypeInfo>,
    /// Resolved types for bindings (keyed by binding name).
    pub binding_resolved_types: rustc_hash::FxHashMap<String, ResolvedTypeInfo>,
    /// Generic type parameter resolutions.
    pub generic_resolutions: rustc_hash::FxHashMap<String, Vec<ResolvedTypeInfo>>,
}

/// TODO(type-provider): Resolved type from TSGO or other type checker.
/// Initially empty — filled in by the type checker integration layer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTypeInfo {
    /// e.g., `"Ref<number>"`, `"string"`, `"() => void"`.
    pub type_string: String,
    /// Whether the type includes `null` or `undefined`.
    pub is_nullable: bool,
    /// Whether the type is `readonly`.
    pub is_readonly: bool,
    /// For object types: member name → type string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<(String, String)>>,
    /// When the resolved type is a finite string literal union, list its values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string_literal_union: Option<Vec<String>>,
}

/// How a binding is used at a particular site in script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptUsageKind {
    /// Value read: `x` in an expression position.
    Read,
    /// Assignment target: `x = 1`.
    Write,
    /// Read-modify-write: `x += 1`, `x++`.
    ReadWrite,
    /// Call expression callee: `x()`.
    Call,
    /// Member access: `x.foo`, `x[0]`.
    MemberAccess,
    /// Typeof operand: `typeof x`.
    Typeof,
    /// Destructure source: `{ a } = x`.
    Destructure,
}

/// A single occurrence of a top-level binding in the script block body.
///
/// Tracks where each binding is referenced (excluding its own declaration).
/// Populated as a second pass gated by `AnalysisScope::SCRIPT_USAGES`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptBindingOccurrence {
    /// The binding name referenced.
    pub name: String,
    /// SFC-absolute byte span of the reference.
    pub span: Span,
    /// How the binding is used at this site.
    pub usage_kind: ScriptUsageKind,
}

/// A CSS variable manipulation via DOM style APIs in script.
///
/// Tracks calls like `el.style.setProperty('--color', val)`,
/// `getComputedStyle(el).getPropertyValue('--color')`, and
/// `el.style.removeProperty('--color')`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssVarManipulation {
    /// The kind of manipulation (set, get, remove).
    pub kind: CssVarManipulationKind,
    /// The CSS variable name (e.g., "--color").
    pub var_name: String,
    /// The value expression for setProperty (e.g., "val", "'red'").
    pub value_expr: Option<String>,
    /// SFC-absolute byte span of the entire call expression.
    pub span: verter_span::Span,
}

impl serde::Serialize for CssVarManipulation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 4 + usize::from(self.value_expr.is_some());
        let mut s = serializer.serialize_struct("CssVarManipulation", count)?;
        s.serialize_field("kind", &self.kind)?;
        s.serialize_field("varName", &self.var_name)?;
        if self.value_expr.is_some() {
            s.serialize_field("valueExpr", &self.value_expr)?;
        }
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for CssVarManipulation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            kind: CssVarManipulationKind,
            var_name: String,
            #[serde(default)]
            value_expr: Option<String>,
            span_start: u32,
            span_end: u32,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            kind: w.kind,
            var_name: w.var_name,
            value_expr: w.value_expr,
            span: verter_span::Span::new(w.span_start, w.span_end),
        })
    }
}

/// Discriminant for CSS variable DOM manipulation APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CssVarManipulationKind {
    /// `el.style.setProperty('--x', val)`
    SetProperty,
    /// `getComputedStyle(el).getPropertyValue('--x')`
    GetPropertyValue,
    /// `el.style.removeProperty('--x')`
    RemoveProperty,
}

// =============================================================================
// Store / State Management
// =============================================================================

/// Classification of store/state management APIs (Pinia, Vuex, convention-based).
///
/// Separate from `VueApiClassification` since stores are third-party, not Vue core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StoreApiClassification {
    // Pinia
    PiniaDefineStore,
    PiniaStoreToRefs,
    PiniaMapState,
    PiniaMapGetters,
    PiniaMapActions,
    PiniaMapWritableState,
    PiniaCreatePinia,
    // Vuex
    VuexCreateStore,
    VuexUseStore,
    VuexMapState,
    VuexMapGetters,
    VuexMapMutations,
    VuexMapActions,
    /// Convention-based: `useXxxStore` from `*/store*` paths.
    StoreComposable,
}

/// A store usage site in a script block.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoreUsage {
    /// The local binding name (e.g., `store`, `userStore`).
    pub binding_name: String,
    /// The callee function name (e.g., `useUserStore`, `defineStore`).
    pub callee: String,
    /// The import source (e.g., `"pinia"`, `"@/stores/user"`).
    pub import_source: String,
    /// How this store API is classified.
    pub store_api: StoreApiClassification,
    /// SFC-absolute byte span of the call expression.
    pub span: Span,
    /// Whether `storeToRefs()` was applied to this binding.
    pub has_store_to_refs: bool,
    /// Property names destructured from the store (e.g., `["count", "name"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destructured_props: Vec<String>,
    /// Whether the store was destructured without `storeToRefs()` (reactivity loss).
    pub destructured_without_store_to_refs: bool,
}

/// A store definition site (e.g., `defineStore('user', { ... })`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoreDefinition {
    /// The store ID (first string arg to `defineStore`, e.g., `"user"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_id: Option<String>,
    /// The export name (e.g., `useUserStore`).
    pub export_name: String,
    /// Which store API created this definition.
    pub store_api: StoreApiClassification,
    /// State property names extracted from the options object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_properties: Vec<String>,
    /// Getter names extracted from the options object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub getters: Vec<String>,
    /// Action names extracted from the options object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    /// Other stores called inside this store definition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub store_dependencies: Vec<String>,
    /// SFC-absolute byte span of the definition.
    pub span: Span,
    /// Canonical file ID where this store is defined (populated by host).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
}

impl CssVarManipulationKind {
    /// Get the JavaScript method name.
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::SetProperty => "setProperty",
            Self::GetPropertyValue => "getPropertyValue",
            Self::RemoveProperty => "removeProperty",
        }
    }
}

#[cfg(test)]
mod analyzed_macro_serde_tests {
    //! Plan Step 1 / D1.2 + sub-task 1.4 — serialization integrity for
    //! `AnalyzedMacro::parsed_type_argument`. The struct uses manual
    //! `Serialize` / `Deserialize` impls (types.rs:1276 / 1322), so a
    //! string-typo in the camelCase field name would silently drop the
    //! field on the wire. These tests catch that.
    //!
    //! FAIL-FIRST contract: writing the field literal `parsedTypeArgument`
    //! into the manual serializer is required for `serializes_field_name_exactly`
    //! to pass. The back-compat test verifies old-shape payloads (no
    //! `parsedTypeArgument` key) deserialize with `parsed_type_argument: None`
    //! — discriminating because if the deserializer's `Wire` struct were
    //! to drop `#[serde(default)]` on the field, the test would fail with
    //! "missing field" error.
    use super::{AnalyzedMacro, AnalyzedMacroKind};
    use std::sync::Arc;
    use verter_span::Span;
    use verter_type_expr::TypeExpr;

    fn empty_macro() -> AnalyzedMacro {
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 0),
        }
    }

    #[test]
    fn serializes_parsed_type_argument_field_name_exactly() {
        let mut m = empty_macro();
        m.parsed_type_argument = Some(Arc::new(TypeExpr::Ref {
            name: Arc::from("Sentinel"),
            type_arguments: Arc::from(Vec::<TypeExpr>::new()),
        }));
        let json = serde_json::to_string(&m).unwrap();
        // Field name appears EXACTLY (catches typo in manual serialize_field).
        assert!(
            json.contains("\"parsedTypeArgument\":"),
            "manual Serialize did not emit the parsedTypeArgument field; \
             check serialize_field call in AnalyzedMacro::serialize. JSON: {json}"
        );
        // Sentinel value also appears (catches the value being silently dropped).
        assert!(
            json.contains("\"Sentinel\""),
            "parsed_type_argument value not in serialized output; JSON: {json}"
        );

        // Round-trip integrity.
        let roundtripped: AnalyzedMacro = serde_json::from_str(&json).unwrap();
        assert_eq!(m.parsed_type_argument, roundtripped.parsed_type_argument);
    }

    #[test]
    fn serializes_none_omits_field() {
        let m = empty_macro();
        assert!(m.parsed_type_argument.is_none());
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            !json.contains("parsedTypeArgument"),
            "None values should be omitted from output; JSON: {json}"
        );
    }

    #[test]
    fn deserializes_old_payload_without_parsed_type_argument_field() {
        // Construct an old-shape payload (no parsedTypeArgument key).
        // Mirrors what existing serialized AnalyzedMacro JSON on disk
        // would have looked like before D1.2 added the field.
        let old_json = r#"{
            "kind": "DefineProps",
            "isTypeBased": true,
            "typeReferences": [],
            "bindingName": null,
            "hasInheritAttrsFalse": false,
            "spanStart": 0,
            "spanEnd": 0
        }"#;
        let parsed: AnalyzedMacro = serde_json::from_str(old_json).unwrap();
        assert_eq!(
            parsed.parsed_type_argument, None,
            "old-shape payload (no parsedTypeArgument key) must \
             deserialize with parsed_type_argument: None"
        );
    }

    #[test]
    fn roundtrip_preserves_arc_typeexpr_payload_structure() {
        let mut m = empty_macro();
        // Construct a non-trivial TypeExpr (Ref with nested args) so the
        // round-trip test exercises the non-empty serialization path.
        m.parsed_type_argument = Some(Arc::new(TypeExpr::Ref {
            name: Arc::from("Props"),
            type_arguments: Arc::from(vec![TypeExpr::Ref {
                name: Arc::from("T"),
                type_arguments: Arc::from(Vec::<TypeExpr>::new()),
            }]),
        }));
        let json = serde_json::to_string(&m).unwrap();
        let back: AnalyzedMacro = serde_json::from_str(&json).unwrap();
        // Arc identity isn't preserved across deserialization (fresh
        // allocation), but the structural equality is.
        assert_eq!(m.parsed_type_argument, back.parsed_type_argument);
    }
}
