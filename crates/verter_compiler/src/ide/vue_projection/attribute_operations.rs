//! Ordered Vue attribute operations and runtime-key interpretation.
//!
//! Three products per component use of an admitted [`ProjectionPlan`]:
//!
//! - [`VueAttributeSequence`] — static attrs, `v-bind` arguments/objects,
//!   `v-on` arguments/objects, `v-model`, directives and modifiers in
//!   authored order, each keeping its raw spelling next to the plan
//!   operation and admitted expressions it came from.
//! - [`RuntimePropertyKeyPlan`] — the runtime property keys those operations
//!   write, grouped the way the runtime compiler assembles them: literal
//!   groups deduplicate static keys first-wins, `v-bind`/`v-on` objects and
//!   dynamic `v-on` arguments open `mergeProps` arguments, and `mergeProps`
//!   overwrites ordinary keys, combines `class`/`style` and accumulates
//!   listeners. Opaque spreads and dynamic keys stay *possible* writers:
//!   they never make an earlier key certainly absent.
//! - [`AttributeConsumerRelation`] — per effective runtime key, every
//!   consumer channel it can reach (declared prop via camelized lookup,
//!   emitted-event handler lookup, fallthrough attr, reserved vnode key)
//!   with inference inputs and validation obligations referencing the same
//!   operations and expressions. A key serving both a declared prop and an
//!   emitted event validates against both.
//!
//! Key spelling reuses the runtime compiler's own [`camelize`] and
//! handler-key assembly. Nothing here answers types: declarations are
//! TypeScript's, so channels are published as reachable contracts, not
//! chosen answers. Vue IDE routing stays on [`crate::ide::template`] until
//! atomic activation.

use crate::ast::types::{AstNodeKind, ElementNode};
use crate::framework_common::projection_plan::{
    classify_op, AdmittedExpressionId, AttributeOpKind, ComponentUse, ComponentUseId,
    PlanSnapshotId, ProjectionPlan,
};
use crate::ide::get_directive_name;
use crate::parser::types::ParsedSfc;
use crate::template::code_gen::vdom::props::{camelize, format_event_handler_key_into};
use crate::types::NodeProp;

/// Authored syntax family of one attribute operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeSyntax {
    /// Static attribute (`title="x"`, bare `disabled`).
    Static,
    /// `:name` / `v-bind:name` / `.name`, static or dynamic argument.
    Bind,
    /// Argument-less `v-bind="obj"` object spread.
    BindObject,
    /// `@name` / `v-on:name`, static or dynamic argument.
    On,
    /// Argument-less `v-on="obj"` listener object.
    OnObject,
    /// `v-model` / `v-model:arg`.
    Model,
    /// Any other directive (`v-show`, `v-focus`, `v-html`, ...).
    Directive,
}

/// One authored attribute operation, in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueAttributeOp {
    /// Index of the matching [`crate::framework_common::OrderedAttributeOp`].
    pub index: u32,
    /// Authored syntax family.
    pub syntax: AttributeSyntax,
    /// Raw authored spelling of the attribute name, including argument and
    /// modifiers (`@save.once`, `:on-save`, `onSave`, `v-model:title`).
    pub raw_spelling: String,
    /// Directive name (`bind`, `on`, `model`, `focus`); `None` for static attrs.
    pub directive: Option<String>,
    /// Static argument spelling (`save`, `on-save`); `None` when absent or dynamic.
    pub argument_spelling: Option<String>,
    /// Admitted dynamic argument expression (`:[key]`, `@[event]`).
    pub dynamic_argument: Option<AdmittedExpressionId>,
    /// Authored modifiers, in source order.
    pub modifiers: Vec<String>,
    /// Admitted value expression (static values included).
    pub value: Option<AdmittedExpressionId>,
    /// Literal text of a static attribute (`""` when bare).
    pub static_text: Option<String>,
}

/// Authored attribute operations of one component use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueAttributeSequence {
    /// Logical component use.
    pub use_id: ComponentUseId,
    /// Written as `<component>`: its `is` operation selects the component.
    pub component_tag: bool,
    /// Operations in authored order.
    pub operations: Vec<VueAttributeOp>,
}

/// Runtime property key written by one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeKey {
    /// Key known from syntax.
    Static(String),
    /// Key computed at runtime (`:[key]`, `@[event]`, `v-model:[arg]`).
    Dynamic,
}

/// What a runtime property write carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteValue {
    /// The operation's admitted value expression.
    Expression,
    /// Literal static attribute text (always a string at runtime).
    StaticText,
    /// Compiler-synthesized `v-model` update listener.
    ModelUpdate,
    /// Compiler-synthesized `v-model` modifiers object.
    ModelModifiers,
}

/// How the runtime combines repeated writes to one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeRule {
    /// Ordinary key: a later `mergeProps` argument overwrites; within one
    /// literal group the first static write wins.
    Overwrite,
    /// `class` / `style`: every write is normalized together.
    Combine,
    /// Listener key (`isOn`): every handler accumulates.
    Accumulate,
}

/// One property written by an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePropertyWrite {
    /// Owning operation index.
    pub op_index: u32,
    /// Authored syntax of the owning operation.
    pub syntax: AttributeSyntax,
    /// Runtime key.
    pub key: RuntimeKey,
    /// Carried value.
    pub value: WriteValue,
    /// `mergeProps` argument ordinal this write is assembled into.
    pub group: u32,
}

/// Object spread whose keys are unknown to syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadKind {
    /// `v-bind="obj"`: may write any key.
    BindObject,
    /// `v-on="obj"`: may write any listener key (`toHandlers`).
    OnObject,
}

/// One opaque spread operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpaqueSpread {
    /// Owning operation index.
    pub op_index: u32,
    /// Spread kind.
    pub kind: SpreadKind,
    /// `mergeProps` argument ordinal.
    pub group: u32,
}

/// Whether a contribution certainly reaches the runtime key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Certainty {
    /// Syntax proves the write.
    Definite,
    /// An opaque spread or dynamic key may write it.
    Possible,
}

/// One operation contributing to an effective runtime key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contribution {
    /// Contributing operation index.
    pub op_index: u32,
    /// Contribution certainty.
    pub certainty: Certainty,
}

/// Effective interpretation of one statically known runtime key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveProperty {
    /// Runtime key.
    pub key: String,
    /// Merge rule.
    pub rule: MergeRule,
    /// Contributions that can reach the component, in merge order.
    pub contributors: Vec<Contribution>,
    /// Operations whose write to this key never reaches the component
    /// (first-wins literal dedupe or a later definite overwrite).
    pub overridden: Vec<u32>,
}

/// Runtime property keys of one component use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePropertyKeyPlan {
    /// Logical component use.
    pub use_id: ComponentUseId,
    /// Property writes in authored order.
    pub writes: Vec<RuntimePropertyWrite>,
    /// Opaque object spreads in authored order.
    pub spreads: Vec<OpaqueSpread>,
    /// Operations that write no property (runtime directives, `<component is>`).
    pub no_property: Vec<u32>,
    /// Effective interpretation per statically known key, first-seen order.
    pub effective: Vec<EffectiveProperty>,
}

impl RuntimePropertyKeyPlan {
    /// Effective interpretation of `key`, when statically known.
    #[must_use]
    pub fn effective(&self, key: &str) -> Option<&EffectiveProperty> {
        self.effective.iter().find(|property| property.key == key)
    }
}

/// A consumer contract a runtime key can reach. Which ones apply depends
/// on the component's declarations, which TypeScript owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumerChannel {
    /// Declared prop `name` (runtime `camelize(key)` lookup). Applies when
    /// the prop is declared; wins the props/attrs split.
    DeclaredProp {
        /// Camelized prop name.
        name: String,
    },
    /// Emitted-event handler lookup. Applies when one of `events` is
    /// declared; the raw key stays reachable by `emit` even when it is also
    /// consumed as a declared prop.
    EmittedEvent {
        /// Declared event spellings that treat this key as their listener.
        events: Vec<String>,
        /// `...Once` listener.
        once: bool,
    },
    /// Fallthrough attribute under the raw key. Applies only when neither a
    /// declared prop nor a declared event consumes the key.
    FallthroughAttr {
        /// Raw runtime key.
        key: String,
    },
    /// Reserved vnode key (`key`, `ref`, `onVnode*`): never a prop or attr.
    Reserved,
}

/// One validation obligation: a contribution checked against a channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationObligation {
    /// Contributing operation index.
    pub op_index: u32,
    /// Admitted value expression, when the value is authored.
    pub expression: Option<AdmittedExpressionId>,
    /// Carried value kind.
    pub value: WriteValue,
    /// Channel to validate against.
    pub channel: ConsumerChannel,
    /// Whether the contribution reaches the component (`false` when overridden).
    pub effective: bool,
}

/// Consumers of one statically known runtime key on one component use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeConsumerRelation {
    /// Logical component use.
    pub use_id: ComponentUseId,
    /// Runtime key.
    pub key: String,
    /// Every channel the key can reach.
    pub channels: Vec<ConsumerChannel>,
    /// Effective contributions feeding the use's inference transaction.
    pub inference_inputs: Vec<Contribution>,
    /// Every authored contribution × every reachable channel.
    pub obligations: Vec<ValidationObligation>,
}

/// Attribute products of one admitted plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeOperationsProjection {
    /// Plan snapshot the products were derived from.
    pub snapshot: PlanSnapshotId,
    /// False when the plan was incomplete or a use's operations could not
    /// be joined to authored props; incomplete products never warm caches.
    pub complete: bool,
    /// Authored sequences, one per plan use.
    pub sequences: Vec<VueAttributeSequence>,
    /// Runtime key plans, one per plan use.
    pub key_plans: Vec<RuntimePropertyKeyPlan>,
    /// Consumer relations for every effective static key.
    pub relations: Vec<AttributeConsumerRelation>,
}

impl AttributeOperationsProjection {
    /// Consumer relation for `key` on `use_id`.
    #[must_use]
    pub fn relation(
        &self,
        use_id: &ComponentUseId,
        key: &str,
    ) -> Option<&AttributeConsumerRelation> {
        self.relations
            .iter()
            .find(|relation| relation.use_id == *use_id && relation.key == key)
    }
}

/// Derive the attribute products of every use in `plan` from the same
/// admitted parse the plan was built from.
#[must_use]
pub fn project_attribute_operations(
    plan: &ProjectionPlan,
    parsed: &ParsedSfc,
    source: &str,
) -> AttributeOperationsProjection {
    let mut complete = plan.is_complete();
    let mut sequences = Vec::with_capacity(plan.uses.len());
    let mut key_plans = Vec::with_capacity(plan.uses.len());
    let mut relations = Vec::new();
    let elements = component_elements(parsed);
    for use_ in &plan.uses {
        let element = plan
            .expression(&use_.component_expression)
            .and_then(|occurrence| element_at(&elements, occurrence.start));
        let Some(sequence) = element.and_then(|el| join_sequence(use_, el, source)) else {
            complete = false;
            continue;
        };
        let key_plan = plan_runtime_keys(&sequence);
        relations.extend(relate_consumers(&sequence, &key_plan));
        sequences.push(sequence);
        key_plans.push(key_plan);
    }
    AttributeOperationsProjection {
        snapshot: plan.snapshot.clone(),
        complete,
        sequences,
        key_plans,
        relations,
    }
}

fn component_elements(parsed: &ParsedSfc) -> Vec<&ElementNode> {
    let Some(ast) = parsed.template_ast() else {
        return Vec::new();
    };
    let mut elements: Vec<&ElementNode> = ast
        .nodes
        .iter()
        .filter_map(|node| match &node.kind {
            AstNodeKind::Element(el) => Some(el.as_ref()),
            _ => None,
        })
        .collect();
    elements.sort_by_key(|el| el.tag_open.start);
    elements
}

/// The element whose open tag holds the use's component expression (tag
/// name or `:is` value). Open tags of distinct elements are disjoint.
fn element_at<'e>(elements: &[&'e ElementNode], offset: u32) -> Option<&'e ElementNode> {
    let after = elements.partition_point(|el| el.tag_open.start <= offset);
    let candidate = elements.get(after.checked_sub(1)?)?;
    (offset < candidate.tag_open.end).then_some(*candidate)
}

fn join_sequence(
    use_: &ComponentUse,
    el: &ElementNode,
    source: &str,
) -> Option<VueAttributeSequence> {
    let props: Vec<&NodeProp> = el
        .props
        .iter()
        .filter(|prop| classify_op(prop, source).is_some())
        .collect();
    if props.len() != use_.operations.len() {
        return None;
    }
    let mut operations = Vec::with_capacity(props.len());
    for (prop, op) in props.into_iter().zip(&use_.operations) {
        let directive = prop
            .is_directive
            .then(|| get_directive_name(prop, source).to_string());
        let dynamic = prop.is_dynamic == Some(true);
        let argument_spelling = if dynamic {
            None
        } else {
            slice(source, prop.arg_start, prop.arg_end)
        };
        let mut modifiers: Vec<String> = prop
            .modifiers
            .iter()
            .filter_map(|span| slice(source, Some(span.start), Some(span.end)))
            .collect();
        let raw_spelling = raw_spelling(prop, source);
        if raw_spelling.starts_with('.') && !modifiers.iter().any(|m| m == "prop") {
            // `.name` is the `v-bind:name.prop` shorthand.
            modifiers.push("prop".to_string());
        }
        let syntax = match op.kind {
            AttributeOpKind::Static => AttributeSyntax::Static,
            AttributeOpKind::VBind => AttributeSyntax::BindObject,
            AttributeOpKind::Bound => AttributeSyntax::Bind,
            AttributeOpKind::VOn if prop.arg_start.is_none() => AttributeSyntax::OnObject,
            AttributeOpKind::VOn => AttributeSyntax::On,
            AttributeOpKind::Model => AttributeSyntax::Model,
            AttributeOpKind::Directive => AttributeSyntax::Directive,
        };
        let static_text = (syntax == AttributeSyntax::Static)
            .then(|| slice(source, prop.value_start, prop.value_end).unwrap_or_default());
        operations.push(VueAttributeOp {
            index: op.index,
            syntax,
            raw_spelling,
            directive,
            argument_spelling,
            dynamic_argument: op.argument.clone(),
            modifiers,
            value: op.expression.clone(),
            static_text,
        });
    }
    let tag = source
        .get(el.tag_open.start as usize + 1..el.tag_open.name_end as usize)
        .unwrap_or_default();
    Some(VueAttributeSequence {
        use_id: use_.id.clone(),
        component_tag: tag == "component" || tag == "Component",
        operations,
    })
}

fn slice(source: &str, start: Option<u32>, end: Option<u32>) -> Option<String> {
    let (start, end) = (start?, end?);
    (start < end)
        .then(|| source.get(start as usize..end as usize))
        .flatten()
        .map(str::to_string)
}

fn raw_spelling(prop: &NodeProp, source: &str) -> String {
    let end = [
        Some(prop.name_end),
        prop.arg_end,
        prop.modifiers.last().map(|span| span.end),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(prop.name_end);
    source
        .get(prop.start as usize..end as usize)
        .unwrap_or_default()
        .to_string()
}

/// Assemble runtime property writes the way the runtime compiler builds
/// component props: literal groups between `mergeProps` arguments.
fn plan_runtime_keys(sequence: &VueAttributeSequence) -> RuntimePropertyKeyPlan {
    let mut writes = Vec::new();
    let mut spreads = Vec::new();
    let mut no_property = Vec::new();
    let mut group = 0u32;
    let mut group_open = false;
    for op in &sequence.operations {
        let write = |key: RuntimeKey, value: WriteValue, group: u32| RuntimePropertyWrite {
            op_index: op.index,
            syntax: op.syntax,
            key,
            value,
            group,
        };
        let literal = |key: String| RuntimeKey::Static(key);
        match op.syntax {
            _ if selects_component(sequence, op) => no_property.push(op.index),
            AttributeSyntax::Static => {
                writes.push(write(
                    literal(op.raw_spelling.clone()),
                    WriteValue::StaticText,
                    group,
                ));
                group_open = true;
            }
            AttributeSyntax::Bind => {
                let key = op
                    .argument_spelling
                    .as_deref()
                    .map_or(RuntimeKey::Dynamic, |arg| {
                        literal(bind_key(arg, &op.modifiers))
                    });
                writes.push(write(key, WriteValue::Expression, group));
                group_open = true;
            }
            AttributeSyntax::On => match op.argument_spelling.as_deref() {
                Some(event) => {
                    let key = literal(handler_key(event, &op.modifiers));
                    writes.push(write(key, WriteValue::Expression, group));
                    group_open = true;
                }
                None => {
                    // A dynamic event argument opens its own merge argument.
                    group += u32::from(group_open);
                    writes.push(write(RuntimeKey::Dynamic, WriteValue::Expression, group));
                    group += 1;
                    group_open = false;
                }
            },
            AttributeSyntax::BindObject | AttributeSyntax::OnObject => {
                group += u32::from(group_open);
                let kind = if op.syntax == AttributeSyntax::BindObject {
                    SpreadKind::BindObject
                } else {
                    SpreadKind::OnObject
                };
                spreads.push(OpaqueSpread {
                    op_index: op.index,
                    kind,
                    group,
                });
                group += 1;
                group_open = false;
            }
            AttributeSyntax::Model => {
                let keys = match op.argument_spelling.as_deref() {
                    _ if op.dynamic_argument.is_some() => None,
                    Some(arg) => Some((
                        arg.to_string(),
                        format!("onUpdate:{}", camelize(arg)),
                        modifier_prop_name(arg),
                    )),
                    None => Some((
                        "modelValue".to_string(),
                        "onUpdate:modelValue".to_string(),
                        "modelModifiers".to_string(),
                    )),
                };
                let key = |pick: fn(&(String, String, String)) -> &String| {
                    keys.as_ref()
                        .map_or(RuntimeKey::Dynamic, |keys| literal(pick(keys).clone()))
                };
                writes.push(write(key(|k| &k.0), WriteValue::Expression, group));
                writes.push(write(key(|k| &k.1), WriteValue::ModelUpdate, group));
                if !op.modifiers.is_empty() {
                    writes.push(write(key(|k| &k.2), WriteValue::ModelModifiers, group));
                }
                group_open = true;
            }
            // compiler-dom lowers `v-html` / `v-text` to DOM properties;
            // every other directive is a runtime directive, not a property.
            AttributeSyntax::Directive => match op.directive.as_deref() {
                Some("html") | Some("text") => {
                    let name = if op.directive.as_deref() == Some("html") {
                        "innerHTML"
                    } else {
                        "textContent"
                    };
                    writes.push(write(
                        literal(name.to_string()),
                        WriteValue::Expression,
                        group,
                    ));
                    group_open = true;
                }
                _ => no_property.push(op.index),
            },
        }
    }
    let effective = resolve_effective(&writes, &spreads);
    RuntimePropertyKeyPlan {
        use_id: sequence.use_id.clone(),
        writes,
        spreads,
        no_property,
        effective,
    }
}

/// The runtime compiler drops `is` / `:is` on `<component>` and a static
/// `is="vue:..."`: they select the component instead of writing a prop.
fn selects_component(sequence: &VueAttributeSequence, op: &VueAttributeOp) -> bool {
    match op.syntax {
        AttributeSyntax::Static if op.raw_spelling == "is" => {
            sequence.component_tag
                || op
                    .static_text
                    .as_deref()
                    .is_some_and(|text| text.starts_with("vue:"))
        }
        AttributeSyntax::Bind => {
            sequence.component_tag && op.argument_spelling.as_deref() == Some("is")
        }
        _ => false,
    }
}

/// `v-bind` key: `.camel` camelizes, `.prop` / `.attr` prefix `.` / `^`.
fn bind_key(arg: &str, modifiers: &[String]) -> String {
    let has = |name: &str| modifiers.iter().any(|m| m == name);
    let mut key = if has("camel") {
        camelize(arg).into_owned()
    } else {
        arg.to_string()
    };
    if has("prop") {
        key.insert(0, '.');
    }
    if has("attr") {
        key.insert(0, '^');
    }
    key
}

/// Component `v-on` key: `toHandlerKey(camelize(event))`, `.right` /
/// `.middle` retarget `click`, event-option modifiers append their postfix.
fn handler_key(event: &str, modifiers: &[String]) -> String {
    let mut key = String::with_capacity(event.len() + 2);
    format_event_handler_key_into(&mut key, event);
    let has = |name: &str| modifiers.iter().any(|m| m == name);
    if key.eq_ignore_ascii_case("onclick") {
        if has("right") {
            key = "onContextmenu".to_string();
        } else if has("middle") {
            key = "onMouseup".to_string();
        }
    }
    for modifier in modifiers {
        if matches!(modifier.as_str(), "passive" | "once" | "capture") {
            key.push_str(&capitalize(modifier));
        }
    }
    key
}

/// Runtime modifiers key: `${arg}Modifiers` for a static `v-model:arg`
/// (`modelModifiers` only when there is no arg). Matches
/// `transformModel` in `@vue/compiler-core`, which uses the raw static
/// arg spelling verbatim (`model-value` keeps its hyphen).
fn modifier_prop_name(model: &str) -> String {
    format!("{model}Modifiers")
}

fn capitalize(input: &str) -> String {
    let mut chars = input.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Runtime `isOn`: `on` followed by a non-lowercase-ASCII character.
fn is_on(key: &str) -> bool {
    let bytes = key.as_bytes();
    bytes.len() > 2 && bytes.starts_with(b"on") && !bytes[2].is_ascii_lowercase()
}

fn merge_rule(key: &str) -> MergeRule {
    match key {
        "class" | "style" => MergeRule::Combine,
        _ if is_on(key) => MergeRule::Accumulate,
        _ => MergeRule::Overwrite,
    }
}

/// Per-key effective contributions in merge order. Dynamic keys and
/// spreads contribute as possible writers; only a definite write can end
/// an earlier contribution.
fn resolve_effective(
    writes: &[RuntimePropertyWrite],
    spreads: &[OpaqueSpread],
) -> Vec<EffectiveProperty> {
    let mut keys: Vec<&str> = Vec::new();
    for write in writes {
        if let RuntimeKey::Static(key) = &write.key {
            if !keys.contains(&key.as_str()) {
                keys.push(key);
            }
        }
    }
    keys.into_iter()
        .map(|key| {
            let rule = merge_rule(key);
            // (group, contribution) in authored order; spreads interleave by group.
            let mut ordered: Vec<(u32, Contribution)> = Vec::new();
            let mut overridden = Vec::new();
            let mut first_static_in_group: Option<u32> = None;
            for write in writes {
                match &write.key {
                    RuntimeKey::Static(k) if k == key => {
                        let duplicate = first_static_in_group == Some(write.group);
                        if duplicate && rule == MergeRule::Overwrite {
                            // Literal-group dedupe keeps the first static write.
                            overridden.push(write.op_index);
                            continue;
                        }
                        first_static_in_group = Some(write.group);
                        ordered.push((write.group, definite(write.op_index)));
                    }
                    // A dynamic `@[event]` key is always a listener key.
                    RuntimeKey::Dynamic
                        if rule == MergeRule::Accumulate || write.syntax != AttributeSyntax::On =>
                    {
                        ordered.push((write.group, possible(write.op_index)));
                    }
                    _ => {}
                }
            }
            for spread in spreads {
                if spread.kind == SpreadKind::BindObject || rule == MergeRule::Accumulate {
                    ordered.push((spread.group, possible(spread.op_index)));
                }
            }
            ordered.sort_by_key(|(group, _)| *group);
            let mut contributors: Vec<Contribution> = ordered.into_iter().map(|(_, c)| c).collect();
            if rule == MergeRule::Overwrite {
                if let Some(last) = contributors
                    .iter()
                    .rposition(|c| c.certainty == Certainty::Definite)
                {
                    overridden.extend(contributors.drain(..last).map(|c| c.op_index));
                }
            }
            EffectiveProperty {
                key: key.to_string(),
                rule,
                contributors,
                overridden,
            }
        })
        .collect()
}

fn definite(op_index: u32) -> Contribution {
    Contribution {
        op_index,
        certainty: Certainty::Definite,
    }
}

fn possible(op_index: u32) -> Contribution {
    Contribution {
        op_index,
        certainty: Certainty::Possible,
    }
}

/// Channels a runtime key can reach, mirroring the runtime props/attrs
/// split and the `emit` handler lookup.
fn consumer_channels(key: &str) -> Vec<ConsumerChannel> {
    if is_reserved(key) {
        return vec![ConsumerChannel::Reserved];
    }
    let mut channels = vec![ConsumerChannel::DeclaredProp {
        name: camelize(key).into_owned(),
    }];
    if is_on(key) {
        let rest = &key[2..];
        let once = rest != "Once" && rest.ends_with("Once");
        let event = if once { &rest[..rest.len() - 4] } else { rest };
        let mut events = Vec::with_capacity(3);
        for candidate in [lower_first(event), hyphenate(event), event.to_string()] {
            if !events.contains(&candidate) {
                events.push(candidate);
            }
        }
        channels.push(ConsumerChannel::EmittedEvent { events, once });
    }
    channels.push(ConsumerChannel::FallthroughAttr {
        key: key.to_string(),
    });
    channels
}

/// Runtime `isReservedProp`.
pub(crate) fn is_reserved(key: &str) -> bool {
    matches!(
        key,
        "" | "key"
            | "ref"
            | "ref_for"
            | "ref_key"
            | "onVnodeBeforeMount"
            | "onVnodeMounted"
            | "onVnodeBeforeUpdate"
            | "onVnodeUpdated"
            | "onVnodeBeforeUnmount"
            | "onVnodeUnmounted"
    )
}

fn lower_first(input: &str) -> String {
    let mut chars = input.chars();
    chars
        .next()
        .map(|first| first.to_lowercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Runtime `hyphenate`: `-` before an ASCII capital preceded by a word
/// character, then lowercase.
fn hyphenate(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 4);
    let mut prev_word = false;
    for c in input.chars() {
        if c.is_ascii_uppercase() && prev_word {
            out.push('-');
        }
        prev_word = c.is_ascii_alphanumeric() || c == '_';
        out.push(c);
    }
    out.to_lowercase()
}

fn relate_consumers(
    sequence: &VueAttributeSequence,
    key_plan: &RuntimePropertyKeyPlan,
) -> Vec<AttributeConsumerRelation> {
    key_plan
        .effective
        .iter()
        .map(|property| {
            let channels = consumer_channels(&property.key);
            let mut obligations = Vec::new();
            let authored = key_plan
                .writes
                .iter()
                .filter(|write| matches!(&write.key, RuntimeKey::Static(k) if *k == property.key));
            for write in authored {
                let effective = !property.overridden.contains(&write.op_index);
                let expression = sequence
                    .operations
                    .iter()
                    .find(|op| op.index == write.op_index)
                    .and_then(|op| op.value.clone())
                    .filter(|_| write.value == WriteValue::Expression);
                for channel in &channels {
                    obligations.push(ValidationObligation {
                        op_index: write.op_index,
                        expression: expression.clone(),
                        value: write.value,
                        channel: channel.clone(),
                        effective,
                    });
                }
            }
            AttributeConsumerRelation {
                use_id: sequence.use_id.clone(),
                key: property.key.clone(),
                channels,
                inference_inputs: property.contributors.clone(),
                obligations,
            }
        })
        .collect()
}
