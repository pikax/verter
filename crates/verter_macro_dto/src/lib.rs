//! Dependency-neutral Vue macro handoff contracts.
//!
//! Runtime generation and TSC/IDE generation are intentionally separate
//! demands. Runtime consumers receive only names, optionality, broad runtime
//! constructors, `skip_check`, and content-free anchors. TSC consumers receive
//! terminal splice text which must never be reparsed for semantic decisions.
//!
//! Every public carrier is structurally proven free of both `TypeExpr` and
//! stored byte spans. This crate has no parser, semantic-engine, session, or
//! compiler dependency.

#![forbid(unsafe_code)]

use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

/// Runtime-only semantic handoff for one SFC.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, NoTypeExpr, NoStoredSpan)]
pub struct MacroRuntimeBundle {
    /// Effective macro outcomes in authored macro order.
    pub entries: Vec<MacroRuntimeEntry>,
}

/// Runtime outcome for one effective macro identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct MacroRuntimeEntry {
    /// Content-free source-order identity of the effective macro call.
    pub macro_index: u32,
    /// Authoritative result or typed failure.
    pub outcome: MacroRuntimeOutcome,
}

/// Runtime projection outcome. Resolved-empty is represented by
/// `Complete` with an empty payload and is never collapsed into a failure.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroRuntimeOutcome {
    /// Complete, authoritative runtime shape.
    Complete(MacroRuntimeShape),
    /// Traversal stopped before an authoritative answer existed.
    Partial(MacroFailure<MacroPartialReason>),
    /// The semantic subject could not be resolved.
    Unresolved(MacroFailure<UnresolvedReason>),
    /// The semantic subject is intentionally outside the supported contract.
    Unsupported(MacroFailure<UnsupportedReason>),
}

/// Closed runtime macro vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroRuntimeShape {
    /// `defineProps`, including the effective props half of `withDefaults`.
    Props(PropsRuntimeShape),
    /// Runtime `defineEmits`; payload types are deliberately absent.
    Emits(Vec<RuntimeEmit>),
    /// `defineModel`, including its synthesized update event and modifier prop.
    Model(ModelRuntimeShape),
}

/// Complete props runtime shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct PropsRuntimeShape {
    /// Root validity remains explicit so resolved-empty object surfaces are
    /// distinct from complete primitive roots.
    pub root_shape: RuntimeRootShape,
    /// Syntax-owned default composition associated with this effective props
    /// identity. Expressions themselves remain compiler-owned.
    pub defaults: PropsDefaultsAssociation,
    /// Top-level props in deterministic declaration order.
    pub props: Vec<RuntimeProp>,
}

/// Shape of the resolved macro root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum RuntimeRootShape {
    /// Object-like top-level surface, including a resolved-empty surface.
    ObjectLike,
    /// Complete resolution proved a non-object macro root.
    NonObject,
}

/// Association between one effective props result and `withDefaults` syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum PropsDefaultsAssociation {
    /// Plain `defineProps`.
    None,
    /// The compiler merges defaults from this authored `withDefaults` call.
    WithDefaults {
        /// Content-free source-order identity of the outer defaults call.
        defaults_macro_index: u32,
    },
}

/// One runtime prop row.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct RuntimeProp {
    /// Public runtime prop name.
    pub name: String,
    /// The single requiredness fact. Consumers derive required as `!optional`.
    pub optional: bool,
    /// Ordered, deterministically deduplicated broad runtime constructors.
    pub constructors: OrderedRuntimeConstructors,
    /// Vue's Unknown-plus-Boolean/Function validation escape.
    pub skip_check: bool,
    /// Content-free provenance used to associate diagnostics/source maps.
    pub anchor: MacroAnchor,
}

/// One runtime emit name. Runtime-only compilation never carries payload text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct RuntimeEmit {
    /// Public event name.
    pub name: String,
    /// Content-free provenance.
    pub anchor: MacroAnchor,
}

/// Complete `defineModel` runtime shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct ModelRuntimeShape {
    /// Runtime model prop, classified exactly like a normal prop.
    pub prop: RuntimeProp,
    /// Synthesized `update:<name>` event.
    pub update_event: RuntimeEmit,
    /// Synthesized `<name>Modifiers`/`modelModifiers` prop.
    pub modifiers_prop: RuntimeProp,
}

/// Closed broad runtime-constructor taxonomy in Vue-compatible terminal order.
///
/// `Null` and `Unknown` are semantic classifications with no JavaScript
/// constructor. BigInt is intentionally absent: pinned Vue 3.5.34 emits no
/// runtime BigInt constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum RuntimeConstructor {
    String,
    Number,
    Boolean,
    Symbol,
    Null,
    Array,
    Function,
    Date,
    Map,
    Set,
    Promise,
    Error,
    Object,
    Unknown,
}

impl RuntimeConstructor {
    /// JavaScript constructor identifier, or `None` for `Null`/`Unknown`.
    #[must_use]
    pub const fn as_constructor(self) -> Option<&'static str> {
        match self {
            Self::String => Some("String"),
            Self::Number => Some("Number"),
            Self::Boolean => Some("Boolean"),
            Self::Symbol => Some("Symbol"),
            Self::Null => None,
            Self::Array => Some("Array"),
            Self::Function => Some("Function"),
            Self::Date => Some("Date"),
            Self::Map => Some("Map"),
            Self::Set => Some("Set"),
            Self::Promise => Some("Promise"),
            Self::Error => Some("Error"),
            Self::Object => Some("Object"),
            Self::Unknown => None,
        }
    }
}

/// Ordered runtime constructors with stable first-occurrence deduplication.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, NoTypeExpr, NoStoredSpan)]
pub struct OrderedRuntimeConstructors(Vec<RuntimeConstructor>);

impl OrderedRuntimeConstructors {
    /// Construct from classifier order, retaining the first occurrence of each
    /// closed constructor kind.
    #[must_use]
    pub fn from_ordered(values: impl IntoIterator<Item = RuntimeConstructor>) -> Self {
        let mut constructors = Vec::new();
        for value in values {
            if !constructors.contains(&value) {
                constructors.push(value);
            }
        }
        Self(constructors)
    }

    /// Ordered constructor view.
    #[must_use]
    pub fn as_slice(&self) -> &[RuntimeConstructor] {
        &self.0
    }

    /// Whether the complete classification produced no runtime constructors.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Optional authored member ordinal. Its optionality is structural: inherited,
/// mapped, merged, or synthesized rows need not fabricate an authored ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct AuthoredMemberOrdinal(u32);

impl AuthoredMemberOrdinal {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Honest content-free anchor for an authored, type-argument, or synthesized row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroAnchor {
    /// Authored member/event. Ordinal is absent when the row was inherited,
    /// mapped, or merged and has no direct member in the macro argument.
    Authored {
        macro_index: u32,
        member_ordinal: Option<AuthoredMemberOrdinal>,
    },
    /// Fallback to the macro type-argument as a whole.
    MacroArgument { macro_index: u32 },
    /// Row synthesized by macro semantics rather than authored syntax.
    Synthesized {
        macro_index: u32,
        row: SynthesizedRowKind,
    },
}

/// Closed synthesized-row vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum SynthesizedRowKind {
    ModelProp,
    ModelUpdateEvent,
    ModelModifiersProp,
}

/// Typed failure plus optional display-only diagnostic text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct MacroFailure<R> {
    pub reason: R,
    /// Human-readable diagnostics only. Consumers must not branch semantics on
    /// this text.
    pub diagnostic: Option<String>,
}

impl<R> MacroFailure<R> {
    #[must_use]
    pub fn new(reason: R, diagnostic: Option<String>) -> Self {
        Self { reason, diagnostic }
    }
}

/// Structural incompleteness reasons. Such results are never authoritative or
/// warm-admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroPartialReason {
    BudgetExceeded,
    Cancelled,
    SupersededGeneration,
    UnstableState,
    Recursion,
    IncompleteTraversal,
}

/// Reasons no semantic macro subject could be established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum UnresolvedReason {
    MissingTypeArgument,
    MissingDeclaration,
    AmbiguousReference,
    MissingDependency,
    NonObjectRoot,
}

/// Closed unsupported-result reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum UnsupportedReason {
    MacroKind,
    SemanticConstruct,
}

/// TSC/IDE-only semantic handoff. Runtime-only targets do not construct it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, NoTypeExpr, NoStoredSpan)]
pub struct MacroTscBundle {
    pub entries: Vec<MacroTscEntry>,
}

/// TSC result for one effective macro identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct MacroTscEntry {
    pub macro_index: u32,
    pub outcome: MacroTscOutcome,
}

/// TSC projection outcome, kept independent from runtime completeness.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroTscOutcome {
    Complete(MacroTscProjection),
    Partial(MacroFailure<MacroPartialReason>),
    Unresolved(MacroFailure<UnresolvedReason>),
    Unsupported(MacroFailure<UnsupportedReason>),
}

/// Closed TSC macro projection vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroTscProjection {
    Props { splice: TscSpliceText },
    Emits { splice: TscSpliceText },
    Model { splice: TscSpliceText },
}

/// Terminal codegen text. It is an output-only splice and must never be
/// reparsed for semantic decisions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TscSpliceText(String);

impl TscSpliceText {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
