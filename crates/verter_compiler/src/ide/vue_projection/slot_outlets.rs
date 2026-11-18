//! `<slot>` outlets checked against the carrier's own declared slots.

use super::attribute_operations::{
    AttributeOperationsProjection, AttributeSyntax, ATTRIBUTE_PAYLOAD_PRELUDE,
};
use super::branches::{TemplateBlockKind, TemplateBranchProjection, TemplateItem};
use super::checking_fragment::{
    CheckingFragment, ObligationFamily, PlacedStatement, PreludeDeclaration, StatementAnchor,
    StatementPhase, TemplateBlockId, UseHost,
};
use super::component_use::{quote, USE_PRELUDE};
use super::provided_slots::{ProvidedSlotProjection, SlotName};
use super::public_constructor::{VuePublicConstructorContract, PUBLIC_INSTANCE};
use super::script_setup::SourceRange;
use crate::framework_common::projection_plan::{
    AttributeOpKind, ComponentUseId, ElementUseId, ElementUseKind, PlanSnapshotId, ProjectionPlan,
};

/// A finite name domain must satisfy every slot it can select. An open
/// runtime name or an undeclared slot surface keeps Vue's open policy.
pub const SLOT_OUTLET_PRELUDE: PreludeDeclaration = PreludeDeclaration {
    key: "33-slot-outlets",
    text: concat!(
        "type __VerterOutletName<S, K extends PropertyKey> = string extends K ? K : 0 extends 1 & S ? K : string extends keyof S ? K : [K] extends [keyof S] ? K : never;\n",
        "type __VerterOutletArguments<S, K extends PropertyKey> = string extends K ? unknown : 0 extends 1 & S ? unknown : ((K extends unknown ? (props: __VerterUseSlotProps<{ readonly $slots: S }, K>) => void : never) extends (props: infer P) => void ? P : never);\n",
        "declare function __VerterOutlet<S, const K extends PropertyKey>(slots: S, name: K & __VerterOutletName<NoInfer<S>, K>, payload: __VerterOutletArguments<NoInfer<S>, NoInfer<K>>): void;\n",
    ),
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotOutletObligation {
    pub element: ElementUseId,
    pub block: TemplateBlockId,
    pub name: SlotName,
    pub payload_ops: Vec<u32>,
}

/// An outlet nested in a provided slot uses that slot's specialized lexical
/// context. Its payload is checked by the same outlet obligation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedSlotContract {
    pub outlet: ElementUseId,
    pub through: ComponentUseId,
    pub slot: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotOutletProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    pub pending: bool,
    pub outlets: Vec<SlotOutletObligation>,
    pub forwarded: Vec<ForwardedSlotContract>,
    placed: Vec<PlacedStatement>,
}

impl SlotOutletProjection {
    #[must_use]
    #[cfg(test)]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            pending: true,
            outlets: Vec::new(),
            forwarded: Vec::new(),
            placed: Vec::new(),
        }
    }

    #[must_use]
    pub fn preludes(&self) -> Vec<PreludeDeclaration> {
        if self.placed.is_empty() {
            Vec::new()
        } else {
            let mut preludes = vec![
                ATTRIBUTE_PAYLOAD_PRELUDE,
                PreludeDeclaration {
                    key: "15-component-use",
                    text: USE_PRELUDE,
                },
                SLOT_OUTLET_PRELUDE,
            ];
            if !self.forwarded.is_empty() {
                preludes.push(super::provided_slots::PROVIDED_SLOT_PRELUDE);
            }
            preludes
        }
    }

    #[must_use]
    pub fn statements(&self) -> Vec<PlacedStatement> {
        self.placed.clone()
    }
}

/// Consume admitted elements and the attribute owner's effective payload.
/// No template or script syntax is rediscovered here. Generic slot contracts
/// are read from the public instance under the setup scope's authored binder.
#[must_use]
pub fn project_slot_outlets(
    plan: &ProjectionPlan,
    branches: &TemplateBranchProjection,
    provided: &ProvidedSlotProjection,
    attributes: &AttributeOperationsProjection,
    public: &VuePublicConstructorContract,
) -> SlotOutletProjection {
    let mut projection = SlotOutletProjection {
        snapshot: plan.snapshot.clone(),
        complete: plan.is_complete() && branches.complete && attributes.complete,
        pending: false,
        outlets: Vec::new(),
        forwarded: Vec::new(),
        placed: Vec::new(),
    };
    if branches.snapshot != plan.snapshot
        || provided.snapshot != plan.snapshot
        || attributes.snapshot != plan.snapshot
    {
        projection.complete = false;
        return projection;
    }
    let args = if public.binder.is_empty() {
        String::new()
    } else {
        format!(
            "<{}>",
            public
                .binder
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    for element in plan
        .elements
        .iter()
        .filter(|e| e.kind == ElementUseKind::SlotOutlet)
    {
        let Some((block, item)) = branches.place_of(&TemplateItem::Element(element.id.clone()))
        else {
            projection.complete = false;
            continue;
        };
        let Some(sequence) = attributes
            .element_sequences
            .iter()
            .find(|s| s.use_id == element.id)
        else {
            projection.complete = false;
            continue;
        };
        let Some(keys) = attributes
            .element_key_plans
            .iter()
            .find(|k| k.use_id == element.id)
        else {
            projection.complete = false;
            continue;
        };
        let name_op = sequence.operations.iter().rev().find(|op| {
            (op.syntax == AttributeSyntax::Static
                && op.raw_spelling == "name"
                && op.value.is_some())
                || (op.syntax == AttributeSyntax::Bind
                    && op.argument_spelling.as_deref() == Some("name"))
        });
        let name = match name_op {
            Some(op) if op.syntax == AttributeSyntax::Static => {
                SlotName::Static(op.static_text.clone().unwrap_or_default())
            }
            Some(op) => match &op.value {
                Some(id) => SlotName::Dynamic(id.clone()),
                None => {
                    projection.complete = false;
                    continue;
                }
            },
            None => SlotName::ImplicitDefault,
        };
        let Some(payload) = keys.payload_fragment(sequence, plan) else {
            projection.complete = false;
            continue;
        };
        let mut fragment = CheckingFragment::synth(format!(
            "__VerterOutlet(null! as {PUBLIC_INSTANCE}{args}[\"$slots\"], "
        ));
        match &name {
            SlotName::ImplicitDefault => {
                fragment.push_alias(
                    "\"default\"",
                    SourceRange {
                        start: element.tag_start,
                        end: element.tag_end,
                    },
                );
            }
            SlotName::Static(name) => {
                let span = name_op
                    .and_then(|op| op.value.as_ref())
                    .and_then(|id| plan.expression(id));
                if let Some(span) = span {
                    fragment.push_alias(
                        quote(name),
                        SourceRange {
                            start: span.start,
                            end: span.end,
                        },
                    );
                } else {
                    fragment.push_synth(quote(name));
                }
            }
            SlotName::Dynamic(id) => {
                fragment.push_expression(id.clone());
            }
        }
        fragment.push_synth(", ").append(payload).push_synth(");\n");
        projection.placed.push(PlacedStatement {
            anchor: StatementAnchor::Item { block, item },
            phase: StatementPhase::Main,
            family: ObligationFamily::SlotOutlet,
            host: Some(UseHost::Element(element.id.clone())),
            fragment,
        });
        projection.outlets.push(SlotOutletObligation {
            element: element.id.clone(),
            block,
            name,
            payload_ops: element
                .operations
                .iter()
                .filter(|op| {
                    matches!(
                        op.kind,
                        AttributeOpKind::Static | AttributeOpKind::Bound | AttributeOpKind::VBind
                    ) && !keys.no_property.contains(&op.index)
                })
                .map(|op| op.index)
                .collect(),
        });
        let mut enclosing = Some(block);
        while let Some(id) = enclosing {
            let Some(parent) = branches.block(id) else {
                projection.complete = false;
                break;
            };
            if let TemplateBlockKind::SlotBody { use_id, slot, .. } = &parent.kind {
                if provided
                    .contexts
                    .iter()
                    .any(|context| context.use_id == *use_id && context.slot == *slot)
                {
                    projection.forwarded.push(ForwardedSlotContract {
                        outlet: element.id.clone(),
                        through: use_id.clone(),
                        slot: *slot,
                    });
                } else {
                    projection.complete = false;
                }
                break;
            }
            enclosing = parent.parent;
        }
    }
    projection
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::ScriptLanguage;
    use crate::framework_common::projection_plan::{build_projection_plan, PlanInput};
    use crate::ide::vue_projection::attribute_operations::project_attribute_operations;
    use crate::ide::vue_projection::branches::project_template_branches;
    use crate::ide::vue_projection::component_use::project_component_uses;
    use crate::ide::vue_projection::provided_slots::project_provided_slots;
    use crate::ide::vue_projection::public_constructor::project_public_constructor;
    use crate::ide::vue_projection::script_setup::ScriptBlockInput;
    use crate::template::topology::{build_template_topology, TopologyRecoveryFacts};
    use crate::work_ledger::CompilerWorkLedger;
    use verter_audit::observe::Uncaptured;

    fn project(
        template: &str,
        script: &str,
        generic: Option<&str>,
    ) -> (ProjectionPlan, SlotOutletProjection) {
        let source =
            format!("<script setup lang=\"ts\">{script}</script><template>{template}</template>");
        let parsed = crate::framework_common::vue_bridge::parse_sfc(&source, None, None);
        let plan = build_projection_plan(PlanInput {
            canonical_id: "file:///Outlet.vue",
            source: &source,
            parsed: &parsed,
            parse_key: None,
            syntax_profile: None,
            custom_elements: None,
        });
        let topology = build_template_topology(
            &source,
            parsed.template_ast().expect("template"),
            TopologyRecoveryFacts::default(),
            &mut CompilerWorkLedger::<Uncaptured>::new(),
        )
        .expect("topology");
        let branches = project_template_branches(&plan, &topology);
        let attributes = project_attribute_operations(&plan, &parsed, &source);
        let uses = project_component_uses(&plan, &attributes);
        let provided = project_provided_slots(&plan, &branches, &uses);
        let public = project_public_constructor(
            None,
            Some(ScriptBlockInput {
                content: script,
                content_start: source.find('>').unwrap() as u32 + 1,
                lang: Some(ScriptLanguage::TypeScript),
            }),
            generic,
        )
        .expect("public constructor");
        let outlets = project_slot_outlets(&plan, &branches, &provided, &attributes, &public);
        (plan, outlets)
    }

    fn flat(plan: &ProjectionPlan, projection: &SlotOutletProjection) -> String {
        projection
            .statements()
            .into_iter()
            .map(|s| {
                s.fragment
                    .flatten("", &|id| plan.expression(id).unwrap().spelling.clone())
            })
            .collect()
    }

    #[test]
    fn slot_outlets_keep_generic_payload_and_fallback_obligations_separate() {
        let (plan, projection) = project(
            "<slot :item=\"item\">{{ fallback }}</slot>",
            "defineSlots<{ default(props: { item: T }): unknown }>();",
            Some("T"),
        );
        assert!(projection.complete && !projection.pending);
        assert_eq!(projection.outlets.len(), 1);
        assert_eq!(projection.outlets[0].name, SlotName::ImplicitDefault);
        assert_eq!(projection.outlets[0].payload_ops, vec![0]);
        assert_eq!(
            projection.statements()[0].host,
            Some(UseHost::Element(plan.elements[0].id.clone()))
        );
        let text = flat(&plan, &projection);
        assert!(
            text.contains("__VerterPublicInstance<T>[\"$slots\"]"),
            "{text}"
        );
        assert!(text.contains("\"item\": (item)"), "{text}");
        assert!(!text.contains("fallback"));
    }

    #[test]
    fn slot_outlets_dynamic_names_are_admitted_and_excluded_from_the_payload() {
        let (plan, projection) = project("<slot :name=\"active\" :value=\"123\" />", "const active = 'first' as 'first' | 'second'; defineSlots<{ first(p: { value: string }): unknown; second(p: { value: number }): unknown }>();", None);
        assert!(projection.complete);
        let SlotName::Dynamic(name) = &projection.outlets[0].name else {
            panic!("dynamic name")
        };
        assert_eq!(plan.expression(name).unwrap().spelling, "active");
        assert_eq!(projection.outlets[0].payload_ops, vec![1]);
        assert!(flat(&plan, &projection).contains(", active, { \"value\": (123)"));
    }

    #[test]
    fn slot_outlets_forward_from_the_enclosing_specialized_slot_context() {
        let (plan, projection) = project("<Child :items=\"items\"><template #default=\"{ item }\"><slot :item=\"item\" /></template></Child>", "import Child from './Child.vue'; const items = [] as T[]; defineSlots<{ default(p: { item: T }): unknown }>();", Some("T"));
        assert!(projection.complete);
        assert_eq!(projection.forwarded.len(), 1);
        assert_eq!(projection.forwarded[0].through, plan.uses[0].id);
        assert_eq!(
            projection.forwarded[0].outlet,
            projection.outlets[0].element
        );
        assert_eq!(projection.forwarded[0].slot, 0);
        let text = flat(&plan, &projection);
        assert!(text.contains("\"item\": (item)"), "{text}");
    }

    #[test]
    fn slot_outlets_without_declarations_still_check_their_authored_expressions() {
        let (plan, projection) = project(
            "<slot name=\"extra\" :anything=\"unknownValue\" />",
            "",
            None,
        );
        assert!(projection.complete);
        assert_eq!(projection.outlets[0].name, SlotName::Static("extra".into()));
        assert!(flat(&plan, &projection).contains("(unknownValue)"));
        assert!(!projection.preludes().is_empty());
        let pending = SlotOutletProjection::pending(plan.snapshot);
        assert!(!pending.complete && pending.pending);
        assert!(pending.statements().is_empty() && pending.preludes().is_empty());
    }

    #[test]
    fn slot_outlets_bare_name_uses_the_default_slot() {
        let (_, projection) = project(
            "<slot name :item=\"item\" />",
            "defineSlots<{ default(p: { item: string }): unknown }>();",
            None,
        );
        assert!(projection.complete);
        assert_eq!(projection.outlets[0].name, SlotName::ImplicitDefault);
    }

    #[test]
    fn slot_outlets_owner_policy_normalizes_a_sole_spread() {
        let (plan, projection) = project(
            "<slot v-bind=\"attrs\" />",
            "const attrs = { class: ['a', 'b'] };",
            None,
        );
        assert!(projection.complete);
        let text = flat(&plan, &projection);
        assert!(
            text.contains("__VerterPayloadNormalizeProps((attrs))"),
            "{text}"
        );
    }

    #[test]
    fn slot_outlets_owner_policy_does_not_wrap_one_style_value_in_an_array() {
        let (plan, projection) = project(
            "<slot :style=\"cssText\" />",
            "const cssText = 'color:red' as string;",
            None,
        );
        assert!(projection.complete);
        let text = flat(&plan, &projection);
        assert!(
            text.contains("__VerterPayloadStyleValue((cssText))"),
            "{text}"
        );
        assert!(!text.contains("__VerterPayloadStyle(["), "{text}");
    }

    #[test]
    fn slot_outlets_owner_policy_camelizes_valued_static_keys() {
        let (plan, projection) = project("<slot item-id=\"ok\" ignored />", "", None);
        assert!(projection.complete);
        let text = flat(&plan, &projection);
        assert!(text.contains("\"itemId\":"), "{text}");
        assert!(!text.contains("\"ignored\":"), "{text}");
    }

    #[test]
    fn slot_outlets_owner_policy_preserves_generated_scalar_literals() {
        let (plan, projection) = project(
            "<slot v-bind=\"{}\" kind=\"ready\" :items=\"[1, 2]\" :class=\"maybeClass\" />",
            "const maybeClass = undefined as string[] | undefined;",
            None,
        );
        assert!(projection.complete);
        let text = flat(&plan, &projection);
        assert!(text.contains("\"ready\" as const"), "{text}");
        assert_eq!(
            text.matches("as const").count(),
            1,
            "only the generated scalar is const"
        );
        assert!(text.contains("([1, 2])"), "{text}");
        assert!(
            !text.contains("normalizeClass"),
            "merge inputs retain their possible undefined values: {text}"
        );
    }

    #[test]
    fn slot_outlets_owner_policy_the_last_valued_name_selects_the_slot() {
        let (plan, projection) = project(
            "<slot name=\"first\" :name=\"selected\" :value=\"123\" />",
            "const selected = 'second' as const;",
            None,
        );
        assert!(projection.complete);
        let SlotName::Dynamic(name) = &projection.outlets[0].name else {
            panic!("the last selector must win")
        };
        assert_eq!(plan.expression(name).unwrap().spelling, "selected");
    }
}
