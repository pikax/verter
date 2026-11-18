//! `v-model` on form controls.
//!
//! Parsed operations select the runtime control. TypeScript computes the
//! payload over the native witness and the model's read type; the shared
//! model writer checks that payload against the target's write domain.

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, AttributeOpKind, ElementNamespace, ElementUseId, OrderedAttributeOp,
    PlanSnapshotId, ProjectionPlan,
};
use crate::ide::vue_projection::attribute_operations::bind_key;
use crate::ide::vue_projection::binding_views::BindingViewsProjection;
use crate::ide::vue_projection::branches::{TemplateBranchProjection, TemplateItem};
#[cfg(test)]
use crate::ide::vue_projection::callback_reentry::project_callback_reentry;
use crate::ide::vue_projection::callback_reentry::CallbackReentryPlan;
use crate::ide::vue_projection::checking_fragment::{
    CheckingFragment, ObligationFamily, PlacedStatement, PreludeDeclaration, StatementAnchor,
    StatementPhase, UseHost,
};
use crate::ide::vue_projection::component_use::quote;
use crate::ide::vue_projection::models::{
    asserted_target, model_target, write_statement, ModelTarget, MODEL_PRELUDE,
};
use crate::ide::vue_projection::native_elements::{static_attribute_text, NativeElementProjection};
use crate::ide::vue_projection::script_setup::SourceRange;

pub const NATIVE_MODEL_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "41-native-models",
    text: concat!(
        "type __VerterNativeNumber<V, N> = N extends true ? number | string | Exclude<V, string | number> : V;\n",
        "type __VerterNativeCheckbox<M, V, T, F> = M extends readonly (infer I)[] ? (I | V)[] : M extends ReadonlySet<infer I> ? Set<I | V> : T | F;\n",
        "type __VerterNativeSelection<M, N> = M extends ReadonlySet<infer I> ? Set<__VerterNativeNumber<I, N>> : M extends readonly (infer I)[] ? __VerterNativeNumber<I, N>[] : unknown[];\n",
        "type __VerterNativeSelect<M, B, N> = B extends false | null | undefined | 0 ? __VerterNativeNumber<M, N> : B extends true | '' ? __VerterNativeSelection<M, N> : __VerterNativeNumber<M, N> | __VerterNativeSelection<M, N>;\n",
        "type __VerterNativeModelWrite<C, M, V, T, F, N, B> = C extends 'checkbox' ? __VerterNativeCheckbox<M, V, T, F> : C extends 'radio' ? V : C extends 'select' ? __VerterNativeSelect<M, B, N> : C extends 'dynamic-input' ? number | string | V | __VerterNativeCheckbox<M, V, T, F> : __VerterNativeNumber<string, N>;\n",
        "declare function __VerterNativeModel<E, C extends string, M, const V, const T, const F, N extends boolean, const B>(element: E, control: C, model: M, value: V, trueValue: T, falseValue: F, modifiers: { number: N; trim: boolean; lazy: boolean; multiple: B }): __VerterNativeModelWrite<C, M, V, T, F, N, B>;\n",
    ),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormControl {
    Text,
    Textarea,
    SelectSingle,
    SelectMultiple,
    Checkbox,
    Radio,
    DynamicInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeModelModifiers {
    pub lazy: bool,
    pub trim: bool,
    pub number: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormControlValueContract {
    pub element: ElementUseId,
    pub control: FormControl,
    pub modifiers: NativeModelModifiers,
    pub true_value: Option<AdmittedExpressionId>,
    pub false_value: Option<AdmittedExpressionId>,
    /// Type text the control writes.
    pub write_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeModelObligation {
    pub element: ElementUseId,
    pub op_index: u32,
    pub target: AdmittedExpressionId,
    pub target_kind: ModelTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeModelProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub contracts: Vec<FormControlValueContract>,
    pub obligations: Vec<NativeModelObligation>,
    placed: Vec<PlacedStatement>,
}

impl NativeModelProjection {
    /// The product before form-control models are checked: it contributes
    /// nothing.
    #[must_use]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            pending: true,
            contracts: Vec::new(),
            obligations: Vec::new(),
            placed: Vec::new(),
        }
    }
    #[must_use]
    pub fn preludes(&self) -> Vec<PreludeDeclaration> {
        if self.placed.is_empty() {
            Vec::new()
        } else {
            vec![MODEL_PRELUDE, NATIVE_MODEL_PRELUDE]
        }
    }
    #[must_use]
    pub fn statements(&self) -> Vec<PlacedStatement> {
        self.placed.clone()
    }
}

#[must_use]
#[cfg(test)]
pub fn project_native_models(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    native: &NativeElementProjection,
    views: &BindingViewsProjection,
) -> NativeModelProjection {
    let reentry = project_callback_reentry(plan, branches);
    project_native_models_with_reentry(plan, branches, native, views, &reentry)
}

/// The composer already owns the callback plan; reuse it without another
/// callback projection. Standalone callers use `project_native_models`.
pub(super) fn project_native_models_with_reentry(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    native: &NativeElementProjection,
    views: &BindingViewsProjection,
    reentry: &CallbackReentryPlan,
) -> NativeModelProjection {
    let mut projection = NativeModelProjection::pending(plan.snapshot.clone());
    if !plan.elements.iter().any(|element| {
        element
            .operations
            .iter()
            .any(|op| op.kind == AttributeOpKind::Model)
    }) {
        projection.pending = false;
        projection.complete = plan.is_complete();
        return projection;
    }
    if branches.blocks.is_empty() || native.pending || reentry.pending {
        return projection;
    }
    projection.pending = false;
    if branches.snapshot != plan.snapshot
        || native.snapshot != plan.snapshot
        || reentry.snapshot != plan.snapshot
    {
        return projection;
    }
    projection.complete =
        plan.is_complete() && branches.complete && native.complete && reentry.complete;
    let contracts: std::collections::HashMap<_, _> = native
        .contracts
        .iter()
        .map(|contract| (&contract.element, contract))
        .collect();
    for element in &plan.elements {
        let operations = &element.operations;
        if !operations
            .iter()
            .any(|op| op.kind == AttributeOpKind::Model)
        {
            continue;
        }
        let Some(contract) = contracts.get(&element.id) else {
            projection.complete = false;
            continue;
        };
        if contract.namespace != ElementNamespace::Html {
            projection.complete = false;
            continue;
        }
        // Opaque attribute writers can also replace value/true-value and
        // multiple. They need the attribute owner's effective payload;
        // absent that payload, no default-valued model contract is exact.
        if operations
            .iter()
            .any(|op| op.kind == AttributeOpKind::VBind || op.argument.is_some())
        {
            projection.complete = false;
            continue;
        }
        let [Ok(input_type), Ok(multiple), Ok(value), Ok(true_value), Ok(false_value)] =
            ["type", "multiple", "value", "true-value", "false-value"]
                .map(|name| attribute(operations, name))
        else {
            projection.complete = false;
            continue;
        };
        let static_type = input_type.and_then(|op| static_attribute_text(plan, op));
        let dynamic = input_type.is_some_and(|op| op.kind != AttributeOpKind::Static);
        let control = match contract.tag.as_str() {
            "textarea" => FormControl::Textarea,
            "select" if multiple.is_some() => FormControl::SelectMultiple,
            "select" => FormControl::SelectSingle,
            "input" if dynamic => FormControl::DynamicInput,
            "input" => match static_type.as_deref() {
                Some("checkbox") => FormControl::Checkbox,
                Some("radio") => FormControl::Radio,
                _ => FormControl::Text,
            },
            _ => {
                projection.complete = false;
                continue;
            }
        };
        let Some((block, item)) = branches.place_of(&TemplateItem::Element(element.id.clone()))
        else {
            projection.complete = false;
            continue;
        };
        for op in operations
            .iter()
            .filter(|op| op.kind == AttributeOpKind::Model)
        {
            let Some(target) = op.expression.as_ref().filter(|_| op.argument.is_none()) else {
                projection.complete = false;
                continue;
            };
            let modifiers = NativeModelModifiers {
                lazy: op.modifiers.iter().any(|m| m == "lazy"),
                trim: op.modifiers.iter().any(|m| m == "trim"),
                number: op.modifiers.iter().any(|m| m == "number"),
            };
            let binding = format!(
                "__VerterNativeModel_{}_{}",
                element.id.digest_hex(),
                op.index
            );
            let write_type = format!("typeof {binding}");
            let control_name = match control {
                FormControl::Text | FormControl::Textarea => "text",
                FormControl::SelectSingle | FormControl::SelectMultiple => "select",
                FormControl::Checkbox => "checkbox",
                FormControl::Radio => "radio",
                FormControl::DynamicInput => "dynamic-input",
            };
            let mut read = CheckingFragment::synth(format!(
                "const {binding} = __VerterNativeModel({}, {}, (",
                contract.binding,
                quote(control_name),
            ));
            read.push_expression(target.clone()).push_synth("), ");
            for (attribute, fallback) in [
                (value, "\"on\""),
                (true_value, "true"),
                (false_value, "false"),
            ] {
                read.append(attribute_value(plan, attribute, fallback))
                    .push_synth(", ");
            }
            read.push_synth(format!(
                "{{ number: {}, trim: {}, lazy: {}, multiple: ",
                modifiers.number
                    || (contract.tag == "input" && static_type.as_deref() == Some("number")),
                modifiers.trim,
                modifiers.lazy,
            ));
            if let Some(multiple) = multiple {
                if multiple.kind == AttributeOpKind::Static {
                    read.push_synth("true");
                } else {
                    read.append(attribute_value(plan, Some(multiple), "false"));
                }
            } else {
                read.push_synth("false");
            }
            read.push_synth(" });\n")
                .push_synth(format!("{binding};\n"));
            let anchor = StatementAnchor::Item { block, item };
            let mut push = |fragment| {
                projection.placed.push(PlacedStatement {
                    anchor: anchor.clone(),
                    phase: StatementPhase::After,
                    family: ObligationFamily::NativeModel,
                    host: Some(UseHost::Element(element.id.clone())),
                    fragment,
                })
            };
            push(read);
            let target_kind = model_target(plan, views, target);
            if let Some(write) = write_statement(
                reentry,
                target,
                asserted_target(plan, op, target),
                &target_kind,
                &write_type,
            ) {
                push(write);
            }
            projection.contracts.push(FormControlValueContract {
                element: element.id.clone(),
                control,
                modifiers,
                true_value: true_value.and_then(|op| op.expression.clone()),
                false_value: false_value.and_then(|op| op.expression.clone()),
                write_type,
            });
            projection.obligations.push(NativeModelObligation {
                element: element.id.clone(),
                op_index: op.index,
                target: target.clone(),
                target_kind,
            });
        }
    }
    projection
}

/// Use the attribute owner's runtime routing. Forced attribute/property
/// writes can bypass Vue's retained value channels, so their ordinary
/// bound expression is not an exact model payload. Keep that contract
/// incomplete until the actual DOM representation is modeled.
fn attribute<'a>(
    operations: &'a [OrderedAttributeOp],
    name: &str,
) -> Result<Option<&'a OrderedAttributeOp>, ()> {
    let mut found = None;
    for op in operations {
        if op.argument.is_some() {
            continue;
        }
        match op.kind {
            AttributeOpKind::Static if op.name == name => {
                found.get_or_insert(op);
            }
            AttributeOpKind::Bound => {
                let key = bind_key(&op.name, &op.modifiers);
                if key == name {
                    found.get_or_insert(op);
                } else if key.strip_prefix('.').or_else(|| key.strip_prefix('^')) == Some(name) {
                    return Err(());
                }
            }
            _ => {}
        }
    }
    Ok(found)
}

/// Attribute expressions already have their primary mapped occurrence in
/// the native-element product. Model inference consumes a replay instead.
fn attribute_value(
    plan: &ProjectionPlan,
    attribute: Option<&OrderedAttributeOp>,
    fallback: &str,
) -> CheckingFragment {
    let Some(op) = attribute else {
        return CheckingFragment::synth(fallback);
    };
    let mut fragment = CheckingFragment::new();
    if let Some(text) = static_attribute_text(plan, op) {
        if let Some(occurrence) = op.expression.as_ref().and_then(|id| plan.expression(id)) {
            fragment.push_alias(
                quote(&text),
                SourceRange {
                    start: occurrence.start,
                    end: occurrence.end,
                },
            );
        } else {
            fragment.push_synth(quote(&text));
        }
    } else if let Some(id) = &op.expression {
        fragment
            .push_synth("(")
            .push_replay(id.clone())
            .push_synth(")");
    } else {
        fragment.push_synth(fallback);
    }
    fragment
}
