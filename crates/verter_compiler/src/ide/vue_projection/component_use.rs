//! One component-use inference transaction and specialized observations.
//!
//! Every component use of an admitted [`ProjectionPlan`] is checked through
//! exactly one construction of the used component, so TypeScript infers the
//! component's generic binder once, from every contributing channel
//! together, and every later observation reads that one specialized
//! instance. Three products per use:
//!
//! - [`InferenceTransaction`] — the members of the one construction:
//!   effective props, static attributes, model values, object spreads and
//!   one listener per listener key, in the runtime's merge order, plus the
//!   validation-only checks (further listeners, further `class` / `style`
//!   writes, inline handlers) that reuse the specialized instance, and the
//!   operations that never reach the construction with the reason why.
//! - [`SpecializedUseObservation`] — slot props, event listener contracts,
//!   model write types and template-ref instances, each read from the
//!   use's witness binding, never from the uninstantiated component.
//! - [`ComponentUseWitness`] — the logical [`ComponentUseId`], the
//!   revision-qualified [`SpecializationKey`] of its checking inputs, the
//!   witness binding, the transaction and the observations.
//!
//! Rendering rules, and why:
//!
//! - The construction is `new (USE_COMPONENT(Comp, USE_CONSTRUCTOR(Comp)))({
//!   ... })`: the [`ForeignComponentContractAdapter`] hands TypeScript the
//!   component's adapted contract (subject to the adapter's documented
//!   generic-overload reflection limits) and then an attribute-tolerant
//!   signature. A single concrete contract uses the tolerant signature alone
//!   to preserve prop assignability diagnostics. TypeScript still
//!   infers the component's binder at the construction, checks and
//!   contextually types every declared key, and keeps literal types, while
//!   a fallthrough attribute is not an excess-key error (excess-key policy
//!   is a separate obligation, not a side effect of inference).
//! - A member is spelled as the component's camel-case prop, which the
//!   runtime's `camelize(key)` prop lookup reads and vue-tsc writes
//!   (`:initial-count` is `initialCount`, listener-shaped keys included);
//!   `data-*` / `aria-*` keep their spelling. Its direct-key check also
//!   accepts the authored spelling, under which an undeclared attribute
//!   falls through (`stroke-width` onto an `<svg>` root). Such an attribute,
//!   like an `EmitsToProps` listener published under a kebab-case name, then
//!   reaches the attribute-tolerant index, so its key is checked and its
//!   value is not, as in vue-tsc. A later key with an already constructed
//!   member's camel-case spelling (`:fooBar` after `:foo-bar`) is a
//!   validation against that prop, and keeps its own direct-key check.
//! - Keys follow the runtime property plan: a certainly overwritten write
//!   never reaches the construction, a later object spread keeps its
//!   position, and a listener key accumulates, so its first handler reaches
//!   the construction after every spread and each further handler is a
//!   validation check against the specialized contract — never a
//!   fabricated array or a synthetic call that loses variance.
//! - An inline handler is validated inside a `$event` handler typed by the
//!   specialized listener contract: a single expression is the handler's
//!   returned body, a statement list its block body.
//! - An event-option listener key (`.once` / `.capture` / `.passive`
//!   append `Once` / `Capture` / `Passive`) is validated against its own
//!   key when declared. Only a single trailing `Once` falls back to the
//!   unsuffixed key, the once-handler component `emit` calls with the event
//!   payload; a `Capture` / `Passive` key is no emit listener, so it never
//!   borrows the event's payload contract.
//! - Dynamic keys, listener objects, reserved vnode keys, DOM bindings,
//!   runtime directives and compiler-synthesized model listeners are not
//!   authored inference contributors here; each is recorded as excluded.
//!
//! Nothing here answers types: TypeScript owns inference, overload selection
//! and contextual typing. Each rendered statement is placed in its use's
//! lexical environment by the projection composer. Vue IDE routing stays on
//! [`crate::ide::script`] until atomic activation.

use verter_identity::encoding::{CanonicalDigest, CanonicalEncoder};

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, ComponentUse, ComponentUseId, ExpressionKind, HandlerShape,
    PlanSnapshotId, ProjectionPlan,
};
use crate::ide::template::props::normalized_component_prop_name;
use crate::ide::vue_projection::attribute_operations::{
    is_reserved, AttributeOperationsProjection, AttributeSyntax, EffectiveProperty, MergeRule,
    RuntimeKey, RuntimePropertyKeyPlan, VueAttributeOp, VueAttributeSequence, WriteValue,
};
use crate::ide::vue_projection::callback_reentry::{
    CallbackReentryPlan, EXPRESSION_HANDLER_OPEN, STATEMENTS_HANDLER_OPEN,
};
use crate::ide::vue_projection::checking_fragment::{CheckingFragment, StatementPhase};
use crate::ide::vue_projection::generic_interop::{
    foreign_contract_declarations, ForeignComponentContractAdapter,
};
use crate::ide::vue_projection::props::{project_props, PropsProjection};
use crate::ide::vue_projection::script_setup::SourceRange;
use crate::template::vue_syntax::{is_member_expression, to_pascal_case, trim_handler_body};

/// Attribute-tolerant construction signature: the component's (last)
/// construct or call signature with an `unknown` attribute index on its
/// props parameter; the adapter's fallback after the exact contract.
pub const USE_CONSTRUCTOR: &str = "__VerterUseConstructor";
/// Specialized `$props` member, `unknown` when the key is not declared.
pub const USE_PROP: &str = "__VerterUseProp";
/// Specialized listener contract of a key, else of its fallback key; an
/// untyped listener only when neither is declared. A declared `any` stays
/// `any`.
pub const USE_LISTENER: &str = "__VerterUseListener";
/// Specialized slot props, `unknown` when the slot is not declared.
pub const USE_SLOT_PROPS: &str = "__VerterUseSlotProps";
/// Spread checker. A finite spread is checked the way TypeScript checks a
/// spread argument: known-key types, missing required props, discriminated
/// unions, exact optional properties and readonly tuples. Overwritten keys
/// are part of that check, so a generic spread matches `{...value, ...keys}`.
/// Keys that arrive only through the spread are not excess-key errors. An
/// index signature, `any`, or a union member that has one stays an open
/// domain. A generic spread is checked through its constraint.
pub const USE_SPREAD: &str = "__VerterUseSpread";
/// Direct attribute key checker. A misspelled key written on the element is
/// rejected. A kebab-case key is accepted when its camel-case spelling is on
/// the props surface (a declared prop) or its authored spelling is (an
/// attribute that falls through as written, `stroke-width` on an `<svg>`
/// root); otherwise it is rejected under the camel-case spelling.
/// `data-*` / `aria-*` attributes, Vue `on*` listeners, and global
/// fallthrough attributes are not excess-key errors. An open props domain
/// is not.
pub const USE_DIRECT: &str = "__VerterUseDirect";
/// Specialized model write type read from its update listener.
pub const USE_MODEL: &str = "__VerterUseModel";
/// Prefix of a witness binding; the use-id digest follows.
pub const WITNESS_PREFIX: &str = "__VerterUse_";

const SPECIALIZATION_DOMAIN: &str =
    "verter.compiler.vue_projection.component_use_specialization.v1";

/// Module-scope declarations every rendered witness reads, rendered once:
/// the foreign component contract adapter, then the observation types.
pub const USE_PRELUDE: &str = concat!(
    foreign_contract_declarations!(),
    "type __VerterUseComponentProps<C> = C extends abstract new (props: infer P) => unknown ? P : never;\n",
    "type __VerterUseMemberOpen<S> = S extends unknown ? (string extends keyof S ? true : number extends keyof S ? true : symbol extends keyof S ? true : false) : never;\n",
    "type __VerterUseDrop<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;\n",
    "type __VerterUseSupply<S, P, O extends PropertyKey> = S & { [K in O & keyof P]: P[K] };\n",
    "type __VerterUseOpen<S> = [true] extends [__VerterUseMemberOpen<S>] ? true : false;\n",
    "type __VerterUseKnownSpread<S, P, O extends PropertyKey> = __VerterUseOpen<S> extends true ? S : [__VerterUseSupply<NoInfer<S>, P, O>] extends [P] ? S : S & __VerterUseDrop<P, O>;\n",
    "declare function __VerterUseSpread<C, S, O extends PropertyKey>(component: C, spread: S & __VerterUseKnownSpread<S, __VerterUseComponentProps<C>, O>, overwritten: readonly O[]): S;\n",
    "type __VerterUseKeyOn<P, K extends PropertyKey> = P extends unknown ? (K extends keyof P ? true : false) : never;\n",
    "type __VerterUseFallthrough<K extends PropertyKey> = K extends `${string}-${string}` ? true : K extends `on${Capitalize<string>}` ? true : K extends \"id\" | \"role\" | \"tabindex\" | \"slot\" | \"lang\" | \"dir\" | \"hidden\" | \"draggable\" | \"autofocus\" | \"inert\" | \"popover\" | \"part\" | \"nonce\" | \"is\" | \"accesskey\" | \"contenteditable\" | \"spellcheck\" | \"translate\" | \"inputmode\" ? true : false;\n",
    "type __VerterUseDirectOk<P, K extends PropertyKey, A extends PropertyKey = K> = 0 extends 1 & P ? true : string extends keyof P ? true : number extends keyof P ? true : symbol extends keyof P ? true : [__VerterUseKeyOn<P, K>] extends [false] ? [__VerterUseKeyOn<P, A>] extends [false] ? __VerterUseFallthrough<K> extends true ? true : false : true : true;\n",
    "declare function __VerterUseDirect<C, I, K extends PropertyKey, A extends PropertyKey = K>(component: C, instance: I, key: 0 extends 1 & C ? K : (__VerterUseDirectOk<I extends { readonly $props: infer P } ? P : never, K, A> extends true ? K : never), authored?: A): void;\n",
    "type __VerterUseProp<I, K extends PropertyKey> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : unknown) : unknown;\n",
    "type __VerterUseListener<I, K extends PropertyKey, F extends PropertyKey = K> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : F extends keyof P ? P[F] : (...args: any[]) => unknown) : (...args: any[]) => unknown;\n",
    "type __VerterUseSlotProps<I, K extends PropertyKey> = I extends { readonly $slots: infer S } ? (K extends keyof S ? (NonNullable<S[K]> extends (props: infer A, ...rest: any[]) => any ? A : unknown) : unknown) : unknown;\n",
    "type __VerterUseModel<I, K extends PropertyKey> = NonNullable<__VerterUseProp<I, K>> extends (value: infer V, ...rest: any[]) => any ? V : unknown;\n",
    "void (null! as [__VerterUseListener<unknown, never>, __VerterUseSlotProps<unknown, never>, __VerterUseModel<unknown, never>]);\n",
);

/// Revision-qualified identity of one use's checking inputs: the logical
/// use plus every contributing spelling, never source offsets, so an edit
/// to another use or a pure position shift leaves it unchanged.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpecializationKey(CanonicalDigest);

impl core::fmt::Debug for SpecializationKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SpecializationKey({})", self.0.to_hex())
    }
}

impl SpecializationKey {
    pub(crate) fn digest_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

/// An authored value as the transaction renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberValue {
    /// An admitted expression, parenthesized.
    Expression {
        /// Admitted expression.
        id: AdmittedExpressionId,
        /// Authored spelling at this snapshot.
        spelling: String,
    },
    /// An inline `v-on` value run inside a `$event` handler.
    InlineHandler {
        /// Admitted expression.
        id: AdmittedExpressionId,
        /// Authored spelling at this snapshot.
        spelling: String,
        /// A statement list (block body) rather than one expression
        /// (returned body).
        statements: bool,
    },
    /// Literal static attribute text (a string at runtime).
    StaticText(String),
    /// A static attribute written without a value (`<Child flag />`). It
    /// renders as `true`, as vue-tsc does, so a boolean prop accepts it; an
    /// attribute with a value, even `flag=""`, stays [`Self::StaticText`].
    Valueless,
}

impl MemberValue {
    fn render(&self) -> String {
        match self {
            Self::Expression { spelling, .. } => format!("({spelling})"),
            // The line break ends a trailing line comment before the block
            // closes.
            Self::InlineHandler {
                spelling,
                statements: true,
                ..
            } => format!("{STATEMENTS_HANDLER_OPEN}{spelling};\n}}"),
            Self::InlineHandler { spelling, .. } => {
                format!("{EXPRESSION_HANDLER_OPEN}{})", trim_handler_body(spelling))
            }
            Self::StaticText(text) => quote(text),
            Self::Valueless => "true".to_string(),
        }
    }

    /// Admitted expression, when the value is authored as one.
    #[must_use]
    pub fn expression(&self) -> Option<&AdmittedExpressionId> {
        match self {
            Self::Expression { id, .. } | Self::InlineHandler { id, .. } => Some(id),
            Self::StaticText(_) | Self::Valueless => None,
        }
    }
}

/// One member of the construction's props argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionMember {
    /// `"key": value`.
    Property {
        /// Contributing operation index.
        op_index: u32,
        /// Runtime key.
        key: String,
        /// Authored bytes the key is spelled from, when it has any.
        key_source: Option<SourceRange>,
        /// Carried value.
        value: MemberValue,
    },
    /// `...(value)`: a `v-bind` object whose keys syntax cannot name.
    Spread {
        /// Contributing operation index.
        op_index: u32,
        /// Carried value.
        value: MemberValue,
        /// Keys certainly replaced by a later definite write. Their spread
        /// values do not participate in the final caller contract.
        overwritten_keys: Vec<String>,
    },
}

impl TransactionMember {
    /// Contributing operation index.
    #[must_use]
    pub fn op_index(&self) -> u32 {
        match self {
            Self::Property { op_index, .. } | Self::Spread { op_index, .. } => *op_index,
        }
    }

    /// Runtime key of a property member.
    #[must_use]
    pub fn key(&self) -> Option<&str> {
        match self {
            Self::Property { key, .. } => Some(key),
            Self::Spread { .. } => None,
        }
    }

    fn render(&self, component: &str) -> String {
        match self {
            Self::Property { key, value, .. } => {
                format!("{}: {}", quote(&contract_key(key)), value.render())
            }
            Self::Spread {
                value,
                overwritten_keys,
                ..
            } => format!(
                "...{USE_SPREAD}({component}, {}, [{}] as const)",
                value.render(),
                overwritten_keys
                    .iter()
                    .map(|key| quote(key))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// Which specialized contract a validation-only check reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckContract {
    /// The specialized listener for the key.
    Listener,
    /// The specialized `$props` member for the key.
    Prop,
}

/// A contribution checked against the specialized instance after the
/// construction, without re-entering inference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationCheck {
    /// Contributing operation index.
    pub op_index: u32,
    /// Runtime key.
    pub key: String,
    /// Contract the value is checked against.
    pub contract: CheckContract,
    /// Listener key the contract falls back to when `key` is not declared:
    /// the key without its event-option postfixes.
    pub fallback: Option<String>,
    /// Carried value.
    pub value: MemberValue,
}

impl ValidationCheck {
    /// The key the contract is read under: a prop under its camel-case
    /// spelling, a listener under its runtime key.
    fn contract_key(&self) -> std::borrow::Cow<'_, str> {
        match self.contract {
            CheckContract::Prop => contract_key(&self.key),
            CheckContract::Listener => std::borrow::Cow::Borrowed(&self.key),
        }
    }
}

/// Why an operation never reaches the construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExclusionReason {
    /// A later definite write or first-wins literal dedupe overwrites it.
    Overridden,
    /// The runtime key is computed (`:[key]`, `@[event]`, `v-model:[arg]`).
    DynamicKey,
    /// `v-on="obj"`: listener keys are derived at runtime.
    ListenerObject,
    /// Compiler-synthesized `v-model` update listener, observed instead.
    ModelUpdate,
    /// Compiler-synthesized `v-model` modifiers object.
    ModelModifiers,
    /// Reserved vnode key (`key`, `ref`, `ref_for`, `ref_key` and the six
    /// `onVnode*` lifecycle hooks).
    Reserved,
    /// `.prop` / `^attr` DOM binding.
    DomBinding,
    /// Writes no property (runtime directive, `<component is>`).
    NoProperty,
    /// Carries no admitted value.
    MissingValue,
}

/// One operation that never reaches the construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExcludedOperation {
    /// Operation index.
    pub op_index: u32,
    /// Reason.
    pub reason: ExclusionReason,
}

/// The one construction of one component use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceTransaction {
    /// Construction members, in the runtime's merge order.
    pub members: Vec<TransactionMember>,
    /// Validation-only checks reusing the specialized instance.
    pub validations: Vec<ValidationCheck>,
    /// Operations that never reach the construction.
    pub excluded: Vec<ExcludedOperation>,
}

/// What a specialized observation reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservationKind {
    /// Props of a provided slot.
    Slot {
        /// Static slot name.
        name: String,
    },
    /// Listener contract of an authored listener key.
    Event {
        /// Runtime listener key.
        key: String,
        /// Listener key the contract falls back to (see
        /// [`ValidationCheck::fallback`]).
        fallback: Option<String>,
    },
    /// Write type of a `v-model`.
    Model {
        /// Model prop name.
        name: String,
    },
    /// The specialized instance, which a template ref of this use receives.
    Instance,
}

/// One observation derived from a use's specialized witness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecializedUseObservation {
    /// Observed channel.
    pub kind: ObservationKind,
    /// TypeScript type text over the witness binding.
    pub type_text: String,
}

/// The specialization witness of one component use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentUseWitness {
    /// Logical component use.
    pub use_id: ComponentUseId,
    /// Revision-qualified identity of the checking inputs.
    pub specialization: SpecializationKey,
    /// Witness binding the construction initializes.
    pub binding: String,
    /// Component expression.
    pub component_expression: AdmittedExpressionId,
    /// Rendered constructor expression.
    pub component: String,
    /// The one construction.
    pub transaction: InferenceTransaction,
    /// Observations over the witness.
    pub observations: Vec<SpecializedUseObservation>,
}

impl ComponentUseWitness {
    /// The rendered construction and its validation checks, one statement
    /// per line, for the use's lexical environment.
    #[must_use]
    pub fn render(&self) -> String {
        let members: Vec<String> = self
            .transaction
            .members
            .iter()
            .map(|member| member.render(&self.component))
            .collect();
        let mut out = format!(
            "const {} = new ({})({{ {} }});\n",
            self.binding,
            ForeignComponentContractAdapter.construction_callee(&self.component),
            members.join(", ")
        );
        for key in self.direct_keys() {
            out.push_str(&format!(
                "{USE_DIRECT}({}, {}, {});\n",
                self.component,
                self.binding,
                direct_key_arguments(key)
            ));
        }
        for (ordinal, check) in self.transaction.validations.iter().enumerate() {
            let contract = match check.contract {
                CheckContract::Listener => USE_LISTENER,
                CheckContract::Prop => USE_PROP,
            };
            out.push_str(&format!(
                "const {}: {contract}<typeof {}, {}{}> = {};\n",
                self.check_binding(ordinal),
                self.binding,
                quote(&check.contract_key()),
                fallback_argument(check.fallback.as_deref()),
                check.value.render()
            ));
        }
        out
    }
}

impl ComponentUseWitness {
    /// The statements of [`Self::render`] as fragments: the construction
    /// (`Main`), then one direct-key check and one validation per line
    /// (`After`). Inline handlers come from `reentry`. With a pending
    /// `reentry`, flattening equals `render()` byte for byte.
    #[must_use]
    pub fn statement_fragments(
        &self,
        reentry: &CallbackReentryPlan,
    ) -> Vec<(StatementPhase, CheckingFragment)> {
        self.statement_fragments_over(reentry, WitnessComponent::Named { tag: None })
    }

    /// Every binding the statements declare: the witness, then one per
    /// validation check.
    #[must_use]
    pub fn declared_bindings(&self) -> Vec<String> {
        std::iter::once(self.binding.clone())
            .chain(
                (0..self.transaction.validations.len()).map(|ordinal| self.check_binding(ordinal)),
            )
            .collect()
    }

    fn check_binding(&self, ordinal: usize) -> String {
        format!("{}_check{ordinal}", self.binding)
    }

    /// Every key a direct-key check covers: each property member, then each
    /// prop validation whose authored key is no member's (a second key
    /// naming one prop, `:fooBar` after `:foo-bar`), so every authored key
    /// is checked once.
    fn direct_keys(&self) -> impl Iterator<Item = &str> {
        let members = &self.transaction.members;
        members.iter().filter_map(TransactionMember::key).chain(
            self.transaction
                .validations
                .iter()
                .filter(move |check| {
                    check.contract == CheckContract::Prop
                        && !members
                            .iter()
                            .any(|member| member.key() == Some(check.key.as_str()))
                })
                .map(|check| check.key.as_str()),
        )
    }

    /// [`Self::statement_fragments`] with the component written as
    /// `component` says: a name stays the witness's own text; an authored
    /// `:is` value is the admitted expression, mapped once and replayed at
    /// every further use.
    #[must_use]
    pub fn statement_fragments_over(
        &self,
        reentry: &CallbackReentryPlan,
        component: WitnessComponent,
    ) -> Vec<(StatementPhase, CheckingFragment)> {
        let mut writer = FragmentWriter {
            reentry,
            witness: self,
            component,
            mapped: Vec::new(),
        };
        let mut construction = CheckingFragment::synth(format!("const {} = new (", self.binding));
        // The adapter owns the callee's shape; the component goes where its
        // rendering puts it.
        let callee = ForeignComponentContractAdapter.construction_callee(CALLEE_SLOT);
        let mut parts = callee.split(CALLEE_SLOT).peekable();
        while let Some(part) = parts.next() {
            construction.push_synth(part);
            if parts.peek().is_some() {
                writer.component(&mut construction);
            }
        }
        construction.push_synth(")({ ");
        for (index, member) in self.transaction.members.iter().enumerate() {
            if index > 0 {
                construction.push_synth(", ");
            }
            match member {
                TransactionMember::Property {
                    key,
                    key_source,
                    value,
                    ..
                } => {
                    // TypeScript reports a member's failure against the
                    // construction on its key: the key stands at the
                    // attribute it was written as, spelled as the
                    // component's camel-case prop.
                    match key_source {
                        Some(source) => construction.push_alias(quote(&contract_key(key)), *source),
                        None => construction.push_synth(quote(&contract_key(key))),
                    };
                    construction.push_synth(": ");
                    writer.value(&mut construction, value);
                }
                TransactionMember::Spread {
                    value,
                    overwritten_keys,
                    ..
                } => {
                    construction.push_synth(format!("...{USE_SPREAD}("));
                    writer.component(&mut construction);
                    construction.push_synth(", ");
                    writer.value(&mut construction, value);
                    construction.push_synth(format!(
                        ", [{}] as const)",
                        overwritten_keys
                            .iter()
                            .map(|key| quote(key))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
        }
        construction.push_synth(" });\n");
        let mut statements = vec![(StatementPhase::Main, construction)];
        for key in self.direct_keys() {
            let mut direct = CheckingFragment::synth(format!("{USE_DIRECT}("));
            writer.component(&mut direct);
            direct.push_synth(format!(
                ", {}, {});\n",
                self.binding,
                direct_key_arguments(key)
            ));
            statements.push((StatementPhase::After, direct));
        }
        for (ordinal, check) in self.transaction.validations.iter().enumerate() {
            let contract = match check.contract {
                CheckContract::Listener => USE_LISTENER,
                CheckContract::Prop => USE_PROP,
            };
            let mut validation = CheckingFragment::synth(format!(
                "const {}: {contract}<typeof {}, {}{}> = ",
                self.check_binding(ordinal),
                self.binding,
                quote(&check.contract_key()),
                fallback_argument(check.fallback.as_deref())
            ));
            writer.value(&mut validation, &check.value);
            validation.push_synth(";\n");
            statements.push((StatementPhase::After, validation));
        }
        statements
    }
}

/// How a witness's component is written in its statements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitnessComponent {
    /// The witness's own component text: a name the module binds. With
    /// `tag`, the text stands for the authored tag name and maps to it.
    Named { tag: Option<SourceRange> },
    /// The authored component expression (`<component :is>`), read in the
    /// template context.
    Authored,
}

/// Placeholder the adapter's callee is rendered around.
const CALLEE_SLOT: &str = "\u{0}";

/// Writes one witness's pieces, mapping each admitted expression once.
struct FragmentWriter<'w> {
    reentry: &'w CallbackReentryPlan,
    witness: &'w ComponentUseWitness,
    component: WitnessComponent,
    mapped: Vec<&'w AdmittedExpressionId>,
}

impl<'w> FragmentWriter<'w> {
    /// The expression, mapped at its first occurrence and replayed after.
    fn expression(&mut self, fragment: &mut CheckingFragment, id: &'w AdmittedExpressionId) {
        if self.mapped.contains(&id) {
            fragment.push_replay(id.clone());
        } else {
            self.mapped.push(id);
            fragment.push_expression(id.clone());
        }
    }

    fn component(&mut self, fragment: &mut CheckingFragment) {
        match self.component {
            WitnessComponent::Named { tag: Some(tag) } => {
                fragment.push_alias(self.witness.component.as_str(), tag);
            }
            WitnessComponent::Named { tag: None } => {
                fragment.push_synth(self.witness.component.as_str());
            }
            WitnessComponent::Authored => {
                let witness = self.witness;
                fragment.push_synth("(");
                self.expression(fragment, &witness.component_expression);
                fragment.push_synth(")");
            }
        }
    }

    /// A value as [`MemberValue::render`] writes it. An inline handler is
    /// the re-entry plan's callback once that plan is projected.
    fn value(&mut self, fragment: &mut CheckingFragment, value: &'w MemberValue) {
        match value {
            MemberValue::Expression { id, .. } => {
                fragment.push_synth("(");
                self.expression(fragment, id);
                fragment.push_synth(")");
            }
            MemberValue::InlineHandler { id, statements, .. } => {
                if !self.reentry.pending {
                    fragment.append(self.reentry.inline_handler(id, *statements));
                } else if *statements {
                    fragment.push_synth(STATEMENTS_HANDLER_OPEN);
                    self.expression(fragment, id);
                    fragment.push_synth(";\n}");
                } else {
                    fragment.push_synth(EXPRESSION_HANDLER_OPEN);
                    self.expression(fragment, id);
                    fragment.push_synth(")");
                }
            }
            MemberValue::StaticText(text) => {
                fragment.push_synth(quote(text));
            }
            MemberValue::Valueless => {
                fragment.push_synth("true");
            }
        }
    }
}

/// Witnesses of every use of one admitted plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentUseProjection {
    /// Plan snapshot the witnesses were derived from.
    pub snapshot: PlanSnapshotId,
    /// False when the plan or its attribute products were incomplete or a
    /// use's component could not be named; incomplete products never warm
    /// caches.
    pub complete: bool,
    /// Witnesses in plan use order.
    pub witnesses: Vec<ComponentUseWitness>,
}

impl ComponentUseProjection {
    /// Witness of `use_id`.
    #[must_use]
    pub fn witness(&self, use_id: &ComponentUseId) -> Option<&ComponentUseWitness> {
        self.witnesses.iter().find(|w| w.use_id == *use_id)
    }

    /// The prelude followed by every witness, in plan use order.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::from(USE_PRELUDE);
        out.push_str(&self.render_witnesses());
        out
    }

    /// Every witness without the prelude, in plan use order, for a scope
    /// whose module already declares the prelude.
    #[must_use]
    pub fn render_witnesses(&self) -> String {
        self.witnesses
            .iter()
            .map(ComponentUseWitness::render)
            .collect()
    }
}

/// Derive one witness per use from the plan and its attribute products.
#[must_use]
pub fn project_component_uses(
    plan: &ProjectionPlan,
    attributes: &AttributeOperationsProjection,
) -> ComponentUseProjection {
    let props = project_props(attributes);
    let mut complete = plan.is_complete() && attributes.complete && props.complete;
    let mut witnesses = Vec::with_capacity(plan.uses.len());
    for use_ in &plan.uses {
        let joined = attributes
            .sequences
            .iter()
            .zip(&attributes.key_plans)
            .find(|(sequence, _)| sequence.use_id == use_.id);
        let witness = joined
            .and_then(|(sequence, key_plan)| build_witness(plan, use_, sequence, key_plan, &props));
        match witness {
            Some(witness) => witnesses.push(witness),
            None => complete = false,
        }
    }
    ComponentUseProjection {
        snapshot: plan.snapshot.clone(),
        complete,
        witnesses,
    }
}

fn build_witness(
    plan: &ProjectionPlan,
    use_: &ComponentUse,
    sequence: &VueAttributeSequence,
    key_plan: &RuntimePropertyKeyPlan,
    props: &PropsProjection,
) -> Option<ComponentUseWitness> {
    let occurrence = plan.expression(&use_.component_expression)?;
    let component = constructor_expression(occurrence.kind, &occurrence.spelling)?;
    let binding = format!("{WITNESS_PREFIX}{}", &use_.id.digest_hex()[..16]);
    let transaction = assemble_transaction(plan, use_, sequence, key_plan, props)?;
    let observations = observe(&binding, use_, key_plan);
    let specialization = specialization_key(use_, &component, &transaction, &observations);
    Some(ComponentUseWitness {
        use_id: use_.id.clone(),
        specialization,
        binding,
        component_expression: use_.component_expression.clone(),
        component,
        transaction,
        observations,
    })
}

/// The constructor expression a use names: a tag resolves to its PascalCase
/// binding, a dynamic `:is` is its own expression.
fn constructor_expression(kind: ExpressionKind, spelling: &str) -> Option<String> {
    match kind {
        ExpressionKind::ComponentIs if !spelling.trim().is_empty() => {
            Some(format!("({})", spelling.trim()))
        }
        ExpressionKind::ComponentTag => {
            let name = if spelling.contains('-') {
                to_pascal_case(spelling)
            } else {
                spelling.to_string()
            };
            is_member_expression(&name).then_some(name)
        }
        _ => None,
    }
}

fn assemble_transaction(
    plan: &ProjectionPlan,
    use_: &ComponentUse,
    sequence: &VueAttributeSequence,
    key_plan: &RuntimePropertyKeyPlan,
    props: &PropsProjection,
) -> Option<InferenceTransaction> {
    let mut members = Vec::new();
    let mut listeners = Vec::new();
    let mut validations = Vec::new();
    let mut excluded = Vec::new();
    let mut exclude = |op_index: u32, reason: ExclusionReason| {
        if !excluded
            .iter()
            .any(|e: &ExcludedOperation| e.op_index == op_index && e.reason == reason)
        {
            excluded.push(ExcludedOperation { op_index, reason });
        }
    };
    for op_index in &key_plan.no_property {
        exclude(*op_index, ExclusionReason::NoProperty);
    }
    for op in &sequence.operations {
        let value = |id: &AdmittedExpressionId| -> Option<MemberValue> {
            let spelling = plan.expression(id)?.spelling.clone();
            let handler = use_
                .operations
                .iter()
                .find(|plan_op| plan_op.index == op.index)
                .and_then(|plan_op| plan_op.handler);
            Some(match handler {
                Some(shape @ (HandlerShape::Inline | HandlerShape::Statements)) => {
                    MemberValue::InlineHandler {
                        id: id.clone(),
                        spelling,
                        statements: shape == HandlerShape::Statements,
                    }
                }
                _ => MemberValue::Expression {
                    id: id.clone(),
                    spelling,
                },
            })
        };
        match op.syntax {
            AttributeSyntax::BindObject => match op.value.as_ref() {
                Some(id) => members.push(TransactionMember::Spread {
                    op_index: op.index,
                    value: value(id)?,
                    overwritten_keys: props
                        .obligations
                        .iter()
                        .find(|obligation| {
                            obligation.use_id == sequence.use_id && obligation.op_index == op.index
                        })
                        .filter(|obligation| obligation.checks_known_keys())
                        .map(|obligation| obligation.overwritten_keys.clone())
                        .unwrap_or_default(),
                }),
                None => exclude(op.index, ExclusionReason::MissingValue),
            },
            AttributeSyntax::OnObject => exclude(op.index, ExclusionReason::ListenerObject),
            _ => {}
        }
        for write in key_plan.writes.iter().filter(|w| w.op_index == op.index) {
            let RuntimeKey::Static(key) = &write.key else {
                exclude(op.index, ExclusionReason::DynamicKey);
                continue;
            };
            let reason = match write.value {
                WriteValue::ModelUpdate => Some(ExclusionReason::ModelUpdate),
                WriteValue::ModelModifiers => Some(ExclusionReason::ModelModifiers),
                _ if is_reserved(key) => Some(ExclusionReason::Reserved),
                _ if key.starts_with('.') || key.starts_with('^') => {
                    Some(ExclusionReason::DomBinding)
                }
                _ => None,
            };
            if let Some(reason) = reason {
                exclude(op.index, reason);
                continue;
            }
            let Some(property) = key_plan.effective(key) else {
                continue;
            };
            if property.overridden.contains(&op.index) {
                exclude(op.index, ExclusionReason::Overridden);
                continue;
            }
            let carried = match write.value {
                // A static value is admitted whenever one is written, even
                // an empty `flag=""`; only a bare attribute has none.
                WriteValue::StaticText if op.value.is_none() => MemberValue::Valueless,
                WriteValue::StaticText => {
                    MemberValue::StaticText(op.static_text.clone().unwrap_or_default())
                }
                _ => match op.value.as_ref() {
                    Some(id) => value(id)?,
                    None => {
                        exclude(op.index, ExclusionReason::MissingValue);
                        continue;
                    }
                },
            };
            place(
                property,
                op,
                key,
                carried,
                &mut members,
                &mut listeners,
                &mut validations,
            );
        }
    }
    // A listener key accumulates at runtime, so no spread can drop its
    // first handler: it reaches the construction after every spread.
    members.extend(listeners);
    Some(InferenceTransaction {
        members,
        validations,
        excluded,
    })
}

/// Route one effective write: the first handler of a listener key and the
/// first write of any other key reach the construction; a later write of a
/// combined or accumulated key, a later key naming an already constructed
/// prop, an inline handler and any handler of an event-option key (whose
/// contract may be another key's) is a validation check.
fn place(
    property: &EffectiveProperty,
    op: &VueAttributeOp,
    key: &str,
    value: MemberValue,
    members: &mut Vec<TransactionMember>,
    listeners: &mut Vec<TransactionMember>,
    validations: &mut Vec<ValidationCheck>,
) {
    let op_index = op.index;
    // A key is placed when a member already has its camel spelling: distinct
    // runtime keys (`:foo-bar` and `:fooBar`) can name one prop or listener,
    // which the construction holds once.
    let contract = contract_key(key);
    let placed = |list: &[TransactionMember]| {
        list.iter()
            .filter_map(TransactionMember::key)
            .any(|placed| contract_key(placed) == contract)
    };
    let fallback = listener_fallback(property.rule, key);
    let constructible = !matches!(value, MemberValue::InlineHandler { .. })
        && !has_event_option(property.rule, key);
    match property.rule {
        MergeRule::Accumulate if constructible && !placed(listeners) => {
            listeners.push(TransactionMember::Property {
                op_index,
                key: key.to_string(),
                key_source: op.key_source,
                value,
            });
        }
        MergeRule::Accumulate => validations.push(ValidationCheck {
            op_index,
            key: key.to_string(),
            contract: CheckContract::Listener,
            fallback,
            value,
        }),
        // A later write of a combined key, or a later key naming an already
        // constructed prop, is checked against that prop.
        MergeRule::Combine | MergeRule::Overwrite if placed(members) => {
            validations.push(ValidationCheck {
                op_index,
                key: key.to_string(),
                contract: CheckContract::Prop,
                fallback: None,
                value,
            });
        }
        MergeRule::Combine | MergeRule::Overwrite => members.push(TransactionMember::Property {
            op_index,
            key: key.to_string(),
            key_source: op.key_source,
            value,
        }),
    }
}

fn observe(
    binding: &str,
    use_: &ComponentUse,
    key_plan: &RuntimePropertyKeyPlan,
) -> Vec<SpecializedUseObservation> {
    let witness = format!("typeof {binding}");
    let mut observations = Vec::new();
    for slot in use_
        .provided_slots
        .iter()
        .filter(|slot| slot.expression.is_none())
    {
        observations.push(SpecializedUseObservation {
            kind: ObservationKind::Slot {
                name: slot.name.clone(),
            },
            type_text: format!("{USE_SLOT_PROPS}<{witness}, {}>", quote(&slot.name)),
        });
    }
    for property in &key_plan.effective {
        let key = &property.key;
        // Whatever its spelling (`@change` or `:onChange`), an authored
        // handler of a listener key is a listener of that key.
        let authored_listener = key_plan.writes.iter().any(|write| {
            write.value == WriteValue::Expression
                && matches!(&write.key, RuntimeKey::Static(k) if k == key)
        });
        if property.rule == MergeRule::Accumulate && authored_listener && !is_reserved(key) {
            let fallback = listener_fallback(property.rule, key);
            observations.push(SpecializedUseObservation {
                type_text: format!(
                    "{USE_LISTENER}<{witness}, {}{}>",
                    quote(key),
                    fallback_argument(fallback.as_deref())
                ),
                kind: ObservationKind::Event {
                    key: key.clone(),
                    fallback,
                },
            });
        }
    }
    for write in &key_plan.writes {
        if let (WriteValue::ModelUpdate, RuntimeKey::Static(key)) = (write.value, &write.key) {
            let name = key.trim_start_matches("onUpdate:").to_string();
            observations.push(SpecializedUseObservation {
                kind: ObservationKind::Model { name },
                type_text: format!("{USE_MODEL}<{witness}, {}>", quote(key)),
            });
        }
    }
    observations.push(SpecializedUseObservation {
        kind: ObservationKind::Instance,
        type_text: witness,
    });
    observations
}

fn specialization_key(
    use_: &ComponentUse,
    component: &str,
    transaction: &InferenceTransaction,
    observations: &[SpecializedUseObservation],
) -> SpecializationKey {
    let mut encoder = CanonicalEncoder::new(SPECIALIZATION_DOMAIN);
    encoder.field_bytes(1, use_.id.canonical_bytes());
    encoder.field_str(2, component);
    encoder.field_u32(3, transaction.members.len() as u32);
    let mut tag = 4u16;
    let mut field = |encoder: &mut CanonicalEncoder, value: &str| {
        encoder.field_str(tag, value);
        tag = tag.saturating_add(1);
    };
    for member in &transaction.members {
        field(&mut encoder, &member.render("component"));
    }
    for check in &transaction.validations {
        field(&mut encoder, &check.key);
        field(&mut encoder, check.fallback.as_deref().unwrap_or_default());
        field(&mut encoder, &check.value.render());
    }
    for observation in observations {
        field(&mut encoder, &observation.type_text);
    }
    SpecializationKey(encoder.digest())
}

/// Whether a listener key carries an event-option postfix (`Once` /
/// `Passive` / `Capture`), whose contract may be another key's.
fn has_event_option(rule: MergeRule, key: &str) -> bool {
    rule == MergeRule::Accumulate
        && ["Once", "Passive", "Capture"].iter().any(|option| {
            // `on` alone names no event.
            key.strip_suffix(option).is_some_and(|base| base.len() > 2)
        })
}

/// Listener key an event-option key falls back to: component `emit` calls
/// `props[handler]` and `props[handler + "Once"]` with the event payload,
/// so exactly one trailing `Once` is stripped. A `Capture` / `Passive`
/// key (or `Once` followed by another option) is no emit listener and has
/// no fallback. `None` for a non-listener key or one without that postfix.
fn listener_fallback(rule: MergeRule, key: &str) -> Option<String> {
    if !has_event_option(rule, key) {
        return None;
    }
    key.strip_suffix("Once").map(str::to_string)
}

fn fallback_argument(fallback: Option<&str>) -> String {
    fallback
        .map(|key| format!(", {}", quote(key)))
        .unwrap_or_default()
}

/// The camel-case prop spelling of a runtime key, which the runtime's
/// `camelize(key)` declared-prop lookup reads (`initial-count` is the
/// `initialCount` prop); `data-*` / `aria-*` attributes keep their authored
/// spelling. The IDE template path's `normalized_component_prop_name` owns
/// this rule; the runtime key plan keeps the authored key. Two runtime keys
/// with one camel spelling name one prop.
fn contract_key(key: &str) -> std::borrow::Cow<'_, str> {
    normalized_component_prop_name(true, key)
}

/// The key arguments of a direct-key check: the camel-case spelling, then
/// the authored spelling when they differ, under which an undeclared
/// attribute falls through (`stroke-width` reaches an `<svg>` root as
/// `stroke-width`).
fn direct_key_arguments(key: &str) -> String {
    let contract = contract_key(key);
    if contract == key {
        quote(key)
    } else {
        format!("{}, {}", quote(&contract), quote(key))
    }
}

pub(super) fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

impl crate::framework_common::vue_projection_backend::VueProjectionProduct
    for ComponentUseProjection
{
    fn project(
        backend: &crate::framework_common::vue_projection_backend::VueProjectionBackend,
        input: &crate::framework_common::vue_projection_backend::VueProductInput<'_>,
    ) -> Result<Self, super::script_setup::SetupProjectionRefusal> {
        backend.component_uses(input.source, input.artifact, input.canonical_id)
    }
}
