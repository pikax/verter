//! Guard replay inside callbacks the framework creates from template text.

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, AttributeOpKind, HandlerShape, LexicalScopeId, OrderedAttributeOp,
    PlanSnapshotId, ProjectionPlan,
};
use crate::ide::vue_projection::branches::{
    GuardPolarity, GuardStep, TemplateBranchProjection, TemplateItem,
};
use crate::ide::vue_projection::checking_fragment::{CheckingFragment, TemplateBlockId, UseHost};

/// Opens an inline handler whose body is a statement list. The handler reads
/// `$event` first, as Vue's own template checker does, so `noUnusedParameters`
/// never reports the parameter of a handler that does not use it.
pub(super) const STATEMENTS_HANDLER_OPEN: &str = "($event) => { void $event;\n";
/// Opens an inline handler whose body is one expression. The comma keeps the
/// expression's value as the callback's result.
pub(super) const EXPRESSION_HANDLER_OPEN: &str = "($event) => (void $event, ";

/// Which framework-created callback this is. An authored arrow or function
/// and a handler reference are never callbacks here: their closures keep
/// ordinary TypeScript narrowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackKind {
    /// `@x="expr"`: body is the returned expression.
    InlineExpression,
    /// `@x="a; b"`: body is a statement list.
    InlineStatements,
    /// The update function of a `v-model`.
    ModelUpdate,
}

/// Why a dominating guard is not replayed in a callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardSkip {
    /// A free name of the guard is rebound between the guard and the
    /// callback body, so a copy would narrow a different binding.
    Rebound { name: String },
}

/// One callback and the guards it re-enters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReentryCallback {
    /// The callback's authored value.
    pub value: AdmittedExpressionId,
    pub host: UseHost,
    pub op_index: u32,
    pub block: TemplateBlockId,
    pub kind: CallbackKind,
    /// Guards replayed at the top of the body, in path order.
    pub replayed: Vec<GuardStep>,
    pub skipped: Vec<(GuardStep, GuardSkip)>,
}

/// The rendered callback: replayed guards exclude by `throw undefined`, so the
/// callback keeps its authored return type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnPreservingGuardProjection {
    pub value: AdmittedExpressionId,
    /// The whole callback expression: `($event) => ...`.
    pub callback: CheckingFragment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackReentryPlan {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub callbacks: Vec<ReentryCallback>,
    pub projections: Vec<ReturnPreservingGuardProjection>,
}

impl CallbackReentryPlan {
    /// The plan before guards are replayed: it contributes nothing.
    #[must_use]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            pending: true,
            callbacks: Vec::new(),
            projections: Vec::new(),
        }
    }
    /// The re-entry record of the callback whose authored value is `value`.
    #[must_use]
    pub fn callback(&self, value: &AdmittedExpressionId) -> Option<&ReentryCallback> {
        self.callbacks
            .iter()
            .find(|callback| callback.value == *value)
    }

    /// The statements that open `value`'s callback body, one per replayed
    /// guard. Empty when nothing is replayed.
    #[must_use]
    pub fn replayed_guards(&self, value: &AdmittedExpressionId) -> CheckingFragment {
        let mut fragment = CheckingFragment::new();
        for step in self
            .callback(value)
            .map_or(&[][..], |callback| &callback.replayed)
        {
            push_guard(&mut fragment, step);
        }
        fragment
    }

    /// The callback expression for an inline handler value. When the plan is
    /// pending or the value has no dominating guard this is the plain
    /// `($event) => (void $event, expr)` or
    /// `($event) => { void $event;\nstmts;\n}` form.
    #[must_use]
    pub fn inline_handler(
        &self,
        value: &AdmittedExpressionId,
        statements: bool,
    ) -> CheckingFragment {
        if let Some(projection) = self
            .projections
            .iter()
            .find(|projection| projection.value == *value)
        {
            return projection.callback.clone();
        }
        let mut fragment = CheckingFragment::new();
        if statements {
            fragment.push_synth(STATEMENTS_HANDLER_OPEN);
            fragment.push_expression(value.clone());
            fragment.push_synth(";\n}");
        } else {
            fragment.push_synth(EXPRESSION_HANDLER_OPEN);
            fragment.push_expression(value.clone());
            fragment.push_synth(")");
        }
        fragment
    }
}

/// `if (!(g)) throw undefined;` for a guard that holds on the way in,
/// `if (g) throw undefined;` for one that fails. The guard is a replay of
/// its authored expression, so an error in it is reported at the authored
/// guard and not at a copy.
fn push_guard(fragment: &mut CheckingFragment, step: &GuardStep) {
    match step.polarity {
        GuardPolarity::Holds => fragment.push_synth("if (!("),
        GuardPolarity::Fails => fragment.push_synth("if ("),
    };
    fragment.push_replay(step.expression.clone());
    match step.polarity {
        GuardPolarity::Holds => fragment.push_synth(")) throw undefined;\n"),
        GuardPolarity::Fails => fragment.push_synth(") throw undefined;\n"),
    };
}

/// The callback that replays `replayed` and then runs `value`. A returned
/// expression keeps its type because the guards leave by `throw`.
fn guarded_callback(
    value: &AdmittedExpressionId,
    kind: CallbackKind,
    replayed: &[GuardStep],
) -> CheckingFragment {
    let mut fragment = CheckingFragment::synth(STATEMENTS_HANDLER_OPEN);
    for step in replayed {
        push_guard(&mut fragment, step);
    }
    if kind == CallbackKind::InlineStatements {
        fragment.push_expression(value.clone());
        fragment.push_synth(";\n}");
    } else {
        fragment.push_synth("return (");
        fragment.push_expression(value.clone());
        fragment.push_synth(");\n}");
    }
    fragment
}

/// Every inline handler of the plan with the guards dominating it. A
/// handler reference and an authored function are not callbacks here.
#[must_use]
pub fn project_callback_reentry(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
) -> CallbackReentryPlan {
    if branches.blocks.is_empty() {
        return CallbackReentryPlan::pending(plan.snapshot.clone());
    }
    let mut reentry = CallbackReentryPlan {
        snapshot: plan.snapshot.clone(),
        complete: plan.is_complete() && branches.complete,
        pending: false,
        callbacks: Vec::new(),
        projections: Vec::new(),
    };
    let hosts = plan
        .uses
        .iter()
        .map(|use_| {
            (
                UseHost::Component(use_.id.clone()),
                TemplateItem::Component(use_.id.clone()),
                &use_.operations,
                &use_.lexical_env,
            )
        })
        .chain(plan.elements.iter().map(|element| {
            (
                UseHost::Element(element.id.clone()),
                TemplateItem::Element(element.id.clone()),
                &element.operations,
                &element.lexical_env,
            )
        }));
    for (host, item, operations, env) in hosts {
        let Some((block, _)) = branches.place_of(&item) else {
            continue;
        };
        let Some(path) = branches.path(block) else {
            continue;
        };
        let scopes = enclosing_scopes(branches, block, env);
        for op in operations {
            let Some((value, kind)) = callback_of(op) else {
                continue;
            };
            let mut replayed = Vec::new();
            let mut skipped = Vec::new();
            for step in &path.steps {
                match rebound_name(plan, &scopes, &step.expression) {
                    Some(name) => skipped.push((step.clone(), GuardSkip::Rebound { name })),
                    None => replayed.push(step.clone()),
                }
            }
            if kind != CallbackKind::ModelUpdate && !replayed.is_empty() {
                reentry.projections.push(ReturnPreservingGuardProjection {
                    value: value.clone(),
                    callback: guarded_callback(value, kind, &replayed),
                });
            }
            reentry.callbacks.push(ReentryCallback {
                value: value.clone(),
                host: host.clone(),
                op_index: op.index,
                block,
                kind,
                replayed,
                skipped,
            });
        }
    }
    reentry
}

/// The scopes around a callback, innermost first: the host's own scope,
/// then each enclosing block's.
fn enclosing_scopes<'p>(
    branches: &'p TemplateBranchProjection,
    block: TemplateBlockId,
    host_env: &'p LexicalScopeId,
) -> Vec<&'p LexicalScopeId> {
    let mut scopes = vec![host_env];
    let mut current = branches.block(block);
    while let Some(held) = current {
        scopes.push(&held.lexical_env);
        current = held.parent.and_then(|parent| branches.block(parent));
    }
    scopes
}

/// The first name a scope deeper than the guard's own rebinds, when the
/// copy of `guard` would read it. A binder the guard was itself written
/// inside is the same binding in the copy, so only the scopes between the
/// guard and the callback body count.
///
/// The names the guard reads from the template context come from the plan's
/// context references. A name a binder of the guard's own scope or an outer
/// one already binds is not among them, so a deeper binder that shadows such
/// a name skips the guards in its reach: replaying them would read the inner
/// binding.
fn rebound_name(
    plan: &ProjectionPlan,
    scopes: &[&LexicalScopeId],
    guard: &AdmittedExpressionId,
) -> Option<String> {
    let guard_env = plan
        .branches
        .iter()
        .find(|edge| edge.expression == *guard)
        .map(|edge| &edge.lexical_env);
    let free = context_names(plan, guard);
    let deeper = scopes
        .iter()
        .take_while(|scope| Some(**scope) != guard_env)
        .count();
    let binds = |range: &[&LexicalScopeId]| -> Vec<&str> {
        plan.origins
            .iter()
            .filter(|origin| range.contains(&&origin.lexical_env))
            .map(|origin| origin.name.as_str())
            .collect()
    };
    let (inner, outer) = scopes.split_at(deeper);
    let outer = binds(outer);
    binds(inner)
        .into_iter()
        .find(|name| free.iter().any(|read| read == name) || outer.contains(name))
        .map(str::to_string)
}

/// The names `guard` reads from the template context.
fn context_names(plan: &ProjectionPlan, guard: &AdmittedExpressionId) -> Vec<String> {
    let Some(occurrence) = plan.expression(guard) else {
        return Vec::new();
    };
    plan.context_references(guard)
        .iter()
        .filter_map(|reference| {
            let from = reference.start.checked_sub(occurrence.start)? as usize;
            let to = reference.end.checked_sub(occurrence.start)? as usize;
            occurrence.spelling.get(from..to).map(str::to_string)
        })
        .collect()
}

/// The value and kind of an operation the framework wraps in a callback.
fn callback_of(op: &OrderedAttributeOp) -> Option<(&AdmittedExpressionId, CallbackKind)> {
    let value = op.expression.as_ref()?;
    match (op.kind, op.handler) {
        (AttributeOpKind::VOn, Some(HandlerShape::Inline)) => {
            Some((value, CallbackKind::InlineExpression))
        }
        (AttributeOpKind::VOn, Some(HandlerShape::Statements)) => {
            Some((value, CallbackKind::InlineStatements))
        }
        (AttributeOpKind::Model, _) => Some((value, CallbackKind::ModelUpdate)),
        _ => None,
    }
}
