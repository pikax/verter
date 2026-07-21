//! Import-resolution characterization matrix (tsserver + TSGO).
//!
//! Four hermetic fixtures, one broad `real_provider_test!` per fixture, each
//! opening multiple files and running multiple assertions in the SAME provider
//! session (so the provider-spawn count stays at four per provider, not one per
//! import form). The matrix characterizes how Verter's carrier-IDE projection +
//! the shared workspace import resolver behave across:
//!
//!  - `import_core_bundler`     — `module: preserve` / `moduleResolution: bundler`
//!    / `verbatimModuleSyntax`: relative / alias /
//!    baseUrl / named / namespace / barrel forms.
//!  - `import_nodenext_packages`— `nodenext` + a vendored package with `exports`
//!    and package `#imports`.
//!  - `import_refs_monorepo`    — composite project references (cross-project
//!    component carrier).
//!  - `import_syntax_passthrough`— TS 6/7 syntax (import attributes, `import
//!    defer`, deprecated `assert`, isolatedDeclarations).
//!
//! Assertion strategy (per the scope consult):
//!  - hover substring (a stable UNIQUE prop name) for static VALUE imports that
//!    should become template components;
//!  - diagnostics (a specific code) for semantic failures (module-not-found, a
//!    deprecated-`assert` diagnostic);
//!  - type-only / side-effect imports assert NO component binding is created.
//!
//! Every assertion genuinely depends on resolution. Under `VERTER_REQUIRE_*=1`
//! an absent provider PANICS (no vacuous skip past a materialized provider).

use crate::test_harness::real_provider_test;

/// Assert a component tag's hover surfaces a UNIQUE prop-name substring,
/// proving its carrier resolved through the import form under test.
async fn assert_tag_hover_has_prop(
    session: &crate::test_harness::RealProviderTestSession,
    uri: &tower_lsp_server::ls_types::Uri,
    tag: &str,
    prop: &str,
) {
    let pos = session.find_position(uri, tag, 1);
    let hover = session.hover_text(uri, pos).await;
    let text = hover.unwrap_or_else(|| panic!("hover on `{tag}` should return a result"));
    assert!(
        text.contains(prop),
        "hover on `{tag}` should surface its unique prop `{prop}` (import form resolved); got: {text}"
    );
}

// ---------------------------------------------------------------------------
// 1. import_core_bundler — bundler resolution + verbatimModuleSyntax
// ---------------------------------------------------------------------------

real_provider_test!(
    import_core_bundler,
    fixture = "import_core_bundler",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        // Open every imported carrier so the provider has each surface.
        for f in [
            "src/DirectComp.vue",
            "src/nested/deep/DeepComp.vue",
            "src/widgets/BaseUrlComp.vue",
            "src/AliasAtComp.vue",
            "src/AliasTildeComp.vue",
            "src/util/CustomAliasComp.vue",
            "src/NamedFromBarrel.vue",
        ] {
            session.open_fixture_file(f).await;
        }

        if !session.require_or_skip_ready(&uri, "{{ count }}", 3, "count").await {
            return;
        }

        // Static VALUE imports that become template components: each unique prop
        // proves its specific import form resolved end-to-end.
        assert_tag_hover_has_prop(session, &uri, "<DirectComp", "directOnly").await;
        assert_tag_hover_has_prop(session, &uri, "<DeepComp", "deepOnly").await;
        assert_tag_hover_has_prop(session, &uri, "<BaseUrlComp", "baseUrlOnly").await;
        assert_tag_hover_has_prop(session, &uri, "<AliasAtComp", "aliasAtOnly").await;
        assert_tag_hover_has_prop(session, &uri, "<AliasTildeComp", "aliasTildeOnly").await;
        assert_tag_hover_has_prop(session, &uri, "<CustomAliasComp", "customAliasOnly").await;
        assert_tag_hover_has_prop(session, &uri, "<NamedFromBarrel", "namedBarrelOnly").await;
        // Named import reached through a barrel-of-barrels (`export *`).
        assert_tag_hover_has_prop(session, &uri, "<WidgetFromStar", "baseUrlOnly").await;

        // type-only import (`import type { OnlyAType }`). The template renders
        // `<OnlyAType :typeFieldOnly="1" />` (an intentional misuse) so the
        // script-imports carrier-linkage map has a name to match — mirroring the
        // accepted `<Lazy>` carrier-linkage discrimination in
        // `import_core_bundler_edge`. This forces the LINKAGE fact rather than
        // leaving the negative vacuous (no tag → nothing to (wrongly) link).
        let analysis = session
            .server()
            .test_documents()
            .get_analysis(&uri)
            .expect("App.vue analysis should be present");
        let type_import = analysis
            .imports
            .iter()
            .find(|i| i.source == "./types")
            .expect("the `./types` import should be analyzed");
        // Genuine, separate Verter-owned fact: the import is flagged type-only.
        assert!(
            type_import.is_type_only,
            "`import type {{ OnlyAType }}` must be classified type-only"
        );
        // Per the carrier-IDE/typed-IR rule a `import type {...}` must NEVER be
        // carrier-linked as a value component: the script-imports carrier-linkage
        // map filters type-only bindings, so a rendered `<OnlyAType>` (an
        // intentional misuse) carries `import_source = None` even though the tag is
        // recorded as a template usage. Asserted UNCONDITIONALLY (App.vue has a
        // `<template>`) so a regression that re-links the type-only binding FAILS.
        // The full desired-behavior assertion is the
        // `type_only_import_is_not_carrier_linked_*` companion.
        let template = analysis
            .template
            .as_ref()
            .expect("App.vue should have a <template>");
        // Assert the rendered `<OnlyAType>` tag IS recorded as a usage first (so
        // the negative below is not vacuous if the tag ever disappears), then
        // assert it is NOT carrier-linked.
        let only_a_type = template
            .components
            .iter()
            .find(|c| c.name == "OnlyAType")
            .expect("the rendered `<OnlyAType>` tag must be recorded as a template component usage");
        assert_eq!(
            only_a_type.import_source, None,
            "a type-only import (`import type {{ OnlyAType }}`) must NEVER be \
             carrier-linked as a value component (import_source must stay None); \
             got: {:?}",
            only_a_type.import_source
        );
    }
);

// A type-only import (`import type { OnlyAType } from './types'`) rendered as a
// template tag (`<OnlyAType :typeFieldOnly="1" />`) must NOT be carrier-linked as
// a value component. The script-imports carrier-linkage map built for the
// template converter (`template_converter_inputs`) filters type-only bindings
// (mirroring `compile_fact_emission::binding_is_value`), so the `<OnlyAType>`
// template-component usage carries `import_source = None` even though the import
// IS correctly flagged `is_type_only`. Per the carrier-IDE / typed-IR rule a
// `import type {...}` is never a value component. Discriminating: asserts the
// resolved non-linkage; a regression that re-adds the type-only binding to the
// linkage map FAILS. The current correct linkage is also pinned by
// `import_core_bundler`'s type-only block.
#[tokio::test(flavor = "multi_thread")]
async fn type_only_import_is_not_carrier_linked_as_value_component_tsserver() {
    let Some(session) = crate::test_harness::TestSessionBuilder::new(
        crate::test_harness::TestProviderKind::Tsserver,
    )
    .fixture("import_core_bundler")
    .build()
    .await
    else {
        return;
    };
    let uri = session.open_fixture_file("src/App.vue").await;
    let _ = session
        .wait_until_ready(&uri, "{{ count }}", 3, "count")
        .await;
    let analysis = session
        .server()
        .test_documents()
        .get_analysis(&uri)
        .expect("App.vue analysis should be present");
    let type_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "./types")
        .expect("the `./types` import should be analyzed");
    assert!(
        type_import.is_type_only,
        "`import type {{ OnlyAType }}` must be classified type-only"
    );
    let template = analysis
        .template
        .as_ref()
        .expect("App.vue should have a <template>");
    // The `<OnlyAType>` tag IS rendered (intentional misuse) so it MUST be
    // recorded as a template-component usage — assert presence first so the test
    // discriminates "recorded-but-unlinked" from "disappeared entirely" (a
    // vanished tag would otherwise pass a bare `import_source == None` check).
    let only_a_type = template
        .components
        .iter()
        .find(|c| c.name == "OnlyAType")
        .expect("the rendered `<OnlyAType>` tag must be recorded as a template component usage");
    assert_eq!(
        only_a_type.import_source, None,
        "a type-only import (`import type {{ OnlyAType }}`) must NEVER be \
         carrier-linked as a value component (import_source must stay None); \
         got: {:?}",
        only_a_type.import_source
    );
    session.shutdown().await;
}

// TRACKED GAP (ESCALATED — genuinely architectural, two coupled sub-gaps):
// a namespaced component tag (`<widgets.WidgetFromStar>`, reached via
// `import * as widgets from './widgets'`) does NOT resolve to its carrier's
// props. Two distinct pieces are involved:
//
//   (1) IDE-TSX codegen ALREADY emits the dotted tag VERBATIM as a valid JSX
//       member-expression element (`<widgets.WidgetFromStar baseUrlOnly={3} />`)
//       — empirically verified — so a provider DOES type-check the member access
//       through the namespace import. The codegen half is NOT the blocker.
//
//   (2) The blockers are: (a) the template-component analysis records the dotted
//       tag with `import_source = None` (no carrier link), because resolving a
//       dotted tag's carrier requires following the namespace binding
//       (`widgets` -> `./widgets`) and then resolving the MEMBER
//       (`WidgetFromStar`) THROUGH the barrel re-export to its carrier
//       (`BaseUrlComp.vue`) at template-conversion time — a cross-file
//       barrel-member resolution that `template_converter_inputs` has no host
//       resolver access for today; and (b) this hover probes the NAMESPACE
//       segment (`<` + 1 lands on `widgets`), where even a correct provider
//       surfaces the namespace type, not the member's props — so satisfying the
//       test additionally needs either a re-aimed hover target or a codegen
//       mapping that routes the whole dotted tag to the member carrier.
//
// Both pieces need a design decision (carrier-linkage semantics for namespace
// members + the hover/codegen mapping for the dotted-tag region); not a
// contained single-site fix. Left `#[ignore]`'d per the escalation policy. This
// test asserts the DESIRED behavior (props resolve) so it stays discriminating
// once the gap is designed and closed; it is not a stub.
#[ignore = "ESCALATED architectural gap: namespaced component tag <ns.Comp> carrier-linkage needs cross-file barrel-member resolution at template-conversion + a dotted-tag hover/codegen mapping decision (IDE-TSX already emits valid JSX member-expr; analysis import_source=None)"]
#[tokio::test(flavor = "multi_thread")]
async fn namespaced_component_tag_resolves_member_props_tsgo() {
    let Some(session) =
        crate::test_harness::TestSessionBuilder::new(crate::test_harness::TestProviderKind::Tsgo)
            .fixture("import_core_bundler")
            .build()
            .await
    else {
        return;
    };
    let uri = session.open_fixture_file("src/App.vue").await;
    session
        .open_fixture_file("src/widgets/BaseUrlComp.vue")
        .await;
    if session
        .wait_until_ready(&uri, "{{ count }}", 3, "count")
        .await
    {
        let pos = session.find_position(&uri, "<widgets.WidgetFromStar", 1);
        let hover = session.hover_text(&uri, pos).await;
        let text = hover.expect("hover on namespaced component tag should return a result");
        assert!(
            text.contains("baseUrlOnly"),
            "a namespaced component tag should surface the member carrier's props \
             (baseUrlOnly); got: {text}"
        );
    }
    session.shutdown().await;
}

// Regression guard (formerly a tracked tsgo-only gap, freshly DISPROVEN):
// tsgo's pull-diagnostics for a Verter-generated carrier `.tsx` now resolve
// tsconfig `paths` aliases — the merged diagnostics carry NO
// `TS2307 Cannot find module '@/...'` for the aliased import, matching hover
// and tsserver-diagnostics behavior. Non-vacuity: the same merged set DOES
// carry the benign no-node_modules `2307 Cannot find module 'vue'`, proving
// the pull-diagnostics pipeline is live when the alias assertion runs.
#[tokio::test(flavor = "multi_thread")]
async fn carrier_diagnostics_resolve_path_alias_tsgo() {
    let Some(session) =
        crate::test_harness::TestSessionBuilder::new(crate::test_harness::TestProviderKind::Tsgo)
            .fixture("path-aliases")
            .build()
            .await
    else {
        return;
    };
    let uri = session.open_fixture_file("src/AppBarrel.vue").await;
    session.open_fixture_file("src/components/MyComp.vue").await;
    if session
        .require_or_skip_ready(&uri, "{{ count }}", 3, "count")
        .await
    {
        let diags = session.merged_diagnostics(&uri).await;
        let alias_not_found = diags.iter().any(|d| {
            matches!(
                d.code.as_ref(),
                Some(tower_lsp_server::ls_types::NumberOrString::String(s)) if s == "2307"
            ) && d.message.contains("@/components")
        });
        assert!(
            !alias_not_found,
            "tsgo carrier diagnostics must resolve the `@/components` alias \
             (no TS2307); got: {diags:?}"
        );
    }
    session.shutdown().await;
}

// ---------------------------------------------------------------------------
// 2. import_nodenext_packages — nodenext package `exports` + package `#imports`
// ---------------------------------------------------------------------------
//
// `node_modules` is gitignored repo-wide, so a vendored package cannot be
// committed. The test MATERIALIZES the minimal `@pkg/ui` package into the
// fixture's (gitignored) `node_modules` at runtime (via the fixture-setup
// harness `materialize_pkg_ui`) before the provider session starts — the test
// creates its own dependency, so it stays hermetic and reproducible without an
// external corpus.

/// Characterize nodenext package `exports` resolution for a Vue carrier. The
/// PRIMARY assertion is a VERTER-OWNED resolver fact: the shared workspace
/// resolver is package.json-`exports`-aware, so the consumer's analysis records
/// a resolved canonical id for the `@pkg/ui` (`exports` "." entry) import — that
/// IS Verter resolving the package, not the TS provider. The tsserver hover is a
/// secondary end-to-end signal that the resolved carrier also surfaces props.
///
/// The package `imports` map (`#internal/*`) is also resolved: the shared
/// resolver populates `resolved_canonical_id` for package.json `#imports`
/// subpaths (with the `nodenext` `.js` -> `.ts` extension rewrite). The full
/// desired-behavior assertion is the
/// `nodenext_package_imports_subpath_populates_resolved_canonical_id_tsserver`
/// companion; here we assert the same `Some(..)` inline.
///
/// Tsserver-only: the nodenext+package-`exports` program shape is exercised on
/// the configured tsserver project. (A tsgo companion gap test characterizes the
/// tsgo carrier-diagnostics divergence separately.)
#[tokio::test(flavor = "multi_thread")]
async fn import_nodenext_packages_tsserver() {
    let _root = crate::test_harness::materialize_pkg_ui("import_nodenext_packages");
    let Some(session) = crate::test_harness::TestSessionBuilder::new(
        crate::test_harness::TestProviderKind::Tsserver,
    )
    .fixture("import_nodenext_packages")
    .build()
    .await
    else {
        return;
    };
    let uri = session.open_fixture_file("src/App.vue").await;
    session
        .open_fixture_file("src/internal/InternalComp.ts")
        .await;

    // PRIMARY (Verter-owned, provider-independent): the shared resolver populates
    // a canonical id for the package-`exports` "." import. Read the analysis
    // snapshot regardless of provider warmth — this is a pure Verter resolver fact.
    let analysis = session
        .server()
        .test_documents()
        .get_analysis(&uri)
        .expect("App.vue analysis should be present");
    let pkg_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "@pkg/ui")
        .expect("the `@pkg/ui` import should be analyzed");
    assert!(
        pkg_import.resolved_canonical_id.is_some(),
        "Verter's shared resolver must be package.json-`exports`-aware and populate \
         resolved_canonical_id for the `@pkg/ui` \".\" entry; got None"
    );
    // The `#internal/*` (`#imports`) subpath IS populated by the shared resolver:
    // the package.json `imports` map resolves `#internal/InternalComp.js` and the
    // `nodenext` extension rewrite substitutes `.js` -> the `.ts` sibling, so the
    // canonical id is `Some(..)`. The full desired-behavior assertion is the
    // `nodenext_package_imports_subpath_populates_*` companion.
    let internal_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "#internal/InternalComp.js")
        .expect("the `#internal/InternalComp.js` import should be analyzed");
    assert!(
        internal_import.resolved_canonical_id.is_some(),
        "the shared resolver must populate resolved_canonical_id for a package.json \
         `#imports` subpath (`#internal/*`, `.js` -> `.ts` extension rewrite); got None"
    );

    // SECONDARY (end-to-end): once the provider is warm, the resolved package
    // carriers also surface their props via hover.
    if !session
        .require_or_skip_ready(&uri, "{{ count }}", 3, "count")
        .await
    {
        session.shutdown().await;
        return;
    }
    // Package `exports` "." entry: `<PkgComp>` surfaces its props.
    assert_tag_hover_has_prop(&session, &uri, "<PkgComp", "pkgRootOnly").await;
    // Package `imports` map (`#internal/*`): resolves at the PROVIDER level so
    // `<InternalComp>` surfaces its props even though Verter's own canonical-id is
    // None (asserted above) — the IDE surface is correct via the provider.
    assert_tag_hover_has_prop(&session, &uri, "<InternalComp", "internalOnly").await;
    session.shutdown().await;
}

// Verter's shared workspace resolver populates
// `AnalyzedImport.resolved_canonical_id` for a package.json `#imports` subpath
// (`#internal/*` -> `./src/internal/*`, with the `nodenext` `.js` -> `.ts`
// extension rewrite resolving `InternalComp.js` to its `.ts` sibling), so
// Verter-owned carrier features keyed on the canonical id (dependency tracking,
// go-to-definition via the resolver) target it. Discriminating: a regression
// that drops `#imports` canonical-id population — or the extension rewrite —
// FAILS.
#[tokio::test(flavor = "multi_thread")]
async fn nodenext_package_imports_subpath_populates_resolved_canonical_id_tsserver() {
    let _root = crate::test_harness::materialize_pkg_ui("import_nodenext_packages");
    let Some(session) = crate::test_harness::TestSessionBuilder::new(
        crate::test_harness::TestProviderKind::Tsserver,
    )
    .fixture("import_nodenext_packages")
    .build()
    .await
    else {
        return;
    };
    let uri = session.open_fixture_file("src/App.vue").await;
    session
        .open_fixture_file("src/internal/InternalComp.ts")
        .await;
    let analysis = session
        .server()
        .test_documents()
        .get_analysis(&uri)
        .expect("App.vue analysis should be present");
    let internal_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "#internal/InternalComp.js")
        .expect("the `#internal/InternalComp.js` import should be analyzed");
    assert!(
        internal_import.resolved_canonical_id.is_some(),
        "the shared resolver should populate resolved_canonical_id for a package.json \
         `#imports` subpath (`#internal/*`); got None"
    );
    session.shutdown().await;
}

// CHARACTERIZATION (PASS — expectation overturned by evidence): a component
// imported from a RAW `.vue` SFC inside `node_modules` DOES resolve its props.
// Verter synthesizes the carrier surface for the external `.vue` and the
// provider resolves the import to it (verified: hover surfaces `vendoredVueOnly`
// and the import resolves to the node_modules `.vue` path). The package is
// materialized at runtime since `node_modules` is gitignored.
#[tokio::test(flavor = "multi_thread")]
async fn node_modules_raw_vue_carrier_resolves_props_tsserver() {
    let _root = crate::test_harness::materialize_pkg_vuecomp("import_nodenext_packages");
    let Some(session) = crate::test_harness::TestSessionBuilder::new(
        crate::test_harness::TestProviderKind::Tsserver,
    )
    .fixture("import_nodenext_packages")
    .build()
    .await
    else {
        return;
    };
    // The consumer (committed on-disk) imports the vendored raw `.vue` from
    // node_modules; the package itself is materialized above.
    let uri = session.open_fixture_file("src/VendoredConsumer.vue").await;
    let _ = session
        .wait_until_ready(&uri, "{{ count }}", 3, "count")
        .await;
    // Assert UNCONDITIONALLY (not gated by the ready check) so a cold provider or
    // an unresolved carrier FAILS rather than skipping green.
    let pos = session.find_position(&uri, "<Vendored ", 1);
    let hover = session.hover_text(&uri, pos).await;
    let text = hover.unwrap_or_default();
    assert!(
        text.contains("vendoredVueOnly"),
        "a raw .vue SFC inside node_modules should resolve its props \
         (vendoredVueOnly); got: {text:?}"
    );
    session.shutdown().await;
}

// ---------------------------------------------------------------------------
// 3. import_refs_monorepo — composite project references (cross-project carrier)
// ---------------------------------------------------------------------------
//
// A composite-references workspace: `packages/app` references `packages/ui`
// (both `composite: true`) and imports the ui project's component through an
// `@ui/*` path alias. Exercises cross-project COMPONENT carrier resolution
// (the existing `monorepo` / `tsconfig-references` fixtures are reference
// models; this one specifically resolves a referenced project's `.vue`).

real_provider_test!(
    import_refs_monorepo,
    fixture = "import_refs_monorepo",
    async fn run(session) {
        let uri = session
            .open_fixture_file("packages/app/src/App.vue")
            .await;
        session
            .open_fixture_file("packages/ui/src/UiButton.vue")
            .await;

        if !session.require_or_skip_ready(&uri, "{{ count }}", 3, "count").await {
            return;
        }

        assert_tag_hover_has_prop(session, &uri, "<UiButton", "uiButtonOnly").await;
    }
);

// ---------------------------------------------------------------------------
// 4. import_syntax_passthrough — TS 6/7 syntax (Verter non-corruption)
// ---------------------------------------------------------------------------
//
// Characterizes that Verter's IDE-TSX codegen PRESERVES modern import syntax
// through to the provider (rather than corrupting/dropping it). Both real
// providers (TS 6.0.x tsserver, TS 7 tsgo) behave identically:
// `with { type: "json" }` resolves the JSON to its typed shape, `import defer
// * as ns` keeps the deferred namespace binding resolvable, and a deprecated
// `assert { type: "json" }` surfaces TS2880. Each form carries a POSITIVE
// resolution signal (a hover on a unique resolved member) plus the absence of a
// corruption diagnostic — not just absence-of-error. `import defer` binds only a
// namespace value (a dotted `<ns.Comp>` template tag is a separate tracked gap),
// so it is exercised here through its namespace member, not a component tag.
//
// Helper to find a TS diagnostic by code in the carrier-merged set.
fn merged_has_ts_code(diags: &[tower_lsp_server::ls_types::Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| {
        matches!(
            d.code.as_ref(),
            Some(tower_lsp_server::ls_types::NumberOrString::String(s)) if s == code
        )
    })
}

real_provider_test!(
    import_syntax_passthrough,
    fixture = "import_syntax_passthrough",
    async fn run(session) {
        // (a) `with { type: "json" }` — Verter must preserve the attribute so the
        // provider accepts it AND resolves the JSON to its typed shape.
        let with_uri = session.open_fixture_file("src/WithJson.vue").await;
        if !session
            .require_or_skip_ready(&with_uri, "{{ count }}", 3, "count")
            .await
        {
            return;
        }
        let diags = session.merged_diagnostics(&with_uri).await;
        // Negative: no corruption of the attribute (would surface the deprecated-
        // assertion TS2880), no module-not-found for the resolved JSON.
        assert!(
            !merged_has_ts_code(&diags, "2880"),
            "a correct `with {{ type: \"json\" }}` import must NOT surface the \
             deprecated-assertion TS2880 (Verter corrupted the attribute?); got: {diags:?}"
        );
        assert!(
            !diags.iter().any(|d| {
                matches!(
                    d.code.as_ref(),
                    Some(tower_lsp_server::ls_types::NumberOrString::String(s)) if s == "2307"
                ) && d.message.contains("data.json")
            }),
            "the JSON module must resolve through the preserved import attribute \
             (no TS2307 for ./data.json); got: {diags:?}"
        );
        // Positive: the JSON import resolved to a TYPED object (not `any`/missing).
        // `data.json` is `{ "jsonValueOnly": "hello" }`; the script binds
        // `const value = data.jsonValueOnly`. Hover on `value` surfaces its type:
        // when the attribute is preserved and the JSON resolves, the member typed
        // as `string` flows through, so the hover reads `const value: string`. A
        // dropped/`any` import would surface `const value: any` — so asserting the
        // resolved `string` (and the absence of `any`) is discriminating.
        let value_pos = session.find_position(&with_uri, "const value", 6);
        let value_hover = session
            .hover_text(&with_uri, value_pos)
            .await
            .unwrap_or_else(|| panic!("hover on the JSON-derived `value` should return a result"));
        assert!(
            value_hover.contains("string") && !value_hover.contains(": any"),
            "the JSON import must resolve to its typed shape so `value` (=\
             `data.jsonValueOnly`) types as `string`, not `any` (attribute \
             preserved + JSON resolved); got: {value_hover}"
        );
        // And the property access itself must not error (TS2339) — it type-checks
        // only against the resolved JSON object type, never against `any`/missing.
        assert!(
            !diags.iter().any(|d| {
                matches!(
                    d.code.as_ref(),
                    Some(tower_lsp_server::ls_types::NumberOrString::String(s)) if s == "2339"
                ) && d.message.contains("jsonValueOnly")
            }),
            "accessing `data.jsonValueOnly` must not surface TS2339 (the JSON \
             resolved to its typed shape); got: {diags:?}"
        );

        // (b) deprecated `assert { type: "json" }` — Verter must PRESERVE the
        // provider's TS2880 mapped onto the carrier (not drop/corrupt it).
        let assert_uri = session.open_fixture_file("src/AssertJson.vue").await;
        if !session
            .require_or_skip_ready(&assert_uri, "{{ count }}", 3, "count")
            .await
        {
            return;
        }
        let assert_diags = session.merged_diagnostics(&assert_uri).await;
        assert!(
            merged_has_ts_code(&assert_diags, "2880")
                || assert_diags
                    .iter()
                    .any(|d| d.message.contains("import attributes")),
            "a deprecated `assert {{ type: \"json\" }}` import must surface TS2880 \
             on the carrier (Verter must preserve the provider diagnostic); got: {assert_diags:?}"
        );

        // (c) `import defer * as ns from "./mod"` (TS 5.9+, legal under
        // `module: preserve`) — Verter must PRESERVE the `defer` modifier through
        // IDE-TSX codegen: the deferred namespace binding still resolves (hover on
        // a member surfaces its unique name) and no spurious diagnostic is emitted
        // for the deferred import on the carrier.
        let defer_uri = session.open_fixture_file("src/WithDefer.vue").await;
        if !session
            .require_or_skip_ready(&defer_uri, "{{ count }}", 3, "count")
            .await
        {
            return;
        }
        // Positive: the deferred namespace member resolves to its typed value.
        // `deferred.ts` exports `deferredValueOnly = "deferred"`; the script binds
        // `const value = deferred.deferredValueOnly`. Hover on the member surfaces
        // its unique resolved name — proving the `import defer * as deferred`
        // namespace binding resolved through Verter's preserved `defer` modifier.
        let ns_member_pos = session.find_position(&defer_uri, "deferred.deferredValueOnly", 9);
        let ns_member_hover = session
            .hover_text(&defer_uri, ns_member_pos)
            .await
            .unwrap_or_else(|| panic!("hover on the deferred namespace member should return a result"));
        assert!(
            ns_member_hover.contains("deferredValueOnly"),
            "the `import defer * as deferred` namespace binding must resolve its \
             member `deferredValueOnly` (the `defer` modifier was preserved); got: {ns_member_hover}"
        );
        // Negative: preserving `import defer` must not introduce a spurious
        // module-not-found error for the deferred module on the carrier. (The
        // fixture vendors no `vue`, so an ambient `vue`-not-found TS2307 + the
        // JSX-intrinsics noise are expected and unrelated — we key on the
        // `./deferred` specifier specifically.)
        let defer_diags = session.merged_diagnostics(&defer_uri).await;
        assert!(
            !defer_diags.iter().any(|d| {
                matches!(
                    d.code.as_ref(),
                    Some(tower_lsp_server::ls_types::NumberOrString::String(s)) if s == "2307"
                ) && d.message.contains("deferred")
            }),
            "an `import defer` of an existing module must resolve (no TS2307 for \
             ./deferred); got: {defer_diags:?}"
        );
    }
);

// ---------------------------------------------------------------------------
// Edge import forms on the bundler fixture: side-effect, dynamic, broken path
// ---------------------------------------------------------------------------
//
// Same fixture, separate session: characterizes the non-value-component import
// forms. Side-effect imports register NO component binding (Verter-owned); a
// broken path surfaces module-not-found (TS2307); a bare dynamic-import arrow
// (`() => import('./X.vue')`) rendered as a tag is recorded as a template usage
// but is NOT carrier-linked (its `import_source` stays None — async-component
// discovery is a tracked gap whose desired linkage the dynamic-import companion
// gap test asserts).

real_provider_test!(
    import_core_bundler_edge,
    fixture = "import_core_bundler",
    async fn run(session) {
        let uri = session.open_fixture_file("src/EdgeImports.vue").await;
        session.open_fixture_file("src/DirectComp.vue").await;

        // Verter-owned: side-effect + dynamic imports register NO template
        // component value binding. Assert on the analysis snapshot regardless of
        // provider warmth (a pure Verter classification).
        let analysis = session
            .server()
            .test_documents()
            .get_analysis(&uri)
            .expect("EdgeImports.vue analysis should be present");
        // Assert UNCONDITIONALLY (EdgeImports.vue has a `<template>`) so a missing
        // template FAILS rather than silently no-op'ing past the carrier checks.
        let template = analysis
            .template
            .as_ref()
            .expect("EdgeImports.vue should have a <template>");
        // Verter-owned classification, keyed on carrier LINKAGE (import_source),
        // not on whether a tag happens to be parsed as a template usage:
        //
        // A side-effect import (`import './sideEffect'`) registers no component
        // binding at all, so no template component links back to it.
        assert!(
            !template
                .components
                .iter()
                .any(|c| c.import_source.as_deref() == Some("./sideEffect")),
            "a side-effect import must NOT register a carrier-linked template component; got: {:?}",
            template
                .components
                .iter()
                .map(|c| (&c.name, &c.import_source))
                .collect::<Vec<_>>()
        );
        // A bare dynamic-import arrow (`const Lazy = () => import('./DirectComp.vue')`)
        // rendered as `<Lazy directOnly="a" />` IS recorded as a template usage,
        // but it is NOT carrier-linked: its `import_source` stays None, so no
        // Verter-owned carrier feature targets `./DirectComp.vue` through it. The
        // render forces the discrimination — were the arrow wrongly classified as
        // a carrier-linked component, a `Lazy` entry would carry
        // `import_source = Some("./DirectComp.vue")`. (The desired future linkage
        // is the `#[ignore]`'d `dynamic_import_component_is_carrier_linked_*`
        // companion; this characterizes the current non-linkage.)
        assert!(
            !template
                .components
                .iter()
                .any(|c| c.name == "Lazy"
                    && c.import_source.as_deref() == Some("./DirectComp.vue")),
            "a bare dynamic-import arrow bound to `Lazy` must NOT be carrier-linked \
             to ./DirectComp.vue (import_source stays None); got: {:?}",
            template
                .components
                .iter()
                .filter(|c| c.name == "Lazy")
                .map(|c| (&c.name, &c.import_source))
                .collect::<Vec<_>>()
        );

        // Provider semantic failure: the broken import path surfaces TS2307.
        if !session
            .require_or_skip_ready(&uri, "{{ count }}", 3, "count")
            .await
        {
            return;
        }
        let diags = session.merged_diagnostics(&uri).await;
        assert!(
            diags.iter().any(|d| {
                matches!(
                    d.code.as_ref(),
                    Some(tower_lsp_server::ls_types::NumberOrString::String(s)) if s == "2307"
                ) && d.message.contains("does-not-exist")
            }),
            "the broken import path must surface module-not-found (TS2307) for \
             ./does-not-exist; got: {diags:?}"
        );
    }
);

// A `defineAsyncComponent(() => import('./Comp.vue'))` IS discovered by Verter
// as a carrier-linked template component: the analyzer captures the static
// loader specifier on the binding initializer (`async_component_source`) and the
// template converter surfaces it in the carrier-linkage map, so the `<Lazy>`
// usage carries `import_source = Some("./DirectComp.vue")` — Verter-owned carrier
// features (auto-import, go-to-definition into the carrier, rename spanning the
// carrier) now target it. This keys on the Verter-owned carrier LINKAGE (not
// provider-inferred hover, which always showed the props via TS). Discriminating:
// a regression that drops async-component loader capture FAILS. A BARE arrow
// (`() => import(...)`) without `defineAsyncComponent` stays unlinked — pinned by
// `import_core_bundler_edge`.
#[tokio::test(flavor = "multi_thread")]
async fn dynamic_import_component_is_carrier_linked_tsserver() {
    let Some(session) = crate::test_harness::TestSessionBuilder::new(
        crate::test_harness::TestProviderKind::Tsserver,
    )
    .fixture("import_core_bundler")
    .build()
    .await
    else {
        return;
    };
    let uri = session
        .open_virtual(
            "src/DynamicConsumer.vue",
            "<script setup lang=\"ts\">\n\
             import { ref, defineAsyncComponent } from 'vue'\n\
             const Lazy = defineAsyncComponent(() => import('./DirectComp.vue'))\n\
             const count = ref(0)\n\
             </script>\n\
             <template>\n<div>{{ count }}</div>\n<Lazy directOnly=\"a\" />\n</template>\n",
        )
        .await;
    session.open_fixture_file("src/DirectComp.vue").await;
    let _ = session
        .wait_until_ready(&uri, "{{ count }}", 3, "count")
        .await;
    let analysis = session
        .server()
        .test_documents()
        .get_analysis(&uri)
        .expect("DynamicConsumer.vue analysis should be present");
    let lazy = analysis
        .template
        .as_ref()
        .and_then(|t| t.components.iter().find(|c| c.name == "Lazy"))
        .expect("the `<Lazy>` component usage should be recorded");
    assert_eq!(
        lazy.import_source.as_deref(),
        Some("./DirectComp.vue"),
        "a dynamically-imported component should be carrier-linked to its `.vue` \
         source; got import_source={:?}",
        lazy.import_source
    );
    session.shutdown().await;
}

// ---------------------------------------------------------------------------
// isolatedDeclarations — carrier codegen declaration-safety characterization
// ---------------------------------------------------------------------------
//
// Under `isolatedDeclarations: true` + `declaration: true`, the provider rejects
// any exported binding whose type cannot be emitted without inference (TS9xxx).
// This is a carrier/codegen declaration-safety characterization, NOT import
// resolution: Verter's generated IDE TSX for a `.vue` must NOT introduce
// spurious TS9xxx isolated-declaration errors. Verified on BOTH providers: the
// only diagnostics are the ambient `vue`-not-found / JSX-intrinsics noise (the
// fixtures vendor no `vue`), never a TS9xxx — and `<IsoChild>` still resolves.

real_provider_test!(
    import_isolated_declarations_no_spurious_9xxx,
    fixture = "import_isolated_declarations",
    async fn run(session) {
        let uri = session.open_fixture_file("src/App.vue").await;
        session.open_fixture_file("src/IsoChild.vue").await;

        if !session.require_or_skip_ready(&uri, "{{ count }}", 3, "count").await {
            return;
        }

        let diags = session.merged_diagnostics(&uri).await;
        // No TS9xxx isolated-declaration-family diagnostic may originate from
        // Verter's generated carrier TSX.
        let iso_decl_err = diags.iter().find(|d| {
            matches!(
                d.code.as_ref(),
                Some(tower_lsp_server::ls_types::NumberOrString::String(s))
                    if s.starts_with('9') && s.len() == 4
            )
        });
        assert!(
            iso_decl_err.is_none(),
            "Verter's generated carrier TSX must not introduce a spurious TS9xxx \
             isolated-declaration error; got: {iso_decl_err:?} (all: {diags:?})"
        );
        // And the imported child still resolves its props under isolatedDeclarations.
        assert_tag_hover_has_prop(session, &uri, "<IsoChild", "isoChildOnly").await;
    }
);
