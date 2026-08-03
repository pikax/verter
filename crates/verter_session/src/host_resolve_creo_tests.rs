use std::sync::Arc;

use verter_type_expr::{PrimitiveName, TypeExpr};

use crate::framework::{
    ComponentContractAvailability, ComponentPublicContract, ContractExactness, ContractProvenance,
    PublicEvent,
};
use crate::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

fn upsert(host: &VerterHost, id: &str, source: &str, language: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_owned(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .expect("fixture upsert succeeds");
}

fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
    upsert(host, id, source, FileLanguage::vue());
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    upsert(host, id, source, FileLanguage::script_ts());
}

fn cold_and_warm_contract(host: &VerterHost, id: &str) -> Arc<ComponentPublicContract> {
    let cold = host
        .get_public_api_projection(id)
        .expect("cold public projection succeeds")
        .expect("fixture has a public projection");
    let warm = host
        .get_public_api_projection(id)
        .expect("warm public projection succeeds")
        .expect("fixture has a public projection");
    assert_eq!(
        cold.contract, warm.contract,
        "cold and warm event contracts must be identical"
    );
    let ComponentContractAvailability::Supported(contract) = cold.contract else {
        panic!("fixture must publish a supported contract");
    };
    assert_eq!(contract.exactness, ContractExactness::Exact);
    assert!(contract.degradation.is_empty());
    assert_eq!(contract.provenance, ContractProvenance::ComponentMetaOutput);
    for event in contract.events.iter() {
        assert_eq!(event.exactness, ContractExactness::Exact);
        assert!(event.degradation.is_empty());
        assert_eq!(event.provenance, ContractProvenance::ComponentMetaOutput);
    }
    contract
}

fn event<'a>(contract: &'a ComponentPublicContract, name: &str) -> &'a PublicEvent {
    contract
        .events
        .iter()
        .find(|event| event.name.as_ref() == name)
        .unwrap_or_else(|| panic!("missing event {name}"))
}

fn assert_overload(
    event: &PublicEvent,
    index: usize,
    parameter_name: &str,
    parameter_type: PrimitiveName,
    return_type: PrimitiveName,
) {
    let overload = &event.overloads[index];
    assert_eq!(overload.parameters.len(), 1);
    assert_eq!(overload.parameters[0].name.as_deref(), Some(parameter_name));
    assert_eq!(
        overload.parameters[0].ty,
        TypeExpr::Primitive(parameter_type)
    );
    assert_eq!(overload.return_type, TypeExpr::Primitive(return_type));
    assert_eq!(
        event.derived_handler.overloads[index].return_type,
        overload.return_type
    );
}

// @ai-generated - CREO must deduplicate one exact instantiated root occurrence
// reached through a diamond while retaining base-before-own declaration order.
#[test]
fn creo_diamond_deduplicates_exact_occurrence_in_resolver_order() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/diamond.ts",
        r#"
export interface RootEmits {
  (event: 'shared', root: string): string
}
export interface LeftEmits extends RootEmits {
  (event: 'left', left: number): number
}
export interface RightEmits extends RootEmits {
  (event: 'right', right: boolean): boolean
}
export interface DerivedEmits extends LeftEmits, RightEmits {
  (event: 'own', own: bigint): bigint
}
"#,
    );
    upsert_vue(
        &host,
        "/src/Diamond.vue",
        r#"<script setup lang="ts">
import type { DerivedEmits } from './diamond'
defineEmits<DerivedEmits>()
</script>"#,
    );

    let contract = cold_and_warm_contract(&host, "/src/Diamond.vue");
    assert_eq!(
        contract
            .events
            .iter()
            .map(|event| event.name.as_ref())
            .collect::<Vec<_>>(),
        ["shared", "left", "right", "own"]
    );
    assert_eq!(event(&contract, "shared").overloads.len(), 1);
    assert_overload(
        event(&contract, "shared"),
        0,
        "root",
        PrimitiveName::String,
        PrimitiveName::String,
    );
}

// @ai-generated - The canonical surface stream must retain lexical
// property/call interleaving for imported producers in both directions.
#[test]
fn creo_imported_mixed_property_and_call_order_is_lexical() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/mixed.ts",
        r#"
export interface PropertyFirst {
  save: [propertyPayload: string]
  (event: 'save', callablePayload: number): boolean
}
export interface CallFirst {
  (event: 'save', callablePayload: number): boolean
  save: [propertyPayload: string]
}
"#,
    );
    for (id, imported) in [
        ("/src/PropertyFirst.vue", "PropertyFirst"),
        ("/src/CallFirst.vue", "CallFirst"),
    ] {
        upsert_vue(
            &host,
            id,
            &format!(
                r#"<script setup lang="ts">
import type {{ {imported} }} from './mixed'
defineEmits<{imported}>()
</script>"#
            ),
        );
    }

    let property_first = cold_and_warm_contract(&host, "/src/PropertyFirst.vue");
    let save = event(&property_first, "save");
    assert_overload(
        save,
        0,
        "propertyPayload",
        PrimitiveName::String,
        PrimitiveName::Void,
    );
    assert_overload(
        save,
        1,
        "callablePayload",
        PrimitiveName::Number,
        PrimitiveName::Boolean,
    );

    let call_first = cold_and_warm_contract(&host, "/src/CallFirst.vue");
    let save = event(&call_first, "save");
    assert_overload(
        save,
        0,
        "callablePayload",
        PrimitiveName::Number,
        PrimitiveName::Boolean,
    );
    assert_overload(
        save,
        1,
        "propertyPayload",
        PrimitiveName::String,
        PrimitiveName::Void,
    );
}

// @ai-generated - A skipped non-event signature cannot reserve identity, and
// literal-union arms must retain their producer's payload and return.
#[test]
fn creo_skips_non_events_and_expands_literal_union_in_place() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_vue(
        &host,
        "/src/Skipped.vue",
        r#"<script setup lang="ts">
defineEmits<{
  (value: Date): Date
  (event: 'save' | 'cancel', unionPayload: string): number
  (event: 'save', literalPayload: boolean): boolean
}>()
</script>"#,
    );

    let contract = cold_and_warm_contract(&host, "/src/Skipped.vue");
    assert_eq!(
        contract
            .events
            .iter()
            .map(|event| event.name.as_ref())
            .collect::<Vec<_>>(),
        ["save", "cancel"]
    );
    let save = event(&contract, "save");
    assert_eq!(save.overloads.len(), 2);
    assert_overload(
        save,
        0,
        "unionPayload",
        PrimitiveName::String,
        PrimitiveName::Number,
    );
    assert_overload(
        save,
        1,
        "literalPayload",
        PrimitiveName::Boolean,
        PrimitiveName::Boolean,
    );
    let cancel = event(&contract, "cancel");
    assert_eq!(cancel.overloads.len(), 1);
    assert_overload(
        cancel,
        0,
        "unionPayload",
        PrimitiveName::String,
        PrimitiveName::Number,
    );
}

// @ai-generated - Cross-kind heritage producers are ordered base-before-own
// and each complete occurrence retains its own return.
#[test]
fn creo_cross_kind_heritage_preserves_producer_association() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/cross-kind.ts",
        r#"
export interface PropertyBase {
  save: [baseProperty: string]
}
export interface PropertyThenCall extends PropertyBase {
  (event: 'save', ownCall: number): boolean
}
export interface CallBase {
  (event: 'save', baseCall: number): boolean
}
export interface CallThenProperty extends CallBase {
  save: [ownProperty: string]
}
"#,
    );
    for (id, imported) in [
        ("/src/PropertyThenCall.vue", "PropertyThenCall"),
        ("/src/CallThenProperty.vue", "CallThenProperty"),
    ] {
        upsert_vue(
            &host,
            id,
            &format!(
                r#"<script setup lang="ts">
import type {{ {imported} }} from './cross-kind'
defineEmits<{imported}>()
</script>"#
            ),
        );
    }

    let property_then_call = cold_and_warm_contract(&host, "/src/PropertyThenCall.vue");
    let save = event(&property_then_call, "save");
    assert_overload(
        save,
        0,
        "baseProperty",
        PrimitiveName::String,
        PrimitiveName::Void,
    );
    assert_overload(
        save,
        1,
        "ownCall",
        PrimitiveName::Number,
        PrimitiveName::Boolean,
    );

    let call_then_property = cold_and_warm_contract(&host, "/src/CallThenProperty.vue");
    let save = event(&call_then_property, "save");
    assert_overload(
        save,
        0,
        "baseCall",
        PrimitiveName::Number,
        PrimitiveName::Boolean,
    );
    assert_overload(
        save,
        1,
        "ownProperty",
        PrimitiveName::String,
        PrimitiveName::Void,
    );
}

// @ai-generated - Heritage order is semantic, not source-span order across
// declarations. A forward-declared base must still precede the derived own
// body, so a post-hoc span sorter cannot manufacture the CREO stream.
#[test]
fn creo_forward_declared_cross_kind_heritage_uses_resolver_order() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/forward-cross-kind.ts",
        r#"
export interface Derived extends Base {
  (event: 'save', ownCall: number): boolean
}
interface Base {
  save: [baseProperty: string]
}
"#,
    );
    upsert_vue(
        &host,
        "/src/ForwardCrossKind.vue",
        r#"<script setup lang="ts">
import type { Derived } from './forward-cross-kind'
defineEmits<Derived>()
</script>"#,
    );

    let contract = cold_and_warm_contract(&host, "/src/ForwardCrossKind.vue");
    let save = event(&contract, "save");
    assert_overload(
        save,
        0,
        "baseProperty",
        PrimitiveName::String,
        PrimitiveName::Void,
    );
    assert_overload(
        save,
        1,
        "ownCall",
        PrimitiveName::Number,
        PrimitiveName::Boolean,
    );
}

// @ai-generated - Heritage arm order owns sibling occurrence order while an
// exact shared root occurrence remains deduplicated.
#[test]
fn creo_heritage_permutation_reorders_siblings_not_shared_root() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/permutation.ts",
        r#"
interface Root {
  (event: 'shared', shared: string): string
}
interface Left extends Root {
  (event: 'side', left: string): string
}
interface Right extends Root {
  (event: 'side', right: number): number
}
export interface LeftRight extends Left, Right {}
export interface RightLeft extends Right, Left {}
"#,
    );
    for (id, imported) in [
        ("/src/LeftRight.vue", "LeftRight"),
        ("/src/RightLeft.vue", "RightLeft"),
    ] {
        upsert_vue(
            &host,
            id,
            &format!(
                r#"<script setup lang="ts">
import type {{ {imported} }} from './permutation'
defineEmits<{imported}>()
</script>"#
            ),
        );
    }

    let left_right = cold_and_warm_contract(&host, "/src/LeftRight.vue");
    assert_eq!(event(&left_right, "shared").overloads.len(), 1);
    let side = event(&left_right, "side");
    assert_overload(
        side,
        0,
        "left",
        PrimitiveName::String,
        PrimitiveName::String,
    );
    assert_overload(
        side,
        1,
        "right",
        PrimitiveName::Number,
        PrimitiveName::Number,
    );

    let right_left = cold_and_warm_contract(&host, "/src/RightLeft.vue");
    assert_eq!(event(&right_left, "shared").overloads.len(), 1);
    let side = event(&right_left, "side");
    assert_overload(
        side,
        0,
        "right",
        PrimitiveName::Number,
        PrimitiveName::Number,
    );
    assert_overload(
        side,
        1,
        "left",
        PrimitiveName::String,
        PrimitiveName::String,
    );
}

// @ai-generated - Occurrence identity combines exact authored origin with the
// instantiated subject: equal instantiations deduplicate, unequal ones remain.
#[test]
fn creo_generic_diamond_identity_includes_instantiation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/generic-diamond.ts",
        r#"
interface Root<T> {
  (event: 'save', value: T): T
}
interface Left<T> extends Root<T> {}
interface Right<T> extends Root<T> {}
export interface Same extends Left<string>, Right<string> {}
interface TextRoot extends Root<string> {}
interface NumberRoot extends Root<number> {}
export interface Different extends TextRoot, NumberRoot {}
"#,
    );
    for (id, imported) in [
        ("/src/Same.vue", "Same"),
        ("/src/Different.vue", "Different"),
    ] {
        upsert_vue(
            &host,
            id,
            &format!(
                r#"<script setup lang="ts">
import type {{ {imported} }} from './generic-diamond'
defineEmits<{imported}>()
</script>"#
            ),
        );
    }

    let same = cold_and_warm_contract(&host, "/src/Same.vue");
    let save = event(&same, "save");
    assert_eq!(save.overloads.len(), 1);
    assert_overload(
        save,
        0,
        "value",
        PrimitiveName::String,
        PrimitiveName::String,
    );

    let different = cold_and_warm_contract(&host, "/src/Different.vue");
    let save = event(&different, "save");
    assert_eq!(save.overloads.len(), 2);
    assert_overload(
        save,
        0,
        "value",
        PrimitiveName::String,
        PrimitiveName::String,
    );
    assert_overload(
        save,
        1,
        "value",
        PrimitiveName::Number,
        PrimitiveName::Number,
    );
}

// @ai-generated - Structurally equal signatures from distinct authored
// declaration origins are distinct canonical occurrences.
#[test]
fn creo_distinct_authored_name_collisions_both_remain() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/name-collision.ts",
        r#"
interface Left {
  (event: 'save', value: string): boolean
}
interface Right {
  (event: 'save', value: string): boolean
}
export interface Both extends Left, Right {}
"#,
    );
    upsert_vue(
        &host,
        "/src/NameCollision.vue",
        r#"<script setup lang="ts">
import type { Both } from './name-collision'
defineEmits<Both>()
</script>"#,
    );

    let contract = cold_and_warm_contract(&host, "/src/NameCollision.vue");
    let save = event(&contract, "save");
    assert_eq!(save.overloads.len(), 2);
    for index in 0..2 {
        assert_overload(
            save,
            index,
            "value",
            PrimitiveName::String,
            PrimitiveName::Boolean,
        );
    }
}
