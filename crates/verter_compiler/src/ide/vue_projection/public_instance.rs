#![allow(dead_code)]
//! Exposed members and template refs.

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, AttributeOpKind, ComponentUseId, ElementNamespace, ElementUseId,
    HandlerShape, OrderedAttributeOp, PlanSnapshotId, ProjectionPlan,
};
use crate::ide::vue_projection::branches::{
    TemplateBlockKind, TemplateBranchProjection, TemplateItem,
};
use crate::ide::vue_projection::checking_fragment::{
    CheckingFragment, ObligationFamily, PlacedStatement, PreludeDeclaration, StatementAnchor,
    StatementPhase, TemplateBlockId, UseHost,
};
use crate::ide::vue_projection::component_use::{quote, ComponentUseProjection, ObservationKind};
use crate::ide::vue_projection::native_elements::NativeElementProjection;
use crate::ide::vue_projection::public_constructor::VuePublicConstructorContract;
use crate::ide::vue_projection::script_setup::{ScriptProjectionFacts, SourceRange};
use verter_parser::types::BindingType;

pub const TEMPLATE_REF_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "51-template-refs",
    text: concat!(
        "declare function __VerterTemplateRef<T>(target: T, value: NoInfer<T>): void;\n",
        "declare function __VerterFunctionRef<I>(callback: (instance: I | null, refs: Record<string, unknown>) => unknown): void;\n",
        "declare function __VerterDynamicRef<I, V>(): <T>(target: T & ([NonNullable<T>] extends [string] ? unknown : [NonNullable<T>] extends [(...args: any[]) => unknown] ? ((instance: I | null, refs: Record<string, unknown>) => unknown) | null | undefined : [V] extends [T extends import(\"vue\").Ref<infer R> ? R : T] ? unknown : never)) => void;\n",
    ),
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExposedInstanceContract {
    pub members: Vec<String>,
    pub open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefTarget {
    Component(ComponentUseId),
    Element(ElementUseId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefSyntax {
    Static(String),
    Dynamic(AdmittedExpressionId),
    Function(AdmittedExpressionId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRefObservation {
    pub target: RefTarget,
    pub block: TemplateBlockId,
    pub syntax: RefSyntax,
    /// Setup binding that receives the ref, when syntax decides it.
    pub binding: Option<String>,
    pub in_loop: bool,
    pub conditional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefNullabilityPolicy {
    pub nullable_always: bool,
    pub loop_collects: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicInstanceRefProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub exposed: ExposedInstanceContract,
    pub refs: Vec<TemplateRefObservation>,
    pub policy: RefNullabilityPolicy,
    placed: Vec<PlacedStatement>,
    captured: Vec<SourceRange>,
}

impl PublicInstanceRefProjection {
    /// The product before refs are checked: nothing exposed, no ref, and
    /// Vue's ref policy (a template ref may be null; a ref inside a loop
    /// collects into an array).
    #[must_use]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            pending: true,
            exposed: ExposedInstanceContract {
                members: Vec::new(),
                open: false,
            },
            refs: Vec::new(),
            policy: RefNullabilityPolicy {
                nullable_always: true,
                loop_collects: true,
            },
            placed: Vec::new(),
            captured: Vec::new(),
        }
    }
    #[must_use]
    pub fn preludes(&self) -> Vec<PreludeDeclaration> {
        if self.placed.is_empty() {
            Vec::new()
        } else {
            vec![TEMPLATE_REF_PRELUDE]
        }
    }
    #[must_use]
    pub fn statements(&self) -> Vec<PlacedStatement> {
        self.placed.clone()
    }
}

#[must_use]
pub fn project_public_instance(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    uses: &ComponentUseProjection,
    native: &NativeElementProjection,
    public: &VuePublicConstructorContract,
    script: &ScriptProjectionFacts,
) -> PublicInstanceRefProjection {
    let mut result = PublicInstanceRefProjection::pending(plan.snapshot.clone());
    result.exposed = ExposedInstanceContract {
        members: public
            .expose
            .members
            .iter()
            .map(|member| member.name.clone())
            .collect(),
        open: public.expose.open,
    };
    result.pending = false;
    if branches.snapshot != plan.snapshot
        || uses.snapshot != plan.snapshot
        || native.snapshot != plan.snapshot
    {
        return result;
    }
    result.complete = plan.is_complete() && branches.complete;
    for use_ in &plan.uses {
        if !has_ref(&use_.static_ref, &use_.operations) {
            continue;
        }
        let instance = uses.witness(&use_.id).and_then(|witness| {
            witness
                .observations
                .iter()
                .find(|observation| observation.kind == ObservationKind::Instance)
        });
        let Some(instance) = instance else {
            result.complete = false;
            continue;
        };
        result.complete &= uses.complete;
        observe(
            &mut result,
            plan,
            branches,
            script,
            RefTarget::Component(use_.id.clone()),
            use_.static_ref.as_deref(),
            &use_.operations,
            &instance.type_text,
        );
    }
    for element in &plan.elements {
        if !has_ref(&element.static_ref, &element.operations) {
            continue;
        }
        let Some(contract) = native
            .contracts
            .iter()
            .find(|contract| contract.element == element.id)
        else {
            result.complete = false;
            continue;
        };
        result.complete &= native.complete;
        let namespace = match contract.namespace {
            ElementNamespace::Html => "html",
            ElementNamespace::Svg => "svg",
            ElementNamespace::MathMl => "mathml",
        };
        let instance = format!(
            "__VerterNativeElement<{}, {}>",
            quote(&contract.tag),
            quote(namespace)
        );
        observe(
            &mut result,
            plan,
            branches,
            script,
            RefTarget::Element(element.id.clone()),
            element.static_ref.as_deref(),
            &element.operations,
            &instance,
        );
    }
    result
}

fn has_ref(static_ref: &Option<String>, operations: &[OrderedAttributeOp]) -> bool {
    static_ref.is_some()
        || operations.iter().any(|op| {
            op.kind == AttributeOpKind::Bound && op.name == "ref" && op.argument.is_none()
        })
}

#[allow(
    clippy::too_many_arguments,
    reason = "One ref site is checked against existing plan, branch, component and native inventories without rebuilding them."
)]
fn observe(
    result: &mut PublicInstanceRefProjection,
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    script: &ScriptProjectionFacts,
    target: RefTarget,
    static_ref: Option<&str>,
    operations: &[OrderedAttributeOp],
    instance: &str,
) {
    let (item, host) = match &target {
        RefTarget::Component(id) => (
            TemplateItem::Component(id.clone()),
            UseHost::Component(id.clone()),
        ),
        RefTarget::Element(id) => (
            TemplateItem::Element(id.clone()),
            UseHost::Element(id.clone()),
        ),
    };
    let Some((block, item_index)) = branches.place_of(&item) else {
        result.complete = false;
        return;
    };
    let mut ancestor = Some(block);
    let (mut in_loop, mut conditional) = (false, false);
    while let Some(id) = ancestor {
        let Some(parent) = branches.block(id) else {
            result.complete = false;
            return;
        };
        in_loop |= matches!(parent.kind, TemplateBlockKind::Loop { .. });
        conditional |= matches!(parent.kind, TemplateBlockKind::Arm { .. });
        ancestor = parent.parent;
    }
    let dynamic = operations
        .iter()
        .rev()
        .find(|op| op.kind == AttributeOpKind::Bound && op.name == "ref" && op.argument.is_none());
    let mut fragment = CheckingFragment::new();
    let mut check_origin = None;
    let (syntax, binding) = if let Some(op) = dynamic {
        let Some(expression) = &op.expression else {
            result.complete = false;
            return;
        };
        let is_function = op.handler == Some(HandlerShape::Function);
        if matches!(target, RefTarget::Element(_)) {
            // Native attribute checking already owns every bound-ref value.
        } else if is_function {
            fragment
                .push_synth(format!("__VerterFunctionRef<{instance}>("))
                .push_expression(expression.clone())
                .push_synth(");\n");
        } else {
            let collection = if in_loop { "[]" } else { "" };
            fragment
                .push_synth(format!(
                    "__VerterDynamicRef<{instance}, ({instance}){collection} | null>()("
                ))
                .push_expression(expression.clone())
                .push_synth(");\n");
        }
        (
            if is_function {
                RefSyntax::Function(expression.clone())
            } else {
                RefSyntax::Dynamic(expression.clone())
            },
            None,
        )
    } else if let Some(key) = static_ref {
        let setup = script.setup.as_ref();
        let receiving = setup.and_then(|setup| {
            let keyed = setup.template_refs.iter().find(|usage| usage.key == key);
            setup
                .bindings
                .iter()
                .find(|binding| {
                    binding.kind == BindingType::SetupRef
                        && match keyed {
                            Some(usage) => binding.span == usage.binding,
                            None => {
                                binding.name == key
                                    && !setup
                                        .template_refs
                                        .iter()
                                        .any(|usage| usage.binding == binding.span)
                            }
                        }
                })
                .map(|binding| (binding, keyed))
        });
        if let Some((binding, usage)) = receiving {
            let capture = if let Some(index) = result
                .captured
                .iter()
                .position(|span| *span == binding.span)
            {
                index
            } else {
                let index = result.captured.len();
                let Some(root) = branches.root() else {
                    result.complete = false;
                    return;
                };
                let mut capture = CheckingFragment::synth(format!("const __VerterRef_{index} = "));
                capture
                    .push_verbatim(binding.span)
                    .push_synth(format!(";\nvoid __VerterRef_{index};\n"));
                result.placed.push(PlacedStatement {
                    anchor: StatementAnchor::Item {
                        block: root.id,
                        item: 0,
                    },
                    phase: StatementPhase::Before,
                    family: ObligationFamily::TemplateRef,
                    host: None,
                    fragment: capture,
                });
                result.captured.push(binding.span);
                index
            };
            let origin = usage.map_or_else(
                || {
                    operations
                        .iter()
                        .rev()
                        .find(|op| op.name == "ref" && op.kind == AttributeOpKind::Static)
                        .and_then(|op| op.expression.as_ref())
                        .and_then(|id| plan.expression(id))
                        .map_or(binding.span, |occurrence| SourceRange {
                            start: occurrence.start,
                            end: occurrence.end,
                        })
                },
                |usage| usage.key_span,
            );
            check_origin = Some(origin);
            fragment
                .push_synth("__VerterTemplateRef(")
                .push_alias(format!("__VerterRef_{capture}.value"), origin);
        }
        (
            RefSyntax::Static(key.to_string()),
            receiving.map(|(binding, _)| binding.name.clone()),
        )
    } else {
        return;
    };
    if let Some(origin) = check_origin {
        let collection = if in_loop { "[]" } else { "" };
        fragment
            .push_synth(", ")
            .push_alias(format!("null! as ({instance}){collection} | null"), origin)
            .push_synth(");\n");
    }
    result.refs.push(TemplateRefObservation {
        target,
        block,
        syntax,
        binding,
        in_loop,
        conditional,
    });
    if !fragment.is_empty() {
        result.placed.push(PlacedStatement {
            anchor: StatementAnchor::Item {
                block,
                item: item_index,
            },
            phase: StatementPhase::After,
            family: ObligationFamily::TemplateRef,
            host: Some(host),
            fragment,
        });
    }
}
