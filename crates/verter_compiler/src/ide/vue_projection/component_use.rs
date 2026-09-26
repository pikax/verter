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
//!   signature, so TypeScript still
//!   infers the component's binder at the construction, checks and
//!   contextually types every declared key, and keeps literal types, while
//!   a fallthrough attribute is not an excess-key error (excess-key policy
//!   is a separate obligation, not a side effect of inference).
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
use crate::ide::vue_projection::attribute_operations::{
    is_reserved, AttributeOperationsProjection, AttributeSyntax, EffectiveProperty, MergeRule,
    RuntimeKey, RuntimePropertyKeyPlan, VueAttributeSequence, WriteValue,
};
use crate::ide::vue_projection::generic_interop::{
    foreign_contract_declarations, ForeignComponentContractAdapter,
};
use crate::template::code_gen::shared::helpers::{
    is_member_expression, to_pascal_case, trim_handler_body,
};

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
    "type __VerterUseProp<I, K extends PropertyKey> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : unknown) : unknown;\n",
    "type __VerterUseListener<I, K extends PropertyKey, F extends PropertyKey = K> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : F extends keyof P ? P[F] : (...args: any[]) => unknown) : (...args: any[]) => unknown;\n",
    "type __VerterUseSlotProps<I, K extends PropertyKey> = I extends { readonly $slots: infer S } ? (K extends keyof S ? (NonNullable<S[K]> extends (props: infer A, ...rest: any[]) => any ? A : unknown) : unknown) : unknown;\n",
    "type __VerterUseModel<I, K extends PropertyKey> = NonNullable<__VerterUseProp<I, K>> extends (value: infer V, ...rest: any[]) => any ? V : unknown;\n",
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
            } => format!("($event) => {{ {spelling};\n}}"),
            Self::InlineHandler { spelling, .. } => {
                format!("($event) => ({})", trim_handler_body(spelling))
            }
            Self::StaticText(text) => quote(text),
        }
    }

    /// Admitted expression, when the value is authored as one.
    #[must_use]
    pub fn expression(&self) -> Option<&AdmittedExpressionId> {
        match self {
            Self::Expression { id, .. } | Self::InlineHandler { id, .. } => Some(id),
            Self::StaticText(_) => None,
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
        /// Carried value.
        value: MemberValue,
    },
    /// `...(value)`: a `v-bind` object whose keys syntax cannot name.
    Spread {
        /// Contributing operation index.
        op_index: u32,
        /// Carried value.
        value: MemberValue,
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

    fn render(&self) -> String {
        match self {
            Self::Property { key, value, .. } => format!("{}: {}", quote(key), value.render()),
            Self::Spread { value, .. } => format!("...{}", value.render()),
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
            .map(TransactionMember::render)
            .collect();
        let mut out = format!(
            "const {} = new ({})({{ {} }});\n",
            self.binding,
            ForeignComponentContractAdapter.construction_callee(&self.component),
            members.join(", ")
        );
        for (ordinal, check) in self.transaction.validations.iter().enumerate() {
            let contract = match check.contract {
                CheckContract::Listener => USE_LISTENER,
                CheckContract::Prop => USE_PROP,
            };
            out.push_str(&format!(
                "const {}_check{ordinal}: {contract}<typeof {}, {}{}> = {};\n",
                self.binding,
                self.binding,
                quote(&check.key),
                fallback_argument(check.fallback.as_deref()),
                check.value.render()
            ));
        }
        out
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
    let mut complete = plan.is_complete() && attributes.complete;
    let mut witnesses = Vec::with_capacity(plan.uses.len());
    for use_ in &plan.uses {
        let joined = attributes
            .sequences
            .iter()
            .zip(&attributes.key_plans)
            .find(|(sequence, _)| sequence.use_id == use_.id);
        let witness =
            joined.and_then(|(sequence, key_plan)| build_witness(plan, use_, sequence, key_plan));
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
) -> Option<ComponentUseWitness> {
    let occurrence = plan.expression(&use_.component_expression)?;
    let component = constructor_expression(occurrence.kind, &occurrence.spelling)?;
    let binding = format!("{WITNESS_PREFIX}{}", &use_.id.digest_hex()[..16]);
    let transaction = assemble_transaction(plan, use_, sequence, key_plan)?;
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
                op.index,
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
/// combined or accumulated key, an inline handler and any handler of an
/// event-option key (whose contract may be another key's) is a validation
/// check.
fn place(
    property: &EffectiveProperty,
    op_index: u32,
    key: &str,
    value: MemberValue,
    members: &mut Vec<TransactionMember>,
    listeners: &mut Vec<TransactionMember>,
    validations: &mut Vec<ValidationCheck>,
) {
    let placed = |list: &[TransactionMember]| list.iter().any(|m| m.key() == Some(key));
    let fallback = listener_fallback(property.rule, key);
    let constructible = !matches!(value, MemberValue::InlineHandler { .. })
        && !has_event_option(property.rule, key);
    match property.rule {
        MergeRule::Accumulate if constructible && !placed(listeners) => {
            listeners.push(TransactionMember::Property {
                op_index,
                key: key.to_string(),
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
        MergeRule::Combine if placed(members) => validations.push(ValidationCheck {
            op_index,
            key: key.to_string(),
            contract: CheckContract::Prop,
            fallback: None,
            value,
        }),
        MergeRule::Combine | MergeRule::Overwrite => members.push(TransactionMember::Property {
            op_index,
            key: key.to_string(),
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
        field(&mut encoder, &member.render());
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

fn quote(text: &str) -> String {
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
