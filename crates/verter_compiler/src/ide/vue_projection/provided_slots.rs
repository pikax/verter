//! Slot bodies a parent provides, typed from the use's specialized instance.
//!
//! A slot body block is joined to the plan's provided slot by the parser
//! node that provides it, so same-name templates and the implicit default
//! need no index arithmetic. The slot's props type is the specialized
//! observation the component-use witness already renders.

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, ComponentUseId, ExpressionKind, PlanSnapshotId, ProjectionPlan,
    ProvidedSlot,
};
use crate::ide::vue_projection::branches::{TemplateBlockKind, TemplateBranchProjection};
use crate::ide::vue_projection::checking_fragment::{
    BlockHead, CheckingFragment, PlacedStatement, PreludeDeclaration, TemplateBlockId,
};
use crate::ide::vue_projection::component_use::{
    ComponentUseProjection, ComponentUseWitness, ObservationKind, USE_SLOT_PROPS,
};
use crate::ide::vue_projection::script_setup::SourceRange;
use crate::template::vue_syntax::escape_js_string_into;

/// Declares `__VerterSlotName`: the name must be a slot the instance
/// declares. An instance whose slots are open (an index signature, `any`)
/// and a name that is not a literal accept every name. Openness is the
/// component-use prelude's `__VerterUseOpen`, which is declared before this.
pub const PROVIDED_SLOT_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "32-provided-slots",
    text: concat!(
        "type __VerterSlotNameOk<I, K extends PropertyKey> = string extends K ? K : 0 extends 1 & I ? K : I extends { readonly $slots: infer S } ? (0 extends 1 & S ? K : __VerterUseOpen<S> extends true ? K : K extends keyof S ? K : never) : K;\n",
        "declare function __VerterSlotName<I, const K extends PropertyKey>(instance: I, name: K & __VerterSlotNameOk<I, K>): K;\n",
    ),
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotName {
    ImplicitDefault,
    Static(String),
    Dynamic(AdmittedExpressionId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidedSlotObligation {
    pub use_id: ComponentUseId,
    pub slot: u32,
    pub name: SlotName,
    pub block: TemplateBlockId,
    pub name_range: Option<SourceRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecializedSlotContext {
    pub use_id: ComponentUseId,
    pub slot: u32,
    pub params: Option<AdmittedExpressionId>,
    pub props_type: String,
    /// Witness binding of the use the slot is provided to.
    pub witness: String,
}

/// Baseline Vue: a caller may omit any declared slot, and a slot's return
/// annotation is not checked against its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotPresencePolicy {
    VueBaseline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidedSlotProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub policy: SlotPresencePolicy,
    pub obligations: Vec<ProvidedSlotObligation>,
    pub contexts: Vec<SpecializedSlotContext>,
}

impl ProvidedSlotProjection {
    /// The product before slot bodies are projected: it contributes nothing.
    #[must_use]
    #[cfg(test)]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            pending: true,
            policy: SlotPresencePolicy::VueBaseline,
            obligations: Vec::new(),
            contexts: Vec::new(),
        }
    }

    /// The declarations the slot checks read, when there is any.
    #[must_use]
    pub fn preludes(&self) -> Vec<PreludeDeclaration> {
        if self.obligations.is_empty() {
            Vec::new()
        } else {
            vec![PROVIDED_SLOT_PRELUDE]
        }
    }

    /// Slot checks sit in a block head, in the scope the use was
    /// constructed in, so no statement is placed on its own.
    #[must_use]
    pub fn statements(&self) -> Vec<PlacedStatement> {
        Vec::new()
    }

    /// `__VerterSlotName(W, "name");` in the parent scope, then
    /// `{ const <params> = null! as <props_type>;` for a `SlotBody` block.
    #[must_use]
    pub fn body_head(&self, block: TemplateBlockId) -> Option<BlockHead> {
        let position = self.obligations.iter().position(|o| o.block == block)?;
        let obligation = &self.obligations[position];
        let context = self
            .contexts
            .iter()
            .find(|c| c.use_id == obligation.use_id && c.slot == obligation.slot)?;
        let mut before = CheckingFragment::new();
        match &obligation.name {
            SlotName::ImplicitDefault => {
                before.push_synth(format!(
                    "__VerterSlotName({}, \"default\");\n",
                    context.witness
                ));
            }
            SlotName::Static(name) => {
                before.push_synth(format!("__VerterSlotName({}, ", context.witness));
                let quoted = quote(name);
                match obligation.name_range {
                    Some(range) => before.push_alias(quoted, range),
                    None => before.push_synth(quoted),
                };
                before.push_synth(");\n");
            }
            SlotName::Dynamic(expression) => {
                before.push_synth(format!(
                    "const {} = __VerterSlotName({}, (",
                    name_binding(block),
                    context.witness
                ));
                let first = self.obligations[..position]
                    .iter()
                    .all(|o| o.name != obligation.name);
                if first {
                    before.push_expression(expression.clone());
                } else {
                    before.push_replay(expression.clone());
                }
                before.push_synth("));\n");
            }
        }
        let mut open = CheckingFragment::synth("{\n");
        if let Some(params) = &context.params {
            open.push_synth("const ")
                .push_expression(params.clone())
                .push_synth(format!(" = null! as {};\n", context.props_type));
        }
        Some(BlockHead { before, open })
    }
}

/// Binding a dynamic slot name is held in, so the props type reads its type.
fn name_binding(block: TemplateBlockId) -> String {
    format!("__VerterSlotKey_{}", block.0)
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    escape_js_string_into(&mut out, text);
    out.push('"');
    out
}

/// One obligation and one context per slot body block, joined to the plan's
/// provided slot by the parser node that provides it. A block whose use has
/// no witness or whose slot the plan does not list is left out and the
/// product is incomplete.
#[must_use]
pub fn project_provided_slots(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    uses: &ComponentUseProjection,
) -> ProvidedSlotProjection {
    let mut projection = ProvidedSlotProjection {
        snapshot: plan.snapshot.clone(),
        complete: branches.complete && uses.complete,
        pending: false,
        policy: SlotPresencePolicy::VueBaseline,
        obligations: Vec::new(),
        contexts: Vec::new(),
    };
    for block in &branches.blocks {
        let TemplateBlockKind::SlotBody {
            use_id,
            slot,
            template,
        } = &block.kind
        else {
            continue;
        };
        let (Some(entry), Some(witness)) =
            (slot_entry(plan, use_id, *template), uses.witness(use_id))
        else {
            projection.complete = false;
            continue;
        };
        let name_range = template
            .and_then(|node| entry.sites.iter().find(|site| site.syntax_node == node))
            .and_then(|site| site.name_range)
            .map(|(start, end)| SourceRange { start, end });
        let (name, props_type) = match (&entry.expression, template) {
            (Some(expression), _) => (
                SlotName::Dynamic(expression.clone()),
                Some(format!(
                    "string extends typeof {key} ? any : {USE_SLOT_PROPS}<typeof {}, typeof {key}>",
                    witness.binding,
                    key = name_binding(block.id)
                )),
            ),
            (None, None) => (SlotName::ImplicitDefault, observed(witness, &entry.name)),
            (None, Some(_)) => (
                SlotName::Static(entry.name.clone()),
                observed(witness, &entry.name),
            ),
        };
        let Some(props_type) = props_type else {
            projection.complete = false;
            continue;
        };
        let params = template.and_then(|node| {
            plan.expressions()
                .iter()
                .find(|o| o.kind == ExpressionKind::SlotParams && o.syntax_node == node)
                .map(|o| o.id.clone())
        });
        projection.obligations.push(ProvidedSlotObligation {
            use_id: use_id.clone(),
            slot: *slot,
            name,
            block: block.id,
            name_range,
        });
        projection.contexts.push(SpecializedSlotContext {
            use_id: use_id.clone(),
            slot: *slot,
            params,
            props_type,
            witness: witness.binding.clone(),
        });
    }
    projection
}

/// The plan's slot a block provides: the entry listing its node, or the
/// implicit default for a block with no node.
fn slot_entry<'p>(
    plan: &'p ProjectionPlan,
    use_id: &ComponentUseId,
    template: Option<u32>,
) -> Option<&'p ProvidedSlot> {
    let provided = &plan.uses.iter().find(|u| u.id == *use_id)?.provided_slots;
    match template {
        Some(node) => provided
            .iter()
            .find(|slot| slot.sites.iter().any(|site| site.syntax_node == node)),
        None => provided
            .iter()
            .find(|slot| slot.sites.is_empty() && slot.name == "default"),
    }
}

/// The props type the use's witness observes for the static slot `name`.
fn observed(witness: &ComponentUseWitness, name: &str) -> Option<String> {
    witness.observations.iter().find_map(|o| match &o.kind {
        ObservationKind::Slot { name: seen } if seen == name => Some(o.type_text.clone()),
        _ => None,
    })
}
