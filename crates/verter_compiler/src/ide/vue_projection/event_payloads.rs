//! Listener assignability against the specialized event contract.
//!
//! Each listener of a component use is one obligation: the authored handler
//! must be assignable to the contract the use's witness reports for its
//! listener key. The construction and the validation checks of the witness
//! already place a named listener, so this product records which of them
//! carries each listener and the contract it is read against; it renders
//! only the one check the witness cannot, `v-on="object"`, where the object's
//! keys are event names and every declared one is checked.
//!
//! The contract is always the witness's whole listener type
//! (`__VerterUseListener<typeof W, key>`), so overloaded and rest payloads
//! keep every signature; no single signature is extracted with
//! `Parameters<>`.

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, ComponentUseId, HandlerShape, PlanSnapshotId, ProjectionPlan,
};
use crate::ide::vue_projection::branches::{TemplateBranchProjection, TemplateItem};
use crate::ide::vue_projection::callback_reentry::{CallbackKind, CallbackReentryPlan};
use crate::ide::vue_projection::checking_fragment::{
    CheckingFragment, ObligationFamily, PlacedStatement, PreludeDeclaration, StatementAnchor,
    StatementPhase, UseHost,
};
use crate::ide::vue_projection::component_use::{
    CheckContract, ComponentUseProjection, ComponentUseWitness, ObservationKind, TransactionMember,
};
use crate::ide::vue_projection::event_keys::{
    EventName, EventTransportPlan, ListenerConsumer, ListenerKey,
};

/// Checks a `v-on="object"` value against the specialized instance: the key
/// `save` is read against the instance's `onSave` contract through the same
/// listener contract and `Once` fallback a named listener uses.
pub const LISTENER_OBJECT_CHECK: &str = "__VerterListeners";
/// The first parameter of a listener contract: what `$event` is in an inline
/// handler. An observation only: the handler itself is checked against the
/// whole contract, so every overload still applies.
pub const LISTENER_EVENT: &str = "__VerterListenerEvent";

pub const EVENT_PAYLOAD_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "30-event-payloads",
    text: concat!(
        "type __VerterListenerFallback<K extends string> = K extends `${infer B}Once` ? B : K;\n",
        "type __VerterListenerContract<I, E> = __VerterUseListener<I, `on${Capitalize<E & string>}`, __VerterListenerFallback<`on${Capitalize<E & string>}`>>;\n",
        "type __VerterListenerObject<I, L> = { [E in keyof L]: __VerterListenerContract<I, E> };\n",
        "declare function __VerterListeners<I, L extends object>(instance: I, listeners: L & __VerterListenerObject<I, NoInfer<L>>): void;\n",
        "type __VerterListenerEvent<F> = F extends (event: infer E, ...rest: any[]) => any ? E : unknown;\n",
    ),
};

/// How a listener reaches its contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerRoute {
    /// A member of the use's construction.
    Construction,
    /// A validation against the specialized instance.
    Validation,
    /// `v-on="object"`: every key of the object is checked.
    ListenerObject,
}

/// One authored listener checked as assignable to its event's contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerAssignabilityObligation {
    pub host: UseHost,
    pub op_index: u32,
    /// Runtime listener key; empty for a listener object, whose keys are
    /// event names read at the type level.
    pub listener_key: String,
    pub value: AdmittedExpressionId,
    pub shape: Option<HandlerShape>,
    pub route: ListenerRoute,
}

/// The emitted event a listener key consumes and where its payload type is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitPayloadRelation {
    pub host: UseHost,
    pub event: String,
    pub listener_key: String,
    /// Type text of the whole callable contract over the witness binding.
    /// Never `Parameters<>` of one signature.
    pub contract_type: String,
}

/// The context an inline handler is checked in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerContextObservation {
    pub host: UseHost,
    pub op_index: u32,
    pub value: AdmittedExpressionId,
    /// Type text of `$event`.
    pub event_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventPayloadProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub obligations: Vec<ListenerAssignabilityObligation>,
    pub relations: Vec<EmitPayloadRelation>,
    pub contexts: Vec<ListenerContextObservation>,
    /// The statements of the listener objects, placed after their use.
    placed: Vec<PlacedStatement>,
}

impl EventPayloadProjection {
    /// The product before listeners are checked: it contributes nothing.
    #[must_use]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            pending: true,
            obligations: Vec::new(),
            relations: Vec::new(),
            contexts: Vec::new(),
            placed: Vec::new(),
        }
    }

    /// The declarations the listener object checks and the `$event`
    /// observations read, when there are any.
    #[must_use]
    pub fn preludes(&self) -> Vec<PreludeDeclaration> {
        if self.placed.is_empty() && self.contexts.is_empty() {
            return Vec::new();
        }
        vec![EVENT_PAYLOAD_PRELUDE]
    }

    /// One `__VerterListeners(W, (object));` per listener object.
    #[must_use]
    pub fn statements(&self) -> Vec<PlacedStatement> {
        self.placed.clone()
    }
}

#[must_use]
pub fn project_event_payloads(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    uses: &ComponentUseProjection,
    transport: &EventTransportPlan,
    reentry: &CallbackReentryPlan,
) -> EventPayloadProjection {
    if plan.uses.is_empty() {
        // No component use, so no listener reaches a component contract: the
        // product is complete with nothing to check, also when the template
        // has no block at all (no expression anywhere in it).
        let mut projection = EventPayloadProjection::pending(plan.snapshot.clone());
        projection.pending = false;
        projection.complete = plan.is_complete();
        return projection;
    }
    if branches.blocks.is_empty() {
        return EventPayloadProjection::pending(plan.snapshot.clone());
    }
    let mut projection = EventPayloadProjection {
        snapshot: plan.snapshot.clone(),
        complete: plan.is_complete() && branches.complete && uses.complete && transport.complete,
        pending: false,
        obligations: Vec::new(),
        relations: Vec::new(),
        contexts: Vec::new(),
        placed: Vec::new(),
    };
    for witness in &uses.witnesses {
        let Some(use_) = plan.uses.iter().find(|use_| use_.id == witness.use_id) else {
            projection.complete = false;
            continue;
        };
        let host = UseHost::Component(witness.use_id.clone());
        let first = projection.obligations.len();
        for alias in transport
            .aliases
            .iter()
            .filter(|alias| alias.use_id == witness.use_id && !alias.synthesized)
        {
            let (ListenerKey::Static(key), EventName::Static(event)) =
                (&alias.listener_key, &alias.event)
            else {
                continue;
            };
            let Some((route, value)) = carrier_of(witness, alias.op_index) else {
                continue;
            };
            let contract =
                witness
                    .observations
                    .iter()
                    .find_map(|observation| match &observation.kind {
                        ObservationKind::Event { key: observed, .. } if observed == key => {
                            Some(observation.type_text.clone())
                        }
                        _ => None,
                    });
            let shape = use_
                .operations
                .iter()
                .find(|op| op.index == alias.op_index)
                .and_then(|op| op.handler);
            if let Some(contract) = contract {
                if !projection
                    .relations
                    .iter()
                    .any(|relation| relation.host == host && relation.listener_key == *key)
                {
                    projection.relations.push(EmitPayloadRelation {
                        host: host.clone(),
                        event: event.clone(),
                        listener_key: key.clone(),
                        contract_type: contract.clone(),
                    });
                }
                if reentry.callback(&value).is_some_and(|callback| {
                    matches!(
                        callback.kind,
                        CallbackKind::InlineExpression | CallbackKind::InlineStatements
                    )
                }) {
                    projection.contexts.push(ListenerContextObservation {
                        host: host.clone(),
                        op_index: alias.op_index,
                        value: value.clone(),
                        event_type: format!("{LISTENER_EVENT}<{contract}>"),
                    });
                }
            }
            projection
                .obligations
                .push(ListenerAssignabilityObligation {
                    host: host.clone(),
                    op_index: alias.op_index,
                    listener_key: key.clone(),
                    value,
                    shape,
                    route,
                });
        }
        for set in transport
            .listeners
            .iter()
            .filter(|set| set.use_id == witness.use_id)
        {
            for consumer in &set.consumers {
                let ListenerConsumer::Object {
                    op_index,
                    expression: Some(value),
                } = consumer
                else {
                    continue;
                };
                projection
                    .obligations
                    .push(ListenerAssignabilityObligation {
                        host: host.clone(),
                        op_index: *op_index,
                        listener_key: String::new(),
                        value: value.clone(),
                        shape: None,
                        route: ListenerRoute::ListenerObject,
                    });
                match anchor_of(branches, &witness.use_id) {
                    Some(anchor) => projection.placed.push(object_statement(
                        anchor,
                        host.clone(),
                        &witness.binding,
                        value,
                    )),
                    None => projection.complete = false,
                }
            }
        }
        projection.obligations[first..].sort_by_key(|obligation| obligation.op_index);
    }
    projection
}

/// Where the witness carries the listener of `op_index` and the value it
/// carries: a member of the construction or a validation check.
fn carrier_of(
    witness: &ComponentUseWitness,
    op_index: u32,
) -> Option<(ListenerRoute, AdmittedExpressionId)> {
    let transaction = &witness.transaction;
    let member = transaction.members.iter().find_map(|member| match member {
        TransactionMember::Property {
            op_index: held,
            value,
            ..
        } if *held == op_index => value.expression(),
        _ => None,
    });
    if let Some(value) = member {
        return Some((ListenerRoute::Construction, value.clone()));
    }
    transaction
        .validations
        .iter()
        .find(|check| check.op_index == op_index && check.contract == CheckContract::Listener)
        .and_then(|check| check.value.expression())
        .map(|value| (ListenerRoute::Validation, value.clone()))
}

fn anchor_of(
    branches: &TemplateBranchProjection,
    use_id: &ComponentUseId,
) -> Option<StatementAnchor> {
    branches
        .place_of(&TemplateItem::Component(use_id.clone()))
        .map(|(block, item)| StatementAnchor::Item { block, item })
}

fn object_statement(
    anchor: StatementAnchor,
    host: UseHost,
    binding: &str,
    value: &AdmittedExpressionId,
) -> PlacedStatement {
    let mut fragment = CheckingFragment::synth(format!("{LISTENER_OBJECT_CHECK}({binding}, ("));
    fragment.push_expression(value.clone()).push_synth("));\n");
    PlacedStatement {
        anchor,
        phase: StatementPhase::After,
        family: ObligationFamily::Listener,
        host: Some(host),
        fragment,
    }
}
