use super::*;

// @ai-generated - Proves an identified Vue slot consumer retains its source-derived fallback
// while the imported child's native slot surface is unavailable.
#[tokio::test]
async fn contract_slot_name_hover_falls_back_when_child_surface_is_unavailable() {
    const PARENT_WITH_UNAVAILABLE_CHILD: &str = "<script setup lang=\"ts\">\n\
import MissingChild from './MissingChild.vue'\n\
</script>\n\
<template>\n\
  <MissingChild>\n\
    <template #header>content</template>\n\
  </MissingChild>\n\
</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", PARENT_WITH_UNAVAILABLE_CHILD)]).await;
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    assert!(
        server
            .hover_native_semantics_enabled
            .load(std::sync::atomic::Ordering::Acquire),
        "precondition: the native hover lane must be enabled"
    );

    let position = find_document_position(server, &app_uri, "#header", 1);
    let doc = server.documents.get(&app_uri).expect("parent document");
    let analysis = server
        .documents
        .get_analysis(&app_uri)
        .expect("parent analysis");
    let offset = doc
        .line_index
        .position_to_offset(&position)
        .expect("slot offset");
    assert!(
        matches!(
            crate::features::hover::child_hover_target_at_offset(offset, &doc.source, &analysis),
            Some(crate::features::hover::ChildHoverTarget::SlotAttribute(_))
        ),
        "precondition: #header must enter the native child-slot path"
    );
    drop(doc);

    let hover = server
        .hover(hover_params(&app_uri, position))
        .await
        .expect("hover request should succeed");
    assert_eq!(
        hover_text(hover),
        "**Slot content** — `#header`\n\nProvides content for the **\"header\"** slot.",
        "an unavailable child slot surface must fall back to the static source-derived answer"
    );

    drain_handle.abort();
    drop(service);
}

// @ai-generated - Negative control proving an available child surface still outranks fallback.
#[tokio::test]
async fn contract_hash_slot_name_hover_shows_child_slot_signature() {
    let text = d3_hover_text("#header", 1)
        .await
        .expect("#header must produce a hover");
    for needle in ["header", "title", "string", "count", "number"] {
        assert!(
            text.contains(needle),
            "#header hover must carry the child slot-props signature ({needle}), got: {text}"
        );
    }
    assert!(
        !text.contains("**Slot content**"),
        "the native child signature must win over the static slot fallback: {text}"
    );
}

#[tokio::test]
async fn contract_default_slot_name_hover_shows_child_slot_signature() {
    let text = d3_hover_text("#default", 1)
        .await
        .expect("#default must produce a hover");
    for needle in ["default", "body", "string"] {
        assert!(
            text.contains(needle),
            "#default hover must carry the child slot-props signature ({needle}), got: {text}"
        );
    }
}

#[tokio::test]
async fn contract_kebab_slot_name_hover_resolves_camel_declared_signature() {
    let text = d3_hover_text("#my-slot", 2)
        .await
        .expect("#my-slot must produce a hover");
    for needle in ["mySlot", "note", "string"] {
        assert!(
            text.contains(needle),
            "#my-slot hover must resolve the camel-declared slot ({needle}), got: {text}"
        );
    }
}

#[tokio::test]
async fn contract_longhand_v_slot_name_hover_shows_child_slot_signature() {
    let text = d3_hover_text("v-slot:header", "v-slot:".len() as u32)
        .await
        .expect("v-slot:header must produce a hover");
    for needle in ["header", "title", "string"] {
        assert!(
            text.contains(needle),
            "v-slot:header hover must carry the child slot-props signature ({needle}), got: {text}"
        );
    }
}

#[tokio::test]
async fn contract_unknown_slot_name_produces_no_hover() {
    let text = d3_hover_text("#nope", 1).await;
    assert!(
        text.is_none(),
        "an authoritative child slot surface must fail closed for an undeclared name, got: {text:?}"
    );
}

// =========================================================================
// D4 — slot-props destructure hover (pattern + usage positions)
// =========================================================================

#[tokio::test]
async fn contract_slot_props_pattern_positions_hover_with_provider_binding_types() {
    let (_temp, service, drain_handle, provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", D3_CHILD_SOURCE),
        ("src/App.vue", "vue", D3_PARENT_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();

    // Pattern positions must map through CodeTransform into the generated TSX
    // (they are verbatim-authored bytes inside the slot IIFE) so the provider's
    // typed binding hover answers at the authored offset — identical to a
    // standalone-TS destructured parameter. The mock matches hovers by EXACT
    // (path, offset), and the merged hover must equal the provider's fenced
    // quickinfo byte-for-byte — no substring needles, no residual verter text.
    for (needle, shift, label, seeded) in [
        (
            "{ title, count: slotCount }",
            2,
            "pattern title",
            "const title: string",
        ),
        (
            "count: slotCount }",
            1,
            "pattern source key count",
            "(property) count: number",
        ),
        (
            "count: slotCount }",
            7,
            "pattern alias slotCount",
            "const slotCount: number",
        ),
    ] {
        let mut position = find_document_position(server, &app_uri, needle, 0);
        position.character += shift;
        set_type_hover_at_vue_position(server, &provider, &app_uri, position, seeded);
        let text = hover_text(
            server
                .hover(hover_params(&app_uri, position))
                .await
                .expect("hover request should succeed"),
        );
        let expected = format!("```typescript\n{seeded}\n```");
        assert_eq!(
            text, expected,
            "{label} hover must be exactly the provider's typed binding hover"
        );
    }

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_slot_props_pattern_positions_hover_with_provider_binding_types_js_mode() {
    // D4 in JS mode: the slot destructure pattern is language-independent in
    // the IDE lowering — the same exact-offset provider round trip must hold
    // for a `<script setup>` (no lang) parent.
    const D4_PARENT_JS_SOURCE: &str = "<script setup>\nimport MyComp from './MyComp.vue'\n</script>\n<template>\n  <MyComp>\n    <template #header=\"{ title, count: slotCount }\">\n      <span>{{ title }}:{{ slotCount }}</span>\n    </template>\n  </MyComp>\n</template>\n";
    let (_temp, service, drain_handle, provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", D3_CHILD_SOURCE),
        ("src/App.vue", "vue", D4_PARENT_JS_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();

    for (needle, shift, label, seeded) in [
        (
            "{ title, count: slotCount }",
            2,
            "js pattern title",
            "const title: string",
        ),
        (
            "count: slotCount }",
            1,
            "js pattern source key count",
            "(property) count: number",
        ),
        (
            "count: slotCount }",
            7,
            "js pattern alias slotCount",
            "const slotCount: number",
        ),
    ] {
        let mut position = find_document_position(server, &app_uri, needle, 0);
        position.character += shift;
        set_type_hover_at_vue_position(server, &provider, &app_uri, position, seeded);
        let text = hover_text(
            server
                .hover(hover_params(&app_uri, position))
                .await
                .expect("hover request should succeed"),
        );
        let expected = format!("```typescript\n{seeded}\n```");
        assert_eq!(
            text, expected,
            "{label} hover must be exactly the provider's typed binding hover"
        );
    }

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_slot_props_usage_positions_hover_with_provider_binding_types() {
    let (_temp, service, drain_handle, provider, workspace_id) = make_definition_test_server(&[
        ("src/MyComp.vue", "vue", D3_CHILD_SOURCE),
        ("src/App.vue", "vue", D3_PARENT_SOURCE),
    ])
    .await;
    let app_uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();

    for (needle, shift, label, seeded) in [
        ("{{ title }}", 3, "usage title", "const title: string"),
        (
            "{{ slotCount }}",
            3,
            "usage slotCount",
            "const slotCount: number",
        ),
    ] {
        let mut position = find_document_position(server, &app_uri, needle, 0);
        position.character += shift;
        set_type_hover_at_vue_position(server, &provider, &app_uri, position, seeded);
        let text = hover_text(
            server
                .hover(hover_params(&app_uri, position))
                .await
                .expect("hover request should succeed"),
        );
        assert!(
            text.contains(seeded),
            "{label} hover must be the provider's typed binding hover, got: {text}"
        );
    }

    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_svelte_snippet_name_hover_maps_to_provider() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/List.svelte", "svelte", D3_SVELTE_SOURCE)]).await;
    let doc_uri = workspace_uri(&workspace_id, "src/List.svelte");
    let server = service.inner();
    let mut position = find_document_position(server, &doc_uri, "{#snippet row(", 0);
    position.character += "{#snippet ".len() as u32;
    set_type_hover_at_vue_position(
        server,
        &provider,
        &doc_uri,
        position,
        "const row: Snippet<[item: number]>",
    );
    let text = hover_text(
        server
            .hover(hover_params(&doc_uri, position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        text.contains("Snippet"),
        "snippet name hover must round-trip the provider's typed snippet hover, got: {text}"
    );
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_svelte_render_callsite_hover_maps_to_provider() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/List.svelte", "svelte", D3_SVELTE_SOURCE)]).await;
    let doc_uri = workspace_uri(&workspace_id, "src/List.svelte");
    let server = service.inner();
    let mut position = find_document_position(server, &doc_uri, "{@render row(", 0);
    position.character += "{@render ".len() as u32;
    set_type_hover_at_vue_position(
        server,
        &provider,
        &doc_uri,
        position,
        "const row: (this: void, item: number) => void",
    );
    let text = hover_text(
        server
            .hover(hover_params(&doc_uri, position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        text.contains("row"),
        "render callsite hover must round-trip the provider's typed hover, got: {text}"
    );
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_svelte_snippet_param_positions_hover_with_provider_binding_types() {
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/List.svelte", "svelte", D3_SVELTE_SOURCE)]).await;
    let doc_uri = workspace_uri(&workspace_id, "src/List.svelte");
    let server = service.inner();

    for (needle, shift, label) in [
        ("row(item: number)", 4, "snippet parameter pattern"),
        ("{item}", 1, "snippet parameter usage"),
    ] {
        let mut position = find_document_position(server, &doc_uri, needle, 0);
        position.character += shift;
        set_type_hover_at_vue_position(
            server,
            &provider,
            &doc_uri,
            position,
            "(parameter) item: number",
        );
        let text = hover_text(
            server
                .hover(hover_params(&doc_uri, position))
                .await
                .expect("hover request should succeed"),
        );
        assert!(
            text.contains("item: number"),
            "{label} hover must be the provider's typed parameter hover, got: {text}"
        );
    }
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_svelte_snippet_param_positions_hover_with_provider_binding_types_js_mode() {
    // D4 Svelte JS mode: the snippet parameter pattern maps into the
    // projection identically without a `lang="ts"` script — the same
    // exact-offset provider round trip must hold.
    const D4_SVELTE_JS_SOURCE: &str = "<script>\n  let items = $state([1, 2]);\n</script>\n\n{#snippet row(item)}\n  <li>{item}</li>\n{/snippet}\n\n<ul>\n  {#each items as it}\n    {@render row(it)}\n  {/each}\n</ul>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/List.svelte", "svelte", D4_SVELTE_JS_SOURCE)]).await;
    let doc_uri = workspace_uri(&workspace_id, "src/List.svelte");
    let server = service.inner();

    for (needle, shift, label) in [
        ("row(item)", 4, "js snippet parameter pattern"),
        ("{item}", 1, "js snippet parameter usage"),
    ] {
        let mut position = find_document_position(server, &doc_uri, needle, 0);
        position.character += shift;
        set_type_hover_at_vue_position(
            server,
            &provider,
            &doc_uri,
            position,
            "(parameter) item: any",
        );
        let text = hover_text(
            server
                .hover(hover_params(&doc_uri, position))
                .await
                .expect("hover request should succeed"),
        );
        let expected = "```typescript\n(parameter) item: any\n```";
        assert_eq!(
            text, expected,
            "{label} hover must be exactly the provider's typed parameter hover"
        );
    }
    drain_handle.abort();
    drop(service);
}

/// A hover answered from an imported child's contract carries that child's
/// source revision as dependency evidence: when the child is edited before the
/// answer settles — the requested document unchanged — the answer built from
/// the superseded contract answers `ContentModified`, and a new request answers
/// from the edited child.
#[tokio::test(flavor = "multi_thread")]
async fn child_contract_hover_rejects_an_answer_whose_child_moved_before_settlement() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = "<script setup lang=\"ts\">\ndefineProps<{ beforeProp: string }>()\n</script>\n<template><div /></template>\n";
    let edited_child = "<script setup lang=\"ts\">\ndefineProps<{ afterProp: string }>()\n</script>\n<template><div /></template>\n";
    let app_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\n</script>\n\n<template>\n  <MyComp before-prop=\"literal\" />\n</template>\n";
    let child_uri = open_test_vue(server, "/workspace/src/MyComp.vue", child_source);
    let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);
    let position = Position {
        line: 5,
        character: 3,
    };
    let unmoved = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("an unmoved hover succeeds"),
    );
    assert!(
        unmoved.contains("beforeProp"),
        "the child contract answers the hover: {unmoved}"
    );

    let moved = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let server = server.clone();
        let moved = Arc::clone(&moved);
        server.request_barriers().clear();
        server.request_barriers().arm(
            super::super::test_support::RequestBarrier::Settlement,
            Arc::new(move |arrival| {
                if arrival == 0 {
                    assert!(
                        server
                            .documents
                            .did_change(&child_uri, 2, edited_child)
                            .changed
                    );
                    moved.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                Box::pin(async {})
            }),
        );
    }
    let raced = server.hover(hover_params(&app_uri, position)).await;
    assert!(moved.load(std::sync::atomic::Ordering::SeqCst));
    assert!(
        matches!(&raced, Err(error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
        "a hover read from a child edited before settlement answers ContentModified: {raced:?}"
    );

    server.request_barriers().clear();
    let current = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("a new hover succeeds"),
    );
    assert!(
        current.contains("afterProp") && !current.contains("beforeProp"),
        "a new request answers from the edited child: {current}"
    );
}

#[tokio::test]
async fn hover_prefers_child_component_summary_over_import_alias_on_component_tag() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
defineProps<{ foo: string; bar: number }>()
const emit = defineEmits<{ custom: [payload: string] }>()
</script>
<template><div /></template>
"#;
    let app_source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>

<template>
  <MyComp foo="literal" :bar="1" @custom="handler($event)" />
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/MyComp.vue", child_source);
    let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

    let mut position = Position {
        line: 5,
        character: 2,
    };
    position.character += 1;

    set_type_hover_at_vue_position(
        server,
        &provider,
        &app_uri,
        position,
        "(alias) import MyComp\nimport MyComp",
    );

    let text = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("hover request should succeed"),
    );

    assert!(
        text.contains("Props:"),
        "hover should show props, got: {text}"
    );
    assert!(
        text.contains("foo"),
        "hover should include foo, got: {text}"
    );
    assert!(
        text.contains("string"),
        "hover should include foo type, got: {text}"
    );
    assert!(
        text.contains("bar"),
        "hover should include bar, got: {text}"
    );
    assert!(
        text.contains("number"),
        "hover should include bar type, got: {text}"
    );
    assert!(
        text.contains("Emits:"),
        "hover should show emits, got: {text}"
    );
    assert!(
        text.contains("custom"),
        "hover should include custom emit, got: {text}"
    );
    assert!(
        text.contains("payload"),
        "hover should include payload label, got: {text}"
    );
    assert!(
        !text.contains("(alias) import MyComp"),
        "hover must not prefer import alias hover, got: {text}"
    );
    assert!(
        !text.contains("DefineComponent<{}, {}>"),
        "hover must not degrade to fallback component shell, got: {text}"
    );
}

/// D7 no-silent-empty: a transient provider failure on a carrier hover must
/// resync + retry — the user-visible tooltip recovers instead of vanishing.
/// Provider-neutral: the recovery lives in the shared handler above the
/// per-route provider trait, so both engine routes exercise it.
///
/// Call budgets: the freshly-seeded surface classifies as repair-pending, so
/// the tsserver route additionally fires its documented one-shot
/// synchronization probe (a discarded ordered response) before the
/// user-visible query — probe + failed query + retry = 3 calls; tsgo queries
/// directly — failed query + retry = 2.
#[tokio::test]
async fn hover_recovers_with_resync_and_retry_after_transient_provider_error() {
    for (kind, scripted_failures, expected_calls) in [
        (crate::TypeProviderKind::Tsserver, 1, 2),
        (crate::TypeProviderKind::Tsgo, 1, 2),
    ] {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service_with_kind(type_provider, kind);
        let server = service.inner();
        install_test_resolver(server);

        let app_source = r#"<script setup lang="ts">
const recoveredHoverTarget: string = "ok"
</script>
<template><div>{{ recoveredHoverTarget.length }}</div></template>
"#;
        let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);
        // `length` is a member the verter-native hover cannot answer — only the
        // provider can — so recovery is observable strictly through the
        // provider rail.
        let position = Position {
            line: 3,
            character: 45,
        };
        set_type_hover_at_vue_position(
            server,
            &provider,
            &app_uri,
            position,
            "(property) String.length: number",
        );
        // Transient failure(s), then the provider answers normally.
        provider.fail_next_hovers(scripted_failures);

        let text = hover_text(
            server
                .hover(hover_params(&app_uri, position))
                .await
                .expect("hover request should succeed"),
        );
        assert!(
            text.contains("String.length"),
            "{kind}: a transient provider error must recover to the typed hover, got: {text}"
        );
        let hover_calls = provider
            .calls()
            .iter()
            .filter(|call| matches!(call, MockCall::GetHover { .. }))
            .count();
        assert_eq!(
            hover_calls, expected_calls,
            "{kind}: exactly one retry after the transient failure, got {hover_calls} hover calls"
        );
    }
}

/// D7 no-silent-empty, fail-closed bound: a persistent provider failure must
/// NOT hang or spin — the handler retries exactly once after a resync and
/// then fails closed (None) rather than fabricating content. (tsserver adds
/// its one-shot synchronization probe, see the transient-case test.)
#[tokio::test]
async fn hover_fails_closed_after_bounded_retry_when_provider_keeps_failing() {
    for (kind, expected_calls) in [
        (crate::TypeProviderKind::Tsserver, 2),
        (crate::TypeProviderKind::Tsgo, 2),
    ] {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service_with_kind(type_provider, kind);
        let server = service.inner();
        install_test_resolver(server);

        let app_source = r#"<script setup lang="ts">
const persistentFailureTarget: string = "ok"
</script>
<template><div>{{ persistentFailureTarget.length }}</div></template>
"#;
        let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);
        // `length` is a member the verter-native hover cannot answer — only the
        // provider can — so a persistent provider failure yields NO tooltip at
        // all (never a fabrication).
        let position = Position {
            line: 3,
            character: 47,
        };
        set_type_hover_at_vue_position(
            server,
            &provider,
            &app_uri,
            position,
            "(property) String.length: number",
        );
        provider.fail_next_hovers(16);

        let result = server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("hover request must not error to the client");
        assert!(
            result.is_none(),
            "{kind}: a persistent provider error fails closed after the bounded retry"
        );
        let hover_calls = provider
            .calls()
            .iter()
            .filter(|call| matches!(call, MockCall::GetHover { .. }))
            .count();
        assert_eq!(
            hover_calls, expected_calls,
            "{kind}: retries are bounded to exactly one resync+retry, got {hover_calls} hover calls"
        );
    }
}

// @ai-generated - Guards canonical child resolution across sequential parent sync/query state.
#[tokio::test]
async fn component_tag_hover_keeps_analysis_resolved_child_across_two_parents() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
defineProps<{ contractProp: string }>()
</script>
<template><div /></template>
"#;
    let parent_source = r#"<script setup lang="ts">
import ContractChild from '../shared/ContractChild'
</script>
<template><ContractChild /></template>
"#;

    open_test_vue(
        server,
        "/workspace/src/shared/ContractChild.vue",
        child_source,
    );
    let first_parent = open_test_vue(
        server,
        "/workspace/src/feature-a/ParentA.vue",
        parent_source,
    );
    let second_parent = open_test_vue(
        server,
        "/workspace/src/feature-b/ParentB.vue",
        parent_source,
    );

    let position = Position {
        line: 3,
        character: 12,
    };
    for parent_uri in [&first_parent, &second_parent] {
        let analysis = server
            .documents
            .get_analysis(parent_uri)
            .expect("parent analysis should exist");
        assert_eq!(
            analysis.imports[0].resolved_canonical_id.as_deref(),
            Some("/workspace/src/shared/ContractChild.vue"),
            "analysis must retain the canonical child identity for each parent",
        );
    }

    set_type_hover_at_vue_position(
        server,
        &provider,
        &first_parent,
        position,
        "PROVIDER_MODULE_FALLBACK",
    );
    let first_text = hover_text(
        server
            .hover(hover_params(&first_parent, position))
            .await
            .expect("first parent hover request should succeed"),
    );
    assert!(
        first_text.contains("contractProp") && first_text.contains("string"),
        "the first parent should resolve the child contract, got: {first_text}",
    );

    // The first request establishes provider sync and exercises every host route
    // that used to influence the second parent's workspace fallback. The second
    // request must still use its analysis-owned canonical import identity.
    set_type_hover_at_vue_position(
        server,
        &provider,
        &second_parent,
        position,
        "PROVIDER_MODULE_FALLBACK",
    );
    let second_text = hover_text(
        server
            .hover(hover_params(&second_parent, position))
            .await
            .expect("second parent hover request should succeed"),
    );
    assert!(
        second_text.contains("contractProp") && second_text.contains("string"),
        "the second parent should retain the same child contract, got: {second_text}",
    );
    assert!(
        !second_text.contains("PROVIDER_MODULE_FALLBACK"),
        "child resolution must not drift into the provider's module fallback: {second_text}",
    );
}

#[tokio::test]
async fn hover_prefers_child_component_summary_over_import_alias_on_vue_import_binding() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
defineProps<{ foo: string; bar: number }>()
const emit = defineEmits<{ custom: [payload: string] }>()
</script>
<template><div /></template>
"#;
    let app_source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>

<template>
  <MyComp />
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/MyComp.vue", child_source);
    let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

    let position = Position {
        line: 1,
        character: 7,
    };

    set_type_hover_at_vue_position(
        server,
        &provider,
        &app_uri,
        position,
        "(alias) import MyComp\nimport MyComp",
    );

    let text = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("hover request should succeed"),
    );

    assert!(
        text.contains("Props:"),
        "hover should show props, got: {text}"
    );
    assert!(
        text.contains("foo"),
        "hover should include foo, got: {text}"
    );
    assert!(
        text.contains("bar"),
        "hover should include bar, got: {text}"
    );
    assert!(
        text.contains("Emits:"),
        "hover should show emits, got: {text}"
    );
    assert!(
        text.contains("custom"),
        "hover should include custom emit, got: {text}"
    );
    assert!(
        !text.contains("(alias) import MyComp"),
        "hover must not prefer import alias hover, got: {text}"
    );
    assert!(
        !text.contains("DefineComponent<{}, {}>"),
        "hover must not degrade to fallback component shell, got: {text}"
    );
}

#[tokio::test]
async fn hover_prefers_child_component_summary_over_barrel_import_alias_on_vue_import_binding() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
defineProps<{ show?: boolean; zIndex?: number }>()
</script>
<template><div /></template>
"#;
    let barrel_uri: Uri = "file:///workspace/src/components/index.ts"
        .parse()
        .expect("valid barrel uri");
    let _ = server.documents.did_open(&TextDocumentItem {
        uri: barrel_uri,
        language_id: "typescript".to_string(),
        version: 1,
        text: "export { default as Overlay } from './Overlay.vue'\n".to_string(),
    });
    let _child_uri = open_test_vue(
        server,
        "/workspace/src/components/Overlay.vue",
        child_source,
    );
    let app_uri = open_test_vue(
        server,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
import { Overlay } from './components'
</script>

<template>
  <Overlay />
</template>
"#,
    );

    let position = Position {
        line: 1,
        character: 9,
    };

    set_type_hover_at_vue_position(
        server,
        &provider,
        &app_uri,
        position,
        "(const) const Overlay: __OmitNew<DefineComponent<{}, {}>>",
    );

    let text = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("hover request should succeed"),
    );

    assert!(
        text.contains("Props:"),
        "hover should show props for barrel-imported components, got: {text}"
    );
    assert!(
        text.contains("show"),
        "hover should include show prop, got: {text}"
    );
    assert!(
        text.contains("zIndex"),
        "hover should include zIndex prop, got: {text}"
    );
    assert!(
        !text.contains("DefineComponent<{}, {}>"),
        "hover must not degrade to the raw type provider shell, got: {text}"
    );
}

#[tokio::test]
async fn hover_rewrites_component_event_attr_to_vue_syntax() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
const emit = defineEmits<{ custom: [payload: string] }>()
</script>
<template><div /></template>
"#;
    let app_source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
function handleCustom(payload: string) {
  console.log(payload)
}
</script>

<template>
  <MyComp @custom="handleCustom($event)" />
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/MyComp.vue", child_source);
    let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

    let position = Position {
        line: 8,
        character: 11,
    };

    set_type_hover_at_vue_position(
        server,
        &provider,
        &app_uri,
        position,
        "(property) onCustom: (payload: string) => void",
    );

    let text = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("hover request should succeed"),
    );

    assert!(
        text.contains("@custom"),
        "hover should use Vue event syntax, got: {text}"
    );
    assert!(
        text.contains("payload"),
        "hover should include payload label, got: {text}"
    );
    assert!(
        text.contains("string"),
        "hover should include payload type, got: {text}"
    );
    assert!(
        !text.contains("onCustom"),
        "hover must not expose TSX on* naming, got: {text}"
    );
    assert!(
        !text.contains(": any"),
        "hover must not degrade to any, got: {text}"
    );
}

#[tokio::test]
async fn hover_rewrites_prop_backed_event_attr_to_vue_syntax() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver(server);

    let child_source = r#"<script setup lang="ts">
defineProps<{ label: string; onAlert?: (payload: string) => void }>()
</script>
<template><button>{{ label }}</button></template>
"#;
    let app_source = r#"<script setup lang="ts">
import OnEventPropComp from './OnEventPropComp.vue'
function handleCustom(payload: string) {
  console.log(payload)
}
</script>

<template>
  <OnEventPropComp label="go" @alert="handleCustom" />
</template>
"#;

    let _child_uri = open_test_vue(server, "/workspace/src/OnEventPropComp.vue", child_source);
    let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

    let position = Position {
        line: 8,
        character: 29,
    };

    // Structured seeding: the provider's display signature is the PLAIN
    // quick-info line; the rendered blob keeps the fence (what a real
    // tsserver-family producer returns).
    set_structured_hover_at_vue_position(
        server,
        &provider,
        &app_uri,
        position,
        crate::type_provider::protocol::HoverInfo {
            contents: "```typescript\n(property) onAlert?: (payload: string) => void\n```"
                .to_string(),
            display_signature: Some(crate::type_provider::mock::test_display_signature(
                "(property) onAlert?: (payload: string) => void",
            )),
            ..Default::default()
        },
    );

    let text = hover_text(
        server
            .hover(hover_params(&app_uri, position))
            .await
            .expect("hover request should succeed"),
    );

    assert!(
        text.contains("@alert"),
        "hover should use Vue event syntax, got: {text}"
    );
    assert!(
        text.contains("payload"),
        "hover should include payload label, got: {text}"
    );
    assert!(
        text.contains("string"),
        "hover should include payload type, got: {text}"
    );
    assert!(
        !text.contains("onAlert"),
        "hover must not expose TSX on* naming, got: {text}"
    );
}

/// A provider re-sync landing a FRESH surface generation while a hover request
/// is awaiting the provider must cause the provider contribution to be DROPPED
/// (fail closed): the response was produced against a surface that no longer
/// matches, and mapping it through the fresh state would be wrong, not stale.
#[tokio::test(flavor = "multi_thread")]
async fn hover_drops_provider_result_when_surface_regenerates_mid_request() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();

    let position = find_document_position(server, &uri, "{{ msg", 3);
    set_type_hover_at_vue_position(server, &provider, &uri, position, "PROVIDER_HOVER_SENTINEL");

    let ide_path = server.active_ide_path_for_uri(&uri).expect("live IDE path");
    let store = server.documents.provider_surfaces().clone();
    let raced_path = ide_path.clone();
    provider.set_on_query(
        &ide_path,
        Box::new(move || {
            // A concurrent re-sync lands a NEW generation with drifted content
            // between the handler's capture and its merge of the response.
            store.record(
                crate::provider_surface_store::RecordSurface::carrier_legacy(
                    crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
                    raced_path,
                    "/workspace/src/App.vue".to_string(),
                    Arc::from("// drifted ide content"),
                    None,
                    Arc::from("// drifted carrier source"),
                ),
            );
        }),
    );

    let text = hover_text(
        server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        !text.contains("PROVIDER_HOVER_SENTINEL"),
        "a provider hover produced against a superseded surface generation must be \
         DROPPED, got: {text}"
    );
}

/// A surface that changes and changes back while a hover awaits the provider
/// ends byte- and map-identical to the one the query was issued against, but
/// the provider may have answered the intermediate surface: the bracket follows
/// the content epoch, not the bytes, so the provider contribution is dropped.
#[tokio::test(flavor = "multi_thread")]
async fn hover_drops_provider_result_when_surface_changes_and_changes_back_mid_request() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();

    let position = find_document_position(server, &uri, "{{ msg", 3);
    set_type_hover_at_vue_position(server, &provider, &uri, position, "PROVIDER_HOVER_SENTINEL");

    let ide_path = server.active_ide_path_for_uri(&uri).expect("live IDE path");
    let raced_server = server.clone();
    let raced_uri = uri.clone();
    provider.set_on_query(
        &ide_path,
        Box::new(move || {
            let original = Arc::clone(
                &raced_server
                    .test_current_ide_surface(&raced_uri)
                    .provider_content,
            );
            raced_server.test_record_ide_surface(
                &raced_uri,
                Some(Arc::from(format!("{original}\n// intermediate"))),
                None,
            );
            raced_server.test_record_ide_surface(&raced_uri, Some(original), None);
        }),
    );

    let text = hover_text(
        server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        !text.contains("PROVIDER_HOVER_SENTINEL"),
        "a provider hover that may describe the intermediate surface must be DROPPED, got: {text}"
    );
}

/// A surface retirement (the `did_close` path forgetting the provider surface)
/// while a hover request is awaiting the provider must fail closed: no provider
/// contribution, no panic.
#[tokio::test(flavor = "multi_thread")]
async fn hover_fails_closed_when_surface_retired_mid_request() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();

    let position = find_document_position(server, &uri, "{{ msg", 3);
    set_type_hover_at_vue_position(server, &provider, &uri, position, "PROVIDER_HOVER_SENTINEL");

    let ide_path = server.active_ide_path_for_uri(&uri).expect("live IDE path");
    let store = server.documents.provider_surfaces().clone();
    let raced_path = ide_path.clone();
    provider.set_on_query(
        &ide_path,
        Box::new(move || {
            // The surface is retired mid-request (a racing close began); the
            // close is not yet confirmed, so the token is deliberately kept.
            let _token = store.forget(&raced_path);
        }),
    );

    let text = hover_text(
        server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        !text.contains("PROVIDER_HOVER_SENTINEL"),
        "a provider hover racing a surface retirement must be DROPPED, got: {text}"
    );
}

/// P1-02 (mock): perturbing `HoverInfo.contents` — the NEUTRAL protocol's
/// rendered blob — changes NOTHING in the `getBindingTypes` response.
///
/// SCOPE (ruling Q2, recorded verbatim): this guarantee is scoped to the
/// neutral protocol's rendered blob. Perturbing the tsgo ENGINE's own hover
/// TEXT does change `display_signature` on that engine — the engine text is
/// the producer's input, and that is irreducible without a second provider
/// round trip, which is forbidden (F-03). Engine-text changes are therefore
/// OUT of P1-02's claim; protocol-blob changes are IN.
#[tokio::test(flavor = "multi_thread")]
async fn binding_types_ignore_perturbed_hover_markdown() {
    // Three perturbations of the rendered blob against ONE fixed structured
    // signature: empty; non-markdown garbage; a well-formed fence naming a
    // DIFFERENT binding with a different type. Byte-equal responses required.
    let perturbations = [
        "",
        "@@@ not markdown @@@",
        "```typescript\nother: PERTURBED_OTHER_TYPE\n```",
    ];

    let mut responses: Vec<String> = Vec::new();
    for contents in perturbations {
        let (service, provider, uri) = make_request_surface_carrier().await;
        let server = service.inner();

        let decl_pos = find_document_position(server, &uri, "const msg", 6);
        set_structured_hover_at_vue_position(
            server,
            &provider,
            &uri,
            decl_pos,
            crate::type_provider::protocol::HoverInfo {
                contents: contents.to_string(),
                display_signature: Some(crate::type_provider::mock::test_display_signature(
                    "const msg: string",
                )),
                ..Default::default()
            },
        );

        let value = server
            .get_binding_types(crate::server::protocol_types::GetAnalysisParams {
                uri: uri.as_str().to_string(),
            })
            .await
            .expect("getBindingTypes request should succeed");
        assert_eq!(
            value.get("msg"),
            Some(&serde_json::json!({ "displaySignature": "const msg: string" })),
            "perturbation {contents:?} must not affect the wire value, got: {value}"
        );
        responses.push(serde_json::to_string(&value).expect("response serializes"));
    }

    // BYTE-EQUAL across all three perturbations — not merely non-null.
    assert_eq!(
        responses[0], responses[1],
        "empty vs garbage contents must serialize byte-equal"
    );
    assert_eq!(
        responses[1], responses[2],
        "garbage vs wrong-binding-fence contents must serialize byte-equal"
    );
}

/// P1-04 (mock): a provider hover arriving AFTER its captured surface is
/// superseded still yields `null` — the post-await, pre-use
/// `provider_context_still_valid` gate stays exactly where it is.
#[tokio::test(flavor = "multi_thread")]
async fn binding_types_drop_hover_from_superseded_surface() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();

    let decl_pos = find_document_position(server, &uri, "const msg", 6);
    set_structured_hover_at_vue_position(
        server,
        &provider,
        &uri,
        decl_pos,
        crate::type_provider::protocol::HoverInfo {
            contents: "```typescript\nconst msg: \"hello\"\n```".to_string(),
            display_signature: Some(crate::type_provider::mock::test_display_signature(
                "const msg: \"hello\"",
            )),
            ..Default::default()
        },
    );

    // Mid-request seam: a concurrent re-sync lands a NEW surface generation
    // with drifted content between the handler's capture and its use of the
    // provider response.
    let ide_path = server.active_ide_path_for_uri(&uri).expect("live IDE path");
    let store = server.documents.provider_surfaces().clone();
    let raced_path = ide_path.clone();
    provider.set_on_query(
        &ide_path,
        Box::new(move || {
            store.record(
                crate::provider_surface_store::RecordSurface::carrier_legacy(
                    crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
                    raced_path,
                    "/workspace/src/App.vue".to_string(),
                    Arc::from("// drifted ide content"),
                    None,
                    Arc::from("// drifted carrier source"),
                ),
            );
        }),
    );

    let value = server
        .get_binding_types(crate::server::protocol_types::GetAnalysisParams {
            uri: uri.as_str().to_string(),
        })
        .await
        .expect("getBindingTypes request should succeed");

    assert_eq!(
        value.get("msg"),
        Some(&serde_json::Value::Null),
        "a hover produced against a superseded surface must fail closed to null, got: {value}"
    );
}

/// BOOTSTRAP-DRAIN reproduction: the editor opens a carrier during bootstrap so
/// the file is queued (never interactively synced); the background drain then
/// direct-opens its IDE surface (tsgo). The drain MUST record the `CarrierIde`
/// surface it delivered to the provider — without the record, the interactive
/// request-surface capture misses and EVERY provider-backed feature silently
/// drops its provider contribution until the next `did_change`.
#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_drained_carrier_records_surface_and_serves_provider_hover() {
    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    let service = make_hover_test_service_tsgo(type_provider);
    let server = service.inner();
    install_test_resolver(server);
    let uri = open_test_vue(server, "/workspace/src/App.vue", REQUEST_SURFACE_APP);

    // Opened during bootstrap: queued for the drain, no interactive sync ran.
    server
        .pending_snapshot_provider_sync
        .insert("/workspace/src/App.vue".to_string());
    drain_pending_snapshot_provider_sync(
        server.project_sync.as_ref(),
        &server.documents,
        &server.vfs_workspace,
        &server.provider_sync_states,
        &server.pending_snapshot_provider_sync,
        true,
        None,
        None,
        &crate::external_ts::CarrierTransactionCoordinator::new(),
    )
    .await;

    let ide_path = server
        .active_ide_path_for_uri(&uri)
        .expect("the drain must commit a live IDE path for the queued carrier");
    let snapshot = server
        .documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("the drain's direct IDE open must record a CarrierIde surface");
    assert_eq!(
        snapshot.kind,
        crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
        "the drain-recorded surface must carry the CarrierIde role"
    );
    let delivered = server
        .project_sync
        .as_ref()
        .and_then(|sync| sync.synced_tsx_content(&ide_path))
        .expect("the drain records the projected provider bytes");
    assert_eq!(
        snapshot.provider_content.as_ref(),
        delivered.as_ref(),
        "the drain-recorded surface must pin the EXACT bytes delivered to the provider"
    );
    assert!(delivered.contains("from \"./App.vue.tsx.__verter_types\""));
    assert!(!delivered.contains("from \"@verter/types\""));

    // End-to-end: hover captures the drain-recorded surface and serves the
    // provider contribution (no silent drop before the next did_change).
    let position = find_document_position(server, &uri, "{{ msg", 3);
    set_type_hover_at_vue_position(server, &provider, &uri, position, "const msg: string");
    let text = hover_text(
        server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        text.contains("const msg: string"),
        "a drain-synced carrier must serve the provider hover, got: {text}"
    );
}

/// The tsserver PUBLISH path (the store-membership route, distinct from the
/// tsgo direct-open the sibling tests model): the carrier-sync gateway records
/// both companion surfaces at publish time (`record_and_version_carrier_
/// companions` in `external_ts/carrier_sync.rs`) and commits a `Published`
/// store-resident provider state. The interactive request-surface capture must
/// resolve THAT surface — a key mismatch between the recorded companion path
/// and `active_ide_path_for_uri`, a missing `ide_background_loaded` flag, or a
/// carrier-source byte-gate miss here would silently drop EVERY provider
/// result on the live tsserver engine. A genuinely torn surface (an edit after
/// the publish) must still fail closed.
#[tokio::test(flavor = "multi_thread")]
async fn tsserver_published_carrier_surface_is_captured_and_serves_hover() {
    let root = unique_server_ws_root("reqsurf_pub");
    let tsconfig = format!("{root}/tsconfig.json");

    let provider = Arc::new(MockTypeProvider::new());
    let type_provider: Arc<dyn TypeProvider> = provider.clone();
    // Tsserver-kind service: the server constructs the carrier-publish
    // coordinator, so the gateway takes the PUBLISH (membership) route.
    let service = make_hover_test_service(type_provider);
    let server = service.inner();
    install_test_resolver_for_root(server, &root, Some(&tsconfig));

    let uri = open_test_vue(server, &format!("{root}/src/App.vue"), REQUEST_SURFACE_APP);
    server.sync_ide_to_provider(&uri).await;

    // The PUBLISH route ran (membership advertised) — not a direct buffer open.
    let canonical = format!("{root}/src/App.vue");
    let advertised = server
        .membership_ledger()
        .expect("tsserver kind constructs the coordinator")
        .is_advertised(&crate::external_ts::CanonicalSource::from(
            canonical.as_str(),
        ));
    assert!(
        advertised,
        "the tsserver publish must advertise the carrier's store membership"
    );

    // CAPTURE the published surface (no over-drop): the companion recorded at
    // publish time is keyed exactly at the committed live IDE path.
    let ide_path = server
        .active_ide_path_for_uri(&uri)
        .expect("the Published commit must mark the IDE path store-resident");
    let snapshot = server.capture_provider_request_surface(&uri).expect(
        "a published-and-current tsserver carrier surface must be CAPTURED — \
         dropping it silently drops every provider result on the live engine",
    );
    assert_eq!(
        snapshot.kind,
        crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
        "the captured surface must be the publish-recorded CarrierIde companion"
    );
    assert_eq!(
        snapshot.stamp.provider_path.as_ref(),
        ide_path.as_str(),
        "the recorded companion key must equal the committed live IDE path"
    );

    // And it SERVES end-to-end (hover through the captured surface).
    let position = find_document_position(server, &uri, "{{ msg", 3);
    set_type_hover_at_vue_position(server, &provider, &uri, position, "const msg: string");
    let text = hover_text(
        server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        text.contains("const msg: string"),
        "a published tsserver carrier must serve the provider hover, got: {text}"
    );

    // A genuinely TORN surface still drops: an edit lands AFTER the publish
    // (the store still serves the old surface) — capture must fail closed.
    let _ = server.documents.did_change(
        &uri,
        2,
        "<script setup lang=\"ts\">\nconst msg = 'edited'\n</script>\n\
         <template><div>{{ msg }}</div></template>\n",
    );
    assert!(
        server.capture_provider_request_surface(&uri).is_none(),
        "an edit after the publish must fail the capture closed — the provider \
         still holds the pre-edit surface"
    );
}

/// With a STABLE captured surface (no concurrent mutation) the provider
/// contribution IS served and mapped — guards against an over-eager
/// fail-closed gate dropping healthy results.
#[tokio::test(flavor = "multi_thread")]
async fn hover_serves_provider_result_from_stable_captured_surface() {
    let (service, provider, uri) = make_request_surface_carrier().await;
    let server = service.inner();

    let position = find_document_position(server, &uri, "{{ msg", 3);
    set_type_hover_at_vue_position(server, &provider, &uri, position, "const msg: string");

    let text = hover_text(
        server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        text.contains("const msg: string"),
        "a stable captured surface must serve the provider hover, got: {text}"
    );
}

#[tokio::test]
async fn contract_builtin_directive_names_hover_shows_documentation() {
    // Every built-in with a doc-table entry hovers on its NAME token — including
    // `v-pre`, whose directive fact the parser records even though the subtree
    // stays uncompiled.
    for (needle, expected_fragments) in [
        ("v-if", ["v-if", "conditionally"]),
        ("v-else-if", ["v-else-if", "falsy"]),
        ("v-else", ["v-else", "falsy"]),
        ("v-show", ["v-show", "display"]),
        ("v-for", ["v-for", "list"]),
        ("v-html", ["v-html", "HTML"]),
        ("v-text", ["v-text", "text"]),
        ("v-pre", ["v-pre", "compilation"]),
        ("v-once", ["v-once", "once"]),
        ("v-memo", ["v-memo", "memo"]),
        ("v-cloak", ["v-cloak", "cloak"]),
    ] {
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[("src/App.vue", "vue", D6_PARENT_SOURCE)]).await;
        let server = service.inner();
        let uri = workspace_uri(&workspace_id, "src/App.vue");
        let position = find_document_position(server, &uri, needle, 1);
        let hover = server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request should succeed");
        let text = hover.map(|h| hover_text(Some(h)));
        drain_handle.abort();
        drop(service);
        let text = text.unwrap_or_else(|| panic!("{needle} name must produce a doc hover"));
        for fragment in expected_fragments {
            assert!(
                text.to_lowercase().contains(&fragment.to_lowercase()),
                "{needle} doc hover must mention {fragment}, got: {text}"
            );
        }
    }
}

#[tokio::test]
async fn contract_custom_directive_name_hover_typed_and_navigates_to_declaration() {
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", D6_PARENT_SOURCE)]).await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, "src/App.vue");
    let position = find_document_position(server, &uri, "v-my-thing", 2);

    let text = hover_text(
        server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        text.contains("vMyThing"),
        "custom directive hover must name the resolved binding vMyThing, got: {text}"
    );

    let response = server
        .goto_definition(goto_definition_params(&uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("custom directive name must navigate to its declaration");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == uri)
        .expect("definition must land in the same file");
    assert_eq!(
        target.range.start.line,
        line_for_snippet(D6_PARENT_SOURCE, "const vMyThing"),
        "custom directive name must navigate to the authored vMyThing declaration"
    );
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_imported_custom_directive_name_hover_and_definition() {
    let (_temp, service, drain_handle, _provider, workspace_id) = make_definition_test_server(&[
        ("src/App.vue", "vue", D6_PARENT_SOURCE),
        ("src/focus.ts", "typescript", D6_FOCUS_SOURCE),
    ])
    .await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, "src/App.vue");
    let focus_uri = workspace_uri(&workspace_id, "src/focus.ts");
    let position = find_document_position(server, &uri, "v-focus", 2);

    let text = hover_text(
        server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        text.contains("vFocus"),
        "imported custom directive hover must name vFocus, got: {text}"
    );

    let response = server
        .goto_definition(goto_definition_params(&uri, position))
        .await
        .expect("goto definition should succeed")
        .expect("imported custom directive must navigate");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == focus_uri)
        .expect("definition must land in focus.ts");
    assert_eq!(
        target.range.start.line,
        line_for_snippet(D6_FOCUS_SOURCE, "export const vFocus"),
        "imported custom directive must navigate to the vFocus export"
    );
    drain_handle.abort();
    drop(service);
}

#[tokio::test]
async fn contract_svelte_directive_keywords_hover_shows_documentation() {
    for (needle, shift, expected) in [
        ("use:highlight", 1u32, "action"),
        ("transition:fade", 2u32, "transition"),
        ("in:fly", 1u32, "transition"),
        ("out:fade", 1u32, "transition"),
        ("animate:flip", 1u32, "animation"),
    ] {
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[("src/App.svelte", "svelte", D6_SVELTE_SOURCE)]).await;
        let server = service.inner();
        let uri = workspace_uri(&workspace_id, "src/App.svelte");
        let mut position = find_document_position(server, &uri, needle, 0);
        position.character += shift;
        let hover = server
            .hover(hover_params(&uri, position))
            .await
            .expect("hover request should succeed");
        let text = hover.map(|h| hover_text(Some(h)));
        drain_handle.abort();
        drop(service);
        let text = text.unwrap_or_else(|| panic!("{needle} keyword must produce a doc hover"));
        assert!(
            text.to_lowercase().contains(expected),
            "{needle} keyword doc hover must mention {expected}, got: {text}"
        );
    }
}

#[tokio::test]
async fn contract_svelte_transition_family_names_hover_typed_without_shim_leak() {
    // The authored function name maps to the projected CALL's authored-bytes
    // identifier — never to the synthetic `__verter_transition` wrapper — so
    // the provider answers with the real function quickinfo.
    for (needle, shift) in [
        ("transition:fade", 12u32),
        ("in:fly", 4u32),
        ("out:fade", 5u32),
        ("animate:flip", 9u32),
    ] {
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_definition_test_server(&[("src/App.svelte", "svelte", D6_SVELTE_SOURCE)]).await;
        let server = service.inner();
        let uri = workspace_uri(&workspace_id, "src/App.svelte");
        let mut position = find_document_position(server, &uri, needle, 0);
        position.character += shift;
        let ctx = synced_type_provider_context(server, &uri).await;
        let tsx_offset = merge::carrier_position_to_tsx_offset_validated(
            &position,
            &ctx.carrier_line_index,
            &ctx.mapper,
            &ctx.tsx_line_index,
        )
        .unwrap_or_else(|| panic!("{needle} name must map into the projected TSX"));
        // Anti-shim: the projected token at the mapped offset must be the
        // authored function name, never the synthetic wrapper.
        let token_end = (tsx_offset as usize + 20).min(ctx.tsx_content.len());
        let projected = &ctx.tsx_content[tsx_offset as usize..token_end];
        assert!(
            !projected.starts_with("__verter_"),
            "{needle} mapped into a synthetic shim: {projected:?}"
        );
        provider.set_hover(
            &ctx.tsx_path,
            tsx_offset,
            Some(HoverInfo {
                contents: "function fade(config: FadeParams): TransitionConfig".to_string(),
                display_signature: Some(crate::type_provider::mock::test_display_signature(
                    "function fade(config: FadeParams): TransitionConfig",
                )),
                ..Default::default()
            }),
        );
        let text = hover_text(
            server
                .hover(hover_params(&uri, position))
                .await
                .expect("hover request should succeed"),
        );
        assert!(
            text.contains("TransitionConfig"),
            "{needle} name hover must be the provider's typed function hover, got: {text}"
        );
        assert!(
            !text.contains("__verter_"),
            "{needle} name hover must not leak shims, got: {text}"
        );
        drain_handle.abort();
        drop(service);
    }
}

/// The depth-ignorant SFC scanner closes the real template block at the
/// first nested `</template>`, leaving later template positions in phantom
/// blocks the SFC attr table cannot serve. Verter-native template hovers
/// must still fire from the typed element tree (D6 — built-in docs AND
/// custom directives in the scanner dead zones).
#[tokio::test]
async fn contract_directive_hovers_survive_carrier_structure_dead_zones() {
    let source = "<script setup lang=\"ts\">\nfunction vMyThing(el: HTMLElement, binding: { value: string }) {\n  void el;\n  void binding;\n}\nconst label = \"x\";\n</script>\n<template>\n  <div>\n    <template #unused=\"{ row }\">\n      <span>{{ row }}</span>\n    </template>\n    <div v-if=\"label\">\n      <span>{{ label }}</span>\n    </div>\n    <b v-my-thing=\"label\">directive</b>\n  </div>\n</template>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", source)]).await;
    let server = service.inner();
    let uri = workspace_uri(&workspace_id, "src/App.vue");

    let if_position = find_document_position(server, &uri, "v-if", 1);
    let text = hover_text(
        server
            .hover(hover_params(&uri, if_position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        text.contains("v-if") && text.contains("onditionally"),
        "built-in doc hover must fire after a nested template closes the scanned block, got: {text}"
    );

    let custom_position = find_document_position(server, &uri, "v-my-thing", 2);
    let text = hover_text(
        server
            .hover(hover_params(&uri, custom_position))
            .await
            .expect("hover request should succeed"),
    );
    assert!(
        text.contains("vMyThing"),
        "custom directive hover must fire in the phantom opening-tag zone, got: {text}"
    );

    let response = server
        .goto_definition(goto_definition_params(&uri, custom_position))
        .await
        .expect("goto definition should succeed")
        .expect("custom directive navigation must survive the dead zone");
    let locations = definition_locations(response);
    let target = locations
        .iter()
        .find(|loc| loc.uri == uri)
        .expect("definition must land in the same file");
    assert_eq!(
        target.range.start.line,
        line_for_snippet(source, "function vMyThing"),
        "custom directive definition must navigate to the authored declaration"
    );

    // The exact editor caret — token start + 1, ON the kebab dash, where no
    // identifier word exists — must resolve identically (the template
    // definition sections below the word guard never run there).
    let mut dash_position = find_document_position(server, &uri, "v-my-thing", 0);
    dash_position.character += 1;
    let response = server
        .goto_definition(goto_definition_params(&uri, dash_position))
        .await
        .expect("goto definition should succeed")
        .expect("custom directive navigation must resolve from the dash caret");
    let locations = definition_locations(response);
    assert!(
        locations.iter().any(|loc| loc.uri == uri),
        "the dash caret must land the same-file declaration: {locations:?}"
    );

    drain_handle.abort();
    drop(service);
}

// =====================================================================
// B4: typed v-bind() hover + completion (declaration-position provider)
// =====================================================================

/// Hover on a style `v-bind(width)` token shows the provider's TypeScript
/// type for the binding, queried at the DECLARATION position (the style
/// token has no TSX projection).
#[tokio::test]
async fn v_bind_hover_shows_provider_type_from_declaration() {
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst width = ref(10)\n</script>\n<template><div>x</div></template>\n<style scoped>\n.x { width: v-bind(width); }\n</style>\n";
    let (_temp, service, drain_handle, provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", source)]).await;

    let uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();

    // Provider quickinfo at the DECLARATION position.
    let decl_pos = find_document_position(server, &uri, "const width", 6);
    set_type_hover_at_vue_position(
        server,
        &provider,
        &uri,
        decl_pos,
        "const width: Ref<number>",
    );

    // Hover ON the v-bind expression token in the style block.
    let vbind_pos = find_document_position(server, &uri, "v-bind(width)", 8);
    let hover = server
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: vbind_pos,
            },
            work_done_progress_params: Default::default(),
        })
        .await
        .expect("hover request should succeed")
        .expect("v-bind token must hover");

    let contents = match hover.contents {
        HoverContents::Markup(m) => m.value,
        other => panic!("expected markup, got {other:?}"),
    };
    assert!(
        contents.contains("v-bind(width)"),
        "hover names the v-bind: {contents}"
    );
    assert!(
        contents.contains("Ref<number>"),
        "hover carries the provider type from the declaration: {contents}"
    );

    drain_handle.abort();
    drop(service);
}

/// A provider await must not let a captured native fallback escape after
/// document, dependency or ownership invalidation.
#[tokio::test]
async fn v_bind_hover_refuses_a_native_fallback_after_its_basis_moves() {
    let source = "<script setup lang=\"ts\">\nconst width = 10\n</script>\n<template><div>x</div></template>\n<style scoped>\n.x { width: v-bind(width); }\n</style>\n";
    for change in ["edit", "reopen", "workspace"] {
        let provider = Arc::new(MockTypeProvider::new());
        let service = make_hover_test_service_tsgo(provider.clone());
        let server = service.inner();
        install_test_resolver(server);
        let uri = open_test_vue(server, "/workspace/src/App.vue", source);
        server.ensure_current_file_synced(&uri).await;
        let position = find_document_position(server, &uri, "v-bind(width)", 8);
        assert!(server
            .hover(hover_params(&uri, position))
            .await
            .unwrap()
            .is_some());

        let documents = Arc::clone(&server.documents);
        let raced_server = server.clone();
        let raced_uri = uri.clone();
        let path = server.active_ide_path_for_uri(&uri).unwrap();
        provider.set_on_query(
            &path,
            Box::new(move || match change {
                "reopen" => {
                    documents.did_close(&raced_uri);
                    documents.did_open(&TextDocumentItem {
                        uri: raced_uri,
                        language_id: "vue".into(),
                        version: 1,
                        text: source.into(),
                    });
                }
                "edit" => {
                    documents.did_change(&raced_uri, 2, &source.replace("width", "height"));
                }
                "workspace" => install_test_resolver(&raced_server),
                _ => unreachable!(),
            }),
        );
        let result = server.hover(hover_params(&uri, position)).await;
        assert!(
            matches!(result, Err(ref error) if error.code == tower_lsp_server::jsonrpc::ErrorCode::ContentModified),
            "a superseded native fallback must return the typed stale-request outcome ({change})"
        );
    }
}

/// Without a provider answer the v-bind hover fails closed to the native
/// description — never a fabricated type.
#[tokio::test]
async fn v_bind_hover_without_provider_answer_falls_back_native() {
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst width = ref(10)\n</script>\n<template><div>x</div></template>\n<style scoped>\n.x { width: v-bind(width); }\n</style>\n";
    let (_temp, service, drain_handle, _provider, workspace_id) =
        make_definition_test_server(&[("src/App.vue", "vue", source)]).await;

    let uri = workspace_uri(&workspace_id, "src/App.vue");
    let server = service.inner();
    let vbind_pos = find_document_position(server, &uri, "v-bind(width)", 8);
    let hover = server
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: vbind_pos,
            },
            work_done_progress_params: Default::default(),
        })
        .await
        .expect("hover request should succeed")
        .expect("native v-bind hover still serves");

    let contents = match hover.contents {
        HoverContents::Markup(m) => m.value,
        other => panic!("expected markup, got {other:?}"),
    };
    assert!(contents.contains("v-bind(width)"), "{contents}");
    assert!(
        !contents.contains("Ref<"),
        "no fabricated provider type: {contents}"
    );

    drain_handle.abort();
    drop(service);
}

/// The shipped hover path waits for the provider until the LSP client cancels.
/// Engine death is handled by the provider lifecycle, not by a latency guess.
#[tokio::test]
async fn shipped_hover_has_no_feature_latency_deadline() {
    let provider = Arc::new(MockTypeProvider::new());
    provider.hang_hover();
    let (service, host) = wedged_provider_server(Arc::clone(&provider), |_| {});
    let server = service.inner();

    let budget = host.config().lsp_method_timeouts.request_deadlines.hover;
    assert!(budget.is_zero(), "production hover must be unbounded");

    let source = "<script setup lang=\"ts\">\nconst count = 1\n</script>\n<template><div>{{ count }}</div></template>\n";
    let uri = open_test_vue(server, "/workspace/src/App.vue", source);
    let position = find_document_position(server, &uri, "{{ count", 3);

    let outcome = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        super::super::nav_features_audit::handle_hover_with_audit(
            server,
            hover_params(&uri, position),
        ),
    )
    .await;
    assert!(
        outcome.is_err(),
        "hover returned before client cancellation, so a hidden latency deadline remains"
    );
    assert!(
        provider
            .calls()
            .iter()
            .any(|c| matches!(c, MockCall::GetHover { .. })),
        "the handler must have reached the wedged get_hover, else the deadline is untested"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hover_joins_a_coordinator_parked_after_compile_without_an_extra_ide_application() {
    let (service, provider, uri) = lane_interleaving_fixture("HoverJoinsCompile").await;
    let server = service.inner();
    let id = crate::documents::uri_to_canonical_id(&uri);
    edit_interleaving_document(server, &uri, 2, "world");
    let deps = lane_interleaving_deps(server);
    let position = find_document_position(server, &uri, "{{ msg", 3);
    let (arrived, release) = crate::sync_coordinator::test_hooks::block_after_ide_compile(&id);
    let (waiting, resume) = server.pause_next_ide_sync_after_lease(&id);
    let request = async {
        arrived.notified().await;
        let control = async {
            waiting.notified().await;
            let writes = ide_application_count(&provider, &id);
            resume.notify_one();
            release.notify_one();
            writes
        };
        tokio::join!(server.hover(hover_params(&uri, position)), control)
    };
    let (settled, (hover, before)) = tokio::join!(
        crate::sync_coordinator::synchronize_document_for_test(&deps, &id, uri.as_str()),
        request
    );
    if !settled {
        assert!(
            server.pending_snapshot_provider_sync.contains(&id),
            "an API leg yielding to the waiting hover stays owed"
        );
    }
    assert_eq!(
        before, 0,
        "the hover reached the held lane before any IDE write"
    );
    assert!(hover_text(hover.unwrap()).contains("const msg: string // provider lane answer"));
    assert!(server.capture_provider_request_surface(&uri).is_some());
    drain_pending_snapshot_provider_sync(
        server.project_sync.as_ref(),
        &server.documents,
        &server.vfs_workspace,
        &server.provider_sync_states,
        &server.pending_snapshot_provider_sync,
        true,
        None,
        server.carrier_publish_coordinator.as_ref(),
        &server.carrier_transaction_coordinator,
    )
    .await;
    assert_eq!(ide_application_count(&provider, &id), 1);
    assert!(
        !server.pending_snapshot_provider_sync.contains(&id),
        "neither commit was superseded"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tick_yields_to_hover_parked_before_write_and_skips_after_its_commit() {
    let (service, provider, uri) = lane_interleaving_fixture("TickJoinsHover").await;
    let server = service.inner();
    let id = crate::documents::uri_to_canonical_id(&uri);
    edit_interleaving_document(server, &uri, 2, "world");
    let deps = lane_interleaving_deps(server);
    let position = find_document_position(server, &uri, "{{ msg", 3);
    let (arrived, release) = server.pause_next_ide_sync_before_provider_write(&id);
    let ticking = async {
        arrived.notified().await;
        let settled =
            crate::sync_coordinator::synchronize_document_for_test(&deps, &id, uri.as_str()).await;
        let writes = ide_application_count(&provider, &id);
        release.notify_one();
        (settled, writes)
    };
    let (hover, (settled, before)) =
        tokio::join!(server.hover(hover_params(&uri, position)), ticking);
    assert!(!settled, "the contended tick requeues instead of waiting");
    assert_eq!(before, 0);
    assert!(hover_text(hover.unwrap()).contains("const msg: string // provider lane answer"));
    assert!(crate::sync_coordinator::synchronize_document_for_test(&deps, &id, uri.as_str()).await);
    assert_eq!(ide_application_count(&provider, &id), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_fences_the_parked_coordinator_and_hover_repairs_the_new_revision() {
    let (service, provider, uri) = lane_interleaving_fixture("EditFencesCompile").await;
    let server = service.inner();
    let id = crate::documents::uri_to_canonical_id(&uri);
    edit_interleaving_document(server, &uri, 2, "world");
    let deps = lane_interleaving_deps(server);
    let position = find_document_position(server, &uri, "{{ msg", 3);
    let (arrived, release) = crate::sync_coordinator::test_hooks::block_after_ide_compile(&id);
    let editing = async {
        arrived.notified().await;
        edit_interleaving_document(server, &uri, 3, "again");
        release.notify_one();
        server.hover(hover_params(&uri, position)).await
    };
    let (settled, hover) = tokio::join!(
        crate::sync_coordinator::synchronize_document_for_test(&deps, &id, uri.as_str()),
        editing
    );
    assert!(
        !settled,
        "the coordinator's obsolete transaction is refused"
    );
    assert!(hover_text(hover.unwrap()).contains("const msg: string // provider lane answer"));
    assert_eq!(ide_application_count(&provider, &id), 1);
    let surface = server.capture_provider_request_surface(&uri).unwrap();
    assert_eq!(
        surface.source_hash,
        crate::provider_surface_store::ContentHash::of(
            &REQUEST_SURFACE_APP.replace("'hello'", "'again'")
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_parked_document_leaves_another_documents_hover_unaffected() {
    let (service, provider, uri) = lane_interleaving_fixture("HeldWhileOtherHovers").await;
    let server = service.inner();
    let other = open_test_vue(
        server,
        "/workspace/src/UnaffectedHover.vue",
        REQUEST_SURFACE_APP,
    );
    server.ensure_current_file_synced(&other).await;
    let position = find_document_position(server, &other, "{{ msg", 3);
    set_type_hover_at_vue_position(
        server,
        &provider,
        &other,
        position,
        "const msg: string // provider lane answer",
    );
    provider.clear_calls();
    let id = crate::documents::uri_to_canonical_id(&uri);
    let other_id = crate::documents::uri_to_canonical_id(&other);
    edit_interleaving_document(server, &uri, 2, "world");
    let deps = lane_interleaving_deps(server);
    let (arrived, release) = crate::sync_coordinator::test_hooks::block_after_ide_compile(&id);
    let requesting = async {
        arrived.notified().await;
        let result = server.hover(hover_params(&other, position)).await;
        let held_writes = ide_application_count(&provider, &id);
        release.notify_one();
        (result, held_writes)
    };
    let (settled, (hover, before)) = tokio::join!(
        crate::sync_coordinator::synchronize_document_for_test(&deps, &id, uri.as_str()),
        requesting
    );
    assert!(settled);
    assert!(hover_text(hover.unwrap()).contains("const msg: string // provider lane answer"));
    assert_eq!(
        before, 0,
        "the other request completed before the holder was released"
    );
    assert_eq!(ide_application_count(&provider, &other_id), 0);
    assert_eq!(ide_application_count(&provider, &id), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_background_api_open_yields_to_hover_and_refuses_a_changed_revision() {
    let (service, provider, uri) = lane_interleaving_fixture("BackgroundApiYields").await;
    let server = service.inner();
    let canonical = crate::documents::uri_to_canonical_id(&uri);
    let path = verter_session_query::resolution::carrier_api_provider_path(&canonical);
    let (arrived, release) = provider.block_open_file(&path);
    let snapshot = server.published_resolver().unwrap();
    let vfs = server.vfs_workspace.read().clone();
    let position = find_document_position(server, &uri, "{{ msg", 3);
    let task = super::super::background_drain::sync_api_to_provider_background_task(
        server.project_sync.clone().unwrap(),
        snapshot,
        vfs,
        Arc::clone(&server.provider_sync_states),
        canonical.clone(),
        false,
        Arc::clone(&server.carrier_transaction_coordinator),
        Arc::clone(&server.pending_snapshot_provider_sync),
        Arc::clone(&server.documents),
    );
    let request = async {
        arrived.notified().await;
        edit_interleaving_document(server, &uri, 2, "world");
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            server.hover(hover_params(&uri, position)),
        )
        .await;
        let before_resume = server
            .documents
            .provider_surfaces()
            .current_snapshot(&path)
            .map(|surface| surface.stamp.clone());
        release.notify_one();
        (result, before_resume)
    };
    let (_, (hover, before_resume)) = tokio::join!(task, request);
    assert!(
        hover.is_ok(),
        "hover must finish before the background API open resumes"
    );
    assert!(
        hover_text(hover.unwrap().unwrap()).contains("const msg: string // provider lane answer")
    );
    assert_eq!(ide_application_count(&provider, &canonical), 1);
    assert_eq!(
        server
            .documents
            .provider_surfaces()
            .current_snapshot(&path)
            .map(|surface| surface.stamp.clone()),
        before_resume,
        "the refused API transaction must leave the hover's current coordinate record untouched"
    );
    assert!(
        !server
            .provider_sync_state_for_source(&canonical)
            .unwrap()
            .api_background_loaded,
        "refused API delivery cannot mark the owed API leg applied"
    );
    assert!(server.pending_snapshot_provider_sync.contains(&canonical));
    assert!(server.capture_provider_request_surface(&uri).is_some());
}
