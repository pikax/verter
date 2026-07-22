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
    /// Source-order identity among top-level compiler-macro syntax items.
    /// This is the compiler join key and is distinct from analyzer inventory
    /// identity because `withDefaults(defineProps())` has two analyzer rows but
    /// one top-level compiler syntax item.
    pub syntax_index: u32,
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
    /// Resolution completed and proved a shape Vue rejects for this macro.
    Invalid(MacroFailure<MacroInvalidReason>),
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
    /// Syntax-owned default composition associated with this effective props
    /// identity. Expressions themselves remain compiler-owned.
    pub defaults: PropsDefaultsAssociation,
    /// Top-level props in deterministic declaration order.
    pub props: Vec<RuntimeProp>,
}

/// Association between one effective props result and `withDefaults` syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum PropsDefaultsAssociation {
    /// Plain `defineProps`.
    None,
    /// The compiler merges defaults from this authored `withDefaults` call.
    WithDefaults {
        /// Content-free source-order identity of the inner `defineProps`
        /// payload whose semantic surface is carried by this entry.
        payload_macro_index: u32,
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
    /// Authoritative classification or a row-local typed degradation.
    pub type_shape: RuntimePropType,
    /// Content-free provenance used to associate diagnostics/source maps.
    pub anchor: MacroAnchor,
}

/// Runtime type policy for one prop row.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum RuntimePropType {
    /// Complete semantic classification. An empty constructor list is valid
    /// semantic Unknown and is distinct from degradation.
    Resolved {
        constructors: OrderedRuntimeConstructors,
        skip_check: bool,
    },
    /// The demand never asked for broad-runtime classification, so none was
    /// computed. Only a target that emits the Vue runtime `props` option
    /// object needs member constructors; a TSX-only (IDE) compile consumes
    /// public binding names and nothing else. Distinct from both a resolved
    /// semantic Unknown and a degradation: no claim is made about the member.
    Unclassified,
    /// Member-position resolution failed. Vue renders `null` while the
    /// compiler surfaces a warning at the row's honest anchor.
    Degraded(MacroFailure<MacroMemberReason>),
}

impl RuntimePropType {
    #[must_use]
    pub fn constructors(&self) -> Option<&OrderedRuntimeConstructors> {
        match self {
            Self::Resolved { constructors, .. } => Some(constructors),
            Self::Unclassified | Self::Degraded(_) => None,
        }
    }

    #[must_use]
    pub const fn skip_check(&self) -> bool {
        matches!(
            self,
            Self::Resolved {
                skip_check: true,
                ..
            }
        )
    }
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
    WeakMap,
    WeakSet,
    Promise,
    Error,
    Object,
    Unknown,
}

impl RuntimeConstructor {
    /// Vue runtime type expression, or `None` for semantic Unknown.
    #[must_use]
    pub const fn as_runtime_expression(self) -> Option<&'static str> {
        match self {
            Self::String => Some("String"),
            Self::Number => Some("Number"),
            Self::Boolean => Some("Boolean"),
            Self::Symbol => Some("Symbol"),
            Self::Null => Some("null"),
            Self::Array => Some("Array"),
            Self::Function => Some("Function"),
            Self::Date => Some("Date"),
            Self::Map => Some("Map"),
            Self::Set => Some("Set"),
            Self::WeakMap => Some("WeakMap"),
            Self::WeakSet => Some("WeakSet"),
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

/// Required source-order identity for a directly authored member.
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
    /// Directly authored member/event.
    Authored {
        macro_index: u32,
        member_ordinal: AuthoredMemberOrdinal,
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
}

/// Resolved macro roots that are semantically invalid for the role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroInvalidReason {
    NonObjectRoot,
    InvalidEmitsShape,
}

/// Row-local degradation reasons. Root failures remain on the enclosing
/// macro outcome and never admit a partial row set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroMemberReason {
    Partial(MacroPartialReason),
    Unresolved(UnresolvedReason),
    Unsupported(UnsupportedReason),
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
    /// Source-order identity among top-level compiler-macro syntax items.
    pub syntax_index: u32,
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
    Invalid(MacroFailure<MacroInvalidReason>),
}

/// Closed TSC macro projection vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum MacroTscProjection {
    Props(TscPropsProjection),
    Emits(TscEmitsProjection),
    Model(TscModelProjection),
}

/// Closed `defineProps` TSC projection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TscPropsProjection {
    /// Public props surface used by declaration/public component output.
    pub public: TscPublicPropsProjection,
    /// One explicit testing-mode binding row per public prop.
    pub testing_rows: Vec<TscPropRow>,
    /// Typed scope requirements referenced by terminal text.
    pub scope: TscScopeRequirements,
}

/// Closed public-props codegen authorization. The compiler preserves the exact
/// parser-owned first type argument; semantic rows only drive testing bindings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum TscPublicPropsProjection {
    AuthoredArgument {
        /// Exact macro payload identity authorizing preservation of the
        /// parser-owned first type argument. The compiler validates this
        /// against the joined effective macro before splicing any bytes.
        anchor: MacroAnchor,
    },
}

/// One testing/public prop row.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TscPropRow {
    pub name: String,
    pub optional: bool,
    pub type_text: TscSpliceText,
    pub anchor: MacroAnchor,
}

/// Closed `defineEmits` TSC projection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TscEmitsProjection {
    pub events: Vec<TscEmitRow>,
    pub scope: TscScopeRequirements,
}

/// One explicit event signature. Parameter strings are terminal codegen text;
/// consumers splice them directly and never recover semantics from them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TscEmitRow {
    pub name: String,
    pub emit_parameters: TscSpliceText,
    pub handler_parameters: TscSpliceText,
    pub anchor: MacroAnchor,
}

/// Closed `defineModel` TSC projection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TscModelProjection {
    pub name: String,
    pub optional: bool,
    pub value_type: TscSpliceText,
    pub anchor: MacroAnchor,
    pub scope: TscScopeRequirements,
}

/// Scope facts required by role-specific terminal output.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, NoTypeExpr, NoStoredSpan)]
pub struct TscScopeRequirements {
    /// Owner-local runtime values referenced directly by the macro type
    /// argument rather than through a retained declaration.
    pub owner_value_dependencies: Vec<TscOwnerValueDependency>,
    /// Local import identities the compiler retains from its typed import
    /// inventory. The DTO never reconstructs import statements.
    pub retained_bindings: Vec<TscRetainedBinding>,
    /// Dependency-ordered compiler-owned local declaration identities.
    pub dependency_declarations: Vec<TscDependencyDeclaration>,
}

/// One script-owner-qualified runtime value required by generated type text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TscOwnerValueDependency {
    pub owner: TscScriptOwner,
    pub name: String,
}

/// One retained local import identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TscRetainedBinding {
    /// Script block that owns the authored import declaration. This closes the
    /// compiler join when setup and companion scripts reuse a local name.
    pub owner: TscScriptOwner,
    pub local_name: String,
    pub usage: TscBindingUsage,
}

/// Authored Vue script block that owns a retained compiler carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, NoTypeExpr, NoStoredSpan)]
pub enum TscScriptOwner {
    Setup,
    Companion,
}

/// How an import identity is consumed by generated TSC output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum TscBindingUsage {
    /// The binding is referenced from generated type syntax. The compiler
    /// joins this fact to its typed import inventory to preserve the authored
    /// import form or promote a value import for declaration output.
    TypePosition,
    /// The binding is the root of a `typeof` query and must remain
    /// value-capable when the authored body is omitted.
    ValueQuery,
    /// The binding is consumed directly as a value by declaration syntax,
    /// such as a class heritage expression.
    ValuePosition,
}

/// One compiler-owned local declaration contributor required by generated
/// type syntax. `contributor_ordinal` is the source-ordered ordinal among
/// declarations with the same local name; the DTO never carries declaration
/// source text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TscDependencyDeclaration {
    /// Script block containing this exact declaration contributor.
    pub owner: TscScriptOwner,
    pub name: String,
    /// Source-order ordinal among same-name contributors in the same owner.
    pub contributor_ordinal: u32,
    /// Owner-local runtime values referenced by this declaration (for example
    /// `type Props = { value: typeof seed }`) that require the owner's
    /// implementation body. Exact dual-space declaration carriers are kept on
    /// [`Self::retained_value_carriers`] instead.
    pub owner_value_dependencies: Vec<TscOwnerValueDependency>,
    /// Exact retained dual-space declaration contributors that satisfy value
    /// roots without requiring the owner's implementation body.
    pub retained_value_carriers: Vec<TscRetainedValueCarrier>,
    /// Declaration-only readiness. Public/testing modes retain exact compiler
    /// carriers even when semantic inference cannot prove an ambient shape.
    pub declaration_failure: Option<TscDeclarationFailureReason>,
    /// Semantic type insertions required to make an implementation class
    /// declaration-safe without weakening inferred public member types.
    pub inferred_class_members: Vec<TscInferredClassMember>,
}

/// Exact value-capable declaration contributor retained as an ambient carrier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TscRetainedValueCarrier {
    pub owner: TscScriptOwner,
    pub name: String,
    pub contributor_ordinal: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum TscDeclarationFailureReason {
    /// Semantic inference stopped at a structural safety budget.
    SemanticInferenceUnavailable(TscSemanticInferenceUnavailableReason),
    /// The declaration uses a resolved construct outside the supported
    /// declaration-inference contract.
    Unsupported(UnsupportedReason),
    /// The declaration-inference subject or one of its required bodies was
    /// unavailable.
    Unresolved(UnresolvedReason),
}

/// Exact structural budget that stopped semantic declaration inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub enum TscSemanticInferenceUnavailableReason {
    DepthBudgetExceeded,
    WorkBudgetExceeded,
}

/// Compiler class-member identity for an inferred declaration-only type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, NoStoredSpan)]
pub struct TscInferredClassMember {
    pub name: String,
    /// Source-ordered ordinal among class elements with the same staticness,
    /// name, and requested annotation position.
    pub occurrence: u32,
    pub is_static: bool,
    pub position: TscInferredClassTypePosition,
    pub type_text: TscSpliceText,
}

/// Which absent authored class annotation an inferred type fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, NoTypeExpr, NoStoredSpan)]
pub enum TscInferredClassTypePosition {
    Property,
    Parameter,
    Return,
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
