//! Component `v-model`: the read into the child and the write back to the parent.
//!
//! The read stays in the use's construction (`"modelValue": (x)`), where
//! the child specializes over it. This product adds what the construction
//! cannot say: the write. The child's update payload, read from the
//! specialized witness, is assigned to the parent's target inside a closure,
//! as Vue's runtime does, so TypeScript checks the payload against the
//! target. A target the views refuse is assigned through a readonly view of
//! the template context, so TypeScript reports the readonly write at the
//! authored target.

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, AttributeOpKind, ComponentUseId, OrderedAttributeOp, PlanSnapshotId,
    ProjectionPlan,
};
use crate::ide::vue_projection::attribute_operations::modifier_prop_name;
use crate::ide::vue_projection::binding_views::{
    BindingViewsProjection, WriteDomain, WriteRejection,
};
use crate::ide::vue_projection::branches::{TemplateBranchProjection, TemplateItem};
use crate::ide::vue_projection::callback_reentry::CallbackReentryPlan;
use crate::ide::vue_projection::checking_fragment::{
    CheckingFragment, ObligationFamily, PlacedStatement, PreludeDeclaration, StatementAnchor,
    StatementPhase, UseHost,
};
use crate::ide::vue_projection::component_use::{
    quote, ComponentUseProjection, ComponentUseWitness, ObservationKind, USE_PROP,
};
use crate::ide::vue_projection::script_setup::SourceRange;
use crate::ide::vue_projection::template_context::TEMPLATE_CONTEXT;
use crate::template::vue_syntax::camelize;

/// Assigns through a readonly view of the template context.
const MODEL_WRITE: &str = "__VerterModelWrite";
/// Checks the authored modifier keys against the child's declared ones.
const MODEL_MODIFIERS: &str = "__VerterModelModifiers";

pub const MODEL_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "31-models",
    text: concat!(
        "declare function __VerterModelWrite<C>(context: C, write: (context: Readonly<C>) => void): void;\n",
        "declare function __VerterModelModifiers<T>(modifiers: T): void;\n",
    ),
};

/// What a model writes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelTarget {
    /// A setup binding with a declared write domain.
    Binding { name: String, domain: WriteDomain },
    /// A member path or other assignable expression.
    Path,
    /// A binding that must not be written.
    Refused {
        name: String,
        rejection: WriteRejection,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInputObligation {
    pub use_id: ComponentUseId,
    pub op_index: u32,
    pub model: String,
    pub value: AdmittedExpressionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelWriteObligation {
    pub use_id: ComponentUseId,
    pub op_index: u32,
    pub model: String,
    pub target: AdmittedExpressionId,
    pub target_kind: ModelTarget,
    /// Type text of the child's update payload over the witness binding.
    pub payload_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelModifierContract {
    pub use_id: ComponentUseId,
    pub op_index: u32,
    pub model: String,
    pub modifiers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub inputs: Vec<ModelInputObligation>,
    pub writes: Vec<ModelWriteObligation>,
    pub modifiers: Vec<ModelModifierContract>,
    /// The write and modifier statements, each at its use.
    placed: Vec<PlacedStatement>,
}

impl ModelProjection {
    /// The product before models are checked: it contributes nothing.
    #[must_use]
    #[cfg(test)]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            complete: false,
            pending: true,
            ..Self::checked(snapshot)
        }
    }

    fn checked(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: true,
            pending: false,
            inputs: Vec::new(),
            writes: Vec::new(),
            modifiers: Vec::new(),
            placed: Vec::new(),
        }
    }

    /// The declarations the statements read, when there are any.
    #[must_use]
    pub fn preludes(&self) -> Vec<PreludeDeclaration> {
        if self.placed.is_empty() {
            Vec::new()
        } else {
            vec![MODEL_PRELUDE]
        }
    }

    #[must_use]
    pub fn statements(&self) -> Vec<PlacedStatement> {
        self.placed.clone()
    }
}

/// Project the models of every component use. A use the witnesses, the
/// branches or the specialized contract cannot reach makes the product
/// incomplete rather than guessed. A model with a computed argument
/// (`v-model:[name]`) names no property, so it carries no obligation.
#[must_use]
pub fn project_models(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    uses: &ComponentUseProjection,
    views: &BindingViewsProjection,
    reentry: &CallbackReentryPlan,
) -> ModelProjection {
    let mut projection = ModelProjection::checked(plan.snapshot.clone());
    let mut complete = plan.is_complete() && uses.complete && branches.complete;
    for use_ in &plan.uses {
        let models: Vec<(&OrderedAttributeOp, &AdmittedExpressionId)> = use_
            .operations
            .iter()
            .filter(|op| op.kind == AttributeOpKind::Model && op.argument.is_none())
            .filter_map(|op| op.expression.as_ref().map(|value| (op, value)))
            .collect();
        if models.is_empty() {
            continue;
        }
        let place = branches.place_of(&TemplateItem::Component(use_.id.clone()));
        let (Some(witness), Some((block, item))) = (uses.witness(&use_.id), place) else {
            complete = false;
            continue;
        };
        let anchor = StatementAnchor::Item { block, item };
        for (op, value) in models {
            let model = camelize(&op.name).into_owned();
            let Some(payload_type) = payload_type(witness, &model) else {
                complete = false;
                continue;
            };
            let target_kind = model_target(plan, views, value);
            let mut push = |fragment: CheckingFragment| {
                projection.placed.push(PlacedStatement {
                    anchor: anchor.clone(),
                    phase: StatementPhase::After,
                    family: ObligationFamily::Model,
                    host: Some(UseHost::Component(use_.id.clone())),
                    fragment,
                });
            };
            if let Some(write) = write_statement(
                reentry,
                value,
                asserted_target(plan, op, value),
                &target_kind,
                &payload_type,
            ) {
                push(write);
            }
            if !op.modifiers.is_empty() {
                push(modifier_statement(witness, &op.name, &op.modifiers));
            }
            projection.inputs.push(ModelInputObligation {
                use_id: use_.id.clone(),
                op_index: op.index,
                model: model.clone(),
                value: value.clone(),
            });
            projection.writes.push(ModelWriteObligation {
                use_id: use_.id.clone(),
                op_index: op.index,
                model: model.clone(),
                target: value.clone(),
                target_kind,
                payload_type,
            });
            if !op.modifiers.is_empty() {
                projection.modifiers.push(ModelModifierContract {
                    use_id: use_.id.clone(),
                    op_index: op.index,
                    model,
                    modifiers: op.modifiers.clone(),
                });
            }
        }
    }
    projection.complete = complete;
    projection
}

/// The type the child's update of `model` carries, read from the witness.
fn payload_type(witness: &ComponentUseWitness, model: &str) -> Option<String> {
    witness
        .observations
        .iter()
        .find_map(|observation| match &observation.kind {
            ObservationKind::Model { name } if name == model => Some(observation.type_text.clone()),
            _ => None,
        })
}

/// What a model's value writes to. A bare name the template context holds
/// is judged by the views: a binding they refuse (a getter-only computed, a
/// readonly prop, a `const`, an import or a function) is refused, and a
/// name they do not know at all is a path the type checker resolves.
pub(super) fn model_target(
    plan: &ProjectionPlan,
    views: &BindingViewsProjection,
    value: &AdmittedExpressionId,
) -> ModelTarget {
    let Some(name) = bare_context_name(plan, value) else {
        return ModelTarget::Path;
    };
    match views.write.write_target(name) {
        Ok(binding) => ModelTarget::Binding {
            name: name.to_string(),
            domain: binding.domain.clone(),
        },
        Err(WriteRejection::UnknownBinding) if views.read.lookup(name).is_none() => {
            ModelTarget::Path
        }
        Err(rejection) => ModelTarget::Refused {
            name: name.to_string(),
            rejection,
        },
    }
}

/// The name of an expression that is exactly one context reference.
fn bare_context_name<'p>(plan: &'p ProjectionPlan, id: &AdmittedExpressionId) -> Option<&'p str> {
    let text = plan.expression(id)?.spelling.trim();
    let [reference] = plan.context_references(id) else {
        return None;
    };
    (text.len() == (reference.end - reference.start) as usize).then_some(text)
}

/// The authored range of a model target that is assignable only inside
/// parentheses (`x as T`): see [`OrderedAttributeOp::asserted_target`].
pub(super) fn asserted_target(
    plan: &ProjectionPlan,
    op: &OrderedAttributeOp,
    value: &AdmittedExpressionId,
) -> Option<SourceRange> {
    let start = op.asserted_target?;
    let spelling = plan.expression(value)?.spelling.trim();
    Some(SourceRange {
        start,
        end: start + spelling.len() as u32,
    })
}

/// `(() => { <guards> target = null! as <payload>; });`: the update the
/// framework builds is a closure, so it opens with the guards that dominate
/// it. A refused target is written through a readonly view of the context.
/// An asserted target (`x as T`) is written inside parentheses, as the
/// runtime's own update is; they stand for the authored target, so an error
/// on the parenthesized target maps to it as an error on a bare one does.
pub(super) fn write_statement(
    reentry: &CallbackReentryPlan,
    value: &AdmittedExpressionId,
    asserted_target: Option<SourceRange>,
    target_kind: &ModelTarget,
    payload_type: &str,
) -> Option<CheckingFragment> {
    let mut fragment = CheckingFragment::new();
    // A writable computed accepts its setter's domain, not its read type:
    // the payload is checked against the declared domain, and an
    // unannotated setter declares none to check.
    if let ModelTarget::Binding {
        domain: WriteDomain::SetterParam(domain),
        ..
    } = target_kind
    {
        if domain.is_empty() {
            return None;
        }
        fragment.push_synth("(() => {\n");
        fragment.append(reentry.replayed_guards(value));
        fragment.push_synth(format!(
            "null! as {payload_type} satisfies {domain};\n}});\n"
        ));
        return Some(fragment);
    }
    if matches!(target_kind, ModelTarget::Refused { .. }) {
        fragment.push_synth(format!(
            "{MODEL_WRITE}({TEMPLATE_CONTEXT}, ({TEMPLATE_CONTEXT}) => {{\n"
        ));
    } else {
        fragment.push_synth("(() => {\n");
    }
    fragment.append(reentry.replayed_guards(value));
    if let Some(target) = asserted_target {
        fragment.push_alias("(", target);
    }
    fragment.push_replay(value.clone());
    if let Some(target) = asserted_target {
        fragment.push_alias(")", target);
    }
    fragment.push_synth(format!(" = null! as {payload_type};\n}});\n"));
    Some(fragment)
}

/// The authored modifiers as an object literal against the child's declared
/// modifiers, so an undeclared one is an excess property. The key the
/// modifiers are passed under is the runtime's own rule: an argument-less
/// `v-model` is named `modelValue` by the plan and passes `modelModifiers`.
fn modifier_statement(
    witness: &ComponentUseWitness,
    model_arg: &str,
    modifiers: &[String],
) -> CheckingFragment {
    let key = modifier_prop_name(if model_arg == "modelValue" {
        "model"
    } else {
        model_arg
    });
    let members: Vec<String> = modifiers
        .iter()
        .map(|modifier| format!("{}: true", quote(modifier)))
        .collect();
    CheckingFragment::synth(format!(
        "{MODEL_MODIFIERS}<{USE_PROP}<typeof {}, {}>>({{ {} }});\n",
        witness.binding,
        quote(&key),
        members.join(", ")
    ))
}
