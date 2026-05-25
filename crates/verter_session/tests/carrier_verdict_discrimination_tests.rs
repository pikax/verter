//! Block 6.j R22 discriminating tests for the synthetic
//! slot-binding carrier-verdict pipeline (T1 / T2 / T3 in the
//! consult brief).
//!
//! T1 — same `binding_name` in two slots within ONE component must
//!      produce DISTINCT `CarrierIdentity` cache entries. Discrimination
//!      contract: temporarily flatten the key to `(scope,
//!      binding_name)` (omit `value_node`) and the second slot's
//!      binding would inherit the first's verdict (poison). With the
//!      full `value_node`-bearing identity the entries are distinct.
//!
//! T2 — a real workspace-owned `type <name> = …` alias declared in
//!      its own surface MUST NOT inherit a `DoNotDeepen` verdict
//!      from a synthetic carrier that happens to share `<name>`, and
//!      the synthetic carrier MUST NOT suppress the real alias's
//!      registry entry. The synthetic carrier's `carrier_provenance`
//!      sidecar is the disambiguator: the real alias has
//!      `carrier_provenance: None` and follows the normal
//!      registration path.
//!
//! T3 — `collect_component_meta_registry_public_field_refs` MUST
//!      NOT enqueue any registry ref for a synthetic carrier. The
//!      refuse-to-enqueue at the function entry is verified via the
//!      observed `published_names` / `type_registry` payload on
//!      `ComponentMetaAnalysis`: the synthetic carrier's
//!      `binding_name` must not appear in `type_registry` for a
//!      component whose ONLY use of that identifier is the
//!      synthetic carrier.

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::carrier_verdict_db::CarrierIdentity;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

fn build_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }))
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert ts");
}

fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

// ---------------------------------------------------------------------------
// T1 — same `binding_name` in two slots, different graph value nodes.
// ---------------------------------------------------------------------------

const T1_TOOLKIT_TS: &str = r#"
export interface AlphaProps<TA = unknown> {
  foo?: TA;
  alphaSpecific?: string;
}

export interface BetaProps<TB = unknown> {
  foo?: TB;
  betaSpecific?: number;
}
"#;

const T1_TWO_SLOTS_VUE: &str = r#"<script lang="ts">
import type { AlphaProps, BetaProps } from './t1-toolkit';

export interface TwoSlotsSlots<TA, TB> {
  alphaSlot?(props: AlphaProps<TA> & { foo: AlphaProps<TA>['foo'] }): unknown;
  betaSlot?(props: BetaProps<TB> & { foo: BetaProps<TB>['foo'] }): unknown;
}
</script>

<script setup lang="ts" generic="TA, TB">
defineSlots<TwoSlotsSlots<TA, TB>>();
</script>
<template><div /></template>
"#;

#[test]
fn t1_same_binding_name_in_two_slots_keep_distinct_cache_entries() {
    let host = build_host();
    upsert_ts(&host, "/t1-toolkit.ts", T1_TOOLKIT_TS);
    upsert_vue(&host, "/TwoSlots.vue", T1_TWO_SLOTS_VUE);

    let expanded = host
        .evaluate_types("/TwoSlots.vue")
        .expect("evaluate_types must return for the TwoSlots fixture");

    let foo_bindings: Vec<_> = expanded
        .slot_bindings
        .iter()
        .filter(|f| f.name.ends_with(".foo"))
        .collect();

    // Sanity: the fixture produces exactly two `.foo` bindings, one
    // per slot.
    assert_eq!(
        foo_bindings.len(),
        2,
        "fixture must publish exactly two `.foo` slot bindings (one per slot). \
         observed slot_bindings={:#?}",
        expanded
            .slot_bindings
            .iter()
            .map(|f| f.name.clone())
            .collect::<Vec<_>>()
    );

    let alpha = foo_bindings
        .iter()
        .find(|f| f.name == "alphaSlot.foo")
        .expect("alphaSlot.foo must be published");
    let beta = foo_bindings
        .iter()
        .find(|f| f.name == "betaSlot.foo")
        .expect("betaSlot.foo must be published");

    // Both `.foo` bindings must be synthetic carriers (no-parser
    // branch). Without that, this T1 test is not discriminating.
    // R22-fix sparse-sidecar variant: the provenance lives in the
    // parent `ExpandedComponentTypes::carrier_provenance_table`
    // (keyed by `(surface_kind, field.name)`), not on the
    // `ExpandedField` itself.
    use verter_semantic::analysis::type_expand::PublishedSurfaceKind;
    let table = &expanded.carrier_provenance_table;
    let alpha_prov = table
        .get(PublishedSurfaceKind::SlotBinding, alpha.name.as_str())
        .expect("alphaSlot.foo must carry CarrierProvenance in the table");
    let beta_prov = table
        .get(PublishedSurfaceKind::SlotBinding, beta.name.as_str())
        .expect("betaSlot.foo must carry CarrierProvenance in the table");

    // The codex TOP RISK invariant: the same `binding_name` in two
    // slots MUST produce distinct cache identities. Different
    // `value_node`s and different `slot_name`s independently
    // disambiguate.
    assert_eq!(
        alpha_prov.binding_name.as_ref(),
        "foo",
        "alphaSlot.foo provenance binding_name mismatch"
    );
    assert_eq!(
        beta_prov.binding_name.as_ref(),
        "foo",
        "betaSlot.foo provenance binding_name mismatch"
    );
    assert_eq!(alpha_prov.slot_name.as_deref(), Some("alphaSlot"));
    assert_eq!(beta_prov.slot_name.as_deref(), Some("betaSlot"));
    assert_ne!(
        alpha_prov.value_node, beta_prov.value_node,
        "two slots' synthetic carriers MUST have distinct value_nodes — \
         the codex TOP RISK invariant. alpha={:?} beta={:?}",
        alpha_prov.value_node, beta_prov.value_node,
    );

    // And critically, the CarrierIdentity keys built from each
    // provenance must NOT be equal.
    let alpha_key = CarrierIdentity::from_provenance(alpha_prov);
    let beta_key = CarrierIdentity::from_provenance(beta_prov);
    assert_ne!(
        alpha_key, beta_key,
        "two slots' CarrierIdentity keys MUST differ. alpha={:?} beta={:?}",
        alpha_key, beta_key,
    );

    // Both entries must be admitted as `DoNotDeepen` independently.
    let verdicts = host.project_type_store().carrier_verdicts();
    assert!(
        verdicts.is_do_not_deepen(&alpha_key),
        "alphaSlot.foo must have a `DoNotDeepen` cache entry"
    );
    assert!(
        verdicts.is_do_not_deepen(&beta_key),
        "betaSlot.foo must have a `DoNotDeepen` cache entry"
    );
}

// ---------------------------------------------------------------------------
// T2 — real workspace-owned alias with the same name as a synthetic
//      carrier must remain resolvable; the synthetic carrier must
//      not suppress the alias's registration.
// ---------------------------------------------------------------------------

const T2_TOOLKIT_TS: &str = r#"
// A real workspace-owned type alias that HAPPENS to share its name
// with the synthetic carrier's `binding_name` below.
export type foo = { realProperty: string };

export interface OwnerProps<T = unknown> {
  foo?: T;
  // A real prop using the real `foo` alias — its registry entry
  // MUST resolve to `{ realProperty: string }`, not to the
  // synthetic carrier's `DoNotDeepen` sentinel.
  realFoo?: foo;
}
"#;

const T2_ALIAS_COLLISION_VUE: &str = r#"<script lang="ts">
import type { OwnerProps, foo } from './t2-toolkit';

export interface CollisionSlots<T> {
  // A slot binding named `foo` — same identifier as the imported
  // alias. The graph-native synthesis publishes a synthetic carrier
  // `Ref { name: "foo" }` for this binding.
  default?(props: OwnerProps<T> & { foo: OwnerProps<T>['foo'] }): unknown;
}
</script>

<script setup lang="ts" generic="T">
defineProps<{ realFoo?: foo }>();
defineSlots<CollisionSlots<T>>();
</script>
<template><div /></template>
"#;

#[test]
fn t2_real_type_alias_with_same_name_remains_resolvable() {
    let host = build_host();
    upsert_ts(&host, "/t2-toolkit.ts", T2_TOOLKIT_TS);
    upsert_vue(&host, "/Collision.vue", T2_ALIAS_COLLISION_VUE);

    let (analysis, _resolved, _audit) = AuditedRequest::builder()
        .attach_to(host.clone())
        .resolve_component_meta("/Collision.vue")
        .expect("resolve_component_meta for the Collision fixture");

    // The synthetic carrier must be admitted with a CarrierProvenance
    // (no-parser-branch published a `Ref { name: "foo" }` carrier
    // for `default.foo`). Without this, T2 is not discriminating.
    let expanded = host
        .evaluate_types("/Collision.vue")
        .expect("evaluate_types for the Collision fixture");
    let default_foo = expanded
        .slot_bindings
        .iter()
        .find(|f| f.name == "default.foo")
        .expect("default.foo slot binding must be published");
    use verter_semantic::analysis::type_expand::PublishedSurfaceKind;
    assert!(
        expanded
            .carrier_provenance_table
            .contains(PublishedSurfaceKind::SlotBinding, default_foo.name.as_str(),),
        "default.foo must be a synthetic carrier (no-parser branch) recorded in the \
         carrier_provenance_table. Without that, T2 does not exercise the \
         shared-name disambiguation."
    );

    // Critical: the prop `realFoo` (typed as the real `foo` alias)
    // resolves to the alias's actual body — NOT to a `Ref { "foo" }`
    // that got short-circuited by the synthetic carrier's
    // `DoNotDeepen` verdict.
    let real_foo_prop = analysis
        .props
        .iter()
        .find(|p| p.name == "realFoo")
        .expect("realFoo prop must be published");
    // Under the project's shallow-by-default contract, the published
    // `type_expr` is the bare alias reference `Ref { "foo" }`. The
    // alias's actual body is published in `type_registry` for
    // downstream consumers to resolve on demand. The discriminator
    // here is that the registry DOES contain a `foo` entry — its
    // absence would mean the synthetic carrier's refuse-to-enqueue
    // shadowed the real alias's registration.
    let real_foo_signature =
        serde_json::to_string(&real_foo_prop.type_expr).expect("serialize PropAnalysis.type_expr");
    assert!(
        real_foo_signature.contains("\"name\":\"foo\""),
        "realFoo prop must reference the real `foo` alias. Observed type_expr={}",
        real_foo_signature,
    );

    let foo_registry = analysis
        .type_registry
        .iter()
        .find(|entry| entry.name == "foo")
        .expect(
            "the real `foo` type alias MUST appear in the type_registry. \
             The synthetic carrier's refuse-to-enqueue MUST NOT suppress \
             the same-named real alias's registration (that would conflate \
             the synthetic carrier's scope with the real alias's surface).",
        );
    let foo_registry_json = serde_json::to_string(&foo_registry.type_expr)
        .expect("serialize ResolvedTypeAnalysis.type_expr");
    assert!(
        foo_registry_json.contains("realProperty"),
        "the registered `foo` alias's body MUST resolve to \
         `{{ realProperty: string }}`; the synthetic carrier's \
         `DoNotDeepen` verdict MUST NOT shadow the real alias's body. \
         Observed registry type_expr={}",
        foo_registry_json,
    );
}

// ---------------------------------------------------------------------------
// T3 — registry non-enqueue: the synthetic carrier's binding name
//      MUST NOT appear in the component's published type registry.
// ---------------------------------------------------------------------------

const T3_TOOLKIT_TS: &str = r#"
export interface UniqueProps<T = unknown> {
  // Use a binding name unlikely to exist as any real type alias
  // anywhere in the workspace.
  __r22_synthetic_only?: T;
}
"#;

const T3_UNIQUE_NAME_VUE: &str = r#"<script lang="ts">
import type { UniqueProps } from './t3-toolkit';

export interface UniqueSlots<T> {
  default?(props: UniqueProps<T> & { __r22_synthetic_only: UniqueProps<T>['__r22_synthetic_only'] }): unknown;
}
</script>

<script setup lang="ts" generic="T">
defineSlots<UniqueSlots<T>>();
</script>
<template><div /></template>
"#;

#[test]
fn t3_synthetic_carrier_binding_name_not_enqueued_in_type_registry() {
    let host = build_host();
    upsert_ts(&host, "/t3-toolkit.ts", T3_TOOLKIT_TS);
    upsert_vue(&host, "/UniqueName.vue", T3_UNIQUE_NAME_VUE);

    let (analysis, _resolved, _audit) = AuditedRequest::builder()
        .attach_to(host.clone())
        .resolve_component_meta("/UniqueName.vue")
        .expect("resolve_component_meta for UniqueName");

    // Sanity: synthetic carrier MUST exist for this test to discriminate.
    let expanded = host
        .evaluate_types("/UniqueName.vue")
        .expect("evaluate_types for UniqueName");
    let synthetic = expanded
        .slot_bindings
        .iter()
        .find(|f| f.name == "default.__r22_synthetic_only")
        .expect("default.__r22_synthetic_only must be published");
    use verter_semantic::analysis::type_expand::PublishedSurfaceKind;
    assert!(
        expanded
            .carrier_provenance_table
            .contains(PublishedSurfaceKind::SlotBinding, synthetic.name.as_str(),),
        "default.__r22_synthetic_only must be a synthetic carrier recorded in the \
         carrier_provenance_table"
    );

    // The codex-required contract: the synthetic carrier's
    // `binding_name` (`__r22_synthetic_only`) MUST NOT appear in
    // the component's published `type_registry`. The
    // refuse-to-enqueue at
    // `collect_component_meta_registry_public_field_refs` is what
    // closes this off; reverting the refusal would let the registry
    // walk discover a missing alias and admit a Miss entry (still
    // visible in the registry as a `__r22_synthetic_only` name —
    // either as a resolved entry or an unresolved one).
    for entry in &analysis.type_registry {
        assert_ne!(
            entry.name, "__r22_synthetic_only",
            "synthetic carrier binding_name `__r22_synthetic_only` must NOT \
             appear in type_registry. The codex-required \
             refuse-to-enqueue at \
             `collect_component_meta_registry_public_field_refs` is the \
             closure. Found entry={:#?}",
            entry
        );
    }
}
