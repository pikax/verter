use sha2::{Digest, Sha256};

/// Truncated SHA-256 hash (first 16 bytes). Used for content-based change detection.
pub type Hash16 = [u8; 16];

/// Compute a truncated SHA-256 hash (first 16 bytes).
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
    }
}

/// Comprehensive script analysis captured during SFC parsing.
/// Powers dependency tracking, type-aware linting, and codegen optimization.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptAnalysisSnapshot {
    /// All import declarations found in the script block(s).
    pub imports: Vec<AnalyzedImport>,

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

    /// TODO(type-provider): Enhanced type info populated by TSGO when connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_enhancements: Option<ScriptTypeEnhancements>,
}

/// A single import declaration extracted from a script block.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedImport {
    /// The import source specifier (e.g., "./types", "vue", "@/utils").
    pub source: String,
    /// Whether this is `import type { ... }` (declaration-level type-only).
    pub is_type_only: bool,
    /// Individual bindings imported.
    pub bindings: Vec<AnalyzedImportBinding>,
    /// Byte offset of import declaration start in the script content.
    #[serde(default)]
    pub span_start: u32,
    /// Byte offset of import declaration end in the script content.
    #[serde(default)]
    pub span_end: u32,
    /// Canonical file ID resolved by the host (None during standalone analysis).
    /// Populated by verter_host after path resolution for cross-file go-to-definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_canonical_id: Option<String>,
}

/// A single specifier within an import declaration (e.g., `Foo` in `import { Foo } from "bar"`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedImportBinding {
    /// Local binding name as used in the script.
    pub name: String,
    /// Whether this specifier is type-only (`import { type Foo }`).
    pub is_type_only: bool,
    /// Vue API classification if the import source is 'vue'.
    pub vue_api: Option<VueApiClassification>,
    /// Byte offset of specifier name start in the script content.
    #[serde(default)]
    pub span_start: u32,
    /// Byte offset of specifier name end in the script content.
    #[serde(default)]
    pub span_end: u32,
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedBinding {
    /// Binding name as declared in source.
    pub name: String,
    /// Declaration kind (`const`, `let`, `function`, etc.).
    pub kind: AnalyzedBindingKind,
    /// Whether the binding holds reactive state (e.g., initialized via `ref()` or `reactive()`).
    pub is_reactive: bool,
    /// Granular reactivity classification (replaces `is_reactive` semantically).
    #[serde(default)]
    pub reactivity_kind: ReactivityKind,
    /// TypeScript type annotation from the AST (e.g., `"Ref<number>"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_annotation: Option<String>,
    /// What expression created this binding, if classifiable.
    pub initializer: Option<BindingInitializer>,
    /// Byte offset of binding name start in the script content.
    #[serde(default)]
    pub span_start: u32,
    /// Byte offset of binding name end in the script content.
    #[serde(default)]
    pub span_end: u32,
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

/// A Vue compiler macro call found in `<script setup>` (e.g., `defineProps`, `defineEmits`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedMacro {
    /// Which macro was called.
    pub kind: AnalyzedMacroKind,
    /// Whether the macro uses type params `<...>`.
    pub is_type_based: bool,
    /// Type names referenced in the type params.
    pub type_references: Vec<String>,
    /// The binding name if `const X = defineProps<...>()`.
    pub binding_name: Option<String>,
    /// Byte offset of macro call start in the script content.
    #[serde(default)]
    pub span_start: u32,
    /// Byte offset of macro call end in the script content.
    #[serde(default)]
    pub span_end: u32,
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

// ── Phase 1c: Non-SFC Deep Analysis ──

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

// ── Phase 2b: Type Enhancement Placeholders ──

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
}
