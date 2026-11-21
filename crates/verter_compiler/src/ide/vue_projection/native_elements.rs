//! Native HTML, SVG and MathML elements against Vue's DOM contracts.
//!
//! Configured custom elements retain explicit JSX augmentations. The default
//! catch-all JSX tag index is excluded from the contract lookup.

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, AttributeOpKind, ElementNamespace, ElementUseId, ElementUseKind,
    HandlerShape, OrderedAttributeOp, PlanSnapshotId, ProjectionPlan,
};
use crate::ide::matches_custom_element;
use crate::ide::vue_projection::attribute_operations::{bind_key, handler_key};
use crate::ide::vue_projection::branches::{TemplateBranchProjection, TemplateItem};
use crate::ide::vue_projection::callback_reentry::CallbackReentryPlan;
use crate::ide::vue_projection::checking_fragment::{
    CheckingFragment, ObligationFamily, PlacedStatement, PreludeDeclaration, StatementAnchor,
    StatementPhase, TemplateBlockId, UseHost,
};
use crate::ide::vue_projection::component_use::quote;
use crate::ide::vue_projection::script_setup::SourceRange;
use crate::template::vue_syntax::{classify_event_modifier, EventModifierKind};
use verter_parser::common::html_entities::decode_html_entities_into;

pub const NATIVE_ELEMENT_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "40-native-elements",
    text: concat!(
        "type __VerterNativeElement<T extends string, N extends string> = N extends 'svg' ? T extends keyof SVGElementTagNameMap ? SVGElementTagNameMap[T] : SVGElement : N extends 'mathml' ? T extends keyof MathMLElementTagNameMap ? MathMLElementTagNameMap[T] : MathMLElement : T extends keyof HTMLElementTagNameMap ? HTMLElementTagNameMap[T] : HTMLElement;\n",
        vue_jsx_intrinsic_elements_alias!("type ", "__VerterJsxIntrinsicElements"),
        "type __VerterDeclaredNativeElements = { [K in keyof __VerterJsxIntrinsicElements as string extends K ? never : number extends K ? never : K]: __VerterJsxIntrinsicElements[K] };\n",
        "type __VerterNativeBase<T extends string, N extends string> = N extends 'svg' ? T extends keyof HTMLElementTagNameMap ? import('vue').SVGAttributes & import('vue').ReservedProps : T extends keyof import('vue').NativeElements ? import('vue').NativeElements[T] : import('vue').SVGAttributes & import('vue').ReservedProps : T extends keyof import('vue').NativeElements ? import('vue').NativeElements[T] : T extends keyof __VerterDeclaredNativeElements ? __VerterDeclaredNativeElements[T] : import('vue').HTMLAttributes & import('vue').ReservedProps;\n",
        "type __VerterNativeAttributes<T extends string, N extends string> = Omit<__VerterNativeBase<T, N>, 'ref'> & { [K in `data-${string}` | `aria-${string}`]?: unknown } & { ref?: string | import('vue').Ref<__VerterNativeElement<T, N> | null> | ((instance: __VerterNativeElement<T, N> | null, refs: Record<string, unknown>) => void) };\n",
        "type __VerterNativeStatic<P> = { [K in keyof P]?: P[K] | (true extends P[K] ? '' : never) };\n",
        "type __VerterNativeKey<K> = K extends `.${infer P}` | `^${infer P}` ? P : K;\n",
        "type __VerterNativeWrite<P, K extends PropertyKey> = { [Q in K]?: __VerterNativeKey<Q> extends keyof P ? P[__VerterNativeKey<Q>] : never };\n",
        "type __VerterNativeEvent<P, K extends string> = K extends keyof P ? P[K] : K extends `${infer E}Once` | `${infer E}Capture` | `${infer E}Passive` ? __VerterNativeEvent<P, E> : (event: Event) => unknown;\n",
        "type __VerterNativeListeners<P> = { [K in keyof P as K extends `on${infer E}` ? Uncapitalize<E> : never]?: P[K] };\n",
        "declare function __VerterNative<T extends string, N extends string>(tag: T, namespace: N): __VerterNativeAttributes<T, N>;\n",
        "void (null! as [__VerterNativeStatic<{}>, __VerterNativeWrite<{}, never>, __VerterNativeEvent<{}, never>, __VerterNativeListeners<{}>]);\n",
    ),
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeElementContract {
    pub element: ElementUseId,
    pub block: TemplateBlockId,
    pub tag: String,
    pub namespace: ElementNamespace,
    /// Binding the element's checked attributes object is held in.
    pub binding: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomEventContext {
    pub element: ElementUseId,
    pub op_index: u32,
    pub listener_key: String,
    pub value: AdmittedExpressionId,
    pub shape: Option<HandlerShape>,
    pub modifiers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAttributePolicy {
    pub custom_elements: Vec<String>,
    pub data_and_aria_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeElementProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub policy: NativeAttributePolicy,
    pub contracts: Vec<NativeElementContract>,
    pub events: Vec<DomEventContext>,
    /// Attribute checks placed at their element, with authored values intact.
    placed: Vec<PlacedStatement>,
}

impl NativeElementProjection {
    /// The product before native elements are checked: it contributes
    /// nothing. The policy is Vue's baseline: no custom-element prefix, and
    /// `data-*` and `aria-*` attributes open.
    #[must_use]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            pending: true,
            policy: NativeAttributePolicy {
                custom_elements: Vec::new(),
                data_and_aria_open: true,
            },
            contracts: Vec::new(),
            events: Vec::new(),
            placed: Vec::new(),
        }
    }
    #[must_use]
    pub fn preludes(&self) -> Vec<PreludeDeclaration> {
        if self.placed.is_empty() {
            Vec::new()
        } else {
            vec![NATIVE_ELEMENT_PRELUDE]
        }
    }
    #[must_use]
    pub fn statements(&self) -> Vec<PlacedStatement> {
        self.placed.clone()
    }
}

#[must_use]
pub fn project_native_elements(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    reentry: &CallbackReentryPlan,
    custom_elements: Option<&[String]>,
) -> NativeElementProjection {
    let mut projection = NativeElementProjection::pending(plan.snapshot.clone());
    projection.policy.custom_elements = custom_elements.unwrap_or_default().to_vec();
    if !plan.elements.iter().any(|element| {
        matches!(
            element.kind,
            ElementUseKind::Native { .. } | ElementUseKind::CustomElement { .. }
        )
    }) {
        projection.pending = false;
        projection.complete = plan.is_complete();
        return projection;
    }
    if branches.blocks.is_empty() {
        return projection;
    }
    projection.pending = false;
    if branches.snapshot != plan.snapshot || reentry.snapshot != plan.snapshot {
        return projection;
    }
    projection.complete = plan.is_complete() && branches.complete && reentry.complete;
    for element in &plan.elements {
        let (tag, namespace) = match &element.kind {
            ElementUseKind::Native { tag, namespace } => (tag, *namespace),
            ElementUseKind::CustomElement { tag }
                if matches_custom_element(custom_elements, tag) =>
            {
                (tag, ElementNamespace::Html)
            }
            ElementUseKind::CustomElement { .. } => {
                projection.complete = false;
                continue;
            }
            ElementUseKind::SlotOutlet | ElementUseKind::TemplateWrapper => continue,
        };
        let Some((block, item)) = branches.place_of(&TemplateItem::Element(element.id.clone()))
        else {
            projection.complete = false;
            continue;
        };
        let binding = format!("__VerterElement_{}", element.id.digest_hex());
        let contract = NativeElementContract {
            element: element.id.clone(),
            block,
            tag: tag.clone(),
            namespace,
            binding,
        };
        let namespace = match namespace {
            ElementNamespace::Html => "html",
            ElementNamespace::Svg => "svg",
            ElementNamespace::MathMl => "mathml",
        };
        let mut declaration =
            CheckingFragment::synth(format!("const {} = __VerterNative(", contract.binding));
        declaration.push_alias(
            quote(tag),
            SourceRange {
                start: element.tag_start,
                end: element.tag_end,
            },
        );
        declaration.push_synth(format!(", {});\n{};\n", quote(namespace), contract.binding));
        let anchor = StatementAnchor::Item { block, item };
        let mut push = |fragment| {
            projection.placed.push(PlacedStatement {
                anchor: anchor.clone(),
                phase: StatementPhase::Main,
                family: ObligationFamily::NativeElement,
                host: Some(UseHost::Element(element.id.clone())),
                fragment,
            })
        };
        push(declaration);
        for op in &element.operations {
            if matches!(op.kind, AttributeOpKind::Model | AttributeOpKind::Directive) {
                continue;
            }
            let event =
                (op.kind == AttributeOpKind::VOn && op.name != "v-on" && op.argument.is_none())
                    .then(|| {
                        let event = op.name.strip_prefix("on").unwrap_or(&op.name);
                        let modifiers: Vec<String> = op
                            .modifiers
                            .iter()
                            .filter(|modifier| listener_modifier(modifier, event).is_some())
                            .cloned()
                            .collect();
                        (handler_key(event, &modifiers), modifiers)
                    });
            if let Some((key, modifiers)) = &event {
                if let Some(value) = &op.expression {
                    projection.events.push(DomEventContext {
                        element: element.id.clone(),
                        op_index: op.index,
                        listener_key: key.clone(),
                        value: value.clone(),
                        shape: op.handler,
                        modifiers: modifiers.clone(),
                    });
                }
            }
            match attribute_statement(
                plan,
                reentry,
                &contract,
                op,
                event.as_ref().map(|(key, _)| key.as_str()),
            ) {
                Some(fragment) => push(fragment),
                None => projection.complete = false,
            }
        }
        projection.contracts.push(contract);
    }
    projection
}

/// Each authored write is checked separately: Vue can merge multiple class,
/// style and listener contributions, which must not become duplicate object
/// literal members in TypeScript.
fn attribute_statement(
    plan: &ProjectionPlan,
    reentry: &CallbackReentryPlan,
    contract: &NativeElementContract,
    op: &OrderedAttributeOp,
    listener: Option<&str>,
) -> Option<CheckingFragment> {
    let binding = &contract.binding;
    let mut fragment = CheckingFragment::synth("({ ");
    let mut target = format!("Partial<typeof {binding}>");
    if op.argument.is_some() {
        // Computed keys need a contextual contract.
        return None;
    }
    match op.kind {
        AttributeOpKind::VBind => {
            fragment
                .push_synth("...(")
                .push_expression(op.expression.clone()?)
                .push_synth(")");
        }
        AttributeOpKind::VOn if op.name == "v-on" => {
            fragment = CheckingFragment::synth("((");
            fragment.push_expression(op.expression.clone()?);
            fragment.push_synth(format!(
                ") satisfies __VerterNativeListeners<typeof {binding}>);\n"
            ));
            return Some(fragment);
        }
        AttributeOpKind::Static => {
            target = format!(
                "__VerterNativeStatic<__VerterNativeWrite<typeof {binding}, {}>>",
                quote(&op.name)
            );
            push_attribute_key(&mut fragment, plan, op, &op.name);
            if let Some(value) = &op.expression {
                let occurrence = plan.expression(value)?;
                let decoded = static_attribute_text(plan, op)?;
                fragment.push_alias(
                    quote(&decoded),
                    SourceRange {
                        start: occurrence.start,
                        end: occurrence.end,
                    },
                );
            } else {
                // A bare attribute (`<input required>`) checks as `true`, as
                // vue-tsc writes it; a written value, even `required=""`,
                // stays its string.
                fragment.push_synth("\"\"");
            }
        }
        AttributeOpKind::Bound | AttributeOpKind::VOn => {
            let bound_key;
            let key = if op.kind == AttributeOpKind::Bound {
                bound_key = bind_key(&op.name, &op.modifiers);
                &bound_key
            } else {
                listener.unwrap_or(&op.name)
            };
            if listener.is_some() {
                target = format!(
                    "{{ {}?: __VerterNativeEvent<typeof {binding}, {}> }}",
                    quote(key),
                    quote(key)
                );
            } else {
                target = format!("__VerterNativeWrite<typeof {binding}, {}>", quote(key));
            }
            push_attribute_key(&mut fragment, plan, op, key);
            let value = op.expression.as_ref()?;
            if op.kind == AttributeOpKind::VOn
                && matches!(
                    op.handler,
                    Some(HandlerShape::Inline | HandlerShape::Statements)
                )
            {
                fragment.append(
                    reentry.inline_handler(value, op.handler == Some(HandlerShape::Statements)),
                );
            } else {
                fragment
                    .push_synth("(")
                    .push_expression(value.clone())
                    .push_synth(")");
            }
        }
        AttributeOpKind::Model | AttributeOpKind::Directive => return None,
    }
    fragment.push_synth(format!(" }} satisfies {target});\n"));
    Some(fragment)
}

/// Decoded runtime text of a parsed static attribute. A bare attribute is
/// the empty string; a bound expression has no static text.
pub(super) fn static_attribute_text(
    plan: &ProjectionPlan,
    op: &OrderedAttributeOp,
) -> Option<String> {
    if op.kind != AttributeOpKind::Static {
        return None;
    }
    let Some(id) = &op.expression else {
        return Some(String::new());
    };
    let spelling = &plan.expression(id)?.spelling;
    let mut decoded = String::with_capacity(spelling.len());
    decode_html_entities_into(&mut decoded, spelling);
    Some(decoded)
}

/// TypeScript often puts a property's value error on its key. A generated
/// key therefore points at the authored value whose contract it checks.
fn push_attribute_key(
    fragment: &mut CheckingFragment,
    plan: &ProjectionPlan,
    op: &OrderedAttributeOp,
    key: &str,
) {
    let key = quote(key);
    if let Some(value) = op
        .expression
        .as_ref()
        .and_then(|value| plan.expression(value))
    {
        fragment.push_alias(
            key,
            SourceRange {
                start: value.start,
                end: value.end,
            },
        );
    } else {
        fragment.push_synth(key);
    }
    fragment.push_synth(": ");
}

/// What one `@event.modifier` token does on a native element listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListenerModifier {
    /// The runtime compiler's class: an event option folded into the listener
    /// key, a runtime guard, or a key filter (legal on keyboard events only).
    pub kind: EventModifierKind,
    /// Text the modifier appends to the listener key (`Capture`, `Once`,
    /// `Passive`), when it appends any.
    pub key_suffix: Option<String>,
}

/// Looks up the effect of `modifier` on a listener for `event`. The class is
/// the runtime compiler's
/// [`classify_event_modifier`](crate::template::vue_syntax::classify_event_modifier)
/// and the key suffix is whatever `handler_key` appends for it, so no list of
/// modifiers lives here. `None` for an empty modifier token.
pub(crate) fn listener_modifier(modifier: &str, event: &str) -> Option<ListenerModifier> {
    if modifier.is_empty() {
        return None;
    }
    let plain = handler_key(event, &[]);
    let keyed = handler_key(event, &[modifier.to_string()]);
    let key_suffix = keyed
        .strip_prefix(&plain)
        .filter(|suffix| !suffix.is_empty())
        .map(str::to_string);
    Some(ListenerModifier {
        kind: classify_event_modifier(modifier, event),
        key_suffix,
    })
}
