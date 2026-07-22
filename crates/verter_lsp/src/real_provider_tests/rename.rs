//! Rename tests ported from E2E suite.

use crate::test_harness::real_provider_test;

real_provider_test!(
    rename_single_project,
    fixture = "single-project",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        let _mycomp = session.open_fixture_file("src/MyComp.vue").await;

        // `handle_rename` never starts import-set work: it CAPTURES the
        // DependencyReady receipt minted by the `did_open`-triggered background
        // publication (or joins one in flight), so no test-only sync helper is
        // needed. `wait_until_ready` runs only as a best-effort WARM-UP (give
        // the publication + provider time to settle and index); its result NO
        // LONGER gates the cross-file assertion — a green run always EXECUTES R9.
        let _warm = session.wait_until_ready(&uri, "action.disabled", 7, "disabled").await;

        // R1: prepare rename on `const count = ref(0)` → accepted
        let pos = session.find_position(&uri, "const count = ref(0)", 6);
        let result = session.prepare_rename(&uri, pos).await;
        assert!(result.is_some(), "prepare rename on count should be accepted");

        // R2: prepare rename on `<h1>` → rejected
        let pos = session.find_position(&uri, "<h1>", 1);
        let result = session.prepare_rename(&uri, pos).await;
        assert!(result.is_none(), "prepare rename on HTML tag should be rejected");

        // R3: rename `doubled` to "doubledValue" → ≥2 edits
        let pos = session.find_position(&uri, "const doubled = computed(", 6);
        let edits = session.rename_edits(&uri, pos, "doubledValue").await;
        assert!(edits.is_some(), "rename doubled should return edits");
        let ws_edit = edits.unwrap();
        assert_no_duplicate_edits(&ws_edit);
        let total = count_edits(&ws_edit);
        assert!(total >= 2, "rename doubled should have >= 2 edits, got: {total}");

        // R4: rename `count` to "counter" → ≥5 edits
        let pos = session.find_position(&uri, "const count = ref(0)", 6);
        let edits = session.rename_edits(&uri, pos, "counter").await;
        assert!(edits.is_some(), "rename count should return edits");
        let ws_edit = edits.unwrap();
        assert_no_duplicate_edits(&ws_edit);
        let total = count_edits(&ws_edit);
        assert!(total >= 5, "rename count should have >= 5 edits, got: {total}");

        // R5: rename `increment` to "inc" → ≥3 edits
        let pos = session.find_position(&uri, "function increment()", 9);
        let edits = session.rename_edits(&uri, pos, "inc").await;
        assert!(edits.is_some(), "rename increment should return edits");
        let ws_edit = edits.unwrap();
        assert_no_duplicate_edits(&ws_edit);
        let total = count_edits(&ws_edit);
        assert!(total >= 3, "rename increment should have >= 3 edits, got: {total}");

        // Exact editor-contract shape: local bindings repeated in both script and
        // template produce one mutation per authored occurrence, never one static
        // edit plus a duplicate provider edit for the same range.
        let contract = session
            .open_virtual(
                "src/RenameContract.vue",
                r#"<script setup lang="ts">
import DirectChild from "./MyComp.vue";

interface ContractValue {
  label: string;
  count: number;
}

const typedValue: ContractValue = { label: "typed", count: 1 };
function renderTyped(): string {
  return `${typedValue.label}:${typedValue.count}`;
}
</script>
<template>
  <main>
    <button @click="renderTyped">{{ typedValue.label }}</button>
    <DirectChild :foo="typedValue.label" />
  </main>
</template>
"#,
            )
            .await;
        let script_pos = session.find_position(&contract, "const typedValue", 6);
        let script_edit = session
            .rename_edits(&contract, script_pos, "typedDatum")
            .await
            .expect("script-origin contract rename returns edits");
        assert_no_duplicate_edits(&script_edit);
        assert_eq!(count_edits(&script_edit), 5, "{script_edit:?}");

        let markup_pos = session.find_position(&contract, "{{ typedValue.label }}", 3);
        let markup_edit = session
            .rename_edits(&contract, markup_pos, "typedDatum")
            .await
            .expect("markup-origin contract rename returns edits");
        assert_no_duplicate_edits(&markup_edit);
        assert_eq!(count_edits(&markup_edit), 5, "{markup_edit:?}");

        // R6: prepare rename on v-if directive → rejected
        let pos = session.find_position(&uri, r#"v-if="selectedUser""#, 2);
        let result = session.prepare_rename(&uri, pos).await;
        assert!(result.is_none(), "prepare rename on v-if should be rejected");

        // R7: prepare rename on $event → rejected
        let pos = session.find_position(&uri, "handleInput($event)", 12);
        let result = session.prepare_rename(&uri, pos).await;
        assert!(result.is_none(), "prepare rename on $event should be rejected");

        // R8: rename `handleCustom` to "onCustomEvent" → ≥2 edits
        let pos = session.find_position(&uri, "function handleCustom(", 9);
        let edits = session.rename_edits(&uri, pos, "onCustomEvent").await;
        assert!(edits.is_some(), "rename handleCustom should return edits");
        let ws_edit = edits.unwrap();
        let total = count_edits(&ws_edit);
        assert!(total >= 2, "rename handleCustom should have >= 2 edits, got: {total}");

        // R9: cross-file rename of the `foo` prop must propagate from App.vue's usage into
        // MyComp.vue's `defineProps` type → edits in ≥ 2 files. The DETERMINISTIC gate for the
        // carrier-API rename mapping: `handle_rename`'s own production sync-before-query syncs
        // MyComp's `{carrier}.ts` API surface, the generation-pinned snapshot is captured under
        // the fence, tsserver reports the renamed prop against that surface, and the merge maps
        // it back onto MyComp.vue through the pinned snapshot. No skip path guards this.
        let pos = session.find_position(&uri, r#"foo="literal""#, 0);
        let edits = session.rename_edits(&uri, pos, "fooRenamed").await;
        assert!(edits.is_some(), "cross-file rename foo should return edits");
        let ws_edit = edits.unwrap();
        let file_count = count_files(&ws_edit);
        // BOTH providers: the cross-file Vue-prop rename MUST touch >= 2 files
        // (App.vue usage + MyComp.vue defineProps decl). For tsgo this is the
        // child-declaration leg Verter SYNTHESIZES itself (provider-agnostic) —
        // tsgo's own rename API does not enumerate it across the synthesized
        // `{carrier}.ts` surface, so the synthesis is what closes the parity gap.
        // It fails (not skips) if the child edit is dropped.
        assert!(
            file_count >= 2,
            "cross-file rename should affect >= 2 files, got: {file_count}"
        );
    }
);

// R9 (child CLOSED) — cross-file-rename end-to-end coverage, discriminating the
// generation-pinned snapshot MAPPING.
//
// Open ONLY the parent (App.vue), keep the child (MyComp.vue) CLOSED, invoke
// rename via the PRODUCTION handler (no test-only sync helper), assert the
// workspace edit touches BOTH files, then APPLY the edit and assert the old prop
// text is gone + the renamed prop DECLARATION (with its type) present in BOTH.
//
// WHAT THIS LANE DISCRIMINATES — the generation-pinned snapshot MAPPING (the
// mechanism Block H-rename owns): snapshot capture
// (`capture_current_carrier_api_set`) → `external_ide_context_from_snapshot`
// (provider_surface_store.rs) → `api_surface_range_to_carrier_range`
// (type_provider/merge/position.rs). The prewarm is ACTIVE here, so tsserver
// REPORTS the child rename location; this lane proves the merge maps that
// `{carrier}.ts` location back onto MyComp.vue at the EXACT byte range. If the
// mapping regresses (wrong-range or dropped child edit), the child edit lands at
// the wrong byte range or is missing, so the renamed-declaration assertion
// (`fooRenamed: string`) and/or the original-prop-gone assertion (`!… foo:
// string`) FAIL. The strengthened `fooRenamed: string` check (the prop name WITH
// its declared `: string` type, not a bare `fooRenamed` substring anywhere)
// catches a mis-ranged edit crisply.
//
// WHAT THIS LANE DOES NOT DISCRIMINATE — the closed-child DELIVERY axis.
// Under tsserver, opening the parent EAGERLY prewarms the imported child's
// `{carrier}.ts` PUBLIC-API surface (the `did_open` imported-carrier prewarm),
// so the child API surface is already synced BEFORE the rename regardless of
// the background dependency publication (the prewarm masks that axis). The
// would-be discriminator for that axis is
// `rename_cross_file_prop_child_closed_unprewarmed_tsserver` below, which
// SUPPRESSES the prewarm; it is `#[ignore]`'d on the tsserver program-membership
// gap tracked as Block H-membership. This lane remains broad end-to-end coverage
// (apply + text assertions across both files), and runs for BOTH providers: under
// tsserver the provider's own rename enumerates the child `{carrier}.ts` location,
// while under tsgo the child-declaration leg is the one Verter SYNTHESIZES
// (provider-agnostic) — tsgo's native rename does not enumerate it. Either way the
// child edit maps back onto MyComp.vue through the same generation-pinned snapshot.
real_provider_test!(
    rename_cross_file_prop_child_closed,
    fixture = "single-project",
    async fn run(session) {
        // Open ONLY the parent. The child MyComp.vue is intentionally NOT opened.
        let app = session.open_fixture_file("src/App.vue").await;
        let child_uri = session.workspace_uri("src/MyComp.vue");

        // Best-effort warm-up only (no gating); the production handler syncs.
        let _warm = session.wait_until_ready(&app, "action.disabled", 7, "disabled").await;

        // Rename the `foo` prop usage in App.vue. `<MyComp foo="literal" …>`.
        let pos = session.find_position(&app, r#"foo="literal""#, 0);
        let edits = session.rename_edits(&app, pos, "fooRenamed").await;

        // BOTH providers: BOTH files must be edited (App.vue usage + MyComp.vue
        // decl). For tsgo, the child-declaration leg is the one Verter SYNTHESIZES
        // (provider-agnostic) — its own rename API does not enumerate the child
        // edit across the synthesized `{carrier}.ts` surface. The child MyComp.vue
        // is CLOSED, so this also exercises the closed-carrier snapshot mapping.
        let ws_edit = edits.expect("cross-file rename (child closed) must return edits");
        assert!(
            edit_touches(&ws_edit, &app),
            "rename must edit the parent App.vue: {ws_edit:?}"
        );
        assert!(
            edit_touches(&ws_edit, &child_uri),
            "rename must edit the CLOSED child MyComp.vue carrier: {ws_edit:?}"
        );

        // APPLY the edit on disk-read content and verify text changed in BOTH.
        let app_before = read_file(&app);
        let app_after = apply_edits(&ws_edit, &app, &app_before);
        assert!(
            app_after.contains("fooRenamed") && !app_after.contains(r#"foo="literal""#),
            "App.vue must have the renamed prop applied:\n{app_after}"
        );

        let child_before = read_file(&child_uri);
        // Precondition: the fixture child declares `defineProps<{ foo: string; … }>`.
        // After a CORRECT in-place rename it must read `fooRenamed: string` — the
        // prop DECLARATION with its type, at the exact mapped byte range.
        assert!(
            child_before.contains("foo: string"),
            "fixture precondition: MyComp.vue must declare `foo: string`:\n{child_before}"
        );
        let child_after = apply_edits(&ws_edit, &child_uri, &child_before);
        // STRONG mapping assertion: the renamed prop must appear in its defineProps
        // DECLARATION context — `fooRenamed: string` (name WITH its declared type),
        // not merely the substring `fooRenamed` somewhere. A mis-ranged snapshot
        // mapping would splice `fooRenamed` at the wrong offset (corrupting the
        // declaration or landing it off the `: string` type), failing this check.
        assert!(
            child_after.contains("fooRenamed: string"),
            "MyComp.vue defineProps must declare the renamed prop `fooRenamed: string` \
             (snapshot mapping must land the edit on the prop decl, not a wrong range):\n{child_after}"
        );
        // The ORIGINAL `foo:` prop declaration must be gone (renamed in place) —
        // discriminates a no-op / wrong-range edit that left `foo` intact.
        assert!(
            !child_after.contains("foo: string"),
            "MyComp.vue must no longer declare the original `foo: string` prop:\n{child_after}"
        );
    }
);

// KEBAB × CAMEL cross-file rename + find-references, EXECUTED end-to-end.
// The parent uses the kebab form (`:my-prop`) of the child's camelCase
// declaration (`myProp`). Both legs must be correct at EXACT ranges:
// the child declaration is Verter-synthesized from the DECLARED name (the
// carrier API spells it verbatim — a kebab usage alias trips the
// byte-equality guard), and the initiating parent usage is re-anchored from
// the authored span (the provider's case-mapped range lands a PREFIX of the
// kebab name). The references lane proves the same binding is discoverable
// from the kebab usage across BOTH files (script + template).
real_provider_test!(
    rename_kebab_prop_usage_spans_script_and_template,
    fixture = "single-project",
    async fn run(session) {
        let child = session
            .open_virtual(
                "src/KebabChild.vue",
                "<script setup lang=\"ts\">\ndefineProps<{ myProp: string }>()\n</script>\n",
            )
            .await;
        let parent = session
            .open_virtual(
                "src/KebabParent.vue",
                "<script setup lang=\"ts\">\nimport KebabChild from './KebabChild.vue'\nconst v = 'x'\n</script>\n<template>\n  <KebabChild :my-prop=\"v\" />\n</template>\n",
            )
            .await;
        let _warm = session
            .wait_until_ready(&parent, ":my-prop=", 1, "my-prop")
            .await;

        // ── EXECUTED rename edit ──────────────────────────────────────────
        let pos = session.find_position(&parent, ":my-prop=", 1);
        let mut ws_edit = None;
        for attempt in 0..12 {
            let edits = session.rename_edits(&parent, pos, "myPropRenamed").await;
            if let Some(edit) = edits {
                let complete =
                    edit_touches(&edit, &parent) && edit_touches(&edit, &child);
                ws_edit = Some(edit);
                if complete {
                    break;
                }
            }
            if attempt < 11 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        let ws_edit =
            ws_edit.expect("kebab cross-file rename must return edits, never fail closed");
        assert!(
            edit_touches(&ws_edit, &parent),
            "kebab rename must edit the parent template usage: {ws_edit:?}"
        );
        assert!(
            edit_touches(&ws_edit, &child),
            "kebab rename must edit the child script declaration: {ws_edit:?}"
        );
        // APPLY: the parent template usage is renamed at its FULL authored
        // span (a prefix edit would corrupt the attribute name). Virtual
        // files are open documents (not on disk) — read the open source.
        let doc_source = |uri: &tower_lsp_server::ls_types::Uri| {
            session
                .server()
                .test_documents()
                .get(uri)
                .expect("virtual document is open")
                .source
                .clone()
        };
        let parent_after = apply_edits(&ws_edit, &parent, &doc_source(&parent));
        assert!(
            parent_after.contains(":myPropRenamed=\"v\""),
            "parent template usage must become `:myPropRenamed=\"v\"`:\n{parent_after}"
        );
        assert!(
            !parent_after.contains("my-prop"),
            "no remnant of the kebab name may survive (a prefix edit corrupts):\n{parent_after}"
        );
        let child_after = apply_edits(&ws_edit, &child, &doc_source(&child));
        assert!(
            child_after.contains("myPropRenamed: string"),
            "child defineProps must declare `myPropRenamed: string`:\n{child_after}"
        );
        assert!(
            !child_after.contains("myProp: string"),
            "the original `myProp` declaration must be renamed in place:\n{child_after}"
        );

        // ── Find-references spanning script + template ────────────────────
        // From the kebab usage, references must discover BOTH the parent
        // template usage (the provider's own leg) AND the child script
        // declaration (Verter injects the resolved `defineProps` declaration
        // — providers do not enumerate it across the synthesized API
        // surface). The camelCase↔kebab-case mapping spans the two files.
        let mut refs = Vec::new();
        for attempt in 0..12 {
            refs = session.references(&parent, pos).await;
            let spans = refs.iter().any(|l| uris_identify_same_file(&l.uri, &parent))
                && refs.iter().any(|l| uris_identify_same_file(&l.uri, &child));
            if spans || attempt == 11 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        assert!(
            refs.iter().any(|l| uris_identify_same_file(&l.uri, &parent)),
            "references must include the parent template usage; got: {refs:?}"
        );
        let decl_ref = refs
            .iter()
            .find(|l| uris_identify_same_file(&l.uri, &child))
            .expect("references must include the child script declaration");
        assert_eq!(
            (decl_ref.range.start.line, decl_ref.range.start.character),
            (1, 14),
            "the declaration reference must land on `myProp` exactly: {decl_ref:?}"
        );
    }
);

// FAIL-CLOSED GUARD — the provider-agnostic child-declaration SYNTHESIS must NEVER
// fabricate a cross-file child edit for a rename that is NOT on a `<Child prop=…>`
// usage. Renaming a LOCAL `<script setup>` binding (`doubled`, a computed) is a
// purely in-/same-file rename: its edits stay within App.vue's own surfaces and
// must NOT spill into the imported, CLOSED MyComp.vue.
//
// DISCRIMINATES: a synthesis that fired on a non-prop position (e.g. mis-resolved
// the cursor to a child component, or located a bogus carrier range) would inject a
// spurious `MyComp.vue.ts` rename location → the merge would land a wrong edit in
// MyComp.vue. The `classify_child_prop_rename` classification returns `NotChildProp`
// for a non-prop cursor (so nothing is synthesized AND the completeness gate does not
// fire), so MyComp.vue stays untouched. Both providers run the SAME classification +
// synthesis, so this guards both.
real_provider_test!(
    rename_local_binding_does_not_synthesize_cross_file_child_edit,
    fixture = "single-project",
    async fn run(session) {
        let app = session.open_fixture_file("src/App.vue").await;
        let child_uri = session.workspace_uri("src/MyComp.vue");
        let _warm = session.wait_until_ready(&app, "action.disabled", 7, "disabled").await;

        // Rename a LOCAL computed binding (NOT a cross-file prop).
        let pos = session.find_position(&app, "const doubled = computed(", 6);
        let edits = session.rename_edits(&app, pos, "doubledValue").await;
        let ws_edit = edits.expect("renaming a local binding must return edits");

        // The rename must touch App.vue (its own usages) …
        assert!(
            edit_touches(&ws_edit, &app),
            "local rename must edit App.vue: {ws_edit:?}"
        );
        // … and must NOT fabricate any edit into the imported, CLOSED MyComp.vue.
        assert!(
            !edit_touches(&ws_edit, &child_uri),
            "local-binding rename must NOT synthesize a cross-file edit into MyComp.vue \
             (fail-closed: synthesis only fires on a `<Child prop=…>` usage): {ws_edit:?}"
        );
    }
);

// IMPORTED-TYPE cross-file rename PARITY (`defineProps<ImportedType>()`), tsserver.
// The child `ImportedPropChild.vue` declares its props via an IMPORTED type
// (`ChildImportedProps`) whose member declaration lives in a THIRD file,
// `childImportedProps.ts` — NOT an inline `defineProps` macro field. Renaming the
// `headline` prop USAGE in the parent must edit BOTH the parent usage AND the
// imported type's member declaration in the third file.
//
// WHAT THIS DISCRIMINATES — the imported-type declaration RESOLUTION + completeness
// gate. Verter resolves the declaration target by a provider `get_definition` at the
// SAME parent-usage TSX offset the rename uses (the provider resolves usage ->
// imported member in one hop), then the completeness gate proves the merged
// `WorkspaceEdit` edits that resolved third-file member. If the gate resolution
// regressed, the rename would ship a usage-only partial or fail entirely; either way
// the third-file edit assertion below fails.
//
// tsserver-only (written directly, not via `real_provider_test!`): tsgo cannot reach
// the imported type's member declaration from the parent's program — it resolves the
// prop usage only to its OWN occurrence (the cross-file project-MEMBERSHIP gap
// tracked as Block H-membership). tsgo therefore correctly FAILS CLOSED for this case
// (see `rename_cross_file_imported_prop_fails_closed` below, which runs for BOTH
// providers, and the tsgo future-parity tracker
// `rename_cross_file_imported_prop_tsgo_member_parity`). The imported member declaration
// EXISTS in the generated carrier surfaces and Verter advertises BOTH the parent and
// child carriers in the on-disk store `ready_files` (the `getExternalFiles` serve set —
// carrier publish/membership is complete and proven). But tsserver hits the SAME Block-H
// program-membership gap as tsgo: it does not materialize the advertised parent
// `.vue.tsx` into a queryable program SourceFile at query time (`getValidSourceFile` =>
// "Could not find source file"), so the rename's first provider hop ERRORS, the
// declaration stays `Unknown`, and the merged-edit completeness gate fails closed. So
// this is `#[ignore]`'d on the same cross-file program-membership gap as the tsgo sibling.
// Mirrors the macro's skip/build gating (build() returns None when absent; hard-fails
// under VERTER_REQUIRE_TSSERVER=1).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Block-H tsserver program-membership gap: Verter advertises the carrier in \
            getExternalFiles `ready_files` (publish complete) but tsserver does not \
            materialize the advertised parent `.vue.tsx` into a queryable program \
            SourceFile (`getValidSourceFile`: could not find source file), so the first \
            provider hop errors and cross-file imported-type rename fails closed. Same \
            class as `rename_cross_file_imported_prop_tsgo_member_parity`; remove this \
            `#[ignore]` when Block-H program-membership lands."]
async fn rename_cross_file_imported_prop_tsserver() {
    use crate::test_harness::{TestProviderKind, TestSessionBuilder};

    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture("single-project")
        .build()
        .await
    else {
        return;
    };

    let parent = session
        .open_fixture_file("src/ImportedPropParent.vue")
        .await;
    let _child = session.open_fixture_file("src/ImportedPropChild.vue").await;
    let decl_uri = session.workspace_uri("src/childImportedProps.ts");

    // Best-effort warm-up only (no gating); the production handler syncs.
    let _warm = session
        .wait_until_ready(&parent, "headline=\"hi there\"", 0, "headline")
        .await;

    // Rename the `headline` prop USAGE on `<ImportedPropChild headline=…>`. tsserver
    // may need a moment to index the imported-carrier surfaces before it
    // cross-references the member; retry in a bounded settle loop until BOTH files are
    // edited (the production sync is what makes the surfaces live — the settle only
    // lets the provider index a surface the sync already sent).
    let pos = session.find_position(&parent, "headline=\"hi there\"", 0);
    let mut ws_edit = None;
    for attempt in 0..12 {
        let edits = session.rename_edits(&parent, pos, "headlineRenamed").await;
        if let Some(edit) = edits {
            if edit_touches(&edit, &parent) && edit_touches(&edit, &decl_uri) {
                ws_edit = Some(edit);
                break;
            }
            ws_edit = Some(edit);
        }
        if attempt < 11 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    // BOTH files must be edited (parent usage + the imported type's member
    // declaration in the THIRD file). It FAILS (not skips) if the declaration edit is
    // dropped — the gate must never ship a usage-only partial.
    let ws_edit = ws_edit.expect(
        "cross-file imported-type rename must return edits — the production sync-before-query \
         must make the imported carrier + type surfaces live for the provider",
    );
    assert!(
        edit_touches(&ws_edit, &parent),
        "rename must edit the parent ImportedPropParent.vue: {ws_edit:?}"
    );
    assert!(
        edit_touches(&ws_edit, &decl_uri),
        "rename must edit the imported type's member declaration in the THIRD file \
         childImportedProps.ts (the imported-type parity case): {ws_edit:?}"
    );

    // APPLY the edit and verify the declaration is renamed in the third file.
    let decl_before = read_file(&decl_uri);
    assert!(
        decl_before.contains("headline: string"),
        "fixture precondition: childImportedProps.ts must declare `headline: string`:\n{decl_before}"
    );
    let decl_after = apply_edits(&ws_edit, &decl_uri, &decl_before);
    // STRONG mapping assertion: the renamed prop must appear in its DECLARATION
    // context — `headlineRenamed: string` (name WITH its declared type), not a bare
    // substring. A mis-ranged edit would corrupt the declaration.
    assert!(
        decl_after.contains("headlineRenamed: string"),
        "childImportedProps.ts must declare the renamed member `headlineRenamed: string`:\n{decl_after}"
    );
    // The ORIGINAL `headline:` member must be gone (renamed in place).
    assert!(
        !decl_after.contains("headline: string"),
        "childImportedProps.ts must no longer declare the original `headline: string`:\n{decl_after}"
    );

    // The parent usage must be renamed too.
    let parent_before = read_file(&parent);
    let parent_after = apply_edits(&ws_edit, &parent, &parent_before);
    assert!(
        parent_after.contains("headlineRenamed=")
            && !parent_after.contains("headline=\"hi there\""),
        "ImportedPropParent.vue must have the renamed prop usage applied:\n{parent_after}"
    );

    session.shutdown().await;
}

// IMPORTED-TYPE cross-file rename PARITY for tsgo — the FUTURE expectation, gated
// `#[ignore]` on a CONFIRMED tsgo cross-file project-MEMBERSHIP gap (the same class as
// `rename_cross_file_prop_child_closed_unprewarmed_tsserver`, tracked as Block
// H-membership).
//
// MEASURED tsgo behavior: `get_definition` AND `get_rename_locations` at the parent
// `<ImportedPropChild headline=…>` usage offset both return ONLY the parent's OWN
// usage occurrence — tsgo does not reach the imported type's member declaration in
// `childImportedProps.ts` from the parent's program. So Verter's declaration
// resolution stays `Unknown` and the completeness gate correctly FAILS CLOSED (no
// usage-only partial — verified by `rename_cross_file_imported_prop_fails_closed`).
// Achieving tsgo member-parity needs the cross-cutting tsgo program-membership /
// sync-ordering fix (affects every nav handler), which is out of scope for the
// fail-closed gate work. When that lands, tsgo will reach the member and this
// expectation becomes assertable — remove `#[ignore]`.
//
// tsgo-only, written directly (mirrors the macro's gating): build() returns None when
// tsgo is absent (skip), hard-fails under VERTER_REQUIRE_TSGO=1.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "confirmed tsgo cross-file project-membership gap (Block H-membership): tsgo resolves a \
            `<Child prop=…>` imported-type prop usage only to its own occurrence, never the imported \
            member in the third file, so imported-type cross-file rename parity for tsgo needs the \
            out-of-scope tsgo program-membership/sync-ordering fix. tsgo correctly FAILS CLOSED today \
            (no usage-only partial); this lane asserts the future member-parity once membership lands."]
async fn rename_cross_file_imported_prop_tsgo_member_parity() {
    use crate::test_harness::{TestProviderKind, TestSessionBuilder};

    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsgo)
        .fixture("single-project")
        .build()
        .await
    else {
        return;
    };

    let parent = session
        .open_fixture_file("src/ImportedPropParent.vue")
        .await;
    let _child = session.open_fixture_file("src/ImportedPropChild.vue").await;
    let decl_uri = session.workspace_uri("src/childImportedProps.ts");
    let _warm = session
        .wait_until_ready(&parent, "headline=\"hi there\"", 0, "headline")
        .await;

    let pos = session.find_position(&parent, "headline=\"hi there\"", 0);
    let mut ws_edit = None;
    for attempt in 0..12 {
        let edits = session.rename_edits(&parent, pos, "headlineRenamed").await;
        if let Some(edit) = edits {
            if edit_touches(&edit, &parent) && edit_touches(&edit, &decl_uri) {
                ws_edit = Some(edit);
                break;
            }
            ws_edit = Some(edit);
        }
        if attempt < 11 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
    let ws_edit = ws_edit.expect("tsgo imported-type cross-file rename must return edits");
    assert!(
        edit_touches(&ws_edit, &parent) && edit_touches(&ws_edit, &decl_uri),
        "tsgo imported-type rename must edit BOTH the parent usage and the third-file member: {ws_edit:?}"
    );

    session.shutdown().await;
}

// FAIL-CLOSED imported-type cross-file rename — BOTH providers. The completeness
// invariant: a CONFIRMED `<Child prop=…>` rename whose declaration is an imported
// type must NEVER ship a usage-only partial. Either the rename completes WITH the
// third-file declaration edit, OR it returns None — never a parent-only edit.
//
// Both providers satisfy the invariant by DIFFERENT routes, and BOTH are exercised
// here: tsserver reaches the imported member and completes (parent + third-file
// edits); tsgo cannot reach the member (the tracked cross-file membership gap), so its
// declaration stays `Unknown` and the gate fails closed (None). Either way no
// usage-only partial is shipped.
//
// DISCRIMINATES the fail-closed boundary end-to-end: with the gate reverted to the
// old "unresolved declaration does not gate" behavior, tsgo would return a usage-only
// `WorkspaceEdit` here; the assertion that a parent edit IMPLIES the third-file edit
// catches it (and the tsgo-without-fix run produces exactly the usage-only shape this
// rejects).
real_provider_test!(
    rename_cross_file_imported_prop_fails_closed,
    fixture = "single-project",
    async fn run(session) {
        let parent = session.open_fixture_file("src/ImportedPropParent.vue").await;
        let _child = session.open_fixture_file("src/ImportedPropChild.vue").await;
        let decl_uri = session.workspace_uri("src/childImportedProps.ts");
        let _warm = session
            .wait_until_ready(&parent, "headline=\"hi there\"", 0, "headline")
            .await;

        // Rename the `headline` USAGE but ASSERT the fail-closed invariant holds for
        // any returned edit: a confirmed child-prop rename must NEVER ship a
        // declaration-less (usage-only) partial. Either the rename completes with the
        // third-file declaration edit present, OR it returns None — never a
        // parent-only WorkspaceEdit.
        let pos = session.find_position(&parent, "headline=\"hi there\"", 0);
        let edits = session.rename_edits(&parent, pos, "headlineRenamed").await;

        if let Some(ws_edit) = edits {
            // If ANY edit is returned, the completeness gate guarantees it is NOT a
            // usage-only partial: a parent edit implies the third-file declaration
            // edit is also present.
            if edit_touches(&ws_edit, &parent) {
                assert!(
                    edit_touches(&ws_edit, &decl_uri),
                    "a confirmed imported-type child-prop rename must never ship a usage-only \
                     partial: if the parent usage is edited, the third-file declaration MUST be \
                     too (the fail-closed completeness gate): {ws_edit:?}"
                );
            }
        }
        // None is an acceptable fail-closed outcome.
    }
);

// PREWARM REGRESSION GUARD — cross-file rename now DEPENDS on the parent's
// `did_open` prewarming the imported child's `{carrier}.ts` PUBLIC-API surface
// into the `ProviderSurfaceStore` (so tsserver can REPORT the cross-file rename
// location, which the generation-pinned snapshot then maps back onto the `.vue`).
// This guard proves the post-condition directly: opening ONLY the parent App.vue
// records a `CarrierApi` snapshot for the imported child MyComp.vue. If a future
// change removes/breaks the prewarm, the snapshot is absent and this guard fails
// LOUDLY — independent of the slower end-to-end rename lanes.
//
// tsserver-only: the imported-carrier prewarm is gated `matches!(.. Tsserver)`
// (lifecycle.rs) — tsgo never records here, so this is written directly (not via
// `real_provider_test!`, which would also emit a tsgo variant) and mirrors the
// macro's skip/build gating. Prewarm is LEFT ON (no `suppress_imported_carrier_prewarm`).
//
// DISCRIMINATION: setting `suppress_imported_carrier_prewarm(true)` (the inverse
// of what this guard protects) makes the snapshot never get recorded, so the
// bounded settle loop exhausts and the assertion fails — verified during
// development.
#[tokio::test(flavor = "multi_thread")]
async fn parent_did_open_prewarms_imported_child_carrier_api() {
    use crate::test_harness::{TestProviderKind, TestSessionBuilder};

    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture("single-project")
        // Prewarm ON — this guard protects exactly that behavior.
        .build()
        .await
    else {
        return;
    };

    // Open ONLY the parent. The child MyComp.vue is intentionally NOT opened; the
    // ONLY thing that can record its API surface is the parent's did_open prewarm.
    let _app = session.open_fixture_file("src/App.vue").await;

    // The child's recorded identity: its canonical id is what `did_open` computes
    // and stores as `source_canonical`; its provider path is the carrier API
    // `{canonical}.ts` virtual path the prewarm syncs/records under.
    let child_canonical =
        crate::documents::uri_to_canonical_id(&session.workspace_uri("src/MyComp.vue"));
    let child_provider_path = verter_workspace::carrier_api_provider_path(&child_canonical);

    // The lightweight imported-carrier sync is async (a no-response provider
    // notification), so give it a BOUNDED settle — a short retry reading the store,
    // NOT a fixed long sleep. The loop exits the moment the snapshot appears.
    let store = session.server().test_documents().provider_surfaces();
    let mut snapshot = None;
    for attempt in 0..40 {
        if let Some(snap) = store.current_snapshot(&child_provider_path) {
            snapshot = Some(snap);
            break;
        }
        if attempt < 39 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    // The store must hold a CarrierApi snapshot for the imported child — recorded
    // by the parent's did_open prewarm (the child itself is closed).
    let snapshot = snapshot.expect(
        "parent App.vue did_open must prewarm the imported child MyComp.vue's \
         {carrier}.ts API surface into the ProviderSurfaceStore (no snapshot recorded)",
    );
    assert!(
        store.is_tracked(&child_provider_path),
        "the child carrier API path {child_provider_path} must be CURRENTLY tracked after prewarm"
    );
    // The snapshot must be the imported CHILD's API surface — not some unrelated
    // path. An empty-store or no-child outcome fails the `expect` above; this
    // pins the recorded snapshot to MyComp specifically.
    assert_eq!(
        &*snapshot.source_canonical,
        child_canonical.as_str(),
        "the recorded snapshot must belong to the imported child MyComp.vue"
    );
    assert_eq!(
        snapshot.kind,
        crate::provider_surface_store::ProviderSurfaceKind::CarrierApi,
        "the prewarmed surface must be the {{carrier}}.ts PUBLIC-API surface"
    );

    // The captured CarrierApi set (the in-flight pinned set a cross-file rename
    // holds) must be NON-EMPTY and contain the child — this is exactly what the
    // rename's `capture_current_carrier_api_set()` would pin.
    let captured = store.capture_current_carrier_api_set();
    assert!(
        !captured.is_empty(),
        "captured CarrierApi set must be non-empty after prewarm"
    );
    assert!(
        captured.snapshot_for(&child_provider_path).is_some(),
        "captured CarrierApi set must contain the imported child MyComp.vue's API surface"
    );

    session.shutdown().await;
}

// R9 (child CLOSED, prewarm SUPPRESSED) — the would-be discriminating cross-file
// rename gate, gated `#[ignore]` on a CONFIRMED production gap (see below).
//
// Intent: identical to `rename_cross_file_prop_child_closed`, but the session
// SUPPRESSES the `did_open` imported-carrier-API prewarm (both the eager and the
// deferred warmup). With the prewarm off, opening the parent App.vue does NOT
// pre-sync the closed child MyComp.vue's `{carrier}.ts` API surface — so the ONLY
// thing that could deliver it is the BACKGROUND import-dependency publication a
// rename's readiness miss enqueues (the rename itself never starts the sync; it
// retries below until a settled publication lets it capture DependencyReady).
// This body asserts BOTH files are edited, so deleting the publication enqueue
// would make it fail (no child edit) — the discrimination the masked
// default-prewarm lane (which records the child at did_open regardless) cannot
// provide.
//
// WHY `#[ignore]` (a CONFIRMED production gap, not a flaky/slow test):
// under tsserver, a closed child is only cross-referenced by a rename initiated
// from the parent if the child's `{carrier}.ts` was opened in tsserver BEFORE the
// parent App.vue.tsx program was built. The did_open prewarm achieves that
// (child opened first, parent IDE synced second). The background dependency
// publication runs the OPPOSITE order (parent first, then children) AND at
// rename time App.vue.tsx is already open from did_open, so the child opens into
// its own inferred project, OUTSIDE App's configured-project program —
// tsserver's rename returns ONLY the App.vue group. This was verified at the raw
// tsserver boundary: prewarmed = 2 rename groups (MyComp.vue.ts + App.vue.tsx),
// unprewarmed = 1 group (App only), stable across 90 one-second retries (so it
// is project membership, not indexing latency). The child sync itself is NOT a
// no-op: the publication's imported-carrier leg discovers MyComp and
// `sync_imported_carrier_api_lightweight` opens its `.d.ts` OK every attempt.
//
// Closing this requires a PRODUCTION fix to the tsserver sync ordering /
// project-membership handling so a child opened at publication time is forced
// into the parent's configured program (e.g. re-sync the parent IDE TSX to
// trigger a program rebuild after a NEW child surface is opened, or open
// imported children before the parent at the relevant sync points). That change
// affects the shared background publication every navigation feature depends on,
// so it is cross-cutting and out of scope for this fail-closed merge/store fix;
// it is tracked as the separate follow-up Block H-membership (tsserver
// program-membership for cross-file nav handlers). The
// `suppress_imported_carrier_prewarm` seam this lane uses is the exact mechanism
// Block H-membership validates against.
//
// tsserver-only (the prewarm/child-sync is on the tsserver path). Written directly
// (not via `real_provider_test!`) because it needs the builder's
// `suppress_imported_carrier_prewarm` seam; it mirrors the macro's gating — `build()`
// returns `None` (skip) when the provider is absent, and HARD-FAILS under
// `VERTER_REQUIRE_TSSERVER=1`.
//
// TODO(follow-up): Block H-membership (tsserver program-membership for cross-file
// nav handlers) lands the tsserver project-membership ordering fix in the shared
// background dependency publication so a closed-child cross-file rename works
// WITHOUT the did_open prewarm, then removes `#[ignore]` here — this lane will
// then go green, and red when the readiness-miss publication enqueue is removed.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "confirmed production gap, tracked as Block H-membership (tsserver program-membership for \
            cross-file nav handlers): closed-child cross-file rename depends on did_open prewarm \
            ordering (tsserver project membership); the background dependency publication opens the \
            child after the parent program is built. Needs the Block H-membership sync-ordering fix \
            before this discriminating lane can pass — see the test comment."]
async fn rename_cross_file_prop_child_closed_unprewarmed_tsserver() {
    use crate::test_harness::{TestProviderKind, TestSessionBuilder};

    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture("single-project")
        .suppress_imported_carrier_prewarm(true)
        .build()
        .await
    else {
        return;
    };

    // Open ONLY the parent. The child MyComp.vue is intentionally NOT opened, and
    // — crucially — the imported-carrier prewarm (BOTH the eager and the deferred
    // did_open warmup) is suppressed, so NOTHING pre-syncs the child's API surface.
    // Do NOT call `wait_until_ready` here: that would `ensure_synced` and could
    // pre-sync paths; the production rename handler performs its OWN sync.
    let app = session.open_fixture_file("src/App.vue").await;
    let child_uri = session.workspace_uri("src/MyComp.vue");

    // Rename the `foo` prop usage in App.vue. `<MyComp foo="literal" …>`.
    let pos = session.find_position(&app, r#"foo="literal""#, 0);

    // EACH rename invocation whose DependencyReady receipt is missing ENQUEUES
    // the background dependency publication, which syncs the closed child's
    // `{carrier}.ts` API surface to tsserver; a later attempt captures the
    // minted receipt and takes the provider leg. The sync is a no-response
    // notification, so tsserver also needs a moment to INDEX the surface before
    // it reports a cross-file rename location against it; retry the rename in a
    // bounded settle loop until BOTH files are edited.
    //
    // This stays DISCRIMINATING: with the readiness-miss publication enqueue
    // removed (and the prewarm suppressed), the child surface is NEVER sent to
    // tsserver, so no amount of settling produces a child edit — the loop
    // exhausts and the assert below fails. The settle window only lets tsserver
    // index a surface the background publication DID send; it never substitutes
    // for that publication.
    let mut ws_edit = None;
    for attempt in 0..12 {
        let edits = session.rename_edits(&app, pos, "fooRenamed").await;
        if let Some(edit) = edits {
            if edit_touches(&edit, &app) && edit_touches(&edit, &child_uri) {
                ws_edit = Some(edit);
                break;
            }
            ws_edit = Some(edit);
        }
        if attempt < 11 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    // tsserver: BOTH files must be edited (App.vue usage + MyComp.vue decl). The
    // child edit can ONLY come from `handle_rename`'s own sync-before-query, since
    // the prewarm that would otherwise pre-sync the child is suppressed.
    let ws_edit = ws_edit.expect(
        "cross-file rename (child closed, prewarm suppressed) must return edits — the production \
         sync-before-query must sync the closed child's API surface",
    );
    assert!(
        edit_touches(&ws_edit, &app),
        "rename must edit the parent App.vue: {ws_edit:?}"
    );
    assert!(
        edit_touches(&ws_edit, &child_uri),
        "rename must edit the CLOSED child MyComp.vue carrier — with the prewarm suppressed this \
         proves handle_rename's OWN sync-before-query synced the child: {ws_edit:?}"
    );

    // APPLY the edit on disk-read content and verify text changed in BOTH.
    let app_before = read_file(&app);
    let app_after = apply_edits(&ws_edit, &app, &app_before);
    assert!(
        app_after.contains("fooRenamed") && !app_after.contains(r#"foo="literal""#),
        "App.vue must have the renamed prop applied:\n{app_after}"
    );

    let child_before = read_file(&child_uri);
    let child_after = apply_edits(&ws_edit, &child_uri, &child_before);
    assert!(
        child_after.contains("fooRenamed"),
        "MyComp.vue defineProps must contain the renamed prop:\n{child_after}"
    );
    // The ORIGINAL `foo:` prop declaration must be gone (renamed in place).
    assert!(
        !child_after.contains("foo: string"),
        "MyComp.vue must no longer declare the original `foo: string` prop:\n{child_after}"
    );

    session.shutdown().await;
}

/// Whether a workspace edit contains any edit for `uri`.
fn edit_touches(
    ws_edit: &tower_lsp_server::ls_types::WorkspaceEdit,
    uri: &tower_lsp_server::ls_types::Uri,
) -> bool {
    if let Some(changes) = &ws_edit.changes {
        if changes
            .keys()
            .any(|edited_uri| uris_identify_same_file(edited_uri, uri))
        {
            return true;
        }
    }
    if let Some(tower_lsp_server::ls_types::DocumentChanges::Edits(doc_edits)) =
        &ws_edit.document_changes
    {
        return doc_edits
            .iter()
            .any(|e| uris_identify_same_file(&e.text_document.uri, uri));
    }
    false
}

/// Compare file URIs using the same filesystem-identity policy as production.
///
/// TypeScript canonicalizes filenames according to the host filesystem (for
/// example `App.vue` can be returned as `app.vue` on Windows). A direct URI-key
/// comparison therefore rejects a valid edit for the same file on
/// case-insensitive filesystems while still being required on Linux, where the
/// differently-cased paths may identify distinct files.
fn uris_identify_same_file(
    left: &tower_lsp_server::ls_types::Uri,
    right: &tower_lsp_server::ls_types::Uri,
) -> bool {
    let left_path = crate::documents::uri_to_canonical_id(left);
    let right_path = crate::documents::uri_to_canonical_id(right);
    verter_span::path::fs_paths_equal(&left_path, &right_path)
}

/// Read the on-disk content for a fixture URI (works for a CLOSED file).
fn read_file(uri: &tower_lsp_server::ls_types::Uri) -> String {
    let path = crate::test_harness::RealProviderTestSession::uri_to_path(uri);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

/// Apply the workspace edit's text edits for `uri` to `source`, returning the
/// new content. Edits are applied from the LATEST offset to the earliest so
/// earlier-offset edits keep their positions valid.
fn apply_edits(
    ws_edit: &tower_lsp_server::ls_types::WorkspaceEdit,
    uri: &tower_lsp_server::ls_types::Uri,
    source: &str,
) -> String {
    use crate::documents::line_index::LineIndex;
    use tower_lsp_server::ls_types::PositionEncodingKind;

    let mut edits: Vec<tower_lsp_server::ls_types::TextEdit> = Vec::new();
    if let Some(changes) = &ws_edit.changes {
        for (edited_uri, file_edits) in changes {
            if uris_identify_same_file(edited_uri, uri) {
                edits.extend(file_edits.iter().cloned());
            }
        }
    }
    if let Some(tower_lsp_server::ls_types::DocumentChanges::Edits(doc_edits)) =
        &ws_edit.document_changes
    {
        for de in doc_edits {
            if uris_identify_same_file(&de.text_document.uri, uri) {
                for e in &de.edits {
                    if let tower_lsp_server::ls_types::OneOf::Left(te) = e {
                        edits.push(te.clone());
                    }
                }
            }
        }
    }

    // The server emits ranges in the negotiated encoding; tests negotiate UTF-16.
    let li = LineIndex::new(source, PositionEncodingKind::UTF16);
    let mut spans: Vec<(u32, u32, String)> = edits
        .into_iter()
        .filter_map(|e| {
            let start = li.position_to_offset(&e.range.start)?;
            let end = li.position_to_offset(&e.range.end)?;
            Some((start, end, e.new_text))
        })
        .collect();
    // Apply from the end so earlier offsets stay valid.
    spans.sort_by_key(|s| std::cmp::Reverse(s.0));
    let mut out = source.to_string();
    for (start, end, new_text) in spans {
        out.replace_range(start as usize..end as usize, &new_text);
    }
    out
}

/// Count total text edits across all files in a workspace edit.
fn count_edits(ws_edit: &tower_lsp_server::ls_types::WorkspaceEdit) -> usize {
    let mut total = 0;
    if let Some(changes) = &ws_edit.changes {
        for edits in changes.values() {
            total += edits.len();
        }
    }
    if let Some(tower_lsp_server::ls_types::DocumentChanges::Edits(doc_edits)) =
        &ws_edit.document_changes
    {
        for edit in doc_edits {
            total += edit.edits.len();
        }
    }
    total
}

/// A rename transaction must never ask an editor to apply the same mutation
/// twice. Duplicate edits can corrupt text when a client applies them
/// sequentially even if the ranges and replacement spelling are identical.
fn assert_no_duplicate_edits(ws_edit: &tower_lsp_server::ls_types::WorkspaceEdit) {
    let mut seen = std::collections::HashSet::new();
    if let Some(changes) = &ws_edit.changes {
        for (uri, edits) in changes {
            for edit in edits {
                assert!(
                    seen.insert(format!("{uri:?}:{:?}:{:?}", edit.range, edit.new_text)),
                    "rename returned a duplicate edit: {ws_edit:?}"
                );
            }
        }
    }
    if let Some(tower_lsp_server::ls_types::DocumentChanges::Edits(doc_edits)) =
        &ws_edit.document_changes
    {
        for edit in doc_edits {
            for operation in &edit.edits {
                assert!(
                    seen.insert(format!("{:?}:{operation:?}", edit.text_document.uri)),
                    "rename returned a duplicate edit: {ws_edit:?}"
                );
            }
        }
    }
}

/// Count number of distinct files in a workspace edit.
fn count_files(ws_edit: &tower_lsp_server::ls_types::WorkspaceEdit) -> usize {
    let mut files = std::collections::HashSet::new();
    if let Some(changes) = &ws_edit.changes {
        for uri in changes.keys() {
            files.insert(uri.to_string());
        }
    }
    if let Some(tower_lsp_server::ls_types::DocumentChanges::Edits(doc_edits)) =
        &ws_edit.document_changes
    {
        for edit in doc_edits {
            files.insert(edit.text_document.uri.to_string());
        }
    }
    files.len()
}
