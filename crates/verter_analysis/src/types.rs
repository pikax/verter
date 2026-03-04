use sha2::{Digest, Sha256};
use verter_span::Span;

/// Truncated SHA-256 hash (first 16 bytes). Used for content-based change detection.
pub type Hash16 = [u8; 16];

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

    /// SFC-absolute byte offset of the first top-level `await` expression (if any).
    /// Used by lint rules to detect lifecycle hooks/watchers called after await.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_await_offset: Option<u32>,

    /// TODO(type-provider): Enhanced type info populated by TSGO when connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_enhancements: Option<ScriptTypeEnhancements>,
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
    /// Populated by verter_host after path resolution for cross-file go-to-definition.
    pub resolved_canonical_id: Option<String>,
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
    /// Whether this specifier is type-only (`import { type Foo }`).
    pub is_type_only: bool,
    /// Vue API classification if the import source is 'vue'.
    pub vue_api: Option<VueApiClassification>,
    /// SFC-absolute byte span of the specifier name.
    pub span: Span,
}

impl serde::Serialize for AnalyzedImportBinding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AnalyzedImportBinding", 5)?;
        s.serialize_field("name", &self.name)?;
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
    /// Whether the first function argument is an async function/arrow.
    /// Used by `no-async-in-computed` rule.
    pub is_async_callback: bool,
}

impl serde::Serialize for VueApiCallSite {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 3 + usize::from(self.arg_value.is_some()) + usize::from(self.is_async_callback);
        let mut s = serializer.serialize_struct("VueApiCallSite", count)?;
        s.serialize_field("api", &self.api)?;
        s.serialize_field("spanStart", &self.span.start)?;
        s.serialize_field("spanEnd", &self.span.end)?;
        if self.arg_value.is_some() {
            s.serialize_field("argValue", &self.arg_value)?;
        }
        if self.is_async_callback {
            s.serialize_field("isAsyncCallback", &self.is_async_callback)?;
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
            is_async_callback: bool,
        }
        let w = Wire::deserialize(deserializer)?;
        Ok(Self {
            api: w.api,
            span: Span::new(w.span_start, w.span_end),
            arg_value: w.arg_value,
            is_async_callback: w.is_async_callback,
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
    pub parsed: Option<crate::style::StructuredSelector>,
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
            parsed: Option<crate::style::StructuredSelector>,
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
}

impl serde::Serialize for AnalyzedBinding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 7 + usize::from(self.type_annotation.is_some());
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
    /// SFC-absolute byte span of the macro call.
    pub span: Span,
}

impl serde::Serialize for AnalyzedMacro {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let count = 7 + usize::from(self.model_name.is_some());
        let mut s = serializer.serialize_struct("AnalyzedMacro", count)?;
        s.serialize_field("kind", &self.kind)?;
        s.serialize_field("isTypeBased", &self.is_type_based)?;
        s.serialize_field("typeReferences", &self.type_references)?;
        s.serialize_field("bindingName", &self.binding_name)?;
        if self.model_name.is_some() {
            s.serialize_field("modelName", &self.model_name)?;
        }
        s.serialize_field("hasInheritAttrsFalse", &self.has_inherit_attrs_false)?;
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
