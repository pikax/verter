//! Framework-neutral local type-surface capture over OXC script ASTs.
//!
//! Lowers TypeScript type annotations that appear in a single script program
//! into structured, owned [`ResolvedElements`] surfaces: object members,
//! named call signatures, heritage closures, and runtime-type inference.
//! This is local OXC-to-owned script surface capture — NO host-backed query
//! resolution and NO cross-file semantic engine. Query-time type resolution
//! is owned exclusively by the shared typed-IR dispatch in `verter_session`;
//! this module only captures what the current program's source declares.
//!
//! When a referenced type lives in another file, the HOST pre-resolves it and
//! passes the result in via
//! [`VerterCompileOptions::external_types`](crate::VerterCompileOptions); the
//! pre-resolved data is merged into [`TypeResolutionContext::companion_types`]
//! so lookups for imported type names fall back to those owned surfaces —
//! the module itself never loads or walks other files.
//!
//! One deliberate Vue-owned dependency: the named-type memo seam
//! (`TypeResolutionContext::named_type_cache`) is typed by the
//! `NamedTypeCache` trait and cache-key identities owned by
//! `crate::utils::oxc::vue::script::named_type_keys` — that cache identity
//! backs the host's Vue resolved-named-type identity and is Vue semantics,
//! not part of this neutral surface capture.

#![allow(dead_code)]

use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use oxc_ast::ast::*;
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_type_expr::{TypeExpr, TypeExprScope};

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
    /// Lowered typed form of the prop's type annotation. Populated by the
    /// producer that has the OXC `TSType<'_>` AST node in scope (local-SFC
    /// inference or cross-file external resolver). Authoritative for
    /// downstream consumers — `type_text` is display-only.
    pub type_expr: Option<TypeExpr>,
    /// Scope of `type_expr`: canonical_id of the file whose OXC parse produced
    /// the typed expression. For local-SFC parses this is the owner SFC's
    /// canonical_id; for the external-resolution path this is the external
    /// file's canonical_id. Pairing invariant:
    /// `type_expr.is_some() <=> type_expr_scope.is_some()`.
    pub type_expr_scope: Option<TypeExprScope>,
    /// Whether this member was explicitly declared in the macro's type
    /// argument's own body (vs reached via heritage / Omit / intersection
    /// from an external source like an imported interface).
    ///
    /// Structural fact, threaded by the resolver chain
    /// (`resolve_interface_with_extends_ctx_ref`, prepared-surface walker)
    /// — `true` for members appearing in the macro T's own body (whether
    /// the T is a local interface, a cross-file imported interface, or an
    /// inline literal); `false` for members reaching the surface ONLY via
    /// heritage / Omit / intersection from sources outside the author's
    /// named structure. Consumers (component-meta `Refined` policy,
    /// fallthrough-attrs root inheritance) read this fact to preserve
    /// `class` / `style` / `on{Event}` shadows of declared emits.
    pub declared_in_macro_type_arg: bool,
}

/// A resolved emit event from defineEmits type parameter.
/// Supports both call signature style `{ (e: 'change', id: number): void }`
/// and shorthand style `{ change: [id: number] }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCallPayloadForm {
    /// Call signature payload params after the event name parameter.
    /// Empty string means the event carries no extra payload.
    Call { params_text: String },
    /// Shorthand tuple payload, including the surrounding `[...]`.
    Tuple { tuple_text: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedNamedCallSignature {
    /// Span of the entire emit signature
    pub span: Span,
    /// The event name (extracted from first parameter literal or property key)
    pub name: String,
    /// Span of the event name in source (if available, for string literal params)
    pub name_span: Option<Span>,
    /// The resolved payload signature, preserved as text so consumers can
    /// inline exact handler / `$emit` types even for cross-file imports.
    pub signature: ResolvedCallPayloadForm,
    /// Whether this span points into the current SFC source and can be used
    /// directly for local source maps.
    pub map_local: bool,
    /// Whether spans on this emit are already SFC-absolute.
    pub span_is_absolute: bool,
    /// Lowered typed form of the emit's payload type. Populated by the
    /// producer that has the OXC `TSType<'_>` AST node in scope.
    /// Authoritative for downstream consumers — `signature` text is display-only.
    pub type_expr: Option<TypeExpr>,
    /// Scope of `type_expr`: canonical_id of the file whose OXC parse produced
    /// the typed expression. Pairing invariant:
    /// `type_expr.is_some() <=> type_expr_scope.is_some()`.
    pub type_expr_scope: Option<TypeExprScope>,
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
    /// Named call signatures: call-signature members (and function-typed
    /// shorthand properties) whose first parameter is a string-literal
    /// discriminant naming the signature.
    pub call_signatures: Vec<ResolvedNamedCallSignature>,
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

    /// Stamp `type_expr_scope` on every prop / emit whose `type_expr` is
    /// populated but whose scope is missing. Called by parser-boundary callers
    /// to attach the canonical_id of the file whose OXC parse produced the
    /// typed expression.
    ///
    /// The local-SFC parse path stamps the owner SFC's canonical_id; the
    /// external-resolution path is stamped inside
    /// `finalize_external_resolution_with_offset` with the external file's
    /// canonical_id.
    ///
    /// This method is the producer-side authority for the pairing invariant
    /// `type_expr.is_some() <=> type_expr_scope.is_some()` enforced by
    /// `assert_typed_form_populated`.
    pub fn stamp_type_expr_scope(&mut self, scope: &TypeExprScope) {
        for prop in &mut self.props {
            if prop.type_expr.is_some() && prop.type_expr_scope.is_none() {
                prop.type_expr_scope = Some(scope.clone());
            }
        }
        for emit in &mut self.call_signatures {
            if emit.type_expr.is_some() && emit.type_expr_scope.is_none() {
                emit.type_expr_scope = Some(scope.clone());
            }
        }
    }

    /// Assert that every `ResolvedProp` and `ResolvedNamedCallSignature` satisfies the typed
    /// form pairing invariant:
    /// - `type_expr.is_some() <=> type_expr_scope.is_some()`, and
    /// - `type_expr.is_some()` whenever `type_span.is_some() || type_text.is_some()`
    ///   (props) or `signature` carries a non-empty payload (emits).
    ///
    /// Returns `Ok(())` if every prop / emit complies. Returns a `Err(message)`
    /// listing every violator. Used for `debug_assert!` at the parser-boundary
    /// exit so consumers can `expect("type_expr+scope populated by parser")`
    /// at read time.
    pub fn assert_typed_form_populated(&self) -> Result<(), String> {
        let mut violators: Vec<String> = Vec::new();
        for prop in &self.props {
            let display_name = prop
                .key_name
                .clone()
                .unwrap_or_else(|| "<anonymous>".to_string());
            if prop.type_expr.is_some() != prop.type_expr_scope.is_some() {
                violators.push(format!(
                    "ResolvedProp `{display_name}`: type_expr/type_expr_scope pairing violated (type_expr.is_some()={}, type_expr_scope.is_some()={})",
                    prop.type_expr.is_some(),
                    prop.type_expr_scope.is_some(),
                ));
            }
            if prop.type_expr.is_none() && (prop.type_span.is_some() || prop.type_text.is_some()) {
                violators.push(format!(
                    "ResolvedProp `{display_name}`: type_span/type_text present but type_expr is None"
                ));
            }
        }
        for emit in &self.call_signatures {
            if emit.type_expr.is_some() != emit.type_expr_scope.is_some() {
                violators.push(format!(
                    "ResolvedNamedCallSignature `{}`: type_expr/type_expr_scope pairing violated (type_expr.is_some()={}, type_expr_scope.is_some()={})",
                    emit.name,
                    emit.type_expr.is_some(),
                    emit.type_expr_scope.is_some(),
                ));
            }
        }
        if violators.is_empty() {
            Ok(())
        } else {
            Err(violators.join("\n"))
        }
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
    /// Optional canonical_id of the file whose source is being resolved by
    /// this context. When set, the lowering producer sites in
    /// `elements.rs` / `decl.rs` populate `ResolvedProp.type_expr_scope` /
    /// `ResolvedNamedCallSignature.type_expr_scope` with this value, completing the
    /// pairing invariant
    /// (`type_expr.is_some() <=> type_expr_scope.is_some()`) at construction
    /// time. When `None`, construction sites leave `type_expr_scope` as
    /// `None` and a downstream stamping helper
    /// (`ResolvedElements::stamp_type_expr_scope`) is responsible for
    /// completing the invariant before the result leaves the parser.
    owner_canonical: Option<TypeExprScope>,
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

use crate::utils::oxc::vue::named_type_keys::{
    self as cache_keys, ResolvedTypeParamBindingCacheKey,
};

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
    call_signatures: Arc<[ResolvedNamedCallSignature]>,
    has_call_signature: bool,
}

impl ShallowResolvedElements {
    fn apply_to(&self, result: &mut ResolvedElements) {
        result.props.extend(self.props.iter().cloned());
        result
            .call_signatures
            .extend(self.call_signatures.iter().cloned());
        if self.has_call_signature {
            result.has_call_signature = true;
        }
    }
}

impl From<ResolvedElements> for ShallowResolvedElements {
    fn from(value: ResolvedElements) -> Self {
        Self {
            props: Arc::from(value.props.into_boxed_slice()),
            call_signatures: Arc::from(value.call_signatures.into_boxed_slice()),
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
            owner_canonical: None,
            named_type_cache: None,
        }
    }

    /// Set the canonical_id of the file this context is resolving against.
    /// When set, construction-site lowering populates `type_expr_scope`
    /// atomically with `type_expr`, satisfying the pairing invariant at the
    /// producer site.
    pub fn set_owner_canonical(&mut self, canonical_id: impl Into<String>) {
        self.owner_canonical = Some(TypeExprScope::new(canonical_id));
    }

    /// Read the canonical_id of the file this context is resolving against.
    /// Returns `None` for standalone callers (tests, direct parsing) that
    /// have not bound the context to a file canonical_id.
    pub(super) fn owner_canonical_scope(&self) -> Option<&TypeExprScope> {
        self.owner_canonical.as_ref()
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
        from_root_body: bool,
    ) -> cache_keys::ResolvedNamedTypeCacheKey {
        cache_keys::ResolvedNamedTypeCacheKey {
            name: name.to_vec().into_boxed_slice(),
            surface: self.current_surface.clone(),
            base_offset,
            from_root_body,
            companion_cache_key: Arc::clone(&self.companion_cache_key),
            type_param_bindings: Arc::clone(&self.type_param_bindings_cache_key),
        }
    }

    fn cached_named_resolution(
        &self,
        name: &[u8],
        base_offset: u32,
        from_root_body: bool,
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
                    .get(&self.cache_key_for_name(name, base_offset, from_root_body))
                    .is_some()
                {
                    component_meta_core_trace_event(
                        "parser_cache_audit_hit",
                        format!(
                            "file={} name={} base_offset={} from_root_body={} bindings={} companions={}",
                            self.trace_label.as_deref().unwrap_or("<unknown>"),
                            String::from_utf8_lossy(name),
                            base_offset,
                            from_root_body,
                            self.type_param_bindings.len(),
                            self.companion_types.len(),
                        ),
                    );
                }
            }
        }
        self.named_type_cache
            .as_ref()?
            .get(&self.cache_key_for_name(name, base_offset, from_root_body))
    }

    fn store_named_resolution(
        &self,
        name: &[u8],
        base_offset: u32,
        from_root_body: bool,
        resolved: Arc<ResolvedElements>,
    ) {
        if let Some(cache) = self.named_type_cache.as_ref() {
            cache.insert(
                self.cache_key_for_name(name, base_offset, from_root_body),
                resolved,
            );
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
///
/// The resolver chain runs with `from_root_body = true`. This makes each
/// produced `ResolvedProp.declared_in_macro_type_arg` carry accurate
/// per-prop provenance:
///   - Own-body literal members → `true` (caller propagates `true` to
///     `resolve_type_literal_members`).
///   - Heritage-injected members (via `extends Omit<...>` etc.) → `false`
///     (the heritage-descent boundary inside
///     `resolve_interface_with_extends_ctx_ref` forces
///     `from_root_body = false` on every named-target lookup).
///
/// The consumer (`resolve_named_local_type_with_ctx_ref_inner`) preserves
/// these per-prop facts when the caller is at the macro-T root, and flips
/// every prop to `false` when the caller is itself at heritage descent.
/// This preserves the invariant that a companion's heritage-injected
/// members never reach a published surface as `declared_in_macro_type_arg
/// = true`, regardless of how the companion is consumed.
pub fn extract_companion_types(
    program: &Program<'_>,
    source: &[u8],
    content_offset: u32,
) -> rustc_hash::FxHashMap<String, ResolvedElements> {
    // Build a full type context so we can resolve extends and cross-references
    let ctx = build_type_context(program, source, content_offset);

    let mut types = rustc_hash::FxHashMap::default();

    // Companion definitions are resolved with `from_root_body = true` so each
    // resolved prop carries accurate per-prop provenance:
    //   - Own-body literal members: `declared_in_macro_type_arg = true`.
    //   - Heritage-injected members (via `extends Omit<...>` etc.): `false`,
    //     because the heritage-descent boundary inside `resolve_interface_with_extends_ctx_ref`
    //     forces `from_root_body = false` on every named-target lookup.
    // The consumer (`resolve_named_local_type_with_ctx_ref_inner`) preserves
    // these per-prop facts when the caller is at the macro-T root, and flips
    // every prop to `false` when the caller is itself at heritage descent.
    let from_root_body = true;

    for stmt in &program.body {
        match stmt {
            Statement::TSTypeAliasDeclaration(alias) => {
                let name = alias.id.name.as_str().to_string();
                let resolved = resolve_type_elements_with_ctx_ref(
                    &alias.type_annotation,
                    content_offset,
                    &ctx,
                    from_root_body,
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
                    from_root_body,
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
                        from_root_body,
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
                                from_root_body,
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
                                from_root_body,
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
                                    from_root_body,
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
/// * `from_root_body` - Whether `node` is the macro T's own body
///   (member-origin fact propagated as `declared_in_macro_type_arg`).
///   Top-level macro entry points (`defineProps<T>()` /
///   `defineEmits<T>()` / `defineSlots<T>()`) pass `true`.
///   `typeof` / companion / heritage-descent contexts pass `false`.
pub fn resolve_type_elements(
    node: &TSType,
    base_offset: u32,
    from_root_body: bool,
) -> ResolvedElements {
    let mut result = ResolvedElements::default();
    resolve_type_elements_inner(node, base_offset, &mut result, b"", from_root_body);
    result.root_runtime_types = infer_runtime_type(node);
    // Standalone (no-ctx) callers have no canonical_id; stamp the empty
    // scope so the pairing invariant holds without requiring callers to
    // know about the typed-form contract.
    let scope = TypeExprScope::new("");
    result.stamp_type_expr_scope(&scope);
    debug_assert!(
        result.assert_typed_form_populated().is_ok(),
        "resolve_type_elements must satisfy the typed-form pairing invariant: {}",
        result
            .assert_typed_form_populated()
            .err()
            .unwrap_or_default()
    );
    result
}

/// Resolve type elements with a type resolution context.
/// This version can resolve local type aliases and interfaces.
///
/// # Arguments
/// * `node` - The TSType node to resolve
/// * `base_offset` - The document offset to apply to all spans
/// * `ctx` - Type resolution context with local type definitions
/// * `from_root_body` - Whether `node` is the macro T's own body. See
///   [`resolve_type_elements`] for the propagation contract.
pub fn resolve_type_elements_with_ctx<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
) -> ResolvedElements {
    let mut result = ResolvedElements::default();
    resolve_type_elements_inner_with_ctx(node, base_offset, &mut result, ctx, from_root_body);
    result.root_runtime_types =
        resolve_root_runtime_type_with_ctx(node, ctx).unwrap_or_else(|| infer_runtime_type(node));
    let scope = ctx
        .owner_canonical_scope()
        .cloned()
        .unwrap_or_else(|| TypeExprScope::new(""));
    result.stamp_type_expr_scope(&scope);
    debug_assert!(
        result.assert_typed_form_populated().is_ok(),
        "resolve_type_elements_with_ctx must satisfy the typed-form pairing invariant: {}",
        result
            .assert_typed_form_populated()
            .err()
            .unwrap_or_default()
    );
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
/// * `from_root_body` - Whether `node` is the macro T's own body. See
///   [`resolve_type_elements`] for the propagation contract.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn resolve_type_elements_with_ctx_ref<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    from_root_body: bool,
) -> ResolvedElements {
    let mut result = ResolvedElements::default();
    resolve_type_elements_inner_with_ctx_ref(node, base_offset, &mut result, ctx, from_root_body);
    result.root_runtime_types = resolve_root_runtime_type_with_ctx_ref(node, ctx)
        .unwrap_or_else(|| infer_runtime_type(node));
    let scope = ctx
        .owner_canonical_scope()
        .cloned()
        .unwrap_or_else(|| TypeExprScope::new(""));
    result.stamp_type_expr_scope(&scope);
    debug_assert!(
        result.assert_typed_form_populated().is_ok(),
        "resolve_type_elements_with_ctx_ref must satisfy the typed-form pairing invariant: {}",
        result
            .assert_typed_form_populated()
            .err()
            .unwrap_or_default()
    );
    result
}

mod decl;
use decl::{
    get_expression_reference_name, resolve_class_with_heritage_ctx_ref,
    resolve_interface_with_extends_ctx_ref, resolve_named_local_type_with_ctx_ref,
    resolve_root_runtime_type_with_ctx, resolve_root_runtime_type_with_ctx_ref,
    resolve_type_elements_inner, resolve_type_elements_inner_with_ctx,
    resolve_type_elements_inner_with_ctx_ref,
};

mod elements;
use elements::{
    callable_signature_text, extract_string_literal_keys_with_ctx, get_property_key_name,
    get_property_key_span, has_immediate_vue_ignore_comment, resolve_mapped_type_with_ctx,
    resolve_type_literal_members, span_text,
};

mod infer;
pub use infer::infer_runtime_type;
use infer::{extract_heritage_type_names, get_type_reference_name, resolve_value_declaration_type};

mod external;
use external::collect_type_reference_names;
pub use external::{
    analyze_external_type_program, analyze_external_type_program_headers,
    analyze_external_type_source, collect_required_import_names_for_external_type,
    collect_statement_dependency_names, extract_export_surface, extract_imported_type_bindings,
    hash_resolved_type, imported_member_name_for_required_alias,
    required_import_alias_names_for_binding, resolve_external_type,
    resolve_external_type_in_context_with_analyzed_symbol_companion,
    resolve_external_type_in_context_with_analyzed_symbol_companion_and_canonical,
    resolve_external_type_in_program_with_analyzed_symbol_companion,
    resolve_external_type_in_program_with_analyzed_symbol_companion_and_canonical,
    resolve_external_type_with_canonical, resolve_external_type_with_companion,
    resolve_external_type_with_companion_and_canonical, AnalyzedExternalTypeSource,
    AnalyzedExternalTypeSourceStats, AnalyzedExternalTypeSymbol, AnalyzedExternalTypeSymbolKind,
    DeclDependencyNames, ExtractedExportSurface, ExtractedTypeBindings, ImportedTypeBinding,
};

#[cfg(test)]
#[path = "../type_surface_tests.rs"]
mod type_surface_tests;
