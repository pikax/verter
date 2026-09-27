//! Vue event transport facts derived from the admitted projection plan.
//!
//! Event identity (the name supplied to `emit`) is deliberately distinct
//! from the raw runtime property key (`onSave`).  That distinction prevents a
//! bound prop such as `:on-save` from being treated as `@save`, while still
//! letting every spelling of a real listener share its consumer set.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, TSLiteral, TSType};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

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
    /// `open` means a non-literal branch remains, so those candidates are
    /// definite names inside an open domain rather than a closed union.
    Dynamic {
        expression: AdmittedExpressionId,
        candidates: Vec<String>,
        open: bool,
    },
}

/// Runtime listener property key(s) associated with one event identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerKey {
    /// A known runtime property key.
    Static(String),
    /// Keys derived from a closed finite dynamic event-name union.
    Finite(Vec<String>),
    /// Definite literal keys plus the open dynamic domain. A non-literal
    /// branch is still a possible listener, so it is not dropped.
    FiniteOpen(Vec<String>),
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
    ModelUpdate { op_index: u32, event: EventName },
}

/// Every author/synthesis route that can feed one listener property key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerConsumerSet {
    /// Logical component use.
    pub use_id: ComponentUseId,
    /// Static runtime key, or `None` for the shared open listener domain.
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
                AttributeSyntax::Static => {}
                AttributeSyntax::On => {
                    let Some(event) = event_name(
                        plan,
                        op.argument_spelling.as_deref(),
                        op.dynamic_argument.as_ref(),
                    ) else {
                        complete = false;
                        continue;
                    };
                    let listener_key = listener_key_for(op.index, key_plan, &event, &op.modifiers);
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
                    let event = model_event_name(
                        plan,
                        op.argument_spelling.as_deref(),
                        op.dynamic_argument.as_ref(),
                    );
                    let listener_key = key_plan
                        .writes
                        .iter()
                        .find(|write| {
                            write.op_index == op.index && write.value == WriteValue::ModelUpdate
                        })
                        .map(|write| match &write.key {
                            RuntimeKey::Static(key) => ListenerKey::Static(key.clone()),
                            RuntimeKey::Dynamic => dynamic_listener_key(&event, &op.modifiers),
                        })
                        .unwrap_or_else(|| dynamic_listener_key(&event, &op.modifiers));
                    aliases.push(EventAliasRelation {
                        use_id: sequence.use_id.clone(),
                        op_index: op.index,
                        event: event.clone(),
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
            open: plan
                .expression(id)
                .is_none_or(|expression| dynamic_domain_open(&expression.spelling)),
        }),
        (None, None) => None,
    }
}

fn listener_key_for(
    op_index: u32,
    key_plan: &crate::ide::vue_projection::attribute_operations::RuntimePropertyKeyPlan,
    event: &EventName,
    modifiers: &[String],
) -> ListenerKey {
    if let Some(write) = key_plan
        .writes
        .iter()
        .find(|write| write.op_index == op_index)
    {
        return match &write.key {
            RuntimeKey::Static(key) => ListenerKey::Static(key.clone()),
            RuntimeKey::Dynamic => dynamic_listener_key(event, modifiers),
        };
    }
    dynamic_listener_key(event, modifiers)
}

fn dynamic_listener_key(event: &EventName, modifiers: &[String]) -> ListenerKey {
    let EventName::Dynamic {
        candidates, open, ..
    } = event
    else {
        return ListenerKey::Dynamic;
    };
    if candidates.is_empty() {
        return if *open {
            ListenerKey::Dynamic
        } else {
            ListenerKey::Finite(Vec::new())
        };
    }
    let mut keys = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let key = super::attribute_operations::handler_key(candidate, modifiers);
        if !keys.iter().any(|existing| existing == &key) {
            keys.push(key);
        }
    }
    if *open {
        ListenerKey::FiniteOpen(keys)
    } else {
        ListenerKey::Finite(keys)
    }
}

fn model_event_name(
    plan: &ProjectionPlan,
    argument: Option<&str>,
    dynamic: Option<&AdmittedExpressionId>,
) -> EventName {
    if let Some(id) = dynamic {
        let spelling = plan
            .expression(id)
            .map(|expression| expression.spelling.as_str())
            .unwrap_or("");
        let candidates = literal_union(spelling)
            .into_iter()
            .map(|candidate| format!("update:{}", camelize(&candidate)))
            .collect();
        return EventName::Dynamic {
            expression: id.clone(),
            candidates,
            open: dynamic_domain_open(spelling),
        };
    }
    EventName::Static(model_event(argument))
}

fn model_event(argument: Option<&str>) -> String {
    match argument {
        Some(argument) => format!("update:{}", camelize(argument)),
        None => "update:modelValue".to_string(),
    }
}

fn add_listener(
    listeners: &mut Vec<ListenerConsumerSet>,
    use_id: ComponentUseId,
    key: ListenerKey,
    consumer: ListenerConsumer,
) {
    match key {
        ListenerKey::Static(key) => push_consumer(listeners, use_id, Some(key), consumer),
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
        ListenerKey::FiniteOpen(keys) => {
            for key in keys {
                add_listener(
                    listeners,
                    use_id.clone(),
                    ListenerKey::Static(key),
                    consumer.clone(),
                );
            }
            add_listener(listeners, use_id, ListenerKey::Dynamic, consumer);
        }
        ListenerKey::Dynamic => push_consumer(listeners, use_id, None, consumer),
    }
}

fn push_consumer(
    listeners: &mut Vec<ListenerConsumerSet>,
    use_id: ComponentUseId,
    key: Option<String>,
    consumer: ListenerConsumer,
) {
    let index = listeners
        .iter()
        .position(|set| set.use_id == use_id && set.key == key);
    let index = index.unwrap_or_else(|| {
        listeners.push(ListenerConsumerSet {
            use_id,
            key,
            consumers: Vec::new(),
            collision: false,
        });
        listeners.len() - 1
    });
    let set = &mut listeners[index];
    set.consumers.push(consumer);
    set.collision = set.consumers.len() > 1;
}

/// Whether a dynamic event expression still has a non-literal branch.
/// A closed result is a string literal, a conditional whose branches are
/// closed, or an assertion/satisfies whose type is a finite string-literal
/// union. Anything else leaves the listener domain open.
fn dynamic_domain_open(expression: &str) -> bool {
    let expression = expression.trim();
    if expression.is_empty() {
        return true;
    }
    let allocator = Allocator::default();
    let Ok(parsed) = Parser::new(&allocator, expression, SourceType::ts()).parse_expression()
    else {
        return true;
    };
    let span = parsed.span();
    if span.start != 0 || span.end as usize != expression.len() {
        return true;
    }
    !event_name_branches_closed(&parsed)
}

fn event_name_branches_closed(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::StringLiteral(_) => true,
        Expression::ParenthesizedExpression(inner) => event_name_branches_closed(&inner.expression),
        Expression::TSNonNullExpression(inner) => event_name_branches_closed(&inner.expression),
        Expression::TSAsExpression(inner) => {
            asserted_name_closed(&inner.expression, &inner.type_annotation)
        }
        Expression::TSSatisfiesExpression(inner) => {
            asserted_name_closed(&inner.expression, &inner.type_annotation)
        }
        Expression::TSTypeAssertion(inner) => {
            asserted_name_closed(&inner.expression, &inner.type_annotation)
        }
        Expression::ConditionalExpression(inner) => {
            event_name_branches_closed(&inner.consequent)
                && event_name_branches_closed(&inner.alternate)
        }
        _ => false,
    }
}

fn asserted_name_closed(expression: &Expression<'_>, ty: &TSType<'_>) -> bool {
    if ty.is_const_type_reference() {
        event_name_branches_closed(expression)
    } else {
        finite_string_union(ty)
    }
}

fn finite_string_union(ty: &TSType<'_>) -> bool {
    fn walk(ty: &TSType<'_>, depth: u32) -> bool {
        if depth > 8 {
            return false;
        }
        match ty {
            TSType::TSParenthesizedType(inner) => walk(&inner.type_annotation, depth + 1),
            TSType::TSLiteralType(literal) => {
                matches!(&literal.literal, TSLiteral::StringLiteral(_))
            }
            TSType::TSUnionType(union) => {
                !union.types.is_empty() && union.types.iter().all(|member| walk(member, depth + 1))
            }
            _ => false,
        }
    }
    walk(ty, 0)
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
