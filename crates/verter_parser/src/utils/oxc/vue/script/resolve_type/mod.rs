//! Cross-file type resolution for Vue compiler macros.
//!
//! Resolves TypeScript type annotations used as type parameters in Vue macros
//! (`defineProps<T>()`, `defineEmits<T>()`, `defineSlots<T>()`) into structured
//! [`ResolvedElements`] that drive runtime props/emits code generation.
//!
//! When `T` is defined inline (e.g. `defineProps<{ title: string }>()`), resolution
//! stays local. When `T` extends or references types from other files, the host
//! must pre-resolve those external types and pass them in via
//! [`VerterCompileOptions::external_types`](crate::VerterCompileOptions). The
//! resolved data is merged into [`TypeResolutionContext::companion_types`] so that
//! lookups for imported type names can fall back to pre-resolved definitions.
//!
//! Based on Vue's `resolveType.ts` implementation.

#![allow(dead_code)]

use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use oxc_ast::ast::*;
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::common::Span;

fn component_meta_core_trace_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn component_meta_core_trace_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    // The legacy `VERTER_COMPONENT_META_TRACE*` env var names are
    // retired. The parser's core-trace keeps its debug aid but reads
    // the narrower `VERTER_PARSER_CORE_TRACE_PATH` so the session-crate
    // env var surface stays clean.
    PATH.get_or_init(|| std::env::var_os("VERTER_PARSER_CORE_TRACE_PATH").map(PathBuf::from))
        .as_ref()
}

fn component_meta_core_trace_enabled() -> bool {
    component_meta_core_trace_path().is_some()
}

fn component_meta_core_trace_event(name: &'static str, detail: impl AsRef<str>) {
    let Some(path) = component_meta_core_trace_path() else {
        return;
    };
    let _lock = component_meta_core_trace_lock().lock();
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "core_event name={} {}", name, detail.as_ref());
    }
}

/// Find the first TSType from a call expression's type parameters at the given span.
///
/// This walks the program AST to find a CallExpression whose span matches,
/// then extracts the first type parameter.
pub fn find_macro_type_param<'a>(
    program: &'a Program<'a>,
    macro_span: Span,
) -> Option<&'a TSType<'a>> {
    for stmt in &program.body {
        if let Some(ts_type) = find_call_type_param_in_statement(stmt, macro_span) {
            return Some(ts_type);
        }
    }
    None
}

fn find_call_type_param_in_statement<'a>(
    stmt: &'a Statement<'a>,
    target: Span,
) -> Option<&'a TSType<'a>> {
    match stmt {
        Statement::ExpressionStatement(expr_stmt) => {
            find_call_type_param_in_expr(&expr_stmt.expression, target)
        }
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                if let Some(init) = &decl.init {
                    if let Some(ts_type) = find_call_type_param_in_expr(init, target) {
                        return Some(ts_type);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn find_call_type_param_in_expr<'a>(
    expr: &'a Expression<'a>,
    target: Span,
) -> Option<&'a TSType<'a>> {
    match expr {
        Expression::CallExpression(call) => {
            // Check if this is our target call
            if call.span.start == target.start && call.span.end == target.end {
                if let Some(type_args) = &call.type_arguments {
                    return type_args.params.first();
                }
            }
            // Check nested calls in arguments
            for arg in &call.arguments {
                if let Argument::SpreadElement(spread) = arg {
                    if let Some(ts_type) = find_call_type_param_in_expr(&spread.argument, target) {
                        return Some(ts_type);
                    }
                } else if let Some(expr) = arg.as_expression() {
                    if let Some(ts_type) = find_call_type_param_in_expr(expr, target) {
                        return Some(ts_type);
                    }
                }
            }
            // Check callee if it's an expression
            find_call_type_param_in_expr(&call.callee, target)
        }
        Expression::ParenthesizedExpression(paren) => {
            find_call_type_param_in_expr(&paren.expression, target)
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                if let Some(ts_type) = find_call_type_param_in_expr(expr, target) {
                    return Some(ts_type);
                }
            }
            None
        }
        Expression::ConditionalExpression(cond) => {
            find_call_type_param_in_expr(&cond.consequent, target)
                .or_else(|| find_call_type_param_in_expr(&cond.alternate, target))
        }
        _ => None,
    }
}

/// Runtime type that can be inferred from TypeScript types.
/// These correspond to JavaScript constructor functions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuntimeType {
    String,
    Number,
    Boolean,
    Object,
    Array,
    Function,
    Symbol,
    /// Built-in types like Date, Map, Set, etc.
    BuiltIn(String),
    /// Null type
    Null,
    /// Type that couldn't be resolved
    Unknown,
}

impl RuntimeType {
    /// Convert to the JavaScript constructor name
    pub fn as_str(&self) -> &str {
        match self {
            RuntimeType::String => "String",
            RuntimeType::Number => "Number",
            RuntimeType::Boolean => "Boolean",
            RuntimeType::Object => "Object",
            RuntimeType::Array => "Array",
            RuntimeType::Function => "Function",
            RuntimeType::Symbol => "Symbol",
            RuntimeType::BuiltIn(name) => name,
            RuntimeType::Null => "null",
            RuntimeType::Unknown => "null",
        }
    }
}

/// Format runtime types as a Vue prop type value.
/// Single type: `String`
/// Multiple types: `[String, Number]`
pub fn format_runtime_types(types: &[RuntimeType]) -> String {
    // Filter out Unknown types
    let valid_types: Vec<_> = types
        .iter()
        .filter(|t| !matches!(t, RuntimeType::Unknown))
        .collect();

    if valid_types.is_empty() {
        return "null".to_string();
    }

    if valid_types.len() == 1 {
        return valid_types[0].as_str().to_string();
    }

    // Multiple types: [Type1, Type2]
    let type_strs: Vec<_> = valid_types.iter().map(|t| t.as_str()).collect();
    format!("[{}]", type_strs.join(", "))
}

/// Visibility of a resolved class/interface member.
///
/// Used by `meta_resolve.rs` to filter private/protected members from
/// component props. The raw resolver preserves all members with their
/// visibility tags — consumers decide whether to filter.
///
/// **Note**: Non-meta codegen paths (template compiler) do not currently
/// filter by visibility. If `defineProps<MyClass>()` is used in a runtime
/// codegen path, private members may leak as runtime props.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedMemberVisibility {
    Public,
    Protected,
    Private,
}

impl ResolvedMemberVisibility {
    pub const fn is_public(self) -> bool {
        matches!(self, Self::Public)
    }
}

/// A resolved property from a type literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProp {
    /// Span of the entire property signature (from key to end of type)
    pub span: Span,
    /// Span of the property key (name) in the source
    pub key: Span,
    /// Pre-resolved key name (set for external/cross-file types where spans
    /// reference a different source than the consuming SFC).
    pub key_name: Option<String>,
    /// Whether the property is optional (has `?`)
    pub optional: bool,
    /// Inferred runtime types for this property
    pub types: Vec<RuntimeType>,
    /// Visibility of the member when it originated from a class member.
    /// Interface/type-literal members are always public.
    pub visibility: ResolvedMemberVisibility,
    /// Span of the type annotation (excluding the `: ` prefix) in the source.
    /// Set for property signatures with explicit type annotations; `None` for
    /// method signatures and companion-script props.
    pub type_span: Option<Span>,
    /// Pre-resolved type annotation text for cross-file types.
    /// Set by `finalize_external_resolution` when spans reference an external
    /// source file. Consumers prefer this over extracting from `type_span`.
    pub type_text: Option<String>,
    /// Whether this span points into the current SFC source and can be used
    /// directly for local source maps.
    pub map_local: bool,
    /// Whether spans on this prop are already SFC-absolute.
    pub span_is_absolute: bool,
}

/// A resolved emit event from defineEmits type parameter.
/// Supports both call signature style `{ (e: 'change', id: number): void }`
/// and shorthand style `{ change: [id: number] }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedEmitSignature {
    /// Call signature payload params after the event name parameter.
    /// Empty string means the event carries no extra payload.
    Call { params_text: String },
    /// Shorthand tuple payload, including the surrounding `[...]`.
    Tuple { tuple_text: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEmit {
    /// Span of the entire emit signature
    pub span: Span,
    /// The event name (extracted from first parameter literal or property key)
    pub name: String,
    /// Span of the event name in source (if available, for string literal params)
    pub name_span: Option<Span>,
    /// The resolved payload signature, preserved as text so consumers can
    /// inline exact handler / `$emit` types even for cross-file imports.
    pub signature: ResolvedEmitSignature,
    /// Whether this span points into the current SFC source and can be used
    /// directly for local source maps.
    pub map_local: bool,
    /// Whether spans on this emit are already SFC-absolute.
    pub span_is_absolute: bool,
}

/// Resolution surface that a `BlockedType` applies to.
/// Controls WHEN the block is applied based on the macro/context being resolved.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BlockedTypeSurface {
    /// Block when resolving `defineSlots` types
    DefineSlots,
    /// Block when resolving `defineProps` types
    DefineProps,
    /// Block when resolving `defineEmits` types
    DefineEmits,
    /// Block when resolving public component surface types
    Public,
    /// Block when resolving root props for fallthrough inheritance
    RootProps,
}

/// A type that should be skipped during resolution expansion.
/// When the resolver encounters a reference to a blocked type, it returns empty
/// `ResolvedElements` rather than expanding the type's members.
///
/// Three dimensions:
/// - `name`: the type name to block (e.g., "VNode")
/// - `import_source`: the package/module the type must originate from (e.g., "vue")
/// - `surface`: which macro/context this block applies to (e.g., DefineSlots)
#[derive(Debug, Clone)]
pub struct BlockedType {
    /// Type name to block (e.g., "VNode").
    pub name: String,
    /// Package/module qualifier. Only blocks the type when it was imported from
    /// this specific package (e.g., "vue"). Uses the resolved import source from
    /// `companion_origins`. When None, blocks by name regardless of origin.
    pub import_source: Option<String>,
    /// Which resolution surface this block applies to. When empty, blocks for
    /// all surfaces. When non-empty, only blocks for the listed surfaces.
    pub surfaces: Vec<BlockedTypeSurface>,
}

/// Result of resolving type elements from a type annotation.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResolvedElements {
    /// Resolved properties from the type
    pub props: Vec<ResolvedProp>,
    /// Resolved emit events from call signatures or shorthand properties
    pub emits: Vec<ResolvedEmit>,
    /// Whether this type has call signatures (is callable)
    pub has_call_signature: bool,
    /// Runtime type inferred from the root type annotation being resolved.
    /// Used to distinguish valid empty object-like macro types from invalid
    /// primitives when cross-file resolution returns no concrete members.
    pub root_runtime_types: Vec<RuntimeType>,
}

impl ResolvedElements {
    /// Deduplicate props by key name (first occurrence wins).
    /// Matches Vue's `mergeElements()` behavior for union/intersection types.
    fn dedup_props(&mut self) {
        let mut seen = rustc_hash::FxHashSet::default();
        self.props.retain(|prop| {
            if let Some(ref name) = prop.key_name {
                seen.insert(name.clone())
            } else {
                // No key_name — always keep (shouldn't happen after resolution)
                true
            }
        });
    }
}

// =============================================================================
// Type Resolution Context (for resolving type references)
// =============================================================================

/// Location/origin of a diagnostic - identifies which plugin/module created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLocation {
    /// Type resolution (resolve_type.rs)
    TypeResolution,
    /// Props macro processing (defineProps, withDefaults)
    PropsMacro,
    /// Emits macro processing (defineEmits)
    EmitsMacro,
    /// Model macro processing (defineModel)
    ModelMacro,
    /// Options macro processing (defineOptions)
    OptionsMacro,
    /// Expose macro processing (defineExpose)
    ExposeMacro,
    /// Slots macro processing (defineSlots)
    SlotsMacro,
    /// Script setup processing
    ScriptSetup,
    /// Script options API processing
    ScriptOptions,
    /// Template processing
    Template,
    /// Style processing
    Style,
}

impl DiagnosticLocation {
    /// Get a short name for this location
    pub const fn name(&self) -> &'static str {
        match self {
            Self::TypeResolution => "type-resolution",
            Self::PropsMacro => "props-macro",
            Self::EmitsMacro => "emits-macro",
            Self::ModelMacro => "model-macro",
            Self::OptionsMacro => "options-macro",
            Self::ExposeMacro => "expose-macro",
            Self::SlotsMacro => "slots-macro",
            Self::ScriptSetup => "script-setup",
            Self::ScriptOptions => "script-options",
            Self::Template => "template",
            Self::Style => "style",
        }
    }
}

/// Diagnostic kind for resolution errors.
/// Uses enum-based messages to avoid String allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionDiagnosticKind {
    /// Type reference could not be resolved
    UnresolvedTypeReference,
    /// Empty type literal (no props)
    EmptyTypeLiteral,
    /// Intersection resulted in no usable types
    EmptyIntersection,
}

impl ResolutionDiagnosticKind {
    /// Get static message for this diagnostic kind - no allocation
    pub const fn message(&self) -> &'static str {
        match self {
            Self::UnresolvedTypeReference => "Could not resolve type reference",
            Self::EmptyTypeLiteral => "Empty type literal has no properties",
            Self::EmptyIntersection => "Intersection type resolved to no properties",
        }
    }
}

/// Diagnostic with enum-based message (no String allocations)
#[derive(Debug, Clone)]
pub struct ResolutionDiagnostic {
    /// Location in source code
    pub span: Span,
    /// What kind of diagnostic this is
    pub kind: ResolutionDiagnosticKind,
    /// Which plugin/module created this diagnostic
    pub location: DiagnosticLocation,
}

/// Context for type resolution with available type information.
/// Uses Span-based lookups with &[u8] comparisons to avoid String allocations.
///
/// Two lifetime parameters:
/// - `'ctx`: borrow lifetime of the program reference (how long we hold references into the AST)
/// - `'a`: arena allocator lifetime (the AST node types are `TSType<'a>`, etc.)
#[derive(Debug, Clone)]
pub struct TypeResolutionContext<'ctx, 'a: 'ctx> {
    /// Source bytes for name comparisons
    pub source: &'ctx [u8],
    /// Local type alias declarations keyed by symbol bytes.
    pub type_aliases: FxHashMap<
        Box<[u8]>,
        (
            &'ctx TSType<'a>,
            Option<&'ctx TSTypeParameterDeclaration<'a>>,
        ),
    >,
    /// Local interface declarations keyed by symbol bytes.
    pub interfaces: FxHashMap<Box<[u8]>, InterfaceResolutionEntry<'ctx, 'a>>,
    /// Local class declarations keyed by symbol bytes.
    /// Classes resolve to their instance-side shape in type position.
    pub classes: FxHashMap<Box<[u8]>, &'ctx Class<'a>>,
    /// Generic type parameters with constraints: (name_span, constraint_type)
    pub type_params: Vec<(Span, Option<&'ctx TSType<'a>>)>,
    /// Bound generic type parameters for the current instantiation.
    pub type_param_bindings: Vec<(Span, &'ctx TSType<'a>)>,
    /// Stable cache-key representation of `type_param_bindings`.
    type_param_bindings_cache_key: Arc<[ResolvedTypeParamBindingCacheKey]>,
    /// Diagnostics collected during resolution
    pub diagnostics: Vec<ResolutionDiagnostic>,
    /// Pre-resolved types from companion `<script>` block.
    /// Keyed by type name string, value is the resolved elements.
    /// Used when a type reference can't be found in the local context.
    pub companion_types: rustc_hash::FxHashMap<String, ResolvedElements>,
    /// Import origins for companion types. Keyed by type name, value is the
    /// package/module specifier the type was imported from (e.g., "vue").
    /// Used for package-qualified `BlockedType` matching.
    pub companion_origins: rustc_hash::FxHashMap<String, String>,
    /// Types that should NOT be expanded during resolution.
    /// Used per-surface to block expansion of specific types (e.g., VNode for slots).
    pub blocked_types: Vec<BlockedType>,
    /// Current resolution surface, used to filter `blocked_types` by surface.
    /// When None, all blocked types apply regardless of surface.
    pub current_surface: Option<BlockedTypeSurface>,
    /// Stable sorted imported companion names available during this resolution.
    /// Included in named-type cache keys so different companion availability
    /// sets do not reuse an incompatible cached local expansion.
    companion_cache_key: Arc<[Box<[u8]>]>,
    /// Optional debug/trace label for the owning source file.
    trace_label: Option<Arc<str>>,
    /// Injected host-owned cache handle for fully-resolved named local symbols.
    /// `None` for standalone callers (tests, direct parsing); resolution still
    /// succeeds but pays no memoization cost. When `Some`, the adapter closes
    /// over a `(canonical_id, whole_hash)` scoping tuple so cache entries are
    /// keyed against the owning file's content generation. See
    /// [`cache_keys::NamedTypeCache`] for the trait contract.
    named_type_cache: Option<Arc<dyn cache_keys::NamedTypeCache + Send + Sync>>,
}

/// Maximum syntactic descent depth for in-file type resolution. This is a
/// stack-safety rail, NOT a semantic budget — AST descent visits distinct
/// nodes (not the same type repeatedly), and the cap bounds on the input's
/// syntactic depth rather than on reusable semantic work.
///
/// `PARSER_SYNTACTIC_DEPTH_LIMIT = 256` is documented as stack-safety
/// rather than a semantic bound. The 256 limit matches TypeScript's
/// own syntactic depth tolerance and avoids false triggers on deeply
/// nested but legitimate generic chains (e.g. library-grade heavy
/// conditional / indexed-access stacks).
pub const PARSER_SYNTACTIC_DEPTH_LIMIT: u16 = 256;

/// Structured failure shape emitted by the parser's syntactic depth
/// guard. Justified as syntactic stack-safety — not a semantic
/// budget — so the record captures the exact depth at which
/// the guard refused entry (`actual`), the configured cap (`limit`),
/// and a short call-site description (`context`) for diagnostics.
///
/// The record is thread-local by design: the guard lives per thread
/// (matching `RESOLUTION_DEPTH`), and the cap-trip is an observable
/// event that the parser's callers can query after a resolution
/// attempt via [`take_last_resolution_budget_exceeded`]. The record
/// is NOT part of the `Option<ResolvedElements>` result contract —
/// guard refusal still produces `None` on the hot path so callers can
/// bail silently — but tests and diagnostics can consult it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionBudgetExceeded {
    pub limit: u16,
    pub actual: u16,
    pub context: &'static str,
}

thread_local! {
    /// Module-local recursion depth tracker for [`resolve_type_elements_inner_with_ctx`]
    /// and its `_ref` variant. Replaces the previous `Rc<Cell<u16>>` field on
    /// `TypeResolutionContext` (V7 in `.claude/feedback/verification-2026-04-17.md` —
    /// that field made the struct `!Send` and blocked the host-owned cache move).
    ///
    /// A module-local thread-local is the correct scope: depth tracks a single
    /// call chain on one thread, always resets to `0` between top-level entries
    /// via the RAII [`ResolutionDepthGuard`]. It is not cache state.
    static RESOLUTION_DEPTH: std::cell::Cell<u16> = const { std::cell::Cell::new(0) };

    /// Last cap-trip record for the current thread. Updated each time
    /// [`ResolutionDepthGuard::try_enter`] refuses entry; consumers read
    /// via [`take_last_resolution_budget_exceeded`] after a resolution
    /// attempt. Cleared on successful `try_enter` of a fresh top-level
    /// call chain (depth transition 0 → 1) so every top-level resolution
    /// starts with no stale record.
    static LAST_BUDGET_EXCEEDED: std::cell::RefCell<Option<ResolutionBudgetExceeded>> =
        const { std::cell::RefCell::new(None) };
}

/// Observe and clear the current thread's most recent
/// [`ResolutionBudgetExceeded`] record. Returns `None` if no cap trip
/// occurred during the most recent resolution attempt. Exposed for
/// tests and diagnostics.
pub fn take_last_resolution_budget_exceeded() -> Option<ResolutionBudgetExceeded> {
    LAST_BUDGET_EXCEEDED.with(|cell| cell.borrow_mut().take())
}

/// RAII guard that increments [`RESOLUTION_DEPTH`] on construction and decrements
/// it on drop. Returns `None` when the depth would exceed
/// [`PARSER_SYNTACTIC_DEPTH_LIMIT`]; callers bail silently in that case
/// and the structured [`ResolutionBudgetExceeded`] record is stored in
/// [`LAST_BUDGET_EXCEEDED`] so tests and diagnostics can observe the
/// exact cap trip.
struct ResolutionDepthGuard;

impl ResolutionDepthGuard {
    fn try_enter_with_context(context: &'static str) -> Option<Self> {
        RESOLUTION_DEPTH.with(|depth| {
            let current = depth.get();
            if current >= PARSER_SYNTACTIC_DEPTH_LIMIT {
                // Structured failure shape: record the exact cap trip
                // (limit + actual + call-site context) so consumers can
                // observe the cap-trip event without parsing string
                // diagnostics.
                LAST_BUDGET_EXCEEDED.with(|cell| {
                    *cell.borrow_mut() = Some(ResolutionBudgetExceeded {
                        limit: PARSER_SYNTACTIC_DEPTH_LIMIT,
                        actual: current,
                        context,
                    });
                });
                return None;
            }
            if current == 0 {
                // Fresh top-level call chain — clear any stale cap-trip
                // record from a previous resolution so callers always
                // observe at most this chain's trip.
                LAST_BUDGET_EXCEEDED.with(|cell| cell.borrow_mut().take());
            }
            depth.set(current + 1);
            Some(ResolutionDepthGuard)
        })
    }

    fn try_enter() -> Option<Self> {
        Self::try_enter_with_context("resolve_type_elements_inner")
    }

    /// Observe the current syntactic depth (exposed for telemetry /
    /// diagnostics; not part of the hot path).
    #[must_use]
    #[allow(dead_code)]
    fn current_depth() -> u16 {
        RESOLUTION_DEPTH.with(|depth| depth.get())
    }
}

impl Drop for ResolutionDepthGuard {
    fn drop(&mut self) {
        RESOLUTION_DEPTH.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

#[derive(Debug, Clone)]
pub struct InterfaceResolutionEntry<'ctx, 'a: 'ctx> {
    pub members: &'ctx oxc_allocator::Vec<'a, TSSignature<'a>>,
    pub extends: Vec<String>,
    pub heritage: &'ctx [TSInterfaceHeritage<'a>],
    pub type_params: Option<&'ctx TSTypeParameterDeclaration<'a>>,
}

/// Public cache-key module exposing the types and trait that host-owned caches
/// (e.g. `VerterHost::host_owned_resolved_named_types`) use to memoize resolved
/// named types across requests within one workspace generation.
///
/// The underlying key is the exact tuple `(name, surface, base_offset,
/// companion_cache_key, type_param_bindings)` that the in-crate resolver already
/// used for per-context memoization — promoted to a host-shared identity, with
/// additional `(canonical_id, whole_hash)` scoping provided by the adapter.
pub mod cache_keys {
    use super::{BlockedTypeSurface, ResolvedElements};
    use std::sync::Arc;

    /// Cache key for a fully-resolved named local symbol.
    ///
    /// Note: `companion_cache_key` and `type_param_bindings` are `Arc<[…]>` so
    /// child contexts produced by `instantiate_type_params_ctx` share the same
    /// underlying slice without deep-cloning.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ResolvedNamedTypeCacheKey {
        pub name: Box<[u8]>,
        pub surface: Option<BlockedTypeSurface>,
        pub base_offset: u32,
        pub companion_cache_key: Arc<[Box<[u8]>]>,
        pub type_param_bindings: Arc<[ResolvedTypeParamBindingCacheKey]>,
    }

    /// Stable identity for a generic parameter binding — matches the semantic
    /// identity used by `type_param_bindings_cache_key` inside the resolver.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ResolvedTypeParamBindingCacheKey {
        pub name: Box<[u8]>,
        pub bound: Box<[u8]>,
    }

    /// Injected cache handle. Implementations must be `Send + Sync` so a single
    /// adapter instance can be cloned into child contexts and shared across
    /// concurrent resolver threads.
    ///
    /// Contract: `get` is read-only and must not mutate the cache. `insert`
    /// overwrites any prior entry under the same key (the resolver never asks
    /// for reconciliation; two callers computing the same key must produce
    /// structurally equal results).
    pub trait NamedTypeCache: std::fmt::Debug + Send + Sync {
        fn get(&self, key: &ResolvedNamedTypeCacheKey) -> Option<Arc<ResolvedElements>>;
        fn insert(&self, key: ResolvedNamedTypeCacheKey, value: Arc<ResolvedElements>);
    }
}

pub use cache_keys::{NamedTypeCache, ResolvedNamedTypeCacheKey, ResolvedTypeParamBindingCacheKey};

#[derive(Debug, Clone)]
enum NamedTypeResolutionPlan<'ctx, 'a: 'ctx> {
    Interface(InterfaceResolutionPlan<'ctx, 'a>),
    Class(ClassResolutionPlan<'ctx, 'a>),
}

#[derive(Debug, Clone)]
struct InterfaceResolutionPlan<'ctx, 'a: 'ctx> {
    own: ShallowResolvedElements,
    heritage: Vec<NamedTypeHeritageEdge<'ctx, 'a>>,
}

#[derive(Debug, Clone)]
struct ClassResolutionPlan<'ctx, 'a: 'ctx> {
    own: ShallowResolvedElements,
    heritage: Vec<NamedTypeHeritageEdge<'ctx, 'a>>,
}

#[derive(Debug, Clone)]
struct ShallowResolvedElements {
    props: Arc<[ResolvedProp]>,
    emits: Arc<[ResolvedEmit]>,
    has_call_signature: bool,
}

impl ShallowResolvedElements {
    fn apply_to(&self, result: &mut ResolvedElements) {
        result.props.extend(self.props.iter().cloned());
        result.emits.extend(self.emits.iter().cloned());
        if self.has_call_signature {
            result.has_call_signature = true;
        }
    }
}

impl From<ResolvedElements> for ShallowResolvedElements {
    fn from(value: ResolvedElements) -> Self {
        Self {
            props: Arc::from(value.props.into_boxed_slice()),
            emits: Arc::from(value.emits.into_boxed_slice()),
            has_call_signature: value.has_call_signature,
        }
    }
}

#[derive(Debug, Clone)]
enum NamedTypeHeritageEdge<'ctx, 'a: 'ctx> {
    Named {
        name: String,
        type_args: Option<&'ctx TSTypeParameterInstantiation<'a>>,
    },
    Utility {
        name: String,
        type_args: &'ctx TSTypeParameterInstantiation<'a>,
    },
}

impl<'ctx, 'a: 'ctx> TypeResolutionContext<'ctx, 'a> {
    /// Create a new empty context
    pub fn new(source: &'ctx [u8]) -> Self {
        Self {
            source,
            type_aliases: FxHashMap::default(),
            interfaces: FxHashMap::default(),
            classes: FxHashMap::default(),
            type_params: Vec::new(),
            type_param_bindings: Vec::new(),
            type_param_bindings_cache_key: Arc::from(
                Vec::<ResolvedTypeParamBindingCacheKey>::new().into_boxed_slice(),
            ),
            diagnostics: Vec::new(),
            companion_types: rustc_hash::FxHashMap::default(),
            companion_origins: rustc_hash::FxHashMap::default(),
            blocked_types: Vec::new(),
            current_surface: None,
            companion_cache_key: Arc::from(Vec::<Box<[u8]>>::new().into_boxed_slice()),
            trace_label: None,
            named_type_cache: None,
        }
    }

    /// Inject a host-owned named-type cache. Subsequent recursive resolutions
    /// (including child contexts produced by [`instantiate_type_params_ctx`])
    /// consult this handle before computing from AST, and store new results
    /// on completion. Calling this with `None` disables memoization.
    pub fn set_named_type_cache(
        &mut self,
        cache: Option<Arc<dyn cache_keys::NamedTypeCache + Send + Sync>>,
    ) {
        self.named_type_cache = cache;
    }

    pub fn refresh_companion_cache_key(&mut self) {
        let mut names = self
            .companion_types
            .keys()
            .map(|name| name.as_bytes().to_vec().into_boxed_slice())
            .collect::<Vec<_>>();
        names.sort_unstable();
        self.companion_cache_key = Arc::from(names.into_boxed_slice());
    }

    pub fn refresh_type_param_bindings_cache_key(&mut self) {
        let bindings = self
            .type_param_bindings
            .iter()
            .map(|(name_span, bound)| ResolvedTypeParamBindingCacheKey {
                name: symbol_key_from_span(self.source, *name_span),
                bound: semantic_type_cache_key(bound, self),
            })
            .collect::<Vec<_>>();
        self.type_param_bindings_cache_key = Arc::from(bindings.into_boxed_slice());
    }

    pub fn clear_type_param_bindings(&mut self) {
        self.type_param_bindings.clear();
        self.refresh_type_param_bindings_cache_key();
    }

    pub fn set_trace_label(&mut self, label: impl Into<Arc<str>>) {
        self.trace_label = Some(label.into());
    }

    pub fn extend_companion_types(
        &mut self,
        imported_companions: &FxHashMap<String, ResolvedElements>,
    ) {
        for (name, resolved) in imported_companions {
            self.companion_types
                .entry(name.clone())
                .or_insert_with(|| resolved.clone());
        }
        self.refresh_companion_cache_key();
    }

    /// Check if a type name is blocked by the per-surface blocklist.
    /// Returns true if the type should be skipped during expansion.
    ///
    /// Checks three dimensions:
    /// 1. Name must match
    /// 2. Import source must match (or be None for unconditional)
    /// 3. Surface must match (empty surfaces list = all surfaces)
    pub fn is_type_blocked(&self, type_name: &str) -> bool {
        self.blocked_types.iter().any(|blocked| {
            // Name must match
            if blocked.name != type_name {
                return false;
            }
            // Surface must match (empty = all surfaces)
            if !blocked.surfaces.is_empty() {
                match &self.current_surface {
                    Some(surface) => {
                        if !blocked.surfaces.contains(surface) {
                            return false;
                        }
                    }
                    None => {
                        // No current surface set — only match if blocked has no surface filter
                        return false;
                    }
                }
            }
            // Import source must match
            match &blocked.import_source {
                None => true, // No import qualifier → block unconditionally
                Some(pkg) => {
                    // Check companion_origins for the import source.
                    // Exact match or scoped package prefix (e.g., "vue" matches "vue",
                    // "@vue/runtime-core" matches "@vue/runtime-core", but "vue" does NOT
                    // match "vue-router").
                    self.companion_origins.get(type_name).is_some_and(|origin| {
                        origin == pkg
                            || origin
                                .strip_prefix(pkg.as_str())
                                .is_some_and(|rest| rest.starts_with('/'))
                    })
                }
            }
        })
    }

    /// Look up a type alias by comparing spans against source bytes
    pub fn find_type_alias(
        &self,
        name: &[u8],
    ) -> Option<(
        &'ctx TSType<'a>,
        Option<&'ctx TSTypeParameterDeclaration<'a>>,
    )> {
        self.type_aliases
            .get(name)
            .map(|(ty, params)| (*ty, *params))
    }

    /// Look up an interface by comparing spans against source bytes.
    /// Returns (body_members, extends_type_names).
    #[allow(clippy::type_complexity)]
    pub fn find_interface(
        &self,
        name: &[u8],
    ) -> Option<(
        &'ctx oxc_allocator::Vec<'a, TSSignature<'a>>,
        &[String],
        &'ctx [TSInterfaceHeritage<'a>],
        Option<&'ctx TSTypeParameterDeclaration<'a>>,
    )> {
        self.interfaces.get(name).map(|entry| {
            (
                entry.members,
                entry.extends.as_slice(),
                entry.heritage,
                entry.type_params,
            )
        })
    }

    /// Look up a class by comparing spans against source bytes.
    pub fn find_class(&self, name: &[u8]) -> Option<&'ctx Class<'a>> {
        self.classes.get(name).copied()
    }

    fn cache_key_for_name(
        &self,
        name: &[u8],
        base_offset: u32,
    ) -> cache_keys::ResolvedNamedTypeCacheKey {
        cache_keys::ResolvedNamedTypeCacheKey {
            name: name.to_vec().into_boxed_slice(),
            surface: self.current_surface.clone(),
            base_offset,
            companion_cache_key: Arc::clone(&self.companion_cache_key),
            type_param_bindings: Arc::clone(&self.type_param_bindings_cache_key),
        }
    }

    fn cached_named_resolution(
        &self,
        name: &[u8],
        base_offset: u32,
    ) -> Option<Arc<ResolvedElements>> {
        #[cfg(feature = "parser_cache_audit")]
        {
            // Emit an audit trace on every cache hit. The full slow-path
            // recompute + `PartialEq` assertion lives at the adapter layer
            // (see `verter_session::host_manage::HostNamedTypeCacheAdapter`'s
            // audit branch) — this trace gives us observability on hit rate
            // and key shape during focused audit runs.
            if let Some(cache) = &self.named_type_cache {
                if cache
                    .get(&self.cache_key_for_name(name, base_offset))
                    .is_some()
                {
                    component_meta_core_trace_event(
                        "parser_cache_audit_hit",
                        format!(
                            "file={} name={} base_offset={} bindings={} companions={}",
                            self.trace_label.as_deref().unwrap_or("<unknown>"),
                            String::from_utf8_lossy(name),
                            base_offset,
                            self.type_param_bindings.len(),
                            self.companion_types.len(),
                        ),
                    );
                }
            }
        }
        self.named_type_cache
            .as_ref()?
            .get(&self.cache_key_for_name(name, base_offset))
    }

    fn store_named_resolution(
        &self,
        name: &[u8],
        base_offset: u32,
        resolved: Arc<ResolvedElements>,
    ) {
        if let Some(cache) = self.named_type_cache.as_ref() {
            cache.insert(self.cache_key_for_name(name, base_offset), resolved);
        }
    }
    /// Look up a type parameter constraint by comparing spans against source bytes
    pub fn find_type_param(&self, name: &[u8]) -> Option<&'ctx TSType<'a>> {
        if let Some(bound) = self
            .type_param_bindings
            .iter()
            .rev()
            .find(|(span, _)| &self.source[span.start as usize..span.end as usize] == name)
        {
            return Some(bound.1);
        }
        self.type_params
            .iter()
            .find(|(span, _)| &self.source[span.start as usize..span.end as usize] == name)
            .and_then(|(_, constraint)| *constraint)
    }
}

fn symbol_key_from_span(source: &[u8], span: Span) -> Box<[u8]> {
    source[span.start as usize..span.end as usize]
        .to_vec()
        .into_boxed_slice()
}

fn normalized_source_key_from_span(source: &[u8], span: Span) -> Box<[u8]> {
    source[span.start as usize..span.end as usize]
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn append_type_name_cache_key(out: &mut Vec<u8>, type_name: &TSTypeName<'_>) {
    match type_name {
        TSTypeName::IdentifierReference(ident) => out.extend_from_slice(ident.name.as_bytes()),
        TSTypeName::QualifiedName(qualified) => {
            append_qualified_type_name_cache_key(out, qualified)
        }
        TSTypeName::ThisExpression(_) => out.extend_from_slice(b"this"),
    }
}

fn append_qualified_type_name_cache_key(out: &mut Vec<u8>, qualified: &TSQualifiedName<'_>) {
    append_type_name_cache_key(out, &qualified.left);
    out.push(b'.');
    out.extend_from_slice(qualified.right.name.as_bytes());
}

fn append_literal_cache_key(out: &mut Vec<u8>, literal: &TSLiteralType<'_>) {
    match &literal.literal {
        TSLiteral::StringLiteral(value) => {
            out.extend_from_slice(b"str:");
            out.extend_from_slice(value.value.as_bytes());
        }
        TSLiteral::NumericLiteral(value) => {
            out.extend_from_slice(b"num:");
            if let Some(raw) = &value.raw {
                out.extend_from_slice(raw.as_bytes());
            } else {
                out.extend_from_slice(value.value.to_string().as_bytes());
            }
        }
        TSLiteral::BooleanLiteral(value) => {
            out.extend_from_slice(if value.value {
                b"bool:true"
            } else {
                b"bool:false"
            });
        }
        TSLiteral::BigIntLiteral(value) => {
            out.extend_from_slice(b"bigint:");
            if let Some(raw) = &value.raw {
                out.extend_from_slice(raw.as_bytes());
            }
        }
        TSLiteral::TemplateLiteral(template) => {
            out.extend_from_slice(b"tpl:");
            for quasi in &template.quasis {
                out.extend_from_slice(quasi.value.raw.as_bytes());
                out.push(b'|');
            }
        }
        TSLiteral::UnaryExpression(unary) => {
            out.extend_from_slice(b"unary:");
            match unary.operator {
                UnaryOperator::UnaryNegation => out.push(b'-'),
                UnaryOperator::UnaryPlus => out.push(b'+'),
                _ => out.push(b'?'),
            }
            if let Expression::NumericLiteral(value) = &unary.argument {
                if let Some(raw) = &value.raw {
                    out.extend_from_slice(raw.as_bytes());
                } else {
                    out.extend_from_slice(value.value.to_string().as_bytes());
                }
            } else if let Expression::BigIntLiteral(value) = &unary.argument {
                if let Some(raw) = &value.raw {
                    out.extend_from_slice(raw.as_bytes());
                }
            }
        }
    }
}

fn append_semantic_type_cache_key<'ctx, 'a: 'ctx>(
    out: &mut Vec<u8>,
    ty: &'ctx TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    active_type_params: &mut Vec<Box<[u8]>>,
) {
    match ty {
        TSType::TSStringKeyword(_) => out.extend_from_slice(b"kw:string"),
        TSType::TSNumberKeyword(_) => out.extend_from_slice(b"kw:number"),
        TSType::TSBooleanKeyword(_) => out.extend_from_slice(b"kw:boolean"),
        TSType::TSAnyKeyword(_) => out.extend_from_slice(b"kw:any"),
        TSType::TSUnknownKeyword(_) => out.extend_from_slice(b"kw:unknown"),
        TSType::TSNeverKeyword(_) => out.extend_from_slice(b"kw:never"),
        TSType::TSVoidKeyword(_) => out.extend_from_slice(b"kw:void"),
        TSType::TSNullKeyword(_) => out.extend_from_slice(b"kw:null"),
        TSType::TSUndefinedKeyword(_) => out.extend_from_slice(b"kw:undefined"),
        TSType::TSObjectKeyword(_) => out.extend_from_slice(b"kw:object"),
        TSType::TSSymbolKeyword(_) => out.extend_from_slice(b"kw:symbol"),
        TSType::TSBigIntKeyword(_) => out.extend_from_slice(b"kw:bigint"),
        TSType::TSLiteralType(literal) => append_literal_cache_key(out, literal),
        TSType::TSParenthesizedType(paren) => {
            out.extend_from_slice(b"paren(");
            append_semantic_type_cache_key(out, &paren.type_annotation, ctx, active_type_params);
            out.push(b')');
        }
        TSType::TSArrayType(array) => {
            out.extend_from_slice(b"arr(");
            append_semantic_type_cache_key(out, &array.element_type, ctx, active_type_params);
            out.push(b')');
        }
        TSType::TSTupleType(tuple) => {
            out.extend_from_slice(b"tuple(");
            for element in &tuple.element_types {
                match element {
                    TSTupleElement::TSOptionalType(optional) => {
                        out.extend_from_slice(b"opt(");
                        append_semantic_type_cache_key(
                            out,
                            &optional.type_annotation,
                            ctx,
                            active_type_params,
                        );
                        out.push(b')');
                    }
                    TSTupleElement::TSRestType(rest) => {
                        out.extend_from_slice(b"rest(");
                        append_semantic_type_cache_key(
                            out,
                            &rest.type_annotation,
                            ctx,
                            active_type_params,
                        );
                        out.push(b')');
                    }
                    TSTupleElement::TSNamedTupleMember(named) => {
                        out.extend_from_slice(named.label.name.as_bytes());
                        out.push(b':');
                        if let Some(ts_type) = named.element_type.as_ts_type() {
                            append_semantic_type_cache_key(out, ts_type, ctx, active_type_params);
                        }
                    }
                    _ => {
                        if let Some(ts_type) = element.as_ts_type() {
                            append_semantic_type_cache_key(out, ts_type, ctx, active_type_params);
                        }
                    }
                }
                out.push(b',');
            }
            out.push(b')');
        }
        TSType::TSUnionType(union) => {
            let mut parts = union
                .types
                .iter()
                .map(|part| semantic_type_cache_key_with_active(part, ctx, active_type_params))
                .collect::<Vec<_>>();
            parts.sort_unstable();
            out.extend_from_slice(b"union(");
            for part in parts {
                out.extend_from_slice(part.as_ref());
                out.push(b',');
            }
            out.push(b')');
        }
        TSType::TSIntersectionType(intersection) => {
            let mut parts = intersection
                .types
                .iter()
                .map(|part| semantic_type_cache_key_with_active(part, ctx, active_type_params))
                .collect::<Vec<_>>();
            parts.sort_unstable();
            out.extend_from_slice(b"inter(");
            for part in parts {
                out.extend_from_slice(part.as_ref());
                out.push(b',');
            }
            out.push(b')');
        }
        TSType::TSTypeReference(type_ref) => {
            let mut name = Vec::new();
            append_type_name_cache_key(&mut name, &type_ref.type_name);
            if let Some(bound) = ctx
                .type_param_bindings
                .iter()
                .rev()
                .find(|(span, _)| {
                    &ctx.source[span.start as usize..span.end as usize] == name.as_slice()
                })
                .map(|(_, bound)| *bound)
            {
                let name_key = name.into_boxed_slice();
                if active_type_params
                    .iter()
                    .any(|active| active.as_ref() == name_key.as_ref())
                {
                    out.extend_from_slice(b"param:");
                    out.extend_from_slice(name_key.as_ref());
                    return;
                }
                active_type_params.push(name_key.clone());
                out.extend_from_slice(b"bound(");
                append_semantic_type_cache_key(out, bound, ctx, active_type_params);
                out.push(b')');
                active_type_params.pop();
                return;
            }

            out.extend_from_slice(b"ref:");
            out.extend_from_slice(&name);
            if let Some(type_args) = &type_ref.type_arguments {
                out.push(b'<');
                for arg in &type_args.params {
                    append_semantic_type_cache_key(out, arg, ctx, active_type_params);
                    out.push(b',');
                }
                out.push(b'>');
            }
        }
        TSType::TSTypeOperatorType(operator) => {
            out.extend_from_slice(b"op:");
            out.extend_from_slice(format!("{:?}", operator.operator).as_bytes());
            out.push(b'(');
            append_semantic_type_cache_key(out, &operator.type_annotation, ctx, active_type_params);
            out.push(b')');
        }
        TSType::TSIndexedAccessType(indexed) => {
            out.extend_from_slice(b"idx(");
            append_semantic_type_cache_key(out, &indexed.object_type, ctx, active_type_params);
            out.push(b',');
            append_semantic_type_cache_key(out, &indexed.index_type, ctx, active_type_params);
            out.push(b')');
        }
        TSType::TSTypeQuery(query) => {
            out.extend_from_slice(b"query:");
            match &query.expr_name {
                TSTypeQueryExprName::IdentifierReference(ident) => {
                    out.extend_from_slice(ident.name.as_bytes());
                }
                TSTypeQueryExprName::QualifiedName(qualified) => {
                    append_qualified_type_name_cache_key(out, qualified);
                }
                TSTypeQueryExprName::ThisExpression(_) => out.extend_from_slice(b"this"),
                TSTypeQueryExprName::TSImportType(import) => {
                    out.extend_from_slice(b"import(");
                    out.extend_from_slice(
                        normalized_source_key_from_span(ctx.source, import.span.into()).as_ref(),
                    );
                    out.push(b')');
                }
            }
        }
        _ => out.extend_from_slice(
            normalized_source_key_from_span(ctx.source, ty.span().into()).as_ref(),
        ),
    }
}

fn semantic_type_cache_key_with_active<'ctx, 'a: 'ctx>(
    ty: &'ctx TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    active_type_params: &mut Vec<Box<[u8]>>,
) -> Box<[u8]> {
    let mut out = Vec::new();
    append_semantic_type_cache_key(&mut out, ty, ctx, active_type_params);
    out.into_boxed_slice()
}

fn semantic_type_cache_key<'ctx, 'a: 'ctx>(
    ty: &'ctx TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Box<[u8]> {
    semantic_type_cache_key_with_active(ty, ctx, &mut Vec::new())
}

fn binding_name_from_span(source: &[u8], span: Span) -> Option<&str> {
    std::str::from_utf8(&source[span.start as usize..span.end as usize]).ok()
}

fn collect_relevant_outer_type_param_bindings<'ctx, 'a: 'ctx>(
    ctx: &TypeResolutionContext<'ctx, 'a>,
    seed_bounds: &[&'ctx TSType<'a>],
) -> Vec<(Span, &'ctx TSType<'a>)> {
    if ctx.type_param_bindings.is_empty() || seed_bounds.is_empty() {
        return Vec::new();
    }

    let mut referenced_names = FxHashSet::default();
    for bound in seed_bounds {
        collect_type_reference_names(bound, &mut referenced_names);
    }

    if referenced_names.is_empty() {
        return Vec::new();
    }

    let mut relevant = Vec::new();
    let mut seen_spans = FxHashSet::default();
    let mut changed = true;

    while changed {
        changed = false;

        for (name_span, bound) in &ctx.type_param_bindings {
            let Some(name) = binding_name_from_span(ctx.source, *name_span) else {
                continue;
            };
            let span_key = (name_span.start, name_span.end);
            if !referenced_names.contains(name) || !seen_spans.insert(span_key) {
                continue;
            }

            relevant.push((*name_span, *bound));
            collect_type_reference_names(bound, &mut referenced_names);
            changed = true;
        }
    }

    relevant
}

/// Build type resolution context from a parsed program.
/// Collects type aliases and interfaces for later lookup.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn build_type_context<'ctx, 'a: 'ctx>(
    program: &'ctx Program<'a>,
    source: &'ctx [u8],
    _content_offset: u32,
) -> TypeResolutionContext<'ctx, 'a> {
    let mut ctx = TypeResolutionContext::new(source);

    for stmt in &program.body {
        match stmt {
            // Collect type aliases: `type Foo = { bar: string }`
            Statement::TSTypeAliasDeclaration(alias) => {
                // alias.id.span is already adjusted by adjust_program_spans() to SFC coordinates,
                // so we use it directly — no additional offset needed.
                let name_span = Span::from(alias.id.span);
                ctx.type_aliases.insert(
                    symbol_key_from_span(source, name_span),
                    (&alias.type_annotation, alias.type_parameters.as_deref()),
                );
            }
            // Collect interfaces: `interface Foo { bar: string }`
            Statement::TSInterfaceDeclaration(interface) => {
                // interface.id.span is already adjusted by adjust_program_spans() to SFC coordinates.
                let name_span = Span::from(interface.id.span);
                let extends = extract_heritage_type_names(&interface.extends);
                ctx.interfaces.insert(
                    symbol_key_from_span(source, name_span),
                    InterfaceResolutionEntry {
                        members: &interface.body.body,
                        extends,
                        heritage: &interface.extends,
                        type_params: interface.type_parameters.as_deref(),
                    },
                );
            }
            Statement::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    ctx.classes
                        .insert(symbol_key_from_span(source, Span::from(id.span)), class);
                }
            }
            // Collect exported type aliases and interfaces:
            // `export type Foo = { bar: string }` / `export interface Foo { bar: string }`
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = &export.declaration {
                    match decl {
                        Declaration::TSTypeAliasDeclaration(alias) => {
                            let name_span = Span::from(alias.id.span);
                            ctx.type_aliases.insert(
                                symbol_key_from_span(source, name_span),
                                (&alias.type_annotation, alias.type_parameters.as_deref()),
                            );
                        }
                        Declaration::TSInterfaceDeclaration(interface) => {
                            let name_span = Span::from(interface.id.span);
                            let extends = extract_heritage_type_names(&interface.extends);
                            ctx.interfaces.insert(
                                symbol_key_from_span(source, name_span),
                                InterfaceResolutionEntry {
                                    members: &interface.body.body,
                                    extends,
                                    heritage: &interface.extends,
                                    type_params: interface.type_parameters.as_deref(),
                                },
                            );
                        }
                        Declaration::ClassDeclaration(class) => {
                            if let Some(id) = &class.id {
                                ctx.classes.insert(
                                    symbol_key_from_span(source, Span::from(id.span)),
                                    class,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                    if let Some(id) = &class.id {
                        ctx.classes
                            .insert(symbol_key_from_span(source, Span::from(id.span)), class);
                    }
                }
                ExportDefaultDeclarationKind::TSInterfaceDeclaration(interface) => {
                    let name_span = Span::from(interface.id.span);
                    let extends = extract_heritage_type_names(&interface.extends);
                    ctx.interfaces.insert(
                        symbol_key_from_span(source, name_span),
                        InterfaceResolutionEntry {
                            members: &interface.body.body,
                            extends,
                            heritage: &interface.extends,
                            type_params: interface.type_parameters.as_deref(),
                        },
                    );
                }
                _ => {}
            },
            _ => {}
        }
    }

    ctx
}

fn instantiate_type_params_ctx<'ctx, 'a: 'ctx>(
    ctx: &TypeResolutionContext<'ctx, 'a>,
    decl_params: Option<&'ctx TSTypeParameterDeclaration<'a>>,
    type_args: Option<&'ctx TSTypeParameterInstantiation<'a>>,
) -> TypeResolutionContext<'ctx, 'a> {
    let mut child = ctx.clone();
    child.diagnostics.clear();
    let Some(decl_params) = decl_params else {
        child.clear_type_param_bindings();
        return child;
    };

    let mut chosen_bounds = Vec::new();
    for (index, param) in decl_params.params.iter().enumerate() {
        let bound = type_args
            .and_then(|args| args.params.get(index))
            .or(param.default.as_ref())
            .or(param.constraint.as_ref());
        if let Some(bound) = bound {
            chosen_bounds.push(bound);
        }
    }

    child.type_param_bindings = collect_relevant_outer_type_param_bindings(ctx, &chosen_bounds);

    for (index, param) in decl_params.params.iter().enumerate() {
        let bound = type_args
            .and_then(|args| args.params.get(index))
            .or(param.default.as_ref())
            .or(param.constraint.as_ref());
        if let Some(bound) = bound {
            child
                .type_param_bindings
                .push((Span::from(param.name.span), bound));
        }
    }

    child.refresh_type_param_bindings_cache_key();

    child
}

/// Extract pre-resolved types from a companion `<script>` program.
///
/// Walks the program's statements and resolves any type aliases and interfaces,
/// returning a map from type name → resolved elements. This allows the setup
/// script's type resolver to look up types defined in the companion block.
pub fn extract_companion_types(
    program: &Program<'_>,
    source: &[u8],
    content_offset: u32,
) -> rustc_hash::FxHashMap<String, ResolvedElements> {
    // Build a full type context so we can resolve extends and cross-references
    let ctx = build_type_context(program, source, content_offset);

    let mut types = rustc_hash::FxHashMap::default();

    for stmt in &program.body {
        match stmt {
            Statement::TSTypeAliasDeclaration(alias) => {
                let name = alias.id.name.as_str().to_string();
                let resolved = resolve_type_elements_with_ctx_ref(
                    &alias.type_annotation,
                    content_offset,
                    &ctx,
                );
                types.insert(name, resolved);
            }
            Statement::TSInterfaceDeclaration(interface) => {
                let name = interface.id.name.as_str().to_string();
                let extends = extract_heritage_type_names(&interface.extends);
                let mut resolved = ResolvedElements::default();
                let mut guard = vec![name.clone()];
                resolve_interface_with_extends_ctx_ref(
                    &interface.body.body,
                    &extends,
                    &interface.extends,
                    content_offset,
                    &mut resolved,
                    &ctx,
                    &mut guard,
                );
                resolved.root_runtime_types = vec![RuntimeType::Object];
                types.insert(name, resolved);
            }
            Statement::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    let name = id.name.as_str().to_string();
                    let mut resolved = ResolvedElements::default();
                    let mut guard = vec![name.clone()];
                    resolve_class_with_heritage_ctx_ref(
                        class,
                        content_offset,
                        &mut resolved,
                        &ctx,
                        &mut guard,
                    );
                    resolved.root_runtime_types = vec![RuntimeType::Object];
                    types.insert(name, resolved);
                }
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = &export.declaration {
                    match decl {
                        Declaration::TSTypeAliasDeclaration(alias) => {
                            let name = alias.id.name.as_str().to_string();
                            let resolved = resolve_type_elements_with_ctx_ref(
                                &alias.type_annotation,
                                content_offset,
                                &ctx,
                            );
                            types.insert(name, resolved);
                        }
                        Declaration::TSInterfaceDeclaration(interface) => {
                            let name = interface.id.name.as_str().to_string();
                            let extends = extract_heritage_type_names(&interface.extends);
                            let mut resolved = ResolvedElements::default();
                            let mut guard = vec![name.clone()];
                            resolve_interface_with_extends_ctx_ref(
                                &interface.body.body,
                                &extends,
                                &interface.extends,
                                content_offset,
                                &mut resolved,
                                &ctx,
                                &mut guard,
                            );
                            resolved.root_runtime_types = vec![RuntimeType::Object];
                            types.insert(name, resolved);
                        }
                        Declaration::ClassDeclaration(class) => {
                            if let Some(id) = &class.id {
                                let name = id.name.as_str().to_string();
                                let mut resolved = ResolvedElements::default();
                                let mut guard = vec![name.clone()];
                                resolve_class_with_heritage_ctx_ref(
                                    class,
                                    content_offset,
                                    &mut resolved,
                                    &ctx,
                                    &mut guard,
                                );
                                resolved.root_runtime_types = vec![RuntimeType::Object];
                                types.insert(name, resolved);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    types
}

/// Resolve type elements from a TSType node.
///
/// This extracts property information from type literals, interfaces,
/// and other TypeScript type constructs.
///
/// # Arguments
/// * `node` - The TSType node to resolve
/// * `base_offset` - The document offset to apply to all spans
pub fn resolve_type_elements(node: &TSType, base_offset: u32) -> ResolvedElements {
    let mut result = ResolvedElements::default();
    resolve_type_elements_inner(node, base_offset, &mut result, b"");
    result.root_runtime_types = infer_runtime_type(node);
    result
}

/// Resolve type elements with a type resolution context.
/// This version can resolve local type aliases and interfaces.
///
/// # Arguments
/// * `node` - The TSType node to resolve
/// * `base_offset` - The document offset to apply to all spans
/// * `ctx` - Type resolution context with local type definitions
pub fn resolve_type_elements_with_ctx<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
) -> ResolvedElements {
    let mut result = ResolvedElements::default();
    resolve_type_elements_inner_with_ctx(node, base_offset, &mut result, ctx);
    result.root_runtime_types =
        resolve_root_runtime_type_with_ctx(node, ctx).unwrap_or_else(|| infer_runtime_type(node));
    result
}

/// Resolve type elements with an immutable type resolution context.
/// This version doesn't collect diagnostics, making it suitable for
/// contexts where we only need the resolved types without error tracking.
///
/// # Arguments
/// * `node` - The TSType node to resolve
/// * `base_offset` - The document offset to apply to all spans
/// * `ctx` - Immutable type resolution context with local type definitions
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn resolve_type_elements_with_ctx_ref<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> ResolvedElements {
    let mut result = ResolvedElements::default();
    resolve_type_elements_inner_with_ctx_ref(node, base_offset, &mut result, ctx);
    result.root_runtime_types = resolve_root_runtime_type_with_ctx_ref(node, ctx)
        .unwrap_or_else(|| infer_runtime_type(node));
    result
}

fn inferred_root_runtime_type_for_companion(companion: &ResolvedElements) -> Vec<RuntimeType> {
    if !companion.root_runtime_types.is_empty() {
        return companion.root_runtime_types.clone();
    }
    if !companion.props.is_empty() || !companion.emits.is_empty() {
        return vec![RuntimeType::Object];
    }
    if companion.has_call_signature {
        return vec![RuntimeType::Function];
    }
    vec![RuntimeType::Unknown]
}

fn resolve_root_runtime_type_with_ctx<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Option<Vec<RuntimeType>> {
    match node {
        TSType::TSTypeReference(type_ref) => {
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            if let Some((aliased_type, _)) = ctx.find_type_alias(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx(aliased_type, ctx)
                        .unwrap_or_else(|| infer_runtime_type(aliased_type)),
                );
            }

            if ctx.find_interface(type_name_bytes).is_some() {
                return Some(vec![RuntimeType::Object]);
            }

            if ctx.find_class(type_name_bytes).is_some() {
                return Some(vec![RuntimeType::Object]);
            }

            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx(constraint, ctx)
                        .unwrap_or_else(|| infer_runtime_type(constraint)),
                );
            }

            ctx.companion_types
                .get(type_name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        TSType::TSTypeQuery(query) => {
            let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name else {
                return None;
            };
            ctx.companion_types
                .get(ident.name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        _ => None,
    }
}

fn resolve_root_runtime_type_with_ctx_ref<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Option<Vec<RuntimeType>> {
    match node {
        TSType::TSTypeReference(type_ref) => {
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            if let Some((aliased_type, _)) = ctx.find_type_alias(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx_ref(aliased_type, ctx)
                        .unwrap_or_else(|| infer_runtime_type(aliased_type)),
                );
            }

            if ctx.find_interface(type_name_bytes).is_some() {
                return Some(vec![RuntimeType::Object]);
            }

            if ctx.find_class(type_name_bytes).is_some() {
                return Some(vec![RuntimeType::Object]);
            }

            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx_ref(constraint, ctx)
                        .unwrap_or_else(|| infer_runtime_type(constraint)),
                );
            }

            ctx.companion_types
                .get(type_name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        TSType::TSTypeQuery(query) => {
            let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name else {
                return None;
            };
            ctx.companion_types
                .get(ident.name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        _ => None,
    }
}

fn resolve_named_local_type_with_ctx_ref<'ctx, 'a: 'ctx>(
    type_name: &str,
    type_args: Option<&'ctx TSTypeParameterInstantiation<'a>>,
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) -> Option<Arc<ResolvedElements>> {
    resolve_named_local_type_with_ctx_ref_inner(
        type_name,
        type_args,
        base_offset,
        ctx,
        recursion_guard,
        true,
    )
}

fn resolve_named_local_type_with_ctx_ref_inner<'ctx, 'a: 'ctx>(
    type_name: &str,
    type_args: Option<&'ctx TSTypeParameterInstantiation<'a>>,
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
    store_result: bool,
) -> Option<Arc<ResolvedElements>> {
    let type_name_bytes = type_name.as_bytes();

    if let Some((aliased_type, type_params)) = ctx.find_type_alias(type_name_bytes) {
        let child = instantiate_type_params_ctx(ctx, type_params, type_args);
        if let Some(cached) = child.cached_named_resolution(type_name_bytes, base_offset) {
            if component_meta_core_trace_enabled() {
                component_meta_core_trace_event(
                    "core_named_resolution",
                    format!(
                        "file={} kind=alias cache=hit name={} bindings={} companions={}",
                        child.trace_label.as_deref().unwrap_or("<unknown>"),
                        type_name,
                        child.type_param_bindings.len(),
                        child.companion_types.len()
                    ),
                );
            }
            return Some(cached);
        }
        if component_meta_core_trace_enabled() {
            component_meta_core_trace_event(
                "core_named_resolution",
                format!(
                    "file={} kind=alias cache=miss name={} bindings={} companions={}",
                    child.trace_label.as_deref().unwrap_or("<unknown>"),
                    type_name,
                    child.type_param_bindings.len(),
                    child.companion_types.len()
                ),
            );
        }
        let resolved = Arc::new(resolve_type_elements_with_ctx_ref(
            aliased_type,
            base_offset,
            &child,
        ));
        if store_result {
            child.store_named_resolution(type_name_bytes, base_offset, Arc::clone(&resolved));
        }
        return Some(resolved);
    }

    if let Some((members, _extends, heritage, type_params)) = ctx.find_interface(type_name_bytes) {
        let child = instantiate_type_params_ctx(ctx, type_params, type_args);
        if let Some(cached) = child.cached_named_resolution(type_name_bytes, base_offset) {
            if component_meta_core_trace_enabled() {
                component_meta_core_trace_event(
                    "core_named_resolution",
                    format!(
                        "file={} kind=interface cache=hit name={} bindings={} companions={} members={} extends={}",
                        child.trace_label.as_deref().unwrap_or("<unknown>"),
                        type_name,
                        child.type_param_bindings.len(),
                        child.companion_types.len(),
                        members.len(),
                        _extends.len()
                    ),
                );
            }
            return Some(cached);
        }
        if component_meta_core_trace_enabled() {
            component_meta_core_trace_event(
                "core_named_resolution",
                format!(
                    "file={} kind=interface cache=miss name={} bindings={} companions={} members={} extends={}",
                    child.trace_label.as_deref().unwrap_or("<unknown>"),
                    type_name,
                    child.type_param_bindings.len(),
                    child.companion_types.len(),
                    members.len(),
                    _extends.len()
                ),
            );
        }
        let plan = NamedTypeResolutionPlan::Interface(build_interface_resolution_plan(
            members,
            _extends,
            heritage,
            base_offset,
            &child,
        ));
        let mut resolved = ResolvedElements::default();
        flatten_named_type_plan_with_ctx_ref(
            &plan,
            base_offset,
            &mut resolved,
            &child,
            recursion_guard,
        );
        resolved.root_runtime_types = vec![RuntimeType::Object];
        let resolved = Arc::new(resolved);
        if store_result {
            child.store_named_resolution(type_name_bytes, base_offset, Arc::clone(&resolved));
        }
        return Some(resolved);
    }

    if let Some(class_decl) = ctx.find_class(type_name_bytes) {
        let type_params = class_decl.type_parameters.as_deref();
        let child = instantiate_type_params_ctx(ctx, type_params, type_args);
        if let Some(cached) = child.cached_named_resolution(type_name_bytes, base_offset) {
            if component_meta_core_trace_enabled() {
                component_meta_core_trace_event(
                    "core_named_resolution",
                    format!(
                        "file={} kind=class cache=hit name={} bindings={} companions={}",
                        child.trace_label.as_deref().unwrap_or("<unknown>"),
                        type_name,
                        child.type_param_bindings.len(),
                        child.companion_types.len()
                    ),
                );
            }
            return Some(cached);
        }
        if component_meta_core_trace_enabled() {
            component_meta_core_trace_event(
                "core_named_resolution",
                format!(
                    "file={} kind=class cache=miss name={} bindings={} companions={}",
                    child.trace_label.as_deref().unwrap_or("<unknown>"),
                    type_name,
                    child.type_param_bindings.len(),
                    child.companion_types.len()
                ),
            );
        }
        let plan = NamedTypeResolutionPlan::Class(build_class_resolution_plan(
            class_decl,
            base_offset,
            &child,
        ));
        let mut resolved = ResolvedElements::default();
        flatten_named_type_plan_with_ctx_ref(
            &plan,
            base_offset,
            &mut resolved,
            &child,
            recursion_guard,
        );
        resolved.root_runtime_types = vec![RuntimeType::Object];
        resolved.dedup_props();
        let resolved = Arc::new(resolved);
        if store_result {
            child.store_named_resolution(type_name_bytes, base_offset, Arc::clone(&resolved));
        }
        return Some(resolved);
    }

    if let Some(companion) = ctx.companion_types.get(type_name).cloned() {
        if component_meta_core_trace_enabled() {
            component_meta_core_trace_event(
                "core_named_resolution",
                format!(
                    "file={} kind=companion cache=hit name={} bindings={} companions={}",
                    ctx.trace_label.as_deref().unwrap_or("<unknown>"),
                    type_name,
                    ctx.type_param_bindings.len(),
                    ctx.companion_types.len()
                ),
            );
        }
        return Some(Arc::new(companion));
    }

    if component_meta_core_trace_enabled() {
        component_meta_core_trace_event(
            "core_named_resolution",
            format!(
                "file={} kind=missing cache=miss name={} bindings={} companions={}",
                ctx.trace_label.as_deref().unwrap_or("<unknown>"),
                type_name,
                ctx.type_param_bindings.len(),
                ctx.companion_types.len()
            ),
        );
    }
    None
}

fn is_supported_heritage_utility(name: &str) -> bool {
    matches!(
        name,
        "Pick" | "Omit" | "Partial" | "Required" | "Readonly" | "Record"
    )
}

fn build_interface_resolution_plan<'ctx, 'a: 'ctx>(
    members: &[TSSignature],
    extends: &[String],
    heritage: &'ctx [TSInterfaceHeritage<'a>],
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> InterfaceResolutionPlan<'ctx, 'a> {
    let mut own = ResolvedElements::default();
    resolve_type_literal_members(members, base_offset, &mut own, ctx.source);

    let heritage = heritage
        .iter()
        .enumerate()
        .filter(|(_, clause)| !has_immediate_vue_ignore_comment(ctx.source, clause.span().start))
        .map(|(index, clause)| {
            let name = extends
                .get(index)
                .cloned()
                .unwrap_or_else(|| "<anonymous>".to_string());
            match clause.type_arguments.as_deref() {
                Some(type_args)
                    if !type_args.params.is_empty()
                        && is_supported_heritage_utility(name.as_str()) =>
                {
                    NamedTypeHeritageEdge::Utility { name, type_args }
                }
                other => NamedTypeHeritageEdge::Named {
                    name,
                    type_args: other,
                },
            }
        })
        .collect();

    InterfaceResolutionPlan {
        own: own.into(),
        heritage,
    }
}

fn build_class_resolution_plan<'ctx, 'a: 'ctx>(
    class: &'ctx Class<'a>,
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> ClassResolutionPlan<'ctx, 'a> {
    let mut own = ResolvedElements::default();
    resolve_class_members(&class.body.body, base_offset, &mut own, ctx.source);

    let mut heritage = Vec::new();
    if let Some(super_class) = &class.super_class {
        if let Some(name) = get_expression_reference_name(super_class) {
            if let Some(type_args) = class.super_type_arguments.as_deref() {
                if !type_args.params.is_empty() && is_supported_heritage_utility(name.as_str()) {
                    heritage.push(NamedTypeHeritageEdge::Utility { name, type_args });
                } else {
                    heritage.push(NamedTypeHeritageEdge::Named {
                        name,
                        type_args: Some(type_args),
                    });
                }
            } else {
                heritage.push(NamedTypeHeritageEdge::Named {
                    name,
                    type_args: None,
                });
            }
        }
    }

    for clause in &class.implements {
        let name = get_type_reference_name(&clause.expression);
        if let Some(type_args) = clause.type_arguments.as_deref() {
            if !type_args.params.is_empty() && is_supported_heritage_utility(name.as_str()) {
                heritage.push(NamedTypeHeritageEdge::Utility { name, type_args });
            } else {
                heritage.push(NamedTypeHeritageEdge::Named {
                    name,
                    type_args: Some(type_args),
                });
            }
        } else {
            heritage.push(NamedTypeHeritageEdge::Named {
                name,
                type_args: None,
            });
        }
    }

    ClassResolutionPlan {
        own: own.into(),
        heritage,
    }
}

fn flatten_named_type_plan_with_ctx_ref<'ctx, 'a: 'ctx>(
    plan: &NamedTypeResolutionPlan<'ctx, 'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    match plan {
        NamedTypeResolutionPlan::Interface(plan) => {
            plan.own.apply_to(result);
            for edge in &plan.heritage {
                apply_named_type_heritage_edge_with_ctx_ref(
                    edge,
                    base_offset,
                    result,
                    ctx,
                    recursion_guard,
                );
            }
        }
        NamedTypeResolutionPlan::Class(plan) => {
            plan.own.apply_to(result);
            for edge in &plan.heritage {
                apply_named_type_heritage_edge_with_ctx_ref(
                    edge,
                    base_offset,
                    result,
                    ctx,
                    recursion_guard,
                );
            }
        }
    }
}

fn apply_named_type_heritage_edge_with_ctx_ref<'ctx, 'a: 'ctx>(
    edge: &NamedTypeHeritageEdge<'ctx, 'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    match edge {
        NamedTypeHeritageEdge::Utility { name, type_args } => {
            let _ = try_resolve_heritage_utility_type(
                name.as_str(),
                type_args,
                base_offset,
                result,
                ctx,
            );
        }
        NamedTypeHeritageEdge::Named { name, type_args } => {
            if recursion_guard.contains(name) {
                return;
            }
            recursion_guard.push(name.clone());
            if let Some(resolved) = resolve_named_local_type_with_ctx_ref_inner(
                name.as_str(),
                *type_args,
                base_offset,
                ctx,
                recursion_guard,
                false,
            ) {
                result.props.extend(resolved.props.iter().cloned());
                result.emits.extend(resolved.emits.iter().cloned());
                if resolved.has_call_signature {
                    result.has_call_signature = true;
                }
            }
            recursion_guard.pop();
        }
    }
}

fn resolve_type_elements_inner(
    node: &TSType,
    base_offset: u32,
    result: &mut ResolvedElements,
    source: &[u8],
) {
    match node {
        // { prop: Type }
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members(&lit.members, base_offset, result, source);
        }

        // Parenthesized: (Type)
        TSType::TSParenthesizedType(paren) => {
            resolve_type_elements_inner(&paren.type_annotation, base_offset, result, source);
        }

        // Union: Type1 | Type2
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                resolve_type_elements_inner(ty, base_offset, result, source);
            }
            result.dedup_props();
        }

        // Intersection: Type1 & Type2
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                if has_immediate_vue_ignore_comment(source, ty.span().start) {
                    continue;
                }
                resolve_type_elements_inner(ty, base_offset, result, source);
            }
            result.dedup_props();
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(_type_ref) => {
            // For now, we can't resolve type references without a scope
            // This would require tracking type declarations
            // Mark as unknown for now
        }

        // Function type: () => Type
        TSType::TSFunctionType(_) => {
            result.has_call_signature = true;
        }

        _ => {}
    }
}

/// Resolve an interface including its extends clauses using mutable context.
/// Recursion guard prevents infinite loops from circular extends.
fn resolve_interface_with_extends_ctx<'ctx, 'a: 'ctx>(
    members: &[TSSignature],
    extends: &[String],
    heritage: &'ctx [TSInterfaceHeritage<'a>],
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    // Resolve own members
    resolve_type_literal_members(members, base_offset, result, ctx.source);

    // Resolve extends — try heritage AST first for utility type support,
    // then fall back to string-based lookup (matches _ctx_ref variant).
    for (i, base_name) in extends.iter().enumerate() {
        if recursion_guard.contains(base_name) {
            continue; // Avoid infinite recursion
        }
        recursion_guard.push(base_name.clone());

        let base_bytes = base_name.as_bytes();

        // When a heritage clause has type arguments (e.g., `extends Pick<T, 'k'>`),
        // resolve through the utility type dispatch inline.
        if let Some(h) = heritage.get(i) {
            if has_immediate_vue_ignore_comment(ctx.source, h.span().start) {
                recursion_guard.pop();
                continue;
            }
            if let Some(type_args) = &h.type_arguments {
                if !type_args.params.is_empty()
                    && try_resolve_heritage_utility_type(
                        base_name.as_str(),
                        type_args,
                        base_offset,
                        result,
                        ctx,
                    )
                {
                    recursion_guard.pop();
                    continue;
                }
            }
        }

        // Check local type aliases
        if let Some((aliased_type, _)) = ctx.find_type_alias(base_bytes) {
            resolve_type_elements_inner_with_ctx(aliased_type, base_offset, result, ctx);
        }
        // Check local interfaces (need to clone extends to avoid borrow conflict)
        else if let Some((iface_members, iface_extends, iface_heritage, _)) =
            ctx.find_interface(base_bytes)
        {
            let iface_extends_owned: Vec<String> = iface_extends.to_vec();
            resolve_interface_with_extends_ctx(
                iface_members,
                &iface_extends_owned,
                iface_heritage,
                base_offset,
                result,
                ctx,
                recursion_guard,
            );
        } else if let Some(class_decl) = ctx.find_class(base_bytes) {
            resolve_class_with_heritage_ctx_ref(
                class_decl,
                base_offset,
                result,
                ctx,
                recursion_guard,
            );
        }
        // Check companion types
        else if let Some(companion) = ctx.companion_types.get(base_name.as_str()) {
            result.props.extend(companion.props.iter().cloned());
            result.emits.extend(companion.emits.iter().cloned());
            if companion.has_call_signature {
                result.has_call_signature = true;
            }
        }

        recursion_guard.pop();
    }
}

/// Resolve an interface including its extends clauses using immutable context.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn resolve_interface_with_extends_ctx_ref<'ctx, 'a: 'ctx>(
    members: &[TSSignature],
    extends: &[String],
    heritage: &'ctx [TSInterfaceHeritage<'a>],
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    let current_name = recursion_guard
        .last()
        .cloned()
        .unwrap_or_else(|| "<anonymous>".to_string());
    if component_meta_core_trace_enabled() {
        component_meta_core_trace_event(
            "core_interface_resolution",
            format!(
                "file={} phase=start name={} depth={} members={} extends={}",
                ctx.trace_label.as_deref().unwrap_or("<unknown>"),
                current_name,
                recursion_guard.len(),
                members.len(),
                extends.len()
            ),
        );
    }
    // Resolve own members
    resolve_type_literal_members(members, base_offset, result, ctx.source);

    // Resolve extends — try heritage AST first for utility type support,
    // then fall back to string-based lookup.
    for (i, base_name) in extends.iter().enumerate() {
        if recursion_guard.contains(base_name) {
            continue;
        }
        recursion_guard.push(base_name.clone());

        // When a heritage clause has type arguments (e.g., `extends Pick<T, 'k'>`),
        // the name-based lookup below won't find it since "Pick" isn't a local type.
        // Resolve the full heritage expression through the type system which handles
        // all TypeScript utility types (Pick, Omit, Partial, Required, Readonly,
        // Record, Extract, Exclude, etc.) in a single code path.
        if let Some(h) = heritage.get(i) {
            if has_immediate_vue_ignore_comment(ctx.source, h.span().start) {
                recursion_guard.pop();
                continue;
            }
            if let Some(type_args) = &h.type_arguments {
                if !type_args.params.is_empty() {
                    // Resolve each type_argument through the normal pipeline.
                    // For utility types like Pick<T, K>, the first param is the
                    // source type; filtering/transformation is handled by the
                    // utility type branch in resolve_type_elements_inner_with_ctx_ref
                    // when it encounters the corresponding TSTypeReference node.
                    //
                    // We can't construct a synthetic TSTypeReference here (needs arena),
                    // so we replicate the utility type dispatch inline. This covers the
                    // most common cases; truly complex types may need the JS-side
                    // type registry fallback.
                    if try_resolve_heritage_utility_type(
                        base_name.as_str(),
                        type_args,
                        base_offset,
                        result,
                        ctx,
                    ) {
                        if result.has_call_signature {
                            result.has_call_signature = true;
                        }
                        recursion_guard.pop();
                        continue;
                    }
                    // Not a recognized utility type — fall through to name-based lookup
                }
            }
        }

        let type_args = heritage.get(i).and_then(|h| h.type_arguments.as_deref());
        if let Some(resolved) = resolve_named_local_type_with_ctx_ref(
            base_name.as_str(),
            type_args,
            base_offset,
            ctx,
            recursion_guard,
        ) {
            result.props.extend(resolved.props.iter().cloned());
            result.emits.extend(resolved.emits.iter().cloned());
            if resolved.has_call_signature {
                result.has_call_signature = true;
            }
        }

        recursion_guard.pop();
    }

    if component_meta_core_trace_enabled() {
        component_meta_core_trace_event(
            "core_interface_resolution",
            format!(
                "file={} phase=end name={} depth={} props={} emits={}",
                ctx.trace_label.as_deref().unwrap_or("<unknown>"),
                current_name,
                recursion_guard.len(),
                result.props.len(),
                result.emits.len()
            ),
        );
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn try_resolve_heritage_utility_type<'ctx, 'a: 'ctx>(
    name: &str,
    type_args: &'ctx TSTypeParameterInstantiation<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> bool {
    match name {
        "Pick" if type_args.params.len() >= 2 => {
            let mut inner = ResolvedElements::default();
            resolve_type_elements_inner_with_ctx_ref(
                &type_args.params[0],
                base_offset,
                &mut inner,
                ctx,
            );
            let keys = extract_string_literal_keys_with_ctx(&type_args.params[1], Some(ctx));
            inner
                .props
                .retain(|p| p.key_name.as_ref().is_some_and(|n| keys.contains(n)));
            inner.emits.retain(|e| keys.contains(&e.name));
            result.props.extend(inner.props);
            result.emits.extend(inner.emits);
            true
        }
        "Omit" if type_args.params.len() >= 2 => {
            let mut inner = ResolvedElements::default();
            resolve_type_elements_inner_with_ctx_ref(
                &type_args.params[0],
                base_offset,
                &mut inner,
                ctx,
            );
            let keys = extract_string_literal_keys_with_ctx(&type_args.params[1], Some(ctx));
            inner
                .props
                .retain(|p| p.key_name.as_ref().is_none_or(|n| !keys.contains(n)));
            inner.emits.retain(|e| !keys.contains(&e.name));
            result.props.extend(inner.props);
            result.emits.extend(inner.emits);
            true
        }
        "Partial" | "Required" | "Readonly" if !type_args.params.is_empty() => {
            let mut inner = ResolvedElements::default();
            resolve_type_elements_inner_with_ctx_ref(
                &type_args.params[0],
                base_offset,
                &mut inner,
                ctx,
            );
            if name == "Partial" {
                for p in &mut inner.props {
                    p.optional = true;
                }
            } else if name == "Required" {
                for p in &mut inner.props {
                    p.optional = false;
                }
            }
            result.props.extend(inner.props);
            result.emits.extend(inner.emits);
            true
        }
        "Record" if type_args.params.len() >= 2 => {
            result.root_runtime_types.push(RuntimeType::Object);
            true
        }
        _ => false,
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn resolve_class_with_heritage_ctx_ref<'ctx, 'a: 'ctx>(
    class: &'ctx Class<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    resolve_class_members(&class.body.body, base_offset, result, ctx.source);

    if let Some(super_class) = &class.super_class {
        if let Some(base_name) = get_expression_reference_name(super_class) {
            if !recursion_guard.contains(&base_name) {
                recursion_guard.push(base_name.clone());
                if let Some(type_args) = class.super_type_arguments.as_deref() {
                    if !type_args.params.is_empty()
                        && try_resolve_heritage_utility_type(
                            base_name.as_str(),
                            type_args,
                            base_offset,
                            result,
                            ctx,
                        )
                    {
                        recursion_guard.pop();
                    } else {
                        resolve_named_class_heritage_target(
                            base_name.as_str(),
                            class.super_type_arguments.as_deref(),
                            base_offset,
                            result,
                            ctx,
                            recursion_guard,
                        );
                        recursion_guard.pop();
                    }
                } else {
                    resolve_named_class_heritage_target(
                        base_name.as_str(),
                        None,
                        base_offset,
                        result,
                        ctx,
                        recursion_guard,
                    );
                    recursion_guard.pop();
                }
            }
        }
    }

    for clause in &class.implements {
        let base_name = get_type_reference_name(&clause.expression);
        if recursion_guard.contains(&base_name) {
            continue;
        }
        recursion_guard.push(base_name.clone());
        if let Some(type_args) = clause.type_arguments.as_deref() {
            if !type_args.params.is_empty()
                && try_resolve_heritage_utility_type(
                    base_name.as_str(),
                    type_args,
                    base_offset,
                    result,
                    ctx,
                )
            {
                recursion_guard.pop();
                continue;
            }
        }
        resolve_named_class_heritage_target(
            base_name.as_str(),
            clause.type_arguments.as_deref(),
            base_offset,
            result,
            ctx,
            recursion_guard,
        );
        recursion_guard.pop();
    }

    result.dedup_props();
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn resolve_named_class_heritage_target<'ctx, 'a: 'ctx>(
    name: &str,
    type_args: Option<&'ctx TSTypeParameterInstantiation<'a>>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    if let Some(resolved) =
        resolve_named_local_type_with_ctx_ref(name, type_args, base_offset, ctx, recursion_guard)
    {
        result.props.extend(resolved.props.iter().cloned());
        result.emits.extend(resolved.emits.iter().cloned());
        if resolved.has_call_signature {
            result.has_call_signature = true;
        }
    }
}

fn resolve_class_members(
    members: &[ClassElement],
    base_offset: u32,
    result: &mut ResolvedElements,
    source: &[u8],
) {
    for member in members {
        match member {
            ClassElement::PropertyDefinition(prop) => {
                if let Some(resolved) = resolve_class_property_definition(prop, base_offset, source)
                {
                    result.props.push(resolved);
                }
            }
            ClassElement::MethodDefinition(method) => {
                if let Some(resolved) = resolve_class_method_definition(method, base_offset, source)
                {
                    result.props.push(resolved);
                }
            }
            ClassElement::AccessorProperty(prop) => {
                if let Some(resolved) = resolve_class_accessor_property(prop, base_offset, source) {
                    result.props.push(resolved);
                }
            }
            _ => {}
        }
    }
}

fn resolve_class_property_definition(
    prop: &PropertyDefinition,
    base_offset: u32,
    source: &[u8],
) -> Option<ResolvedProp> {
    if prop.r#static {
        return None;
    }

    let key = get_property_key_span(&prop.key, base_offset)?;
    let types = prop
        .type_annotation
        .as_ref()
        .map(|ann| infer_runtime_type(&ann.type_annotation))
        .or_else(|| prop.value.as_ref().map(infer_runtime_type_from_expression))
        .unwrap_or_else(|| vec![RuntimeType::Unknown]);
    let type_span = prop.type_annotation.as_ref().map(|ann| Span {
        start: ann.type_annotation.span().start + base_offset,
        end: ann.type_annotation.span().end + base_offset,
    });
    let type_text = prop
        .type_annotation
        .as_ref()
        .and_then(|ann| span_text(source, ann.type_annotation.span().into()));

    Some(ResolvedProp {
        span: Span {
            start: prop.span.start + base_offset,
            end: prop.span.end + base_offset,
        },
        key,
        key_name: get_property_key_name(&prop.key),
        optional: prop.optional,
        types,
        visibility: visibility_from_accessibility(prop.accessibility),
        type_span,
        type_text,
        map_local: true,
        span_is_absolute: base_offset != 0,
    })
}

fn resolve_class_method_definition(
    method: &MethodDefinition,
    base_offset: u32,
    source: &[u8],
) -> Option<ResolvedProp> {
    if method.r#static || method.kind == MethodDefinitionKind::Constructor {
        return None;
    }

    let key = get_property_key_span(&method.key, base_offset)?;
    let type_text = callable_signature_text(
        source,
        &method.value.params.items,
        method
            .value
            .return_type
            .as_ref()
            .map(|return_type| &return_type.type_annotation),
    );
    Some(ResolvedProp {
        span: Span {
            start: method.span.start + base_offset,
            end: method.span.end + base_offset,
        },
        key,
        key_name: get_property_key_name(&method.key),
        optional: method.optional,
        types: vec![RuntimeType::Function],
        visibility: visibility_from_accessibility(method.accessibility),
        type_span: None,
        type_text,
        map_local: true,
        span_is_absolute: base_offset != 0,
    })
}

fn resolve_class_accessor_property(
    prop: &AccessorProperty,
    base_offset: u32,
    source: &[u8],
) -> Option<ResolvedProp> {
    if prop.r#static {
        return None;
    }

    let key = get_property_key_span(&prop.key, base_offset)?;
    let types = prop
        .type_annotation
        .as_ref()
        .map(|ann| infer_runtime_type(&ann.type_annotation))
        .or_else(|| prop.value.as_ref().map(infer_runtime_type_from_expression))
        .unwrap_or_else(|| vec![RuntimeType::Unknown]);
    let type_span = prop.type_annotation.as_ref().map(|ann| Span {
        start: ann.type_annotation.span().start + base_offset,
        end: ann.type_annotation.span().end + base_offset,
    });
    let type_text = prop
        .type_annotation
        .as_ref()
        .and_then(|ann| span_text(source, ann.type_annotation.span().into()));

    Some(ResolvedProp {
        span: Span {
            start: prop.span.start + base_offset,
            end: prop.span.end + base_offset,
        },
        key,
        key_name: get_property_key_name(&prop.key),
        optional: false,
        types,
        visibility: visibility_from_accessibility(prop.accessibility),
        type_span,
        type_text,
        map_local: true,
        span_is_absolute: base_offset != 0,
    })
}

fn infer_runtime_type_from_expression(expr: &Expression<'_>) -> Vec<RuntimeType> {
    match expr {
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => vec![RuntimeType::String],
        Expression::NumericLiteral(_) => vec![RuntimeType::Number],
        Expression::BooleanLiteral(_) => vec![RuntimeType::Boolean],
        Expression::ArrayExpression(_) => vec![RuntimeType::Array],
        Expression::ObjectExpression(_) => vec![RuntimeType::Object],
        Expression::NullLiteral(_) => vec![RuntimeType::Null],
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
            vec![RuntimeType::Function]
        }
        _ => vec![RuntimeType::Unknown],
    }
}

fn visibility_from_accessibility(
    accessibility: Option<TSAccessibility>,
) -> ResolvedMemberVisibility {
    match accessibility {
        Some(TSAccessibility::Private) => ResolvedMemberVisibility::Private,
        Some(TSAccessibility::Protected) => ResolvedMemberVisibility::Protected,
        _ => ResolvedMemberVisibility::Public,
    }
}

fn get_expression_reference_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

/// Inner resolution function that uses the context for type reference lookup.
///
/// Recursion depth is tracked via a module-local thread-local counter
/// (see [`RESOLUTION_DEPTH`]) rather than a shared `Rc<Cell<u16>>` field on the
/// context. Removing that field from `TypeResolutionContext` unblocks the
/// host-owned cache migration (the resolver context no longer carries `!Send`
/// interior-mutability state). Depth guarding still bails at
/// [`PARSER_SYNTACTIC_DEPTH_LIMIT`] to prevent stack overflow on deeply nested
/// generic types (syntactic stack-safety, not a semantic budget).
fn resolve_type_elements_inner_with_ctx<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
) {
    let Some(_guard) = ResolutionDepthGuard::try_enter() else {
        return;
    };
    resolve_type_elements_inner_with_ctx_guarded(node, base_offset, result, ctx);
}

fn resolve_type_elements_inner_with_ctx_guarded<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
) {
    match node {
        // { prop: Type }
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members(&lit.members, base_offset, result, ctx.source);
        }

        // Parenthesized: (Type)
        TSType::TSParenthesizedType(paren) => {
            resolve_type_elements_inner_with_ctx(&paren.type_annotation, base_offset, result, ctx);
        }

        // Union: Type1 | Type2
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                resolve_type_elements_inner_with_ctx(ty, base_offset, result, ctx);
            }
            result.dedup_props();
        }

        // Intersection: Type1 & Type2
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                if has_immediate_vue_ignore_comment(ctx.source, ty.span().start) {
                    continue;
                }
                resolve_type_elements_inner_with_ctx(ty, base_offset, result, ctx);
            }
            result.dedup_props();
        }

        TSType::TSMappedType(mapped) => {
            resolve_mapped_type_with_ctx(mapped, base_offset, result, &*ctx);
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(type_ref) => {
            // Get the type name for lookup
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            // 0. Check per-surface type blocklist — skip expansion entirely
            if ctx.is_type_blocked(&type_name) {
                return;
            }

            // 1. Check local type aliases
            if let Some((aliased_type, type_params)) = ctx.find_type_alias(type_name_bytes) {
                let mut child = instantiate_type_params_ctx(
                    ctx,
                    type_params,
                    type_ref.type_arguments.as_deref(),
                );
                resolve_type_elements_inner_with_ctx(aliased_type, base_offset, result, &mut child);
                ctx.diagnostics.append(&mut child.diagnostics);
                return;
            }

            // 2. Check local interfaces (with extends support)
            if let Some((interface_members, iface_extends, iface_heritage, iface_type_params)) =
                ctx.find_interface(type_name_bytes)
            {
                let extends_owned: Vec<String> = iface_extends.to_vec();
                let mut guard = vec![type_name.clone()];
                let mut child = instantiate_type_params_ctx(
                    ctx,
                    iface_type_params,
                    type_ref.type_arguments.as_deref(),
                );
                resolve_interface_with_extends_ctx(
                    interface_members,
                    &extends_owned,
                    iface_heritage,
                    base_offset,
                    result,
                    &mut child,
                    &mut guard,
                );
                ctx.diagnostics.append(&mut child.diagnostics);
                return;
            }

            // 3. Check local classes (instance-side shape with heritage)
            if let Some(class_decl) = ctx.find_class(type_name_bytes) {
                let mut guard = vec![type_name.clone()];
                let child = instantiate_type_params_ctx(
                    ctx,
                    class_decl.type_parameters.as_deref(),
                    type_ref.type_arguments.as_deref(),
                );
                resolve_class_with_heritage_ctx_ref(
                    class_decl,
                    base_offset,
                    result,
                    &child,
                    &mut guard,
                );
                return;
            }

            // 4. Check generic type parameter constraints
            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                resolve_type_elements_inner_with_ctx(constraint, base_offset, result, ctx);
                return;
            }

            // 5. Check companion <script> block's pre-resolved types
            if let Some(companion) = ctx.companion_types.get(type_name.as_str()) {
                result.props.extend(companion.props.iter().cloned());
                result.emits.extend(companion.emits.iter().cloned());
                if companion.has_call_signature {
                    result.has_call_signature = true;
                }
                return;
            }

            // 6. Handle built-in TypeScript utility types (Omit, Pick, Partial, etc.)
            if let Some(args) = &type_ref.type_arguments {
                match type_name.as_str() {
                    "Omit" if args.params.len() >= 2 => {
                        // Omit<T, K>: resolve T, then remove keys in K
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                        );
                        let keys =
                            extract_string_literal_keys_with_ctx(&args.params[1], Some(&*ctx));
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_none_or(|n| !keys.contains(n)));
                        inner.emits.retain(|e| !keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.emits.extend(inner.emits);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                        return;
                    }
                    "Pick" if args.params.len() >= 2 => {
                        // Pick<T, K>: resolve T, then keep only keys in K
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                        );
                        let keys =
                            extract_string_literal_keys_with_ctx(&args.params[1], Some(&*ctx));
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_some_and(|n| keys.contains(n)));
                        inner.emits.retain(|e| keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.emits.extend(inner.emits);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                        return;
                    }
                    "Partial" | "Required" | "Readonly" if !args.params.is_empty() => {
                        // These preserve structure, just change modifiers
                        resolve_type_elements_inner_with_ctx(
                            &args.params[0],
                            base_offset,
                            result,
                            ctx,
                        );
                        return;
                    }
                    _ => {}
                }
            }

            // 6. Couldn't resolve - add diagnostic
            // Note: We don't add to result.props here because we can't determine the structure
            ctx.diagnostics.push(ResolutionDiagnostic {
                span: Span {
                    start: type_ref.span.start + base_offset,
                    end: type_ref.span.end + base_offset,
                },
                kind: ResolutionDiagnosticKind::UnresolvedTypeReference,
                location: DiagnosticLocation::TypeResolution,
            });
        }

        // Type query: typeof X — look up in companion types
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                let type_name = ident.name.as_str();
                if let Some(companion) = ctx.companion_types.get(type_name) {
                    result.props.extend(companion.props.iter().cloned());
                    result.emits.extend(companion.emits.iter().cloned());
                    if companion.has_call_signature {
                        result.has_call_signature = true;
                    }
                }
            }
        }

        // Function type: () => Type
        TSType::TSFunctionType(_) => {
            result.has_call_signature = true;
        }

        _ => {}
    }
}

/// Inner resolution function that uses an immutable context (doesn't collect diagnostics).
///
/// Uses the module-local [`RESOLUTION_DEPTH`] thread-local counter — see
/// [`resolve_type_elements_inner_with_ctx`] for the rationale.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn resolve_type_elements_inner_with_ctx_ref<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) {
    let Some(_guard) = ResolutionDepthGuard::try_enter() else {
        return;
    };
    resolve_type_elements_inner_with_ctx_ref_guarded(node, base_offset, result, ctx);
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn resolve_type_elements_inner_with_ctx_ref_guarded<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) {
    match node {
        // { prop: Type }
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members(&lit.members, base_offset, result, ctx.source);
        }

        // Parenthesized: (Type)
        TSType::TSParenthesizedType(paren) => {
            resolve_type_elements_inner_with_ctx_ref(
                &paren.type_annotation,
                base_offset,
                result,
                ctx,
            );
        }

        // Union: Type1 | Type2
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                resolve_type_elements_inner_with_ctx_ref(ty, base_offset, result, ctx);
            }
            result.dedup_props();
        }

        // Intersection: Type1 & Type2
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                if has_immediate_vue_ignore_comment(ctx.source, ty.span().start) {
                    continue;
                }
                resolve_type_elements_inner_with_ctx_ref(ty, base_offset, result, ctx);
            }
            result.dedup_props();
        }

        TSType::TSMappedType(mapped) => {
            resolve_mapped_type_with_ctx(mapped, base_offset, result, ctx);
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(type_ref) => {
            // Get the type name for lookup
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            // 0. Check per-surface type blocklist — skip expansion entirely
            if ctx.is_type_blocked(&type_name) {
                return;
            }

            let mut guard = vec![type_name.clone()];
            if let Some(resolved) = resolve_named_local_type_with_ctx_ref(
                type_name.as_str(),
                type_ref.type_arguments.as_deref(),
                base_offset,
                ctx,
                &mut guard,
            ) {
                result.props.extend(resolved.props.iter().cloned());
                result.emits.extend(resolved.emits.iter().cloned());
                if resolved.has_call_signature {
                    result.has_call_signature = true;
                }
                return;
            }

            // 4. Check generic type parameter constraints
            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                resolve_type_elements_inner_with_ctx_ref(constraint, base_offset, result, ctx);
                return;
            }

            // 6. Handle built-in TypeScript utility types (Omit, Pick, Partial, etc.)
            if let Some(args) = &type_ref.type_arguments {
                match type_name.as_str() {
                    "Omit" if args.params.len() >= 2 => {
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx_ref(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                        );
                        let keys = extract_string_literal_keys_with_ctx(&args.params[1], Some(ctx));
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_none_or(|n| !keys.contains(n)));
                        inner.emits.retain(|e| !keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.emits.extend(inner.emits);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                    }
                    "Pick" if args.params.len() >= 2 => {
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx_ref(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                        );
                        let keys = extract_string_literal_keys_with_ctx(&args.params[1], Some(ctx));
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_some_and(|n| keys.contains(n)));
                        inner.emits.retain(|e| keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.emits.extend(inner.emits);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                    }
                    "Partial" | "Required" | "Readonly" if !args.params.is_empty() => {
                        resolve_type_elements_inner_with_ctx_ref(
                            &args.params[0],
                            base_offset,
                            result,
                            ctx,
                        );
                    }
                    _ => {}
                }
            }

            // 6. Couldn't resolve - skip silently (no diagnostics in immutable version)
        }

        // Type query: typeof X — look up in companion types
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                let type_name = ident.name.as_str();
                if let Some(companion) = ctx.companion_types.get(type_name) {
                    result.props.extend(companion.props.iter().cloned());
                    result.emits.extend(companion.emits.iter().cloned());
                    if companion.has_call_signature {
                        result.has_call_signature = true;
                    }
                }
            }
        }

        // Function type: () => Type
        TSType::TSFunctionType(_) => {
            result.has_call_signature = true;
        }

        _ => {}
    }
}

/// Resolve members from a type literal's members array.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn resolve_type_literal_members(
    members: &[TSSignature],
    base_offset: u32,
    result: &mut ResolvedElements,
    source: &[u8],
) {
    for member in members {
        match member {
            TSSignature::TSPropertySignature(prop) => {
                // Check if this is a shorthand emit: { change: [id: number] }
                // Properties with tuple/array type values are treated as emits
                if let Some(emit) = resolve_property_as_emit(prop, base_offset, source) {
                    result.emits.push(emit);
                } else if let Some(resolved) = resolve_property_signature(prop, base_offset, source)
                {
                    result.props.push(resolved);
                }
            }
            TSSignature::TSMethodSignature(method) => {
                if let Some(resolved) = resolve_method_signature(method, base_offset, source) {
                    result.props.push(resolved);
                }
            }
            TSSignature::TSCallSignatureDeclaration(call_sig) => {
                result.has_call_signature = true;
                // Extract emit from call signature: (e: 'change', id: number): void
                if let Some(emit) = resolve_call_signature_as_emit(call_sig, base_offset, source) {
                    result.emits.push(emit);
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedMappedKey {
    name: String,
    key: Span,
    optional: bool,
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn resolve_mapped_type_with_ctx<'ctx, 'a: 'ctx>(
    mapped: &'ctx TSMappedType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) {
    // Renamed mapped keys (`as ...`) need a dedicated key-evaluation path.
    // Until that exists, only materialize direct finite key sets.
    if mapped.name_type.is_some() {
        return;
    }

    let keys = resolve_mapped_type_keys_with_ctx(&mapped.constraint, ctx);
    if keys.is_empty() {
        return;
    }

    let span = Span {
        start: mapped.span.start + base_offset,
        end: mapped.span.end + base_offset,
    };
    let type_span = mapped.type_annotation.as_ref().map(|ann| Span {
        start: ann.span().start + base_offset,
        end: ann.span().end + base_offset,
    });
    let type_text = mapped
        .type_annotation
        .as_ref()
        .and_then(|ann| span_text(ctx.source, ann.span().into()));
    let types = mapped
        .type_annotation
        .as_ref()
        .map(|ann| infer_runtime_type(ann))
        .unwrap_or_else(|| vec![RuntimeType::Unknown]);
    let optional_override = mapped_optional_override(mapped.optional);

    for key in keys {
        result.props.push(ResolvedProp {
            span,
            key: Span {
                start: key.key.start + base_offset,
                end: key.key.end + base_offset,
            },
            key_name: Some(key.name),
            optional: optional_override.unwrap_or(key.optional),
            types: types.clone(),
            visibility: ResolvedMemberVisibility::Public,
            type_span,
            type_text: type_text.clone(),
            map_local: true,
            span_is_absolute: base_offset != 0,
        });
    }

    result.dedup_props();
}

fn mapped_optional_override(modifier: Option<TSMappedTypeModifierOperator>) -> Option<bool> {
    match modifier {
        Some(TSMappedTypeModifierOperator::True | TSMappedTypeModifierOperator::Plus) => Some(true),
        Some(TSMappedTypeModifierOperator::Minus) => Some(false),
        None => None,
    }
}

fn resolve_mapped_type_keys_with_ctx<'ctx, 'a: 'ctx>(
    constraint: &'ctx TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Vec<ResolvedMappedKey> {
    match constraint {
        TSType::TSTypeOperatorType(op) if matches!(op.operator, TSTypeOperatorOperator::Keyof) => {
            let resolved = resolve_type_elements_with_ctx_ref(&op.type_annotation, 0, ctx);
            resolved
                .props
                .into_iter()
                .filter_map(|prop| {
                    let name = prop
                        .key_name
                        .clone()
                        .or_else(|| span_text(ctx.source, prop.key))?;
                    Some(ResolvedMappedKey {
                        name,
                        key: prop.key,
                        optional: prop.optional,
                    })
                })
                .collect()
        }
        TSType::TSLiteralType(literal) => resolve_mapped_string_literal_key(literal),
        TSType::TSUnionType(union) => union
            .types
            .iter()
            .flat_map(|ty| resolve_mapped_type_keys_with_ctx(ty, ctx))
            .collect(),
        TSType::TSParenthesizedType(paren) => {
            resolve_mapped_type_keys_with_ctx(&paren.type_annotation, ctx)
        }
        TSType::TSTypeReference(type_ref) => {
            let name = get_type_reference_name(&type_ref.type_name);
            if let Some((aliased_type, _)) = ctx.find_type_alias(name.as_bytes()) {
                resolve_mapped_type_keys_with_ctx(aliased_type, ctx)
            } else {
                extract_string_literal_keys_with_ctx(constraint, Some(ctx))
                    .into_iter()
                    .map(|name| ResolvedMappedKey {
                        name,
                        key: Span::new(0, 0),
                        optional: false,
                    })
                    .collect()
            }
        }
        _ => extract_string_literal_keys_with_ctx(constraint, Some(ctx))
            .into_iter()
            .map(|name| ResolvedMappedKey {
                name,
                key: Span::new(0, 0),
                optional: false,
            })
            .collect(),
    }
}

fn resolve_mapped_string_literal_key(literal: &TSLiteralType<'_>) -> Vec<ResolvedMappedKey> {
    match &literal.literal {
        TSLiteral::StringLiteral(value) => vec![ResolvedMappedKey {
            name: value.value.to_string(),
            key: Span::from(value.span),
            optional: false,
        }],
        _ => Vec::new(),
    }
}

/// Try to resolve a property signature as an emit (shorthand style).
/// Shorthand style: `{ change: [id: number] }` or `{ update: [] }`
fn resolve_property_as_emit(
    prop: &TSPropertySignature,
    base_offset: u32,
    source: &[u8],
) -> Option<ResolvedEmit> {
    // Get the property key as the event name
    let name = get_property_key_name(&prop.key)?;
    let key_span = get_property_key_span(&prop.key, base_offset)?;

    // Check if the type is a tuple type - this indicates emit shorthand
    // Note: Only TSTupleType (e.g., `[id: number]`) is emit shorthand.
    // TSArrayType (e.g., `string[]`) is a regular array prop type.
    if let Some(ann) = &prop.type_annotation {
        if let TSType::TSTupleType(_) = &ann.type_annotation {
            let tuple_text = slice_source_span(
                source,
                ann.type_annotation.span().start,
                ann.type_annotation.span().end,
            )?;
            return Some(ResolvedEmit {
                span: Span {
                    start: prop.span.start + base_offset,
                    end: prop.span.end + base_offset,
                },
                name,
                name_span: Some(key_span),
                signature: ResolvedEmitSignature::Tuple { tuple_text },
                map_local: true,
                span_is_absolute: base_offset != 0,
            });
        }
    }

    None
}

/// Resolve a call signature as an emit event.
/// Call signature style: `(e: 'change', id: number): void`
/// The event name is extracted from the first parameter's type if it's a string literal.
fn resolve_call_signature_as_emit(
    call_sig: &TSCallSignatureDeclaration,
    base_offset: u32,
    source: &[u8],
) -> Option<ResolvedEmit> {
    // Get the first parameter - should be like `e: 'eventName'`
    let first_param = call_sig.params.items.first()?;

    // The type annotation is on the FormalParameter, not the pattern
    let type_ann = first_param.type_annotation.as_ref()?;

    // Extract event name from string literal type
    if let TSType::TSLiteralType(lit) = &type_ann.type_annotation {
        if let TSLiteral::StringLiteral(s) = &lit.literal {
            let mut params_text = String::new();
            for param in call_sig.params.items.iter().skip(1) {
                if !params_text.is_empty() {
                    params_text.push_str(", ");
                }
                params_text.push_str(&slice_source_span(
                    source,
                    param.span().start,
                    param.span().end,
                )?);
            }
            if let Some(rest) = &call_sig.params.rest {
                if !params_text.is_empty() {
                    params_text.push_str(", ");
                }
                params_text.push_str(&slice_source_span(source, rest.span.start, rest.span.end)?);
            }
            return Some(ResolvedEmit {
                span: Span {
                    start: call_sig.span.start + base_offset,
                    end: call_sig.span.end + base_offset,
                },
                name: s.value.to_string(),
                name_span: Some(Span {
                    start: s.span.start + base_offset,
                    end: s.span.end + base_offset,
                }),
                signature: ResolvedEmitSignature::Call { params_text },
                map_local: true,
                span_is_absolute: base_offset != 0,
            });
        }
    }

    None
}

/// Get the name of a property key as a string.
fn get_property_key_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => n.raw.as_ref().map(|r| r.to_string()),
        _ => None,
    }
}

/// Extract string literal keys from a type argument (supports single literal and unions).
/// Used for `Omit<T, 'a' | 'b'>` and `Pick<T, 'a' | 'b'>`.
fn extract_string_literal_keys(ty: &TSType) -> Vec<String> {
    extract_string_literal_keys_with_ctx(ty, None)
}

/// Extract string literal keys from a type, optionally following type alias references
/// when a context is available. This is critical for `Omit<T, KeysAlias | 'literal'>`
/// where `KeysAlias` is a type alias expanding to a union of string literals.
fn extract_string_literal_keys_with_ctx<'ctx, 'a: 'ctx>(
    ty: &TSType<'a>,
    ctx: Option<&TypeResolutionContext<'ctx, 'a>>,
) -> Vec<String> {
    let mut visited = Vec::new();
    extract_string_literal_keys_inner(ty, ctx, &mut visited)
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn extract_string_literal_keys_inner<'ctx, 'a: 'ctx>(
    ty: &TSType<'a>,
    ctx: Option<&TypeResolutionContext<'ctx, 'a>>,
    visited: &mut Vec<String>,
) -> Vec<String> {
    match ty {
        TSType::TSLiteralType(lit) => {
            if let TSLiteral::StringLiteral(s) = &lit.literal {
                vec![s.value.to_string()]
            } else {
                vec![]
            }
        }
        TSType::TSUnionType(union) => union
            .types
            .iter()
            .flat_map(|t| extract_string_literal_keys_inner(t, ctx, visited))
            .collect(),
        TSType::TSParenthesizedType(paren) => {
            extract_string_literal_keys_inner(&paren.type_annotation, ctx, visited)
        }
        TSType::TSTypeReference(type_ref) if ctx.is_some() => {
            let ctx = ctx.unwrap();
            let name = type_ref.type_name.to_string();
            // Recursion guard: prevent infinite loops on circular type aliases
            if visited.contains(&name) {
                return vec![];
            }
            visited.push(name.clone());
            let name_bytes = name.as_bytes();
            // Follow local type aliases to extract their string literal keys
            let result = if let Some((aliased_type, _)) = ctx.find_type_alias(name_bytes) {
                extract_string_literal_keys_inner(aliased_type, Some(ctx), visited)
            } else {
                vec![]
            };
            visited.pop();
            result
        }
        _ => vec![],
    }
}

fn slice_source_span(source: &[u8], start: u32, end: u32) -> Option<String> {
    let start = start as usize;
    let end = end as usize;
    if end > source.len() || start > end {
        return None;
    }
    std::str::from_utf8(&source[start..end])
        .ok()
        .map(|s| s.trim().to_string())
}

fn has_immediate_vue_ignore_comment(source: &[u8], start: u32) -> bool {
    let start = start as usize;
    if start == 0 || start > source.len() {
        return false;
    }

    let window_start = start.saturating_sub(160);
    let prefix = match std::str::from_utf8(&source[window_start..start]) {
        Ok(text) => text.trim_end(),
        Err(_) => return false,
    };

    if let Some(comment_start) = prefix.rfind("/*") {
        let comment = &prefix[comment_start..];
        return comment.ends_with("*/") && comment.contains("@vue-ignore");
    }

    false
}

/// Resolve a property signature to a ResolvedProp.
fn resolve_property_signature(
    prop: &TSPropertySignature,
    base_offset: u32,
    source: &[u8],
) -> Option<ResolvedProp> {
    let key = get_property_key_span(&prop.key, base_offset)?;
    let optional = prop.optional;

    let types = prop
        .type_annotation
        .as_ref()
        .map(|ann| infer_runtime_type(&ann.type_annotation))
        .unwrap_or_else(|| vec![RuntimeType::Unknown]);

    // Full span from the property signature, adjusted by base_offset
    let span = Span {
        start: prop.span.start + base_offset,
        end: prop.span.end + base_offset,
    };

    let type_span = prop.type_annotation.as_ref().map(|ann| Span {
        start: ann.type_annotation.span().start + base_offset,
        end: ann.type_annotation.span().end + base_offset,
    });
    let type_text = prop
        .type_annotation
        .as_ref()
        .and_then(|ann| span_text(source, ann.type_annotation.span().into()));

    Some(ResolvedProp {
        span,
        key,
        key_name: get_property_key_name(&prop.key),
        optional,
        types,
        visibility: ResolvedMemberVisibility::Public,
        type_span,
        type_text,
        map_local: true,
        span_is_absolute: base_offset != 0,
    })
}

/// Resolve a method signature to a ResolvedProp (methods are function-typed properties).
fn resolve_method_signature(
    method: &TSMethodSignature,
    base_offset: u32,
    source: &[u8],
) -> Option<ResolvedProp> {
    let key = get_property_key_span(&method.key, base_offset)?;
    let optional = method.optional;

    // Full span from the method signature, adjusted by base_offset
    let span = Span {
        start: method.span.start + base_offset,
        end: method.span.end + base_offset,
    };

    Some(ResolvedProp {
        span,
        key,
        key_name: get_property_key_name(&method.key),
        optional,
        types: vec![RuntimeType::Function],
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: callable_signature_text(
            source,
            &method.params.items,
            method
                .return_type
                .as_ref()
                .map(|return_type| &return_type.type_annotation),
        ),
        map_local: true,
        span_is_absolute: base_offset != 0,
    })
}

fn callable_signature_text<'a>(
    source: &[u8],
    params: &[FormalParameter<'a>],
    return_type: Option<&TSType<'a>>,
) -> Option<String> {
    let params = params
        .iter()
        .map(|param| {
            let name = span_text(source, param.pattern.span().into()).unwrap_or("_".to_string());
            let mut rendered = name.trim().to_string();
            if let Some(type_annotation) = &param.type_annotation {
                if let Some(type_text) =
                    span_text(source, type_annotation.type_annotation.span().into())
                {
                    rendered.push_str(": ");
                    rendered.push_str(type_text.trim());
                }
            }
            rendered
        })
        .collect::<Vec<_>>();
    let return_type = return_type
        .and_then(|return_type| span_text(source, return_type.span().into()))
        .unwrap_or_else(|| "void".to_string());
    Some(format!("({}) => {}", params.join(", "), return_type.trim()))
}

fn span_text(source: &[u8], span: Span) -> Option<String> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end || end > source.len() {
        return None;
    }
    std::str::from_utf8(&source[start..end])
        .ok()
        .map(ToString::to_string)
}

/// Extract the span of a property key.
fn get_property_key_span(key: &PropertyKey, base_offset: u32) -> Option<Span> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(Span {
            start: id.span.start + base_offset,
            end: id.span.end + base_offset,
        }),
        PropertyKey::StringLiteral(s) => Some(Span {
            start: s.span.start + base_offset,
            end: s.span.end + base_offset,
        }),
        PropertyKey::NumericLiteral(n) => Some(Span {
            start: n.span.start + base_offset,
            end: n.span.end + base_offset,
        }),
        // Computed keys are not supported
        _ => None,
    }
}

/// Infer runtime type(s) from a TypeScript type annotation.
///
/// Returns a list of possible runtime types. For union types,
/// this returns all possible types. For simple types, returns a single type.
pub fn infer_runtime_type(node: &TSType) -> Vec<RuntimeType> {
    match node {
        // Primitive types
        TSType::TSStringKeyword(_) => vec![RuntimeType::String],
        TSType::TSNumberKeyword(_) => vec![RuntimeType::Number],
        TSType::TSBooleanKeyword(_) => vec![RuntimeType::Boolean],
        TSType::TSObjectKeyword(_) => vec![RuntimeType::Object],
        TSType::TSSymbolKeyword(_) => vec![RuntimeType::Symbol],
        TSType::TSNullKeyword(_) => vec![RuntimeType::Null],
        TSType::TSUndefinedKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSVoidKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSAnyKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSUnknownKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSNeverKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSBigIntKeyword(_) => vec![RuntimeType::Number],

        // Literal types
        TSType::TSLiteralType(lit) => infer_literal_type(lit),

        // Object/interface types
        TSType::TSTypeLiteral(_) => vec![RuntimeType::Object],

        // Array types
        TSType::TSArrayType(_) | TSType::TSTupleType(_) => vec![RuntimeType::Array],

        // Function types
        TSType::TSFunctionType(_) | TSType::TSConstructorType(_) => vec![RuntimeType::Function],

        // Parenthesized: (Type)
        TSType::TSParenthesizedType(paren) => infer_runtime_type(&paren.type_annotation),

        // Union: Type1 | Type2
        TSType::TSUnionType(union) => {
            let mut types = Vec::new();
            for ty in &union.types {
                for t in infer_runtime_type(ty) {
                    if !types.contains(&t) {
                        types.push(t);
                    }
                }
            }
            types
        }

        // Intersection: Type1 & Type2 - typically results in Object
        TSType::TSIntersectionType(intersection) => {
            // For intersections, try to infer from all types
            let mut types = Vec::new();
            for ty in &intersection.types {
                for t in infer_runtime_type(ty) {
                    if t != RuntimeType::Unknown && !types.contains(&t) {
                        types.push(t);
                    }
                }
            }
            if types.is_empty() {
                vec![RuntimeType::Object]
            } else {
                types
            }
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(type_ref) => infer_type_reference(type_ref),

        // Conditional type: T extends U ? X : Y — union both branches
        TSType::TSConditionalType(cond) => {
            let mut types = infer_runtime_type(&cond.true_type);
            for t in infer_runtime_type(&cond.false_type) {
                if !types.contains(&t) {
                    types.push(t);
                }
            }
            if types.is_empty() {
                vec![RuntimeType::Unknown]
            } else {
                types
            }
        }

        // Mapped type: { [K in keyof T]: T[K] }
        TSType::TSMappedType(_) => vec![RuntimeType::Object],

        // Indexed access: T[K]
        TSType::TSIndexedAccessType(_) => vec![RuntimeType::Unknown],

        // Template literal type: `${string}`
        TSType::TSTemplateLiteralType(_) => vec![RuntimeType::String],

        // Type query: typeof x — in defineProps context, always refers to an object shape
        TSType::TSTypeQuery(_) => vec![RuntimeType::Object],

        // Import type: import("...").Type
        TSType::TSImportType(_) => vec![RuntimeType::Unknown],

        // Type operator: keyof T, readonly T, unique symbol
        TSType::TSTypeOperatorType(op) => {
            if matches!(op.operator, TSTypeOperatorOperator::Keyof) {
                // keyof usually results in string | number | symbol
                vec![
                    RuntimeType::String,
                    RuntimeType::Number,
                    RuntimeType::Symbol,
                ]
            } else {
                infer_runtime_type(&op.type_annotation)
            }
        }

        // Infer type: infer T
        TSType::TSInferType(_) => vec![RuntimeType::Unknown],

        // This type
        TSType::TSThisType(_) => vec![RuntimeType::Object],

        // Intrinsic keyword
        TSType::TSIntrinsicKeyword(_) => vec![RuntimeType::Unknown],

        // Catch-all for any new types
        _ => vec![RuntimeType::Unknown],
    }
}

/// Infer runtime type from a literal type.
fn infer_literal_type(lit: &TSLiteralType) -> Vec<RuntimeType> {
    match &lit.literal {
        TSLiteral::StringLiteral(_) => vec![RuntimeType::String],
        TSLiteral::NumericLiteral(_) => vec![RuntimeType::Number],
        TSLiteral::BooleanLiteral(_) => vec![RuntimeType::Boolean],
        TSLiteral::BigIntLiteral(_) => vec![RuntimeType::Number],
        TSLiteral::TemplateLiteral(_) => vec![RuntimeType::String],
        TSLiteral::UnaryExpression(unary) => {
            // -1, +1, etc.
            match &unary.argument {
                Expression::NumericLiteral(_) | Expression::BigIntLiteral(_) => {
                    vec![RuntimeType::Number]
                }
                _ => vec![RuntimeType::Unknown],
            }
        }
    }
}

/// Infer runtime type from a type reference.
fn infer_type_reference(type_ref: &TSTypeReference) -> Vec<RuntimeType> {
    let name = get_type_reference_name(&type_ref.type_name);

    match name.as_str() {
        // Built-in JavaScript types
        "Array" | "ReadonlyArray" => vec![RuntimeType::Array],
        "Function" => vec![RuntimeType::Function],
        "Object" => vec![RuntimeType::Object],
        "String" => vec![RuntimeType::String],
        "Number" => vec![RuntimeType::Number],
        "Boolean" => vec![RuntimeType::Boolean],
        "Symbol" => vec![RuntimeType::Symbol],

        // Built-in object types
        "Date" | "RegExp" | "Error" | "Map" | "Set" | "WeakMap" | "WeakSet" | "Promise" => {
            vec![RuntimeType::BuiltIn(name)]
        }

        // TypeScript utility types
        "Partial" | "Required" | "Readonly" | "Record" | "Pick" | "Omit" | "InstanceType" => {
            vec![RuntimeType::Object]
        }
        "Parameters" | "ConstructorParameters" => vec![RuntimeType::Array],
        "ReturnType" => vec![RuntimeType::Unknown],
        "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize" => vec![RuntimeType::String],
        "NonNullable" => {
            // Try to infer from the type parameter
            if let Some(args) = &type_ref.type_arguments {
                if let Some(first) = args.params.first() {
                    return infer_runtime_type(first)
                        .into_iter()
                        .filter(|t| *t != RuntimeType::Null)
                        .collect();
                }
            }
            vec![RuntimeType::Unknown]
        }
        "Extract" => {
            // Extract<T, U> - returns U
            if let Some(args) = &type_ref.type_arguments {
                if let Some(second) = args.params.get(1) {
                    return infer_runtime_type(second);
                }
            }
            vec![RuntimeType::Unknown]
        }
        "Exclude" | "OmitThisParameter" => {
            // Exclude<T, U> - returns T without U
            if let Some(args) = &type_ref.type_arguments {
                if let Some(first) = args.params.first() {
                    return infer_runtime_type(first);
                }
            }
            vec![RuntimeType::Unknown]
        }

        // Unknown type reference - can't resolve without scope
        _ => vec![RuntimeType::Unknown],
    }
}

/// Extract type names from interface heritage/extends clauses.
fn extract_heritage_type_names(extends: &[TSInterfaceHeritage]) -> Vec<String> {
    extends
        .iter()
        .filter_map(|heritage| match &heritage.expression {
            Expression::Identifier(id) => Some(id.name.to_string()),
            _ => None,
        })
        .collect()
}

/// Get the name from a type reference's type name.
///
/// For qualified names like `Namespace.Props`, returns the full path
/// (`"Namespace.Props"`) by recursively walking the left side.
fn get_type_reference_name(type_name: &TSTypeName) -> String {
    match type_name {
        TSTypeName::IdentifierReference(id) => id.name.to_string(),
        TSTypeName::QualifiedName(qualified) => {
            let left = get_type_reference_name(&qualified.left);
            format!("{}.{}", left, qualified.right.name)
        }
        TSTypeName::ThisExpression(_) => "this".to_string(),
    }
}

/// Resolve a value declaration's type shape (for `typeof X` support).
///
/// Looks for variable declarations matching `type_name` in both exported and
/// non-exported positions. If the variable has a type annotation, resolves that.
/// Otherwise, if it has an object literal initializer, infers prop types from
/// the property values.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn resolve_value_declaration_type<'ctx, 'a: 'ctx>(
    type_name: &str,
    program: &'ctx Program<'a>,
    source_bytes: &[u8],
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Option<ResolvedElements> {
    let name_bytes = type_name.as_bytes();

    for stmt in &program.body {
        // Check both `export const X` and plain `const X`
        let var_decl = match stmt {
            Statement::ExportNamedDeclaration(export) => match &export.declaration {
                Some(Declaration::VariableDeclaration(decl)) => Some(decl.as_ref()),
                _ => None,
            },
            Statement::VariableDeclaration(decl) => Some(decl.as_ref()),
            _ => None,
        };

        if let Some(decl) = var_decl {
            for declarator in &decl.declarations {
                let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                    continue;
                };
                if id.name.as_bytes() != name_bytes {
                    continue;
                }

                // 1. Type annotation on the declarator: `const X: { foo: string } = ...`
                if let Some(ref annotation) = declarator.type_annotation {
                    return Some(resolve_type_elements_with_ctx_ref(
                        &annotation.type_annotation,
                        base_offset,
                        ctx,
                    ));
                }

                // 2. Object literal initializer: `const X = { foo: 'str', bar: 42 }`
                if let Some(Expression::ObjectExpression(obj)) = &declarator.init {
                    return Some(infer_props_from_object_literal(obj, source_bytes));
                }
            }
        }
    }

    None
}

/// Infer prop types from an object literal's property values.
fn infer_props_from_object_literal(
    obj: &oxc_ast::ast::ObjectExpression<'_>,
    _source_bytes: &[u8],
) -> ResolvedElements {
    let mut result = ResolvedElements {
        root_runtime_types: vec![RuntimeType::Object],
        ..ResolvedElements::default()
    };

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        let key_span: oxc_span::Span = p.key.span();
        let runtime_type = match &p.value {
            Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => {
                vec![RuntimeType::String]
            }
            Expression::NumericLiteral(_) => vec![RuntimeType::Number],
            Expression::BooleanLiteral(_) => vec![RuntimeType::Boolean],
            Expression::ArrayExpression(_) => vec![RuntimeType::Array],
            Expression::ObjectExpression(_) => vec![RuntimeType::Object],
            Expression::NullLiteral(_) => vec![RuntimeType::Null],
            _ => vec![RuntimeType::Unknown],
        };

        result.props.push(ResolvedProp {
            span: crate::common::Span::new(key_span.start, key_span.end),
            key: crate::common::Span::new(key_span.start, key_span.end),
            key_name: None,
            types: runtime_type,
            optional: false,
            visibility: ResolvedMemberVisibility::Public,
            type_span: None,
            type_text: None,
            map_local: true,
            span_is_absolute: false,
        });
    }

    result
}

mod external;
use external::collect_type_reference_names;
pub use external::{
    analyze_external_type_program, analyze_external_type_source,
    collect_required_import_names_for_external_type, extract_export_surface,
    extract_imported_type_bindings, hash_resolved_type, imported_member_name_for_required_alias,
    required_import_alias_names_for_binding, resolve_external_type,
    resolve_external_type_in_context_with_analyzed_symbol_companion,
    resolve_external_type_in_program_with_analyzed_symbol_companion,
    resolve_external_type_with_companion, AnalyzedExternalTypeSource,
    AnalyzedExternalTypeSourceStats, AnalyzedExternalTypeSymbol, AnalyzedExternalTypeSymbolKind,
    ExtractedExportSurface, ExtractedTypeBindings, ImportedTypeBinding,
};

#[cfg(test)]
#[path = "../resolve_type_tests.rs"]
mod resolve_type_tests;
