//! Public-boundary completion contracts for the template surface, exercised
//! against a REAL type provider (tsserver + TSGO) — the merged path a user
//! actually gets, not a `MockTypeProvider` running Verter's native half in
//! isolation.
//!
//! Each lane asserts BOTH what must be offered and what must never be: a
//! slot-name list is non-empty and names the child's declared slots; no
//! generated carrier identifier (`__verter_*` / `__Verter*` / `___VERTER___*`
//! / `$V_*`) ever reaches the user; and a Vue `v-bind` shorthand position is
//! bounded to the attribute surface instead of dumping the provider's global
//! scope through the JSX→Vue attribute transform.

use crate::test_harness::real_provider_test;

/// Every generated-carrier identifier shape that must never surface.
fn generated_identifier_leaks(labels: &[String]) -> Vec<String> {
    labels
        .iter()
        .filter(|label| verter_compiler::framework_common::is_generated_identifier(label))
        .cloned()
        .collect()
}

real_provider_test!(
    completion_vue_slot_name_offers_child_declared_slots,
    fixture = "vue-parity",
    async fn run(session) {
        // `<template #|` and `<template v-slot:|` inside a resolved child.
        // `IdeSurfaceChild.vue` declares `header` / `default` / `mySlot`
        // through `defineSlots`.
        for (path, marker) in [
            ("src/ide/SlotShorthandCompletion.vue", "<template #"),
            ("src/ide/SlotLonghandCompletion.vue", "<template v-slot:"),
        ] {
            let uri = session
                .open_virtual(
                    path,
                    &format!(
                        r#"<script setup lang="ts">
import IdeSurfaceChild from './IdeSurfaceChild.vue'
</script>
<template>
  <IdeSurfaceChild :label="'x'" :count="1">
    {marker}
  </IdeSurfaceChild>
</template>
"#
                    ),
                )
                .await;
            let position = session.find_position(&uri, marker, marker.len());
            let mut labels = Vec::new();
            for _ in 0..16 {
                labels = session.completion_labels(&uri, position, None).await;
                if labels.iter().any(|label| label == "header") {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            assert!(
                !labels.is_empty(),
                "`{marker}|` must offer the child's slot names, got an empty list"
            );
            for expected in ["header", "default", "mySlot"] {
                assert!(
                    labels.iter().any(|label| label == expected),
                    "`{marker}|` must offer declared slot {expected}, got: {labels:?}"
                );
            }
            let leaks = generated_identifier_leaks(&labels);
            assert!(
                leaks.is_empty(),
                "`{marker}|` must not leak generated carrier identifiers: {leaks:?}"
            );
            // The slot-name position is Verter-owned: the provider's identifier
            // scope is not a slot name, so the list stays the declared surface.
            assert!(
                labels.len() <= 16,
                "`{marker}|` must stay bounded to the declared slot surface, got {} items: {labels:?}",
                labels.len()
            );
        }
    }
);

real_provider_test!(
    completion_vue_bind_shorthand_is_bounded_to_the_attribute_surface,
    fixture = "vue-parity",
    async fn run(session) {
        let uri = session
            .open_virtual(
                "src/ide/BindShorthandCompletion.vue",
                r#"<script setup lang="ts">
import IdeSurfaceChild from './IdeSurfaceChild.vue'
</script>
<template>
  <div :></div>
  <IdeSurfaceChild ></IdeSurfaceChild>
</template>
"#,
            )
            .await;

        // CONTROL — a component attribute-name position keeps the provider's
        // real prop/event surface. Establishing this first also settles the
        // provider so the `<div :` lane below reads a warm surface.
        let control_position =
            session.find_position(&uri, "<IdeSurfaceChild ", "<IdeSurfaceChild ".len());
        let mut control = Vec::new();
        for _ in 0..16 {
            control = session.completion_labels(&uri, control_position, None).await;
            if control.iter().any(|label| label == "label") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        for expected in ["label", "count", "my-prop"] {
            assert!(
                control.iter().any(|label| label == expected),
                "a component attribute position must keep the provider's declared prop \
                 {expected}, got: {control:?}"
            );
        }

        // The `v-bind` shorthand position must NOT dump the provider's global
        // scope through the JSX→Vue attribute transform.
        let position = session.find_position(&uri, "<div :", "<div :".len());
        let labels = session.completion_labels(&uri, position, None).await;
        assert!(
            !labels.is_empty(),
            "`<div :|` must still offer the directive/attribute table"
        );
        for expected in ["v-bind", "v-if", "class"] {
            assert!(
                labels.iter().any(|label| label == expected),
                "`<div :|` must keep the directive/attribute table entry {expected}, \
                 got: {labels:?}"
            );
        }
        let leaks = generated_identifier_leaks(&labels);
        assert!(
            leaks.is_empty(),
            "`<div :|` must not leak generated carrier identifiers: {leaks:?}"
        );
        // Global-scope identifiers are not element attributes. These are the
        // kebab-cased spellings the JSX→Vue transform produced from the
        // provider's whole global scope.
        for forbidden in [
            "structured-clone",
            "weak-set",
            "onwaiting",
            "c-s-s-matrix-component",
            "page-reveal-event",
        ] {
            assert!(
                !labels.iter().any(|label| label == forbidden),
                "`<div :|` must not offer the global-scope identifier {forbidden}, \
                 got {} items",
                labels.len()
            );
        }
        assert!(
            labels.len() <= 64,
            "`<div :|` must stay bounded to the attribute surface, got {} items: {:?}",
            labels.len(),
            labels.iter().take(40).collect::<Vec<_>>()
        );
    }
);

real_provider_test!(
    completion_svelte_markup_expression_hides_generated_helpers,
    fixture = "svelte-parity",
    async fn run(session) {
        let uri = session
            .open_virtual(
                "src/ide/MarkupExpressionCompletion.svelte",
                r#"<script lang="ts">
  let markupCount = 1;
</script>
<p>{}</p>
"#,
            )
            .await;
        let position = session.find_position(&uri, "<p>{", "<p>{".len());
        let mut labels = Vec::new();
        for _ in 0..16 {
            labels = session.completion_labels(&uri, position, None).await;
            if labels.iter().any(|label| label == "markupCount") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        // The list must be real: the authored script binding is reachable from
        // a Svelte markup expression.
        assert!(
            labels.iter().any(|label| label == "markupCount"),
            "a Svelte markup expression must offer the authored script binding, got {} items",
            labels.len()
        );
        // …and it must not carry the Svelte IDE prelude's projection helpers.
        let leaks = generated_identifier_leaks(&labels);
        assert!(
            leaks.is_empty(),
            "a Svelte markup expression must not leak generated carrier identifiers: {leaks:?}"
        );
    }
);
