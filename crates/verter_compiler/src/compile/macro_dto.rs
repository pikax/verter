//! Compiler-owned macro-surface DTOs.
//!
//! These types are the owned, lifetime-free hand-off shape for a fully
//! resolved Vue SFC macro surface (`defineProps` / `defineEmits` / `defineSlots`
//! / `defineExpose` / `defineOptions`, plus `withDefaults`). They carry an owned
//! equivalent of everything the `verter_compiler` codegen paths read today from
//! the parser's `ResolvedElements` / `ResolvedProp` / `ResolvedEmit` /
//! `RuntimeType`:
//!
//! - the VDOM/runtime script path ([`crate::script::macros`]) — runtime props
//!   object (name, runtime constructors, required, default), emits array, and the
//!   `withDefaults` merge;
//! - the IDE TSX path ([`crate::tsc::script`]) — typed prop / emit / slot / expose
//!   surfaces (name, optionality, rendered TS type text);
//! - the diagnostics path ([`crate::compile`]) — the object-like check and the
//!   per-macro UNRESOLVED signal that drives `XInvalidMacroType`.
//!
//! Shape rationale: the bundle exposes **named per-kind surfaces** (props / emits
//! / slots / expose / options) rather than one heterogeneous keyed list. The
//! compiler's consumer sites are already split per macro kind
//! (`process_props` / `process_emits` / `process_slots` / `process_expose` /
//! `DefineOptions`), and each kind carries its own `unresolved` flag, so a named
//! surface per kind maps 1:1 onto the call sites without forcing any consumer to
//! filter a mixed collection.
//!
//! Every field is owned (`String` / `Vec` / `Option` / plain enums) — there are
//! no borrows, no lifetimes, no `&str`, and no `verter_session` / parser AST
//! types. This is the property that lets the session/host produce the surface
//! from the shared typed-IR dispatch and the `verter_compiler` codegen paths
//! consume it without either side leaking the other's internals. The
//! DTO-boundary invariant (`verter_compiler` must never depend on
//! `verter_session`) is pinned by
//! `crates/verter_compiler/tests/no_session_dependency.rs`.
//!
//! These types are the owned hand-off shape the `verter_compiler` codegen paths
//! route through in place of the parser's `ResolvedElements`.

/// Runtime constructor kind inferred for a macro prop's type, mirroring the
/// parser's `RuntimeType` (`verter_parser::utils::oxc::vue::RuntimeType`) on the
/// type-argument inference path (`infer_runtime_type`).
///
/// The compiler's VDOM/runtime path turns these into the JS constructor value
/// of a runtime prop declaration (`{ type: String }`, `{ type: [String, Number] }`).
/// The full variant set is verified against the parser's
/// `resolve_type/infer.rs`: every `RuntimeType` the inference path can emit has a
/// 1:1 counterpart here.
///
/// `BuiltIn(name)` carries the constructor identifier for recognised built-in
/// classes (e.g. `Date`, `Map`, `Set`) exactly as `RuntimeType::BuiltIn(String)`
/// does. `Unknown` is the un-inferable case (`RuntimeType::Unknown`); a consumer
/// rendering the runtime value filters it out (yielding `null`), matching
/// `format_runtime_types`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuntimeCtorKind {
    /// `String` constructor (`RuntimeType::String`).
    String,
    /// `Number` constructor (`RuntimeType::Number`).
    Number,
    /// `Boolean` constructor (`RuntimeType::Boolean`).
    Boolean,
    /// `Object` constructor (`RuntimeType::Object`).
    Object,
    /// `Array` constructor (`RuntimeType::Array`).
    Array,
    /// `Function` constructor (`RuntimeType::Function`).
    Function,
    /// `Symbol` constructor (`RuntimeType::Symbol`).
    Symbol,
    /// `null` literal type (`RuntimeType::Null`).
    Null,
    /// Recognised built-in class constructor, e.g. `Date` / `Map` / `Set`
    /// (`RuntimeType::BuiltIn(String)`). Carries the constructor name.
    BuiltIn(String),
    /// Type that could not be reduced to a runtime constructor
    /// (`RuntimeType::Unknown`). Rendered as `null` / filtered by consumers.
    Unknown,
}

impl RuntimeCtorKind {
    /// The JavaScript constructor identifier for this kind, matching
    /// `RuntimeType::as_str`. `Null` and `Unknown` both render as `null`.
    pub fn as_constructor(&self) -> &str {
        match self {
            RuntimeCtorKind::String => "String",
            RuntimeCtorKind::Number => "Number",
            RuntimeCtorKind::Boolean => "Boolean",
            RuntimeCtorKind::Object => "Object",
            RuntimeCtorKind::Array => "Array",
            RuntimeCtorKind::Function => "Function",
            RuntimeCtorKind::Symbol => "Symbol",
            RuntimeCtorKind::BuiltIn(name) => name,
            RuntimeCtorKind::Null => "null",
            RuntimeCtorKind::Unknown => "null",
        }
    }
}

/// Visibility of a macro prop that originated from a class member, mirroring the
/// parser's `ResolvedMemberVisibility` (`Public` / `Protected` / `Private`) and
/// the OXC `TSAccessibility` mapping in `resolve_type/decl.rs`.
///
/// Interface / type-literal members are always [`MacroVisibility::Public`]. This
/// is the field the `native_props` FFI carrier
/// (`ResolvedMacroMeta.native_props` → `FfiResolvedNativeProp.visibility`)
/// requires: the session/host populates it from the resolved class member and
/// the `native_props` FFI carrier re-sources its visibility (alongside name /
/// optional / type / span) from this surface, so the visibility fact must
/// survive on the prop surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MacroVisibility {
    /// `public` member (or any non-class / interface member — the default).
    #[default]
    Public,
    /// `protected` class member.
    Protected,
    /// `private` class member.
    Private,
}

impl MacroVisibility {
    /// Whether this member is publicly visible. Mirrors
    /// `ResolvedMemberVisibility::is_public`.
    pub const fn is_public(self) -> bool {
        matches!(self, Self::Public)
    }

    /// The lowercase wire string for this visibility, matching the FFI
    /// `member_visibility_to_string` mapping consumed by `native_props`.
    pub const fn as_wire_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Protected => "protected",
            Self::Private => "private",
        }
    }
}

/// A half-open SFC-absolute byte-offset span, `[start, end)`.
///
/// Both offsets are measured against the **whole SFC source**, not a
/// block-relative coordinate: the producer normalises every span it emits onto
/// SFC-absolute coordinates before constructing this DTO, so the consumer can
/// slice the original SFC source directly (e.g. to recover the exact default
/// expression text or to drive a source map back onto the SFC) without
/// re-resolving block offsets. `start < end` for any real span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacroSourceSpanDto {
    /// SFC-absolute start byte offset (inclusive).
    pub start: u32,
    /// SFC-absolute end byte offset (exclusive).
    pub end: u32,
}

/// How a `withDefaults` default value was written for a single prop.
///
/// Method shorthand (`{ items() { return [] } }`) and an arrow/expression
/// default (`{ items: () => [] }`) are *not* interchangeable at the byte level:
/// a method-shorthand entry must be reconstructed as a function when the
/// runtime default object is generated, whereas an expression entry is spliced
/// verbatim. The consumer renders the default from [`MacroDefaultDto::expr`] +
/// this kind; it must NOT re-scan the expression text to re-derive whether the
/// source used method shorthand.
///
/// Rendering contract for the runtime default object (the consumer emits the
/// `default` key of one prop entry):
///
/// - [`MacroDefaultKindDto::Expression`] — the renderer emits `default: ` +
///   [`MacroDefaultDto::expr`] verbatim. For `expr == "() => []"` that yields
///   `default: () => []`.
/// - [`MacroDefaultKindDto::MethodShorthand`] — `expr` is the method VALUE TAIL
///   (the parameter list + body that follow the method name, e.g.
///   `() { return [] }`), NOT the full `name() { ... }` property text. The
///   renderer emits `default: ` + an arrow-converted form of `expr` (the value
///   tail with `=> ` inserted between the parameter list and the body), so
///   `expr == "() { return [] }"` yields `default: () => { return [] }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MacroDefaultKindDto {
    /// A value expression: `{ count: 0 }`, `{ items: () => [] }`. The renderer
    /// emits `default: ` + [`MacroDefaultDto::expr`] verbatim.
    Expression,
    /// ES method shorthand: `{ items() { return [] } }`. [`MacroDefaultDto::expr`]
    /// holds the method VALUE TAIL (`() { return [] }`), not the full
    /// `name() { ... }` text. The renderer emits `default: ` + an arrow-converted
    /// form of `expr` (yielding `default: () => { return [] }`), reconstructing a
    /// function default rather than splicing shorthand into a value position.
    MethodShorthand,
}

/// A single `withDefaults` default value, resolved for one prop.
///
/// - `expr` — the default-value source text, already normalised by the producer
///   (no surrounding key/punctuation). Display/codegen text — the consumer
///   renders it per `kind` (see [`MacroDefaultKindDto`]) and must NOT re-parse it
///   for semantics. Its exact content depends on `kind`:
///     - for [`MacroDefaultKindDto::Expression`] it is the value expression
///       verbatim (`false`, `() => []`, `new Date()`);
///     - for [`MacroDefaultKindDto::MethodShorthand`] it is the method VALUE TAIL
///       — the parameter list + body that follow the method name, e.g.
///       `() { return [] }` for the source `items() { return [] }`, NOT the full
///       `items() { return [] }` property text.
/// - `kind` — [`MacroDefaultKindDto`]: whether the source wrote an expression
///   or method shorthand. Drives how `expr` is reconstructed in the output.
/// - `span` — the SFC-absolute span of the default value in the original
///   source, so a source map can point the generated default back onto the SFC.
///   For a method-shorthand default the span covers the same value-tail region
///   `expr` was sliced from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDefaultDto {
    /// Default-value source text (key/punctuation stripped). For an
    /// [`MacroDefaultKindDto::Expression`] this is the value expression verbatim;
    /// for an [`MacroDefaultKindDto::MethodShorthand`] this is the method VALUE
    /// TAIL (`() { return [] }`), not the full `name() { ... }` text.
    /// Display/codegen text — consumers must not re-parse it for semantics.
    pub expr: String,
    /// Whether the default was written as an expression or method shorthand.
    pub kind: MacroDefaultKindDto,
    /// SFC-absolute span of the default value in the original source.
    pub span: MacroSourceSpanDto,
}

/// One `(prop name → default)` entry inside a `withDefaults` defaults object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDefaultEntryDto {
    /// Prop name the default applies to.
    pub name: String,
    /// The resolved default value for this prop.
    pub default: MacroDefaultDto,
}

/// The raw `withDefaults` second argument (the defaults object), as written.
///
/// - `expr` — the exact source text of the defaults-object argument, already
///   normalised by the producer. Display/codegen text — the consumer may emit
///   it verbatim (e.g. as the `mergeDefaults` argument) but must NOT re-parse it
///   to recover individual entries; [`MacroWithDefaultsDto::entries`] carries
///   the per-prop breakdown.
/// - `span` — the SFC-absolute span of that argument in the original source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDefaultsArgDto {
    /// Source text of the defaults-object argument. Display/codegen text —
    /// consumers must not re-parse it for semantics.
    pub expr: String,
    /// SFC-absolute span of the defaults-object argument.
    pub span: MacroSourceSpanDto,
}

/// Why the `withDefaults` defaults could not be fully resolved into per-prop
/// entries, and what the consumer should fall back to.
///
/// The producer resolves the defaults object into [`MacroDefaultEntryDto`]
/// entries when it can. When it cannot enumerate the entries statically, it
/// records this fallback so the consumer knows whether the original argument was
/// an object literal (it may still be spliced) or a runtime expression (it must
/// be passed through to `mergeDefaults` at runtime), without the consumer
/// re-classifying the argument text itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroDefaultsFallbackKindDto {
    /// The defaults argument was an object literal whose entries could not all
    /// be enumerated (e.g. a spread). The object text is still available.
    ObjectLiteral,
    /// The defaults argument was a non-object runtime expression — the consumer
    /// must defer to a runtime `mergeDefaults` call rather than splice entries.
    RuntimeExpression,
}

/// The fallback signal for a `withDefaults` call whose defaults the producer
/// could not fully resolve into per-prop entries.
///
/// - `kind` — [`MacroDefaultsFallbackKindDto`]: object literal vs runtime
///   expression.
/// - `suppress_unresolved_import_diagnostic` — when `true`, an unresolved
///   imported props type on this `withDefaults` call must NOT raise the
///   `XInvalidMacroType` "could not be resolved" diagnostic, because the runtime
///   defaults fallback makes the unresolved import non-fatal (mirrors the
///   `skip_unresolved_import_error` decision in the diagnostics path). The
///   consumer reads this flag directly; it must NOT re-derive the suppression by
///   re-scanning the defaults text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroDefaultsFallbackDto {
    /// Object-literal vs runtime-expression fallback classification.
    pub kind: MacroDefaultsFallbackKindDto,
    /// Whether an unresolved imported props type is non-fatal here (suppresses
    /// the `XInvalidMacroType` unresolved-import diagnostic).
    pub suppress_unresolved_import_diagnostic: bool,
}

/// The resolved `withDefaults(defineProps<T>(), { ... })` defaults surface.
///
/// Carries the owned breakdown of a `withDefaults` call so the runtime path can
/// generate the merged props object and the diagnostics path can decide
/// unresolved-import suppression, without re-parsing any source text:
///
/// - `arg` — the raw defaults-object argument ([`MacroDefaultsArgDto`]).
/// - `entries` — the per-prop `(name → default)` breakdown
///   ([`MacroDefaultEntryDto`]); empty when the producer could only record a
///   fallback.
/// - `fallback` — [`MacroDefaultsFallbackDto`], present only when the defaults
///   could not be fully enumerated into `entries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroWithDefaultsDto {
    /// The raw defaults-object argument as written.
    pub arg: MacroDefaultsArgDto,
    /// Per-prop default entries, in declaration order. Empty when only a
    /// `fallback` could be recorded.
    pub entries: Vec<MacroDefaultEntryDto>,
    /// Fallback signal, present when the defaults could not be fully enumerated.
    pub fallback: Option<MacroDefaultsFallbackDto>,
}

/// One binding brought into scope by a single type import in a macro type
/// argument's dependency graph.
///
/// A macro type argument (`defineProps<T>()`) may reference type names declared
/// in other modules. The consumer that re-renders the macro's type surface in a
/// standalone TS context (e.g. the IDE/TSX path) must re-emit the imports those
/// names came from. This enum mirrors the three TS import-binding forms so the
/// consumer can reconstruct an exact `import` statement per source. The consumer
/// renders the import from these structured fields and must NOT re-scan a
/// rendered import string to recover the bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroTypeImportBindingDto {
    /// A named import: `import { A }` (no alias) or `import { A as B }` (alias
    /// in `local`). `imported` is the exported name; `local` is the in-scope
    /// alias when it differs from `imported`.
    Named {
        /// The name as exported by the source module.
        imported: String,
        /// The local alias, present only when it differs from `imported`.
        local: Option<String>,
    },
    /// A default import: `import D from "..."`. `local` is the in-scope name.
    Default {
        /// The local name bound to the module's default export.
        local: String,
    },
    /// A namespace import: `import * as NS from "..."`. `local` is the in-scope
    /// namespace name.
    Namespace {
        /// The local namespace name.
        local: String,
    },
}

/// A single type import reachable from a macro type argument's dependency graph.
///
/// - `source` — the import specifier (module path) as written.
/// - `bindings` — the bindings this import statement brings into scope
///   ([`MacroTypeImportBindingDto`]).
///
/// The consumer reconstructs the `import` statement from these structured
/// fields; it must NOT re-scan a rendered import string to recover the source or
/// bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroTypeImportDto {
    /// Import specifier (module path) as written.
    pub source: String,
    /// Bindings this import brings into scope, in declaration order.
    pub bindings: Vec<MacroTypeImportBindingDto>,
}

/// A single local type declaration reachable from a macro type argument's
/// dependency graph.
///
/// When a macro type argument references a type declared in the same module
/// (`type Foo = ...`, `interface Bar { ... }`), the consumer that re-renders the
/// macro's type surface in a standalone TS context must re-emit that declaration
/// too.
///
/// - `name` — the declared type name.
/// - `decl_ts` — the full rendered TS source of the declaration. Display/codegen
///   text — the consumer emits it verbatim and must NOT re-parse it for
///   semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroLocalTypeDeclDto {
    /// Declared type name.
    pub name: String,
    /// Full rendered TS source of the declaration. Display/codegen text —
    /// consumers must not re-parse it for semantics.
    pub decl_ts: String,
}

/// The transitive type-dependency closure of a single macro type argument.
///
/// A macro type argument (`defineProps<T>()` / `defineEmits<T>()`) may reference
/// type names declared in other modules or locally. To re-render that surface in
/// a standalone TS context the consumer needs those names in scope. This carries
/// the imports + local declarations the producer collected for the surface so
/// the consumer can re-emit them structurally:
///
/// - `imports` — the type imports the surface depends on
///   ([`MacroTypeImportDto`]).
/// - `local_declarations` — the local type declarations the surface depends on
///   ([`MacroLocalTypeDeclDto`]).
///
/// The consumer renders imports + declarations from these structured fields and
/// must NOT re-scan the rendered text strings to recover dependency structure.
/// An absent dependency closure is `Default` (both lists empty).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MacroTypeDepsDto {
    /// Type imports the surface depends on, in discovery order.
    pub imports: Vec<MacroTypeImportDto>,
    /// Local type declarations the surface depends on, in discovery order.
    pub local_declarations: Vec<MacroLocalTypeDeclDto>,
}

/// How the `defineProps` *root* props type is rendered onto the TSX `$props`
/// surface, the owned replacement for the parser-derived `PropsTs` root text.
///
/// The IDE/TSX path emits the component instance's `$props` member from this
/// surface. The three variants distinguish how that root text is sourced, so the
/// consumer renders the right `$props` annotation without re-classifying any type
/// text itself:
///
/// - [`MacroPropsTypeDto::Inline`] — there is no single named/raw root type; the
///   root is reconstructed structurally from the per-prop surface
///   ([`MacroPropsSurface::props`]). `ts` is `None`: the consumer renders the
///   `$props` object from the individual props rather than emitting a root text.
/// - [`MacroPropsTypeDto::TypeRef`] — the root is a named public root the
///   producer already rendered (e.g. `$props: PublicProps & Props`); `ts` is that
///   exact rendered text. The consumer emits it verbatim and re-emits `deps`.
/// - [`MacroPropsTypeDto::TypeText`] — the root is a raw root text that is not a
///   plain object the per-prop surface can reconstruct: a `Record<string, T>`, a
///   mapped / index-signature type, an empty `{}`, or an unresolved raw type. `ts`
///   is that raw text, emitted verbatim with `deps` re-emitted.
///
/// In every source-backed variant `deps` is the full import + local-declaration
/// closure for the root text ([`MacroTypeDepsDto`]) so the consumer can re-render
/// the root in a standalone TS context, and `span` is `Some` for a source-backed
/// macro type argument — `None` only for a source-less synthetic root. The
/// consumer renders from `ts` + structured `deps`; it must NOT re-scan the
/// rendered root text to recover dependency structure or re-classify the variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroPropsTypeDto {
    /// The root is rendered from the per-prop surface ([`MacroPropsSurface::props`]) —
    /// there is no single root type text. `ts` is `None`.
    Inline {
        /// Always `None`: the root has no standalone text; render from the props.
        ts: Option<String>,
        /// Type-dependency closure for the root, if any names need re-emitting.
        deps: MacroTypeDepsDto,
        /// SFC-absolute span of the source-backed macro type argument; `None` for
        /// a source-less synthetic root.
        span: Option<MacroSourceSpanDto>,
    },
    /// The root is a named public root the producer rendered (e.g.
    /// `$props: PublicProps & Props`). `ts` is that exact text.
    TypeRef {
        /// The rendered named-root text, emitted verbatim onto `$props`.
        ts: String,
        /// Type-dependency closure for the root text.
        deps: MacroTypeDepsDto,
        /// SFC-absolute span of the source-backed macro type argument; `None` for
        /// a source-less synthetic root.
        span: Option<MacroSourceSpanDto>,
    },
    /// The root is a raw root text (`Record<string, T>`, mapped / index-signature,
    /// `{}`, or an unresolved raw type). `ts` is that raw text.
    TypeText {
        /// The raw root text, emitted verbatim onto `$props`.
        ts: String,
        /// Type-dependency closure for the root text.
        deps: MacroTypeDepsDto,
        /// SFC-absolute span of the source-backed macro type argument; `None` for
        /// a source-less synthetic root.
        span: Option<MacroSourceSpanDto>,
    },
}

/// A macro type-argument carrier (`define*<T>()`), carrying the rendered argument
/// text plus its dependency closure and source span.
///
/// This is the owned form of a macro's `<T>` type argument as written, used where
/// a consumer needs the argument itself (not the resolved per-member surface) — in
/// particular the `defineEmits<T>()` diagnostics path, which reports on the type
/// argument as a whole. The producer renders `ts` once; the consumer emits it
/// verbatim and re-emits `deps`, and must NOT re-parse `ts` for semantics.
///
/// - `ts` — the rendered TS text of the `<T>` type argument as written.
/// - `deps` — the import + local-declaration closure the argument depends on
///   ([`MacroTypeDepsDto`]), so the argument can be re-rendered in a standalone TS
///   context.
/// - `span` — the SFC-absolute span of the source-backed type argument; `None`
///   only for a source-less synthetic argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroTypeArgDto {
    /// Rendered TS text of the `<T>` type argument. Display/codegen text —
    /// consumers must not re-parse it for semantics.
    pub ts: String,
    /// Type-dependency closure for the type argument.
    pub deps: MacroTypeDepsDto,
    /// SFC-absolute span of the source-backed type argument; `None` for a
    /// source-less synthetic argument.
    pub span: Option<MacroSourceSpanDto>,
}

/// A single resolved prop on the `defineProps` / `withDefaults` surface.
///
/// Carries the owned equivalent of every field the compiler reads from the
/// parser's `ResolvedProp`:
///
/// - `name` — `ResolvedProp.key_name` (pre-resolved key); the runtime + TSX
///   paths key the generated prop entry on it.
/// - `optional` / `required` — `ResolvedProp.optional`; the runtime path emits
///   `required: true` for non-optional props, the TSX path emits the `?` marker.
///   `required` is the explicit positive form (`!optional`) so consumers do not
///   re-derive it; the producer keeps the two consistent.
/// - `default` — the resolved `withDefaults` default for this prop, when present
///   ([`MacroDefaultDto`]): the default expression text, whether it was written
///   as an expression or method shorthand, and the SFC-absolute span of the
///   default value. Drives `default: <expr>` / `default() { ... }` in the
///   runtime props object. The producer normalises the span SFC-absolute; the
///   consumer renders from the structured default and must NOT re-scan the
///   expression text to re-classify it.
/// - `map_span` — the SFC-absolute span of this prop's declaration in the
///   original source ([`MacroSourceSpanDto`]), when the prop originates from a
///   real source location. The TSX path uses it to drive a source map from the
///   generated prop entry back onto the SFC; `None` for synthesised props that
///   have no source span.
/// - `ts_type_deps` — the transitive type-dependency closure of this prop's
///   type ([`MacroTypeDepsDto`]): the imports + local declarations a consumer
///   must re-emit to re-render the prop's type in a standalone TS context.
/// - `constructors` — `ResolvedProp.types` lowered to [`RuntimeCtorKind`];
///   rendered as the runtime `{ type: ... }` value via `format_runtime_types`.
/// - `ts_type` — the rendered TypeScript type text the TSX path emits for this
///   prop (today derived by `render_resolved_prop_ts_type` from
///   `type_text` / `type_span` / indexed-access fallback / runtime types). The
///   producer renders it once into this owned field.
/// - `declared_in_macro_type_arg` — `ResolvedProp.declared_in_macro_type_arg`;
///   the structural fact that the member appeared in the macro T's own body
///   (vs reached via heritage / Omit / intersection). Component-meta's `Refined`
///   policy and fallthrough root-inheritance read it.
/// - `jsdoc` — leading JSDoc comment text for the prop, when present (the TSX
///   path attaches it via `find_leading_jsdoc`).
/// - `visibility` — [`MacroVisibility`]; class-member visibility required by the
///   `native_props` FFI carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroPropDto {
    /// Prop name (resolved key). Mirrors `ResolvedProp.key_name`.
    pub name: String,
    /// Whether the prop is optional (`?`). Mirrors `ResolvedProp.optional`.
    pub optional: bool,
    /// Whether the prop is required (the positive form of `!optional`). Kept
    /// explicit so runtime codegen does not re-derive it; the producer keeps
    /// `required == !optional`.
    pub required: bool,
    /// Resolved `withDefaults` default for this prop, if any. Carries the
    /// default expression text, whether it was written as an expression or
    /// method shorthand, and the SFC-absolute span of the default value. Drives
    /// the `default: <expr>` / `default() { ... }` entry in the runtime props
    /// object.
    pub default: Option<MacroDefaultDto>,
    /// SFC-absolute span of this prop's declaration in the original source, if
    /// the prop has a real source location. The TSX path drives a source map
    /// from the generated prop entry back onto the SFC with it; `None` for
    /// synthesised props.
    pub map_span: Option<MacroSourceSpanDto>,
    /// Transitive type-dependency closure of this prop's type: imports + local
    /// declarations a consumer must re-emit to re-render the type in a
    /// standalone TS context. The consumer renders from the structured deps and
    /// must NOT re-scan the rendered text strings.
    pub ts_type_deps: MacroTypeDepsDto,
    /// Runtime constructor kinds inferred for this prop's type, in order.
    /// Lowered from `ResolvedProp.types`. Rendered as the runtime `{ type: ... }`
    /// value.
    pub constructors: Vec<RuntimeCtorKind>,
    /// Rendered TypeScript type text the IDE/TSX path emits for this prop.
    /// Display/codegen text — consumers must not re-parse it for semantics.
    pub ts_type: String,
    /// Structural fact: member declared in the macro type argument's own body
    /// (vs reached via heritage / Omit / intersection). Mirrors
    /// `ResolvedProp.declared_in_macro_type_arg`.
    pub declared_in_macro_type_arg: bool,
    /// Leading JSDoc comment text for the prop, if present.
    pub jsdoc: Option<String>,
    /// Class-member visibility. Public for interface / type-literal members.
    /// Required by the `native_props` FFI carrier.
    pub visibility: MacroVisibility,
}

/// The resolved payload shape of a single `defineEmits` event, mirroring the
/// parser's `ResolvedEmitSignature` (call-signature params vs shorthand tuple).
///
/// The IDE/TSX path renders the handler / `$emit` overload from this payload;
/// the text is preserved so cross-file imports inline the exact payload type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroEmitPayload {
    /// No extra payload beyond the event name.
    None,
    /// Call-signature payload — the parameter list text *after* the leading
    /// event-name parameter (`(e: 'change', id: number): void` → `id: number`).
    /// Mirrors `ResolvedEmitSignature::Call { params_text }`.
    Call { params_ts: String },
    /// Shorthand tuple payload, including the surrounding `[...]`
    /// (`{ change: [id: number] }` → `[id: number]`). Mirrors
    /// `ResolvedEmitSignature::Tuple { tuple_text }`.
    Tuple { tuple_ts: String },
}

/// A single resolved emit event on the `defineEmits` surface.
///
/// Carries the owned equivalent of every field the compiler reads from
/// `ResolvedEmit`:
///
/// - `name` — `ResolvedEmit.name`; the runtime path emits it into the emits
///   array (`["change", ...]`), the TSX path keys the emit overload on it.
/// - `payload` — the resolved payload shape ([`MacroEmitPayload`]) derived from
///   `ResolvedEmit.signature`.
/// - `payload_ts` — the flat rendered payload text the TSX path consumes
///   directly (the inner text of `payload`, or empty for [`MacroEmitPayload::None`]).
///   Kept as a convenience field so consumers that only need the text do not
///   match on `payload`.
/// - `map_span` — the SFC-absolute span of this emit's declaration in the
///   original source ([`MacroSourceSpanDto`]), when the emit has a real source
///   location. The TSX path drives a source map from the generated emit overload
///   back onto the SFC with it; `None` for synthesised emits.
/// - `payload_deps` — the transitive type-dependency closure of this emit's
///   payload type ([`MacroTypeDepsDto`]): the imports + local declarations a
///   consumer must re-emit to re-render the payload type in a standalone TS
///   context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroEmitDto {
    /// Event name. Mirrors `ResolvedEmit.name`.
    pub name: String,
    /// Resolved payload shape (call params vs tuple vs none).
    pub payload: MacroEmitPayload,
    /// Flat rendered payload type text (inner text of `payload`; empty for
    /// `None`). Display/codegen text — consumers must not re-parse it.
    pub payload_ts: String,
    /// SFC-absolute span of this emit's declaration in the original source, if
    /// the emit has a real source location. The TSX path drives a source map
    /// from the generated emit overload back onto the SFC with it; `None` for
    /// synthesised emits.
    pub map_span: Option<MacroSourceSpanDto>,
    /// Transitive type-dependency closure of this emit's payload type: imports +
    /// local declarations a consumer must re-emit to re-render the payload type
    /// in a standalone TS context. The consumer renders from the structured deps
    /// and must NOT re-scan the rendered text strings.
    pub payload_deps: MacroTypeDepsDto,
}

/// A single resolved slot on the `defineSlots` surface.
///
/// `defineSlots<T>()` resolves to a map of named slots whose value is a
/// function whose first parameter object is the slot's binding surface. The
/// IDE/TSX path renders `$slots` from this; the runtime path only needs the
/// slot names.
///
/// - `name` — the slot name.
/// - `bindings_ts` — the rendered TypeScript type text of the slot's binding
///   object (the first-parameter object of the slot function), if the slot
///   carries typed bindings.
/// - `slot_ts` — the full rendered TypeScript type text of the slot's function
///   type, preserved for the `$slots` surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroSlotDto {
    /// Slot name.
    pub name: String,
    /// Rendered TS type of the slot's binding object (first-parameter object),
    /// if any. Display/codegen text.
    pub bindings_ts: Option<String>,
    /// Full rendered TS type of the slot's function type. Display/codegen text.
    pub slot_ts: String,
}

/// A single native-only prop on the `defineProps` surface, carrying the
/// class-member visibility surface and source span that the `native_props` FFI
/// carrier requires.
///
/// This is the owned equivalent of the session-side native prop carrier
/// (`ResolvedNativeProp`): the session/host projects it from the eager OXC
/// resolved elements, and the `native_props` FFI carrier
/// (`FfiResolvedNativeProp`) re-sources every field here — `name`, `is_optional`,
/// `type_annotation`, `visibility`, and the span (`span_start` / `span_end`) —
/// onto the `@verter/component-meta` `nativeProps` surface.
///
/// Unlike [`MacroPropDto`] (the published props/emits/slots surface), this
/// carrier exists solely for the native `nativeProps` consumer, which the
/// published surface does not cover: it preserves private/protected member
/// visibility and the member's source span.
///
/// - `name` — the member name.
/// - `is_optional` — whether the member is optional (`?`).
/// - `type_annotation` — the rendered TS type annotation text, if any.
///   Display/codegen text — consumers must not re-parse it for semantics.
/// - `visibility` — [`MacroVisibility`]; preserves `public` / `protected` /
///   `private` class-member visibility.
/// - `span_start` / `span_end` — the member's SFC-absolute byte-offset span
///   (`[start, end)`, half-open), re-sourced onto the FFI `span_start` /
///   `span_end` fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroNativePropDto {
    /// Member name.
    pub name: String,
    /// Whether the member is optional (`?`).
    pub is_optional: bool,
    /// Rendered TS type annotation text, if any. Display/codegen text —
    /// consumers must not re-parse it for semantics.
    pub type_annotation: Option<String>,
    /// Class-member visibility (`public` / `protected` / `private`).
    pub visibility: MacroVisibility,
    /// SFC-absolute start byte offset of the member span (inclusive).
    pub span_start: u32,
    /// SFC-absolute end byte offset of the member span (exclusive).
    pub span_end: u32,
}

/// The resolved `defineProps` / `withDefaults` surface.
///
/// `unresolved` is the per-macro UNRESOLVED signal: `true` when the type
/// argument was a type reference that resolved to nothing (mirrors the parser's
/// `MacroTypeParams.unresolved_type_ref`). It drives the `XInvalidMacroType`
/// diagnostic in [`crate::compile`]. `root_constructors` mirrors
/// `ResolvedElements.root_runtime_types`, used by the object-like check
/// (`props_type_is_object_like`) to accept an empty-but-object-like props type.
/// `native_props` carries the native-only class-member visibility + span surface
/// that the `native_props` FFI carrier re-sources for `@verter/component-meta`'s
/// `nativeProps`. `with_defaults` carries the resolved `withDefaults(...)` surface
/// when the props were declared through a `withDefaults` call, so the runtime
/// path can generate the merged props object and the diagnostics path can decide
/// unresolved-import suppression without re-parsing the defaults source.
/// `props_type` carries the owned `$props` root surface ([`MacroPropsTypeDto`])
/// the IDE/TSX path renders in place of the parser-derived `PropsTs` root text:
/// an inline root reconstructed from `props`, a named public root, or a raw root
/// text. `None` when no `defineProps` root was declared.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroPropsSurface {
    /// Resolved props, in declaration order.
    pub props: Vec<MacroPropDto>,
    /// The `$props` root surface the IDE/TSX path renders in place of the
    /// parser-derived `PropsTs` root text: an inline root (rendered from `props`),
    /// a named public root (e.g. `$props: PublicProps & Props`), or a raw root
    /// text (`Record<string, T>`, mapped/index-signature, `{}`, or unresolved
    /// raw). `None` when no `defineProps` root was declared.
    pub props_type: Option<MacroPropsTypeDto>,
    /// Runtime constructor kinds inferred for the *root* type annotation.
    /// Mirrors `ResolvedElements.root_runtime_types`; an `Object` entry marks
    /// an empty-but-object-like props type as valid.
    pub root_constructors: Vec<RuntimeCtorKind>,
    /// Native-only props, in declaration order: the class-member visibility +
    /// span surface re-sourced by the `native_props` FFI carrier for
    /// `@verter/component-meta`'s `nativeProps`.
    pub native_props: Vec<MacroNativePropDto>,
    /// The resolved `withDefaults(...)` surface, present only when the props were
    /// declared through a `withDefaults` call ([`MacroWithDefaultsDto`]): the raw
    /// defaults argument, the per-prop `(name → default)` breakdown, and the
    /// unresolved-import fallback signal. `None` for a plain `defineProps` surface
    /// with no `withDefaults` wrapper.
    pub with_defaults: Option<MacroWithDefaultsDto>,
    /// Whether the props type argument resolved to nothing (drives
    /// `XInvalidMacroType`). Mirrors `MacroTypeParams.unresolved_type_ref`.
    pub unresolved: bool,
}

/// The resolved `defineEmits` surface.
///
/// `type_arg` carries the `defineEmits<T>()` type argument itself
/// ([`MacroTypeArgDto`] — rendered text + dependency closure + span) so the emit
/// diagnostics path is fully DTO-owned: it reports on the type argument as written
/// without re-reading any parser AST. `None` when emits were declared without a
/// `<T>` type argument (e.g. the runtime array form `defineEmits([...])`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroEmitsSurface {
    /// Resolved emit events, in declaration order.
    pub emits: Vec<MacroEmitDto>,
    /// The `defineEmits<T>()` type argument (rendered text + dependency closure +
    /// span), so emit diagnostics are fully DTO-owned. `None` when emits were
    /// declared without a `<T>` type argument.
    pub type_arg: Option<MacroTypeArgDto>,
    /// Whether the emits type argument resolved to nothing (drives
    /// `XInvalidMacroType`). Mirrors `MacroTypeParams.unresolved_type_ref`.
    pub unresolved: bool,
}

/// The resolved `defineSlots` surface.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroSlotsSurface {
    /// Resolved slots, in declaration order.
    pub slots: Vec<MacroSlotDto>,
    /// Whether the slots type argument resolved to nothing.
    pub unresolved: bool,
}

/// The resolved `defineExpose` surface.
///
/// Expose is a pass-through object surface — the IDE/TSX path emits its TS type
/// text verbatim onto the instance type. `type_ts` carries that rendered text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroExposeSurface {
    /// Rendered TS type text of the expose surface, if a typed expose was
    /// declared. Display/codegen text.
    pub type_ts: Option<String>,
    /// Whether the expose type argument resolved to nothing.
    pub unresolved: bool,
}

/// The resolved `defineOptions` surface.
///
/// Options is a pass-through object surface — the runtime path inlines its inner
/// object text into the component definition. `inner_ts` carries that text
/// (without the surrounding braces), matching the `DefineOptions` consumer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroOptionsSurface {
    /// Inner object text of the options object (braces stripped), if declared.
    /// Display/codegen text.
    pub inner_ts: Option<String>,
    /// Whether the options surface resolved to nothing.
    pub unresolved: bool,
}

/// The full per-SFC resolved macro-surface bundle.
///
/// This is the owned hand-off shape the session/host produces from the shared
/// typed-IR dispatch and the `verter_compiler` runtime / IDE-TSX / diagnostics
/// paths consume, in place of the parser's `ResolvedElements`. Each surface is
/// named per macro kind so it maps 1:1 onto the existing per-kind consumer
/// sites, and each surface carries its own `unresolved` flag (the
/// `XInvalidMacroType` driver).
///
/// A macro that is simply absent from the SFC is represented by its surface's
/// `Default` (empty collections, `unresolved == false`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedMacroSurfaces {
    /// `defineProps` / `withDefaults` surface.
    pub props: MacroPropsSurface,
    /// `defineEmits` surface.
    pub emits: MacroEmitsSurface,
    /// `defineSlots` surface.
    pub slots: MacroSlotsSurface,
    /// `defineExpose` surface.
    pub expose: MacroExposeSurface,
    /// `defineOptions` surface.
    pub options: MacroOptionsSurface,
}
