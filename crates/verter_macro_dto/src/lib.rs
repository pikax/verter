//! Dependency-neutral, span-free macro-codegen DTO vocabulary.
//!
//! These types are the owned semantic hand-off between the ONE shared
//! type-resolution engine (which produces a resolved macro surface) and the
//! SFC macro codegen paths (which consume it). The crate sits BELOW both
//! sides of that boundary: its production dependencies are exactly the two
//! structural marker crates, so a resolution-side producer and
//! `verter_compiler` can both name these types without either reaching the
//! other's dependency graph.
//!
//! # What the vocabulary carries — and what it deliberately does not
//!
//! Semantic-codegen data ONLY, per SFC, indexed per authored macro
//! occurrence:
//!
//! - WHICH macro surface an entry describes and WHERE it was authored
//!   ([`MacroCodegenEntry`]);
//! - WHETHER resolution produced a usable surface — the three-state
//!   [`MacroCodegenOutcome`] taxonomy, in which a resolved-but-EMPTY surface
//!   ([`MacroCodegenOutcome::Complete`] with no members/events) is a
//!   DIFFERENT fact from an unresolved or partial one;
//! - the per-member semantic facts codegen needs: prop name, the single
//!   positive `optional` fact, runtime constructor classification
//!   ([`RuntimeCtorKind`]); emit name and structured payload form
//!   ([`MacroEmitPayload`]);
//! - a CONTENT-FREE syntax anchor ([`MacroSyntaxAnchor`]) that lets a
//!   consumer re-associate a member/event with its authored position in the
//!   current SFC (e.g. to select a syntax span for source maps) WITHOUT this
//!   crate storing any byte offset.
//!
//! Excluded by design: spans and byte ranges (every aggregate is
//! `NoStoredSpan`); symbolic typed IR (every aggregate is `NoTypeExpr`);
//! parser/session/compiler AST types; display/rendered text that duplicates
//! a structured field; duplicated positive/negative fact pairs (requiredness
//! is the single positive `optional`); and compiler-local SYNTAX facts
//! (`withDefaults` default expressions, raw type-argument text, runtime
//! object/array macro arguments, native class-member surfaces, import
//! reconstruction, local source-map spans), which stay owned by
//! `verter_compiler` / the session.
//!
//! # Marker enforcement
//!
//! Every public aggregate derives BOTH `verter_no_typeexpr::NoTypeExpr` and
//! `verter_no_storedspan::NoStoredSpan`, so a field owning a transitive
//! `TypeExpr` or `verter_span::Span` fails to compile
//! (`tests/cases/marker_witnesses.rs`). The dependency-neutral closure is
//! pinned by the resolve-graph firewall guard in
//! `tests/cases/dependency_closure_guard.rs`.

#![forbid(unsafe_code)]

use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

/// The per-SFC top-level carrier: one [`MacroCodegenEntry`] per authored
/// macro occurrence the resolution engine produced an outcome for, in
/// authored source order.
///
/// An SFC without any resolvable macro surface is the `Default` (no
/// entries) — absence of a macro is the absence of its entry, never a
/// synthesized `Unresolved` row.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, NoTypeExpr, NoStoredSpan)]
pub struct ResolvedMacroCodegenBundle {
    /// Per-macro outcomes, in authored source order.
    pub entries: Vec<MacroCodegenEntry>,
}

/// One resolved macro occurrence: WHICH macro it is ([`MacroCodegenKind`]),
/// WHERE it was authored (`macro_index`), and WHAT resolution produced
/// ([`MacroCodegenOutcome`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct MacroCodegenEntry {
    /// Stable authored identity of this macro occurrence in the SFC: the
    /// source-order index of the macro call among the SFC's macro calls.
    /// Content-free — an identity for re-associating the entry with the
    /// authored syntax, never a byte offset.
    pub macro_index: u32,
    /// Which macro surface this entry describes. Authoritative even when the
    /// outcome carries no surface (`Partial` / `Unresolved`).
    pub kind: MacroCodegenKind,
    /// What resolution produced for this occurrence.
    pub outcome: MacroCodegenOutcome,
}

/// The macro surface family a [`MacroCodegenEntry`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroCodegenKind {
    /// A `defineProps` surface (including the props half of `withDefaults`).
    Props,
    /// A `defineEmits` surface.
    Emits,
}

/// The three-state resolution outcome for one macro occurrence.
///
/// The states are semantically DISTINCT and none may be coerced into
/// another:
///
/// - [`Complete`](Self::Complete) — resolution finished and the surface is
///   authoritative. A `Complete` surface with ZERO members/events means the
///   macro's type genuinely has an empty surface (resolved-empty); it is NOT
///   an unresolved or unavailable surface.
/// - [`Partial`](Self::Partial) — resolution ended early (e.g. a budget or
///   fence); whatever was gathered is NOT carried, because a partial surface
///   must never be consumed as an authoritative one.
/// - [`Unresolved`](Self::Unresolved) — the macro's type could not be
///   resolved at all (e.g. the type argument resolved to nothing).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroCodegenOutcome {
    /// Resolution finished; `surface` is the authoritative macro surface.
    /// An empty surface here is the resolved-empty fact, distinct from
    /// `Unresolved`.
    Complete(MacroCodegenSurface),
    /// Resolution ended early and produced no authoritative surface.
    /// `reason` is diagnostic display text only — consumers must not branch
    /// semantics on it.
    Partial {
        /// Human-readable diagnostic for WHY resolution ended early.
        reason: String,
    },
    /// The macro's type could not be resolved. `reason` is diagnostic
    /// display text only — consumers must not branch semantics on it.
    Unresolved {
        /// Human-readable diagnostic for WHY the type did not resolve.
        reason: String,
    },
}

/// The per-kind resolved surface carried by
/// [`MacroCodegenOutcome::Complete`]. The arm agrees with the owning
/// entry's [`MacroCodegenKind`]; the producer keeps the two consistent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroCodegenSurface {
    /// A resolved `defineProps` surface.
    Props(MacroPropsCodegenSurface),
    /// A resolved `defineEmits` surface.
    Emits(MacroEmitsCodegenSurface),
}

/// The resolved `defineProps` surface: the root-shape fact plus the member
/// list. `members` may be empty while the surface is still `Complete` —
/// an object-like props type with no members is resolved-empty, not
/// unresolved.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct MacroPropsCodegenSurface {
    /// Whether the props root type is object-like. Drives the object-like
    /// validity check without re-deriving it from members (an empty
    /// object-like root is valid; a non-object root is not).
    pub root_shape: MacroRootShape,
    /// Resolved props, in declaration order.
    pub members: Vec<MacroPropCodegen>,
}

/// Shape classification of the `defineProps` root type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroRootShape {
    /// The root type is object-like (an object/interface-shaped surface —
    /// including an EMPTY one).
    ObjectLike,
    /// The root type is not an object-like surface (e.g. a primitive or a
    /// bare unresolvable non-object expression).
    NonObject,
}

/// One resolved prop on the `defineProps` surface.
///
/// Requiredness is the SINGLE positive fact `optional` — there is
/// deliberately no redundant `required` twin; consumers derive `!optional`
/// where they need the positive form.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct MacroPropCodegen {
    /// Prop name (the resolved member key).
    pub name: String,
    /// Whether the prop is optional (`?`). The single requiredness fact.
    pub optional: bool,
    /// Runtime constructor kinds inferred for this prop's type, in order.
    /// Rendered by the runtime path as the `{ type: ... }` value.
    pub runtime_ctors: Vec<RuntimeCtorKind>,
    /// Content-free anchor back to this member's authored position.
    pub anchor: MacroSyntaxAnchor,
}

/// The resolved `defineEmits` surface. `events` may be empty while the
/// surface is still `Complete` (resolved-empty).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct MacroEmitsCodegenSurface {
    /// Resolved emit events, in declaration order.
    pub events: Vec<MacroEmitCodegen>,
}

/// One resolved emit event on the `defineEmits` surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct MacroEmitCodegen {
    /// Event name.
    pub name: String,
    /// Structured payload form. The ONLY payload carrier — there is
    /// deliberately no duplicated flat rendered-text sibling.
    pub payload: MacroEmitPayload,
    /// Content-free anchor back to this event's authored position.
    pub anchor: MacroSyntaxAnchor,
}

/// The structured payload form of one `defineEmits` event.
///
/// The form distinction (call-signature params vs shorthand tuple) is a
/// structured fact the consumer must read from the variant, never re-derive
/// by scanning text. The inner text is the payload's rendered content —
/// codegen splice text, not a semantic field to re-parse.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroEmitPayload {
    /// No payload beyond the event name.
    None,
    /// Call-signature payload — the parameter list after the leading
    /// event-name parameter (`(e: 'change', id: number): void` →
    /// `id: number`).
    Call {
        /// Rendered parameter-list text (codegen splice text only).
        params_text: String,
    },
    /// Shorthand tuple payload, including the surrounding `[...]`
    /// (`{ change: [id: number] }` → `[id: number]`).
    Tuple {
        /// Rendered tuple text (codegen splice text only).
        tuple_text: String,
    },
}

/// Runtime constructor kind inferred for a macro prop's type — a 1:1 mirror
/// of the parser's `RuntimeType`
/// (`verter_parser::utils::oxc::script::type_surface::RuntimeType`, produced
/// by the `type_surface/infer.rs` inference path): every variant that path
/// can emit has exactly one counterpart here.
///
/// The runtime path turns these into the JS constructor value of a runtime
/// prop declaration (`{ type: String }`, `{ type: [String, Number] }`).
/// `BuiltIn(name)` carries the constructor identifier for recognised
/// built-in classes (e.g. `Date`, `Map`, `Set`). `Unknown` is the
/// un-inferable case; a consumer rendering the runtime value filters it out
/// (yielding `null`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
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

/// A CONTENT-FREE anchor onto the current SFC's authored syntax.
///
/// It is NOT a span and NOT source text: it carries only the authored macro
/// occurrence identity plus the member/event ordinal within that macro's
/// authored surface, so a consumer holding the current SFC's parse can
/// select the corresponding syntax span (e.g. for source maps) itself. No
/// byte offsets ever live here — the aggregate is `NoStoredSpan` like every
/// other type in this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct MacroSyntaxAnchor {
    /// The authored macro occurrence this element belongs to — the same
    /// identity space as [`MacroCodegenEntry::macro_index`].
    pub macro_index: u32,
    /// Source-order position of the member/event within that macro's
    /// authored surface.
    pub ordinal: u32,
}
