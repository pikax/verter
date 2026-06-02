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
/// - `default_value` — the `withDefaults` defaults-object value text for this
///   prop, when present (drives `default: <expr>` in the runtime props object).
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
    /// `withDefaults` default-value expression text for this prop, if any.
    /// Drives the `default: <expr>` entry in the runtime props object.
    pub default_value: Option<String>,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroEmitDto {
    /// Event name. Mirrors `ResolvedEmit.name`.
    pub name: String,
    /// Resolved payload shape (call params vs tuple vs none).
    pub payload: MacroEmitPayload,
    /// Flat rendered payload type text (inner text of `payload`; empty for
    /// `None`). Display/codegen text — consumers must not re-parse it.
    pub payload_ts: String,
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
/// `nativeProps`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroPropsSurface {
    /// Resolved props, in declaration order.
    pub props: Vec<MacroPropDto>,
    /// Runtime constructor kinds inferred for the *root* type annotation.
    /// Mirrors `ResolvedElements.root_runtime_types`; an `Object` entry marks
    /// an empty-but-object-like props type as valid.
    pub root_constructors: Vec<RuntimeCtorKind>,
    /// Native-only props, in declaration order: the class-member visibility +
    /// span surface re-sourced by the `native_props` FFI carrier for
    /// `@verter/component-meta`'s `nativeProps`.
    pub native_props: Vec<MacroNativePropDto>,
    /// Whether the props type argument resolved to nothing (drives
    /// `XInvalidMacroType`). Mirrors `MacroTypeParams.unresolved_type_ref`.
    pub unresolved: bool,
}

/// The resolved `defineEmits` surface.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroEmitsSurface {
    /// Resolved emit events, in declaration order.
    pub emits: Vec<MacroEmitDto>,
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
