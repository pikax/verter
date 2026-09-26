//! Vue event transport facts derived from the admitted projection plan.
//!
//! Event identity (the name supplied to `emit`) is deliberately distinct
//! from the raw runtime property key (`onSave`).  That distinction prevents a
//! bound prop such as `:on-save` from being treated as `@save`, while still
//! letting every spelling of a real listener share its consumer set.

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, ComponentUseId, PlanSnapshotId, ProjectionPlan,
};
use crate::ide::vue_projection::attribute_operations::{
    AttributeOperationsProjection, AttributeSyntax, RuntimeKey, WriteValue,
};
use crate::template::code_gen::vdom::props::camelize;

/// Name supplied to Vue's event transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventName {
    /// A statically authored or synthesized event name.
    Static(String),
    /// A dynamic argument. Finite string-literal candidates are retained so
    /// every possible listener can be checked by a later TypeScript consumer.
    Dynamic {
        expression: AdmittedExpressionId,
        candidates: Vec<String>,
    },
}

/// Runtime listener property key(s) associated with one event identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerKey {
    /// A known runtime property key.
    Static(String),
    /// Keys derived from a finite dynamic event-name union.
    Finite(Vec<String>),
    /// An open dynamic key.
    Dynamic,
}

/// One relation between an event identity and its listener key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventAliasRelation {
    /// Logical component use owning the event.
    pub use_id: ComponentUseId,
    /// Authored operation index, or the model operation that synthesized it.
    pub op_index: u32,
    /// Event identity retained for diagnostics and rename.
    pub event: EventName,
    /// Runtime listener key(s), kept separate from `event`.
    pub listener_key: ListenerKey,
    /// Authored modifiers in source order.
    pub modifiers: Vec<String>,
    /// Whether this is `v-model`'s synthesized update listener.
    pub synthesized: bool,
}

/// A contributor to a listener consumer set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerConsumer {
    /// A statically named listener directive.
    Directive { op_index: u32, event: EventName },
    /// An argument-less `v-on` object. It remains an open listener domain;
    /// it is never flattened into one guessed event.
    Object {
        op_index: u32,
        expression: Option<AdmittedExpressionId>,
    },
    /// A compiler-synthesized model update listener.
    ModelUpdate { op_index: u32, event: String },
}

/// Every author/synthesis route that can feed one listener property key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerConsumerSet {
    /// Logical component use.
    pub use_id: ComponentUseId,
    /// Static runtime key, or `None` for the open `v-on` object domain.
    pub key: Option<String>,
    /// All consumers in authored order. No collision is silently discarded.
    pub consumers: Vec<ListenerConsumer>,
    /// More than one consumer reaches this key.
    pub collision: bool,
}

/// Event products for an admitted projection-plan snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventTransportPlan {
    /// Source snapshot shared with the attribute and use products.
    pub snapshot: PlanSnapshotId,
    /// Incomplete inputs never publish a complete transport plan.
    pub complete: bool,
    /// Event identity ↔ runtime-key relations.
    pub aliases: Vec<EventAliasRelation>,
    /// Per-key listener ownership sets.
    pub listeners: Vec<ListenerConsumerSet>,
}

impl EventTransportPlan {
    /// Look up all consumers of a static listener key for a use.
    #[must_use]
    pub fn consumers(&self, use_id: &ComponentUseId, key: &str) -> Option<&ListenerConsumerSet> {
        self.listeners
            .iter()
            .find(|set| set.use_id == *use_id && set.key.as_deref() == Some(key))
    }
}

/// Derive event aliases and listener ownership without answering any types.
/// TypeScript consumers use these facts to choose their own declared handler
/// contracts and payloads.
#[must_use]
pub fn project_event_transport(
    plan: &ProjectionPlan,
    attributes: &AttributeOperationsProjection,
) -> EventTransportPlan {
    let mut complete = plan.is_complete() && attributes.complete;
    let mut aliases = Vec::new();
    let mut listeners = Vec::new();

    for sequence in &attributes.sequences {
        let Some(key_plan) = attributes
            .key_plans
            .iter()
            .find(|keys| keys.use_id == sequence.use_id)
        else {
            complete = false;
            continue;
        };
        for op in &sequence.operations {
            match op.syntax {
                AttributeSyntax::On => {
                    let Some(event) = event_name(
                        plan,
                        op.argument_spelling.as_deref(),
                        op.dynamic_argument.as_ref(),
                    ) else {
                        complete = false;
                        continue;
                    };
                    let listener_key = listener_key_for(op.index, key_plan, &event);
                    aliases.push(EventAliasRelation {
                        use_id: sequence.use_id.clone(),
                        op_index: op.index,
                        event: event.clone(),
                        listener_key: listener_key.clone(),
                        modifiers: op.modifiers.clone(),
                        synthesized: false,
                    });
                    add_listener(
                        &mut listeners,
                        sequence.use_id.clone(),
                        listener_key,
                        ListenerConsumer::Directive {
                            op_index: op.index,
                            event,
                        },
                    );
                }
                AttributeSyntax::OnObject => add_listener(
                    &mut listeners,
                    sequence.use_id.clone(),
                    ListenerKey::Dynamic,
                    ListenerConsumer::Object {
                        op_index: op.index,
                        expression: op.value.clone(),
                    },
                ),
                AttributeSyntax::Model => {
                    let event = model_event(op.argument_spelling.as_deref());
                    let listener_key = key_plan
                        .writes
                        .iter()
                        .find(|write| {
                            write.op_index == op.index && write.value == WriteValue::ModelUpdate
                        })
                        .map(|write| match &write.key {
                            RuntimeKey::Static(key) => ListenerKey::Static(key.clone()),
                            RuntimeKey::Dynamic => ListenerKey::Dynamic,
                        })
                        .unwrap_or(ListenerKey::Dynamic);
                    aliases.push(EventAliasRelation {
                        use_id: sequence.use_id.clone(),
                        op_index: op.index,
                        event: EventName::Static(event.clone()),
                        listener_key: listener_key.clone(),
                        modifiers: op.modifiers.clone(),
                        synthesized: true,
                    });
                    add_listener(
                        &mut listeners,
                        sequence.use_id.clone(),
                        listener_key,
                        ListenerConsumer::ModelUpdate {
                            op_index: op.index,
                            event,
                        },
                    );
                }
                _ => {}
            }
        }
    }

    EventTransportPlan {
        snapshot: plan.snapshot.clone(),
        complete,
        aliases,
        listeners,
    }
}

fn event_name(
    plan: &ProjectionPlan,
    static_name: Option<&str>,
    dynamic: Option<&AdmittedExpressionId>,
) -> Option<EventName> {
    match (static_name, dynamic) {
        (Some(name), _) => Some(EventName::Static(name.to_string())),
        (None, Some(id)) => Some(EventName::Dynamic {
            expression: id.clone(),
            candidates: plan
                .expression(id)
                .map(|expression| literal_union(&expression.spelling))
                .unwrap_or_default(),
        }),
        (None, None) => None,
    }
}

fn listener_key_for(
    op_index: u32,
    key_plan: &crate::ide::vue_projection::attribute_operations::RuntimePropertyKeyPlan,
    event: &EventName,
) -> ListenerKey {
    if let Some(write) = key_plan
        .writes
        .iter()
        .find(|write| write.op_index == op_index)
    {
        return match &write.key {
            RuntimeKey::Static(key) => ListenerKey::Static(key.clone()),
            RuntimeKey::Dynamic => match event {
                EventName::Dynamic { candidates, .. } if !candidates.is_empty() => {
                    ListenerKey::Finite(
                        candidates
                            .iter()
                            .map(|candidate| handler_key(candidate))
                            .collect(),
                    )
                }
                _ => ListenerKey::Dynamic,
            },
        };
    }
    ListenerKey::Dynamic
}

fn model_event(argument: Option<&str>) -> String {
    match argument {
        Some(argument) => format!("update:{}", camelize(argument)),
        None => "update:modelValue".to_string(),
    }
}

fn handler_key(event: &str) -> String {
    let mut key = String::with_capacity(event.len() + 2);
    crate::template::code_gen::vdom::props::format_event_handler_key_into(&mut key, event);
    key
}

fn add_listener(
    listeners: &mut Vec<ListenerConsumerSet>,
    use_id: ComponentUseId,
    key: ListenerKey,
    consumer: ListenerConsumer,
) {
    match key {
        ListenerKey::Static(key) => {
            let index = listeners
                .iter()
                .position(|set| set.use_id == use_id && set.key.as_deref() == Some(&key));
            let index = index.unwrap_or_else(|| {
                listeners.push(ListenerConsumerSet {
                    use_id,
                    key: Some(key),
                    consumers: Vec::new(),
                    collision: false,
                });
                listeners.len() - 1
            });
            let set = &mut listeners[index];
            set.consumers.push(consumer);
            set.collision = set.consumers.len() > 1;
        }
        ListenerKey::Finite(keys) => {
            for key in keys {
                add_listener(
                    listeners,
                    use_id.clone(),
                    ListenerKey::Static(key),
                    consumer.clone(),
                );
            }
        }
        ListenerKey::Dynamic => listeners.push(ListenerConsumerSet {
            use_id,
            key: None,
            consumers: vec![consumer],
            collision: false,
        }),
    }
}

/// Extract the finite string values in a dynamic event-name union. This is
/// intentionally conservative: any non-literal branch leaves the dynamic
/// domain open while retaining the literals that are definitely present.
fn literal_union(expression: &str) -> Vec<String> {
    let mut values = Vec::new();
    let bytes = expression.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let quote = bytes[i];
        if quote != b'\'' && quote != b'"' {
            i += 1;
            continue;
        }
        let start = i + 1;
        i += 1;
        let mut escaped = false;
        while i < bytes.len() {
            if !escaped && bytes[i] == quote {
                let value = &expression[start..i];
                if !values.iter().any(|candidate| candidate == value) {
                    values.push(value.to_string());
                }
                i += 1;
                break;
            }
            escaped = !escaped && bytes[i] == b'\\';
            if bytes[i] != b'\\' {
                escaped = false;
            }
            i += 1;
        }
    }
    values
}
