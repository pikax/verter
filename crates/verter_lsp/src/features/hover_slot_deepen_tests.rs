use std::sync::Arc;

use verter_session::framework::{
    ComponentContractAvailability, ComponentPublicContract, ContractExactness, ContractProvenance,
    FrameworkAdapterId, PublicSlot, PublicSlotBinding, PublicSlotInput, PublicTypeReference,
};
use verter_type_expr::{PrimitiveName, SyntheticCarrierKey, TypeExpr};

use super::{matched_slot_carrier_bindings, select_contract_slot};

fn carrier_key(slot: &str, binding: &str, value_node: u64) -> Arc<SyntheticCarrierKey> {
    Arc::new(SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/src/Child.vue"),
        surface_kind: verter_type_expr::SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from(slot)),
        binding_name: Arc::from(binding),
        value_node,
    })
}

fn published(expression: TypeExpr) -> PublicTypeReference {
    let selected_source = verter_type_expr::facts::SemanticTypeSource::Closed(
        verter_type_expr::facts::ClosedTypeFact::Leaf(
            verter_type_expr::facts::LeafTypeFact::Primitive(PrimitiveName::Unknown),
        ),
    );
    PublicTypeReference {
        publication: verter_session::meta_resolve::MaterializedTypePublication::for_test(
            verter_type_expr::PublicationResult::Published {
                selected_source: Arc::new(selected_source),
                semantic_authority: verter_type_expr::SemanticAuthority::Resolved,
                exactness: verter_type_expr::ResolutionExactness::ExactConcrete,
                reason: Box::new(verter_type_expr::PublicationReason::ResolvedExactConcrete),
                provenance: verter_type_expr::PublicationProvenance::Resolved {
                    provenance: verter_type_expr::ResolutionProvenance::FrameworkSurface,
                },
            },
            Some(expression),
            None,
        ),
    }
}

fn slot(name: &str, bindings: Vec<PublicSlotBinding>) -> PublicSlot {
    PublicSlot {
        name: Arc::from(name),
        optional: false,
        input: PublicSlotInput {
            bindings: bindings.into(),
        },
        return_type: None,
        exactness: ContractExactness::Exact,
        degradation: Arc::from([]),
        provenance: ContractProvenance::ComponentMetaOutput,
    }
}

fn binding(name: &str, ty: TypeExpr) -> PublicSlotBinding {
    PublicSlotBinding {
        name: Arc::from(name),
        ty: published(ty),
    }
}

fn two_slot_contract() -> ComponentContractAvailability {
    ComponentContractAvailability::Supported(Arc::new(ComponentPublicContract {
        adapter_id: FrameworkAdapterId::vue(),
        exactness: ContractExactness::Exact,
        degradation: Arc::from([]),
        provenance: ContractProvenance::ComponentMetaOutput,
        props: Arc::from([]),
        events: Arc::from([]),
        slots: Arc::from([
            slot(
                "header",
                vec![
                    binding(
                        "title",
                        TypeExpr::SyntheticSlotBinding(carrier_key("header", "title", 11)),
                    ),
                    binding("plain", TypeExpr::Primitive(PrimitiveName::Number)),
                ],
            ),
            slot(
                "mySlot",
                vec![binding(
                    "note",
                    TypeExpr::SyntheticSlotBinding(carrier_key("mySlot", "note", 22)),
                )],
            ),
        ]),
    }))
}

// @ai-generated - Path-precision at the deepen input: only the RANK-MATCHED
// slot's carrier bindings enter the deepen fan-out; sibling slots' carriers
// and non-carrier bindings never do.
#[test]
fn only_the_rank_matched_slots_carrier_bindings_enter_the_deepen_fan_out() {
    let contract = two_slot_contract();

    let header = matched_slot_carrier_bindings(&contract, "header");
    assert_eq!(
        header
            .iter()
            .map(|(name, _)| name.as_ref())
            .collect::<Vec<_>>(),
        vec!["title"],
        "the matched slot contributes ONLY its carrier bindings (the plain \
         binding renders its published form; the sibling slot never deepens)"
    );
    assert_eq!(header[0].1.binding_name.as_ref(), "title");
    assert_eq!(header[0].1.slot_name.as_deref(), Some("header"));

    // Kebab-authored spelling rank-matches the camel-declared sibling slot —
    // and STILL only that one slot contributes.
    let kebab = matched_slot_carrier_bindings(&contract, "my-slot");
    assert_eq!(
        kebab
            .iter()
            .map(|(name, _)| name.as_ref())
            .collect::<Vec<_>>(),
        vec!["note"]
    );
    assert_eq!(kebab[0].1.slot_name.as_deref(), Some("mySlot"));

    // An absent slot name contributes nothing (no deepen demand at all).
    assert!(
        matched_slot_carrier_bindings(&contract, "missing").is_empty(),
        "an unmatched slot name must make zero deepen demands"
    );

    // An Unsupported contract carries no rows to deepen.
    let unsupported = ComponentContractAvailability::Unsupported(
        verter_session::framework::ComponentContractUnsupported {
            adapter_id: FrameworkAdapterId::vue(),
            reason: verter_session::framework::ComponentContractUnsupportedReason::ComponentMetaUnavailable,
            diagnostics: Arc::from([]),
        },
    );
    assert!(matched_slot_carrier_bindings(&unsupported, "header").is_empty());
}

// @ai-generated - The shared slot selection is the same rank-match the hover
// builder uses (kebab↔camel equivalence, deterministic pick).
#[test]
fn select_contract_slot_shares_the_hover_builders_rank_match() {
    let ComponentContractAvailability::Supported(contract) = two_slot_contract() else {
        unreachable!("fixture is supported")
    };
    assert_eq!(
        select_contract_slot(&contract.slots, "header").map(|slot| slot.name.as_ref()),
        Some("header")
    );
    assert_eq!(
        select_contract_slot(&contract.slots, "my-slot").map(|slot| slot.name.as_ref()),
        Some("mySlot"),
        "kebab-authored names resolve the camel-declared slot"
    );
    assert_eq!(
        select_contract_slot(&contract.slots, "missing").map(|slot| slot.name.as_ref()),
        None
    );
}
