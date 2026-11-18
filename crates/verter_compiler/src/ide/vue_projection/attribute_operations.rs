//! Ordered Vue attribute operations and runtime-key interpretation.
//!
//! Ordered operations and effective keys for the component and element uses
//! of an admitted [`ProjectionPlan`], plus component consumer relations:
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
    classify_op, AdmittedExpressionId, AttributeOpKind, ComponentUseId, ElementUseId,
    ElementUseKind, OrderedAttributeOp, PlanSnapshotId, ProjectionPlan,
};
use crate::ide::get_directive_name;
use crate::ide::vue_projection::script_setup::SourceRange;
use crate::parser::types::ParsedSfc;
use crate::template::vue_syntax::{camelize, format_event_handler_key_into};
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
    /// Authored bytes the runtime key is spelled from: a static attribute's
    /// name, or a directive's static argument (`:title`, `@save`,
    /// `v-model:title`). `None` when the key has no authored spelling.
    pub key_source: Option<SourceRange>,
}

/// Authored attribute operations of one typed use host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueAttributeSequence<H = ComponentUseId> {
    /// Logical use host (a component by default).
    pub use_id: H,
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

/// Runtime property keys of one typed use host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePropertyKeyPlan<H = ComponentUseId> {
    /// Logical use host (a component by default).
    pub use_id: H,
    /// Property writes in authored order.
    pub writes: Vec<RuntimePropertyWrite>,
    /// Opaque object spreads in authored order.
    pub spreads: Vec<OpaqueSpread>,
    /// Operations that write no property (runtime directives, `<component is>`).
    pub no_property: Vec<u32>,
    /// Effective interpretation per statically known key, first-seen order.
    pub effective: Vec<EffectiveProperty>,
}

impl<H> RuntimePropertyKeyPlan<H> {
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
    /// Element sequences retain element identities and share the component op path.
    pub element_sequences: Vec<VueAttributeSequence<ElementUseId>>,
    /// Effective runtime keys for the admitted elements.
    pub element_key_plans: Vec<RuntimePropertyKeyPlan<ElementUseId>>,
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
        let Some(sequence) =
            element.and_then(|el| join_sequence(&use_.id, &use_.operations, el, source))
        else {
            complete = false;
            continue;
        };
        let key_plan = plan_runtime_keys(&sequence, false);
        relations.extend(relate_consumers(&sequence, &key_plan));
        sequences.push(sequence);
        key_plans.push(key_plan);
    }
    let mut element_sequences = Vec::with_capacity(plan.elements.len());
    let mut element_key_plans = Vec::with_capacity(plan.elements.len());
    for use_ in &plan.elements {
        let Some(sequence) = element_at(&elements, use_.tag_start)
            .and_then(|el| join_sequence(&use_.id, &use_.operations, el, source))
        else {
            complete = false;
            continue;
        };
        let keys = plan_runtime_keys(&sequence, use_.kind == ElementUseKind::SlotOutlet);
        element_sequences.push(sequence);
        element_key_plans.push(keys);
    }
    AttributeOperationsProjection {
        snapshot: plan.snapshot.clone(),
        complete,
        sequences,
        key_plans,
        relations,
        element_sequences,
        element_key_plans,
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

fn join_sequence<H: Clone>(
    use_id: &H,
    admitted: &[OrderedAttributeOp],
    el: &ElementNode,
    source: &str,
) -> Option<VueAttributeSequence<H>> {
    let props: Vec<&NodeProp> = el
        .props
        .iter()
        .filter(|prop| classify_op(prop, source).is_some())
        .collect();
    if props.len() != admitted.len() {
        return None;
    }
    let mut operations = Vec::with_capacity(props.len());
    for (prop, op) in props.into_iter().zip(admitted) {
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
        let key_source = if prop.is_directive {
            prop.arg_start.zip(prop.arg_end).filter(|_| !dynamic)
        } else {
            Some((prop.start, prop.name_end))
        }
        .filter(|(start, end)| start < end)
        .map(|(start, end)| SourceRange { start, end });
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
            key_source,
        });
    }
    let tag = source
        .get(el.tag_open.start as usize + 1..el.tag_open.name_end as usize)
        .unwrap_or_default();
    Some(VueAttributeSequence {
        use_id: use_id.clone(),
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
fn plan_runtime_keys<H: Clone>(
    sequence: &VueAttributeSequence<H>,
    outlet: bool,
) -> RuntimePropertyKeyPlan<H> {
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
            _ if outlet
                && !matches!(
                    op.syntax,
                    AttributeSyntax::Static | AttributeSyntax::Bind | AttributeSyntax::BindObject
                ) =>
            {
                no_property.push(op.index)
            }
            AttributeSyntax::Static if outlet && op.raw_spelling == "name" => {
                no_property.push(op.index)
            }
            AttributeSyntax::Static if outlet && op.value.is_none() => no_property.push(op.index),
            AttributeSyntax::Bind if outlet && op.argument_spelling.as_deref() == Some("name") => {
                no_property.push(op.index)
            }
            _ if selects_component(sequence, op) => no_property.push(op.index),
            AttributeSyntax::Static => {
                writes.push(write(
                    literal(if outlet {
                        camelize(&op.raw_spelling).into_owned()
                    } else {
                        op.raw_spelling.clone()
                    }),
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
                        literal(if outlet {
                            bind_key(&camelize(arg), &op.modifiers)
                        } else {
                            bind_key(arg, &op.modifiers)
                        })
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
fn selects_component<H>(sequence: &VueAttributeSequence<H>, op: &VueAttributeOp) -> bool {
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
pub(super) fn bind_key(arg: &str, modifiers: &[String]) -> String {
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
pub(super) fn handler_key(event: &str, modifiers: &[String]) -> String {
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
pub(super) fn modifier_prop_name(model: &str) -> String {
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

impl crate::framework_common::vue_projection_backend::VueProjectionProduct
    for AttributeOperationsProjection
{
    fn project(
        backend: &crate::framework_common::vue_projection_backend::VueProjectionBackend,
        input: &crate::framework_common::vue_projection_backend::VueProductInput<'_>,
    ) -> Result<Self, super::script_setup::SetupProjectionRefusal> {
        backend.attribute_operations(input.source, input.artifact, input.canonical_id)
    }
}

/// A spread may have keys only TypeScript can see. These checking contracts
/// preserve its optional keys and Vue's merge behavior without a Rust type
/// reader. Normalizing a style array always produces an object.
///
/// The props-normalizing type shares its name with its function, and
/// TypeScript does not count that self-reference as a use. It is declared
/// ambient, which `noUnusedLocals` never reports, so a module that emits the
/// prelude without normalizing props stays clean.
pub const ATTRIBUTE_PAYLOAD_PRELUDE: super::checking_fragment::PreludeDeclaration =
    super::checking_fragment::PreludeDeclaration {
        key: "14-attribute-payload",
        text: concat!(
            "declare function __VerterPayloadField<S, K extends PropertyKey>(spread: S, key: K): K extends keyof S ? S[K] : undefined;\n",
            "declare function __VerterPayloadStyle(values: readonly unknown[]): Extract<ReturnType<typeof import(\"vue\")[\"normalizeStyle\"]>, object>;\n",
            "type __VerterPayloadFalsy = false | 0 | 0n | \"\" | null | undefined;\n",
            "type __VerterPayloadStyleValue<T> = unknown extends T ? ReturnType<typeof import(\"vue\")[\"normalizeStyle\"]> : T extends readonly unknown[] ? Extract<ReturnType<typeof import(\"vue\")[\"normalizeStyle\"]>, object> : T extends (...args: any[]) => unknown ? undefined : T extends abstract new (...args: any[]) => unknown ? undefined : T extends string | object ? T : undefined;\n",
            "declare function __VerterPayloadStyleValue<T>(value: T): __VerterPayloadStyleValue<T>;\n",
            "type __VerterPayloadClassProp<T> = T extends __VerterPayloadFalsy | string ? T : unknown extends T ? string | __VerterPayloadFalsy : string;\n",
            "type __VerterPayloadStyleProp<T> = T extends __VerterPayloadFalsy ? T : unknown extends T ? ReturnType<typeof import(\"vue\")[\"normalizeStyle\"]> | __VerterPayloadFalsy : __VerterPayloadStyleValue<T>;\n",
            "declare type __VerterPayloadNormalizeProps<S> = S extends object ? { [K in keyof S]: K extends \"class\" ? __VerterPayloadClassProp<S[K]> : K extends \"style\" ? __VerterPayloadStyleProp<S[K]> : S[K] } : S;\n",
            "declare function __VerterPayloadNormalizeProps<S>(props: S): __VerterPayloadNormalizeProps<S>;\n",
            "type __VerterPayloadRequired<T> = keyof { [K in keyof T as {} extends Pick<T, K> ? never : K]: T[K] };\n",
            "type __VerterPayloadWithout<T, K> = { [P in keyof T as P extends K ? never : P]: T[P] };\n",
            "type __VerterPayloadObject<T> = T extends object ? T : {};\n",
            "type __VerterPayloadSpread<A, B> = __VerterPayloadWithout<A, keyof B> & __VerterPayloadWithout<B, keyof A> & { [K in keyof A & __VerterPayloadRequired<B>]: B[K] } & { [K in __VerterPayloadRequired<A> & Exclude<keyof B, __VerterPayloadRequired<B>>]: A[K] | B[K] } & { [K in Exclude<keyof A, __VerterPayloadRequired<A>> & Exclude<keyof B, __VerterPayloadRequired<B>>]?: A[K] | B[K] };\n",
            "type __VerterPayloadAt<T, K> = K extends keyof T ? T[K] : never;\n",
            "type __VerterPayloadCertain<T, K> = K extends __VerterPayloadRequired<T> ? undefined extends __VerterPayloadAt<T, K> ? false : true : false;\n",
            "type __VerterPayloadElement<T> = T extends readonly (infer E)[] ? E : T;\n",
            "type __VerterPayloadHandlers<A, B, K> = __VerterPayloadCertain<A, K> extends true ? __VerterPayloadCertain<B, K> extends true ? __VerterPayloadElement<Exclude<__VerterPayloadAt<A, K> | __VerterPayloadAt<B, K>, __VerterPayloadFalsy>>[] : __VerterPayloadAt<A, K> | __VerterPayloadElement<Exclude<__VerterPayloadAt<A, K> | __VerterPayloadAt<B, K>, __VerterPayloadFalsy>>[] : __VerterPayloadAt<A, K> | __VerterPayloadAt<B, K> | __VerterPayloadElement<Exclude<__VerterPayloadAt<A, K> | __VerterPayloadAt<B, K>, __VerterPayloadFalsy>>[];\n",
            "type __VerterPayloadIsListener<K> = K extends `on${infer C}${string}` ? C extends \"a\" | \"b\" | \"c\" | \"d\" | \"e\" | \"f\" | \"g\" | \"h\" | \"i\" | \"j\" | \"k\" | \"l\" | \"m\" | \"n\" | \"o\" | \"p\" | \"q\" | \"r\" | \"s\" | \"t\" | \"u\" | \"v\" | \"w\" | \"x\" | \"y\" | \"z\" | \"\" ? false : true : false;\n",
            "type __VerterPayloadMerged<A, B, R = __VerterPayloadSpread<A, B>> = { [K in keyof R]: K extends \"class\" ? __VerterPayloadCertain<A, K> extends true ? string : __VerterPayloadCertain<B, K> extends true ? string : string | undefined : K extends \"style\" ? Extract<ReturnType<typeof import(\"vue\")[\"normalizeStyle\"]>, object> : K extends keyof A & keyof B ? __VerterPayloadIsListener<K> extends true ? __VerterPayloadHandlers<A, B, K> : R[K] : R[K] };\n",
            "type __VerterPayloadMergePair<A, B> = A extends unknown ? B extends unknown ? __VerterPayloadMerged<__VerterPayloadObject<A>, __VerterPayloadObject<B>> : never : never;\n",
            "type __VerterPayloadFold<T extends readonly unknown[], A> = T extends readonly [infer H, ...infer R] ? __VerterPayloadFold<R, __VerterPayloadMergePair<A, H>> : A;\n",
            "declare function __VerterPayloadMerge<T extends readonly unknown[]>(...groups: T): T extends readonly [infer H, ...infer R] ? __VerterPayloadFold<R, H> : {};\n",
        ),
    };

impl RuntimePropertyKeyPlan<ElementUseId> {
    /// Emit the payload from effective writes, preserving the admitted value
    /// identities. A sole spread uses guarded props normalization; literal
    /// groups with computed keys use the same props normalization. Static
    /// literal groups and ordered merges use their own normalization contracts.
    /// TypeScript checks spread-only keys. The key plan owns dead writes.
    #[must_use]
    pub fn payload_fragment(
        &self,
        sequence: &VueAttributeSequence<ElementUseId>,
        plan: &ProjectionPlan,
    ) -> Option<super::checking_fragment::CheckingFragment> {
        use super::checking_fragment::CheckingFragment;
        if self.spreads.is_empty() {
            let Some(dynamic) = self.writes.iter().find(|w| w.key == RuntimeKey::Dynamic) else {
                return self.literal_payload_fragment(sequence, plan, true);
            };
            let span = sequence
                .operations
                .iter()
                .find(|op| op.index == dynamic.op_index)?
                .value
                .as_ref()
                .and_then(|id| plan.expression(id))?;
            let mut out = CheckingFragment::new();
            out.push_alias(
                "__VerterPayloadNormalizeProps",
                SourceRange {
                    start: span.start,
                    end: span.end,
                },
            )
            .push_synth("(")
            .append(self.literal_payload_fragment(sequence, plan, false)?)
            .push_synth(")");
            return Some(out);
        }
        let mut groups: Vec<u32> = self
            .writes
            .iter()
            .map(|w| w.group)
            .chain(self.spreads.iter().map(|s| s.group))
            .collect();
        groups.sort_unstable();
        groups.dedup();
        let mut arguments = Vec::new();
        for group in groups {
            if let Some(spread) = self.spreads.iter().find(|s| s.group == group) {
                let id = sequence
                    .operations
                    .iter()
                    .find(|op| op.index == spread.op_index)?
                    .value
                    .as_ref()?;
                plan.expression(id)?;
                let mut argument = CheckingFragment::synth("(");
                argument.push_expression(id.clone()).push_synth(")");
                arguments.push(argument);
            } else {
                let writes: Vec<_> = self
                    .writes
                    .iter()
                    .filter(|w| w.group == group)
                    .filter(|w| match &w.key {
                        RuntimeKey::Static(key) => self
                            .effective(key)
                            .is_some_and(|p| !p.overridden.contains(&w.op_index)),
                        RuntimeKey::Dynamic => true,
                    })
                    .cloned()
                    .collect();
                if writes.is_empty() {
                    continue;
                }
                let effective = self
                    .effective
                    .iter()
                    .filter(|p| {
                        writes
                            .iter()
                            .any(|w| matches!(&w.key, RuntimeKey::Static(key) if *key == p.key))
                    })
                    .map(|p| EffectiveProperty {
                        key: p.key.clone(),
                        rule: p.rule,
                        contributors: p
                            .contributors
                            .iter()
                            .filter(|c| writes.iter().any(|w| w.op_index == c.op_index))
                            .copied()
                            .collect(),
                        overridden: p
                            .overridden
                            .iter()
                            .filter(|op| writes.iter().any(|w| w.op_index == **op))
                            .copied()
                            .collect(),
                    })
                    .collect();
                let literal = Self {
                    use_id: self.use_id.clone(),
                    writes,
                    spreads: Vec::new(),
                    no_property: Vec::new(),
                    effective,
                };
                arguments.push(literal.literal_payload_fragment(sequence, plan, false)?);
            }
        }
        let span = self
            .spreads
            .first()
            .and_then(|s| sequence.operations.iter().find(|op| op.index == s.op_index))
            .and_then(|op| op.value.as_ref())
            .and_then(|id| plan.expression(id))?;
        let mut out = CheckingFragment::new();
        out.push_alias(
            if arguments.len() == 1 {
                "__VerterPayloadNormalizeProps"
            } else {
                "__VerterPayloadMerge"
            },
            SourceRange {
                start: span.start,
                end: span.end,
            },
        )
        .push_synth("(");
        for (index, argument) in arguments.into_iter().enumerate() {
            if index != 0 {
                out.push_synth(", ");
            }
            out.append(argument);
        }
        out.push_synth(")");
        Some(out)
    }

    fn literal_payload_fragment(
        &self,
        sequence: &VueAttributeSequence<ElementUseId>,
        plan: &ProjectionPlan,
        normalize: bool,
    ) -> Option<super::checking_fragment::CheckingFragment> {
        use super::checking_fragment::CheckingFragment;
        use super::component_use::quote;
        let mut out = CheckingFragment::synth("{ ");
        let mut emitted = rustc_hash::FxHashSet::default();
        let mut value = |out: &mut CheckingFragment, op: &VueAttributeOp| -> Option<()> {
            if op.syntax == AttributeSyntax::Static {
                let text = quote(op.static_text.as_deref().unwrap_or_default());
                if let Some(span) = op.value.as_ref().and_then(|id| plan.expression(id)) {
                    out.push_alias(
                        text,
                        SourceRange {
                            start: span.start,
                            end: span.end,
                        },
                    );
                } else {
                    out.push_synth(text);
                }
                out.push_synth(" as const");
            } else {
                let id = op.value.as_ref()?;
                plan.expression(id)?;
                out.push_synth("(");
                if emitted.insert(id.clone()) {
                    out.push_expression(id.clone());
                } else {
                    out.push_replay(id.clone());
                }
                out.push_synth(")");
            }
            Some(())
        };
        for op in &sequence.operations {
            if self
                .spreads
                .iter()
                .any(|spread| spread.op_index == op.index)
            {
                out.push_synth("...");
                value(&mut out, op)?;
                out.push_synth(", ");
            }
            for write in self.writes.iter().filter(|w| w.op_index == op.index) {
                match &write.key {
                    RuntimeKey::Static(key) => {
                        let effective = self.effective(key)?;
                        if effective.overridden.contains(&op.index)
                            || effective.rule == MergeRule::Combine
                            || (effective.rule == MergeRule::Accumulate
                                && effective.contributors.len() > 1)
                        {
                            continue;
                        }
                        let text = quote(key);
                        // TypeScript often locates a contextual property error
                        // on its key. Attribute it to the authored value.
                        if let Some(span) = op.value.as_ref().and_then(|id| plan.expression(id)) {
                            out.push_alias(
                                text,
                                SourceRange {
                                    start: span.start,
                                    end: span.end,
                                },
                            );
                        } else {
                            out.push_synth(text);
                        }
                    }
                    RuntimeKey::Dynamic => {
                        let id = op.dynamic_argument.as_ref()?;
                        plan.expression(id)?;
                        out.push_synth("[")
                            .push_expression(id.clone())
                            .push_synth("]");
                    }
                }
                out.push_synth(": ");
                value(&mut out, op)?;
                out.push_synth(", ");
            }
        }
        for property in self.effective.iter().filter(|p| {
            p.rule == MergeRule::Combine
                || (p.rule == MergeRule::Accumulate && p.contributors.len() > 1)
        }) {
            let normalizer = if property.key == "class" {
                "normalizeClass"
            } else {
                "normalizeStyle"
            };
            let anchor = property.contributors.iter().find_map(|c| {
                sequence
                    .operations
                    .iter()
                    .find(|op| op.index == c.op_index)
                    .and_then(|op| op.value.as_ref())
                    .and_then(|id| plan.expression(id))
            });
            if let Some(span) = anchor {
                out.push_alias(
                    quote(&property.key),
                    SourceRange {
                        start: span.start,
                        end: span.end,
                    },
                );
            } else {
                out.push_synth(quote(&property.key));
            }
            if property.rule == MergeRule::Combine && property.contributors.len() == 1 {
                let op = sequence
                    .operations
                    .iter()
                    .find(|op| op.index == property.contributors[0].op_index)?;
                out.push_synth(": ");
                if op.syntax == AttributeSyntax::Static || !normalize {
                    value(&mut out, op)?;
                } else {
                    out.push_synth(if property.key == "style" {
                        "__VerterPayloadStyleValue("
                    } else {
                        "(null! as typeof import(\"vue\")[\"normalizeClass\"])("
                    });
                    value(&mut out, op)?;
                    out.push_synth(")");
                }
                out.push_synth(", ");
                continue;
            }
            let array_only = property.rule == MergeRule::Accumulate || !normalize;
            if array_only {
                out.push_synth(": [");
            } else if property.key == "style" {
                out.push_synth(": __VerterPayloadStyle([");
            } else {
                out.push_synth(format!(
                    ": (null! as typeof import(\"vue\")[{}])([",
                    quote(normalizer)
                ));
            }
            for contribution in &property.contributors {
                let op = sequence
                    .operations
                    .iter()
                    .find(|op| op.index == contribution.op_index)?;
                if contribution.certainty == Certainty::Possible {
                    out.push_synth("__VerterPayloadField(");
                    if op.syntax == AttributeSyntax::BindObject {
                        value(&mut out, op)?;
                    } else {
                        out.push_synth("{ [")
                            .push_replay(op.dynamic_argument.clone()?)
                            .push_synth("]: ");
                        value(&mut out, op)?;
                        out.push_synth(" }");
                    }
                    out.push_synth(format!(", {})", quote(&property.key)));
                } else {
                    value(&mut out, op)?;
                }
                out.push_synth(", ");
            }
            out.push_synth(if array_only { "], " } else { "]), " });
        }
        out.push_synth("}");
        Some(out)
    }
}

#[cfg(test)]
mod element_payload_tests {
    use super::*;
    use crate::framework_common::projection_plan::{build_projection_plan, PlanInput};

    #[test]
    fn slot_outlets_computed_payload_keys_normalize_props() {
        let source = r#"<template><slot :[key]="classes" /></template>"#;
        let parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, None);
        let plan = build_projection_plan(PlanInput {
            canonical_id: "file:///Computed.vue",
            source,
            parsed: &parsed,
            parse_key: None,
            syntax_profile: None,
            custom_elements: None,
        });
        let attributes = project_attribute_operations(&plan, &parsed, source);
        let fragment = attributes.element_key_plans[0]
            .payload_fragment(&attributes.element_sequences[0], &plan)
            .expect("payload");
        let text = fragment.flatten(source, &|id| plan.expression(id).unwrap().spelling.clone());
        assert!(
            text.starts_with("__VerterPayloadNormalizeProps({ "),
            "{text}"
        );
        assert!(text.contains("[key]: (classes)"), "{text}");
        assert_eq!(text.matches("classes").count(), 1);
    }

    #[test]
    fn slot_outlets_listener_array_contract_filters_absent_contributors() {
        // Array accumulation must filter the incoming value before unwrapping
        // an authored array: absent listeners never become array elements.
        let handlers = ATTRIBUTE_PAYLOAD_PRELUDE
            .text
            .lines()
            .find(|line| line.starts_with("type __VerterPayloadHandlers<"))
            .expect("listener accumulation contract");
        let array_element = "__VerterPayloadElement<Exclude<__VerterPayloadAt<A, K> | __VerterPayloadAt<B, K>, __VerterPayloadFalsy>>[]";
        assert_eq!(handlers.matches(array_element).count(), 3, "{handlers}");
    }

    #[test]
    fn slot_outlets_share_effective_keys_and_keep_element_identity() {
        let source = r#"<template><Child :title="title"/><slot name="card" :item-id="123" v-bind="{ itemId: 'ok' }" :item-id="'last'" /></template>"#;
        let parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, None);
        let plan = build_projection_plan(PlanInput {
            canonical_id: "file:///Payload.vue",
            source,
            parsed: &parsed,
            parse_key: None,
            syntax_profile: None,
            custom_elements: None,
        });
        let attributes = project_attribute_operations(&plan, &parsed, source);
        assert!(attributes.complete);
        assert_eq!(attributes.sequences.len(), 1);
        assert_eq!(attributes.element_sequences.len(), 1);
        assert_eq!(attributes.element_sequences[0].use_id, plan.elements[0].id);
        let keys = &attributes.element_key_plans[0];
        assert!(keys.effective("name").is_none());
        let item = keys.effective("itemId").expect("camelized key");
        assert_eq!(item.contributors, vec![definite(3)]);
        assert!(item.overridden.contains(&1) && item.overridden.contains(&2));
        let fragment = keys
            .payload_fragment(&attributes.element_sequences[0], &plan)
            .expect("payload");
        let text = fragment.flatten(source, &|id| plan.expression(id).unwrap().spelling.clone());
        assert!(!text.contains("123"), "{text}");
        assert!(text.contains("'last'"), "{text}");
        let output = format!("const payload = {text};");
        let alloc = oxc_allocator::Allocator::default();
        let parsed =
            verter_parser::oxc_parse::Parser::new(&alloc, &output, oxc_span::SourceType::ts())
                .parse();
        assert!(
            parsed.diagnostics.is_empty(),
            "{:?}\n{output}",
            parsed.diagnostics
        );
    }

    #[test]
    fn slot_outlets_combined_fields_preserve_each_authored_value() {
        let source = r#"<template><slot :class="firstClass" v-bind="attrs" :class="lastClass" /></template>"#;
        let parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, None);
        let plan = build_projection_plan(PlanInput {
            canonical_id: "file:///Classes.vue",
            source,
            parsed: &parsed,
            parse_key: None,
            syntax_profile: None,
            custom_elements: None,
        });
        let attributes = project_attribute_operations(&plan, &parsed, source);
        assert!(attributes.complete);
        let fragment = attributes.element_key_plans[0]
            .payload_fragment(&attributes.element_sequences[0], &plan)
            .expect("payload");
        let text = fragment.flatten(source, &|id| plan.expression(id).unwrap().spelling.clone());
        assert_eq!(text.matches("firstClass").count(), 1);
        assert_eq!(text.matches("lastClass").count(), 1);
        assert!(text.contains("__VerterPayloadMerge("));
        let output = format!("const payload = {text};");
        let alloc = oxc_allocator::Allocator::default();
        let parsed =
            verter_parser::oxc_parse::Parser::new(&alloc, &output, oxc_span::SourceType::ts())
                .parse();
        assert!(
            parsed.diagnostics.is_empty(),
            "{:?}\n{output}",
            parsed.diagnostics
        );
    }

    #[test]
    fn slot_outlets_spread_groups_use_a_merge_instead_of_object_spreads() {
        use oxc_ast::ast::{Expression, Statement};
        for template in [
            r#"<slot id="x" v-bind="attrs" />"#,
            r#"<slot :onSave="first" v-bind="extra" />"#,
            r#"<slot v-bind="left" v-bind="right" />"#,
        ] {
            let source = format!("<template>{template}</template>");
            let parsed = crate::framework_common::vue_bridge::parse_sfc(&source, None, None);
            let plan = build_projection_plan(PlanInput {
                canonical_id: "file:///Merge.vue",
                source: &source,
                parsed: &parsed,
                parse_key: None,
                syntax_profile: None,
                custom_elements: None,
            });
            let attributes = project_attribute_operations(&plan, &parsed, &source);
            assert!(attributes.complete);
            let fragment = attributes.element_key_plans[0]
                .payload_fragment(&attributes.element_sequences[0], &plan)
                .expect("payload");
            let text =
                fragment.flatten(&source, &|id| plan.expression(id).unwrap().spelling.clone());
            let output = format!(
                "{}\nconst payload = {text};",
                ATTRIBUTE_PAYLOAD_PRELUDE.text
            );
            let alloc = oxc_allocator::Allocator::default();
            let parsed =
                verter_parser::oxc_parse::Parser::new(&alloc, &output, oxc_span::SourceType::ts())
                    .parse();
            assert!(
                parsed.diagnostics.is_empty(),
                "{:?}\n{output}",
                parsed.diagnostics
            );
            let Statement::VariableDeclaration(declaration) =
                parsed.program.body.last().expect("payload declaration")
            else {
                panic!("variable declaration")
            };
            let Expression::CallExpression(call) =
                declaration.declarations[0].init.as_ref().expect("payload")
            else {
                panic!("spread groups must be merged: {text}")
            };
            assert_eq!(call.arguments.len(), 2);
            let mut expressions = rustc_hash::FxHashSet::default();
            for piece in fragment.pieces {
                if let super::super::checking_fragment::CheckingPiece::Expression(id) = piece {
                    assert!(expressions.insert(id));
                }
            }
        }
    }

    #[test]
    fn slot_outlets_repeated_listener_fields_form_one_array() {
        let source = r#"<template><slot :onSave="first" :on-save="second" /></template>"#;
        let parsed = crate::framework_common::vue_bridge::parse_sfc(source, None, None);
        let plan = build_projection_plan(PlanInput {
            canonical_id: "file:///Listeners.vue",
            source,
            parsed: &parsed,
            parse_key: None,
            syntax_profile: None,
            custom_elements: None,
        });
        let attributes = project_attribute_operations(&plan, &parsed, source);
        let fragment = attributes.element_key_plans[0]
            .payload_fragment(&attributes.element_sequences[0], &plan)
            .expect("payload");
        let text = fragment.flatten(source, &|id| plan.expression(id).unwrap().spelling.clone());
        assert_eq!(text.matches("\"onSave\":").count(), 1);
        assert!(text.contains("[(first), (second), ]"), "{text}");
    }
}
