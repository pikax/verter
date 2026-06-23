//! Import-resolution characterization matrix (tsserver + TSGO).
//!
//! Four hermetic fixtures, one broad `real_provider_test!` per fixture, each
//! opening multiple files and running multiple assertions in the SAME provider
//! session (so the provider-spawn count stays at four per provider, not one per
//! import form). The matrix characterizes how Verter's carrier-IDE projection +
//! the shared workspace import resolver behave across:
//!
//!  - `import_core_bundler`     — `module: preserve` / `moduleResolution: bundler`
//!                                / `verbatimModuleSyntax`: relative / alias /
//!                                baseUrl / named / namespace / barrel forms.
//!  - `import_nodenext_packages`— `nodenext` + a vendored package with `exports`
//!                                and package `#imports`.
//!  - `import_refs_monorepo`    — composite project references (cross-project
//!                                component carrier).
//!  - `import_syntax_passthrough`— TS 6/7 syntax (import attributes, `import
//!                                defer`, deprecated `assert`, isolatedDeclarations).
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

        if !session.wait_until_ready(&uri, "{{ count }}", 3, "count").await {
            eprintln!("skipping: provider not warmed up");
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

        // type-only import (`import type { OnlyAType }`) — Verter-owned
        // classification: it must NOT register a template component value
        // binding. Read the analysis snapshot and assert the import is flagged
        // type-only AND no component usage links back to its source.
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
        if let Some(template) = &analysis.template {
            assert!(
                !template
                    .components
                    .iter()
                    .any(|c| c.import_source.as_deref() == Some("./types")),
                "a type-only import must NOT register a template component; \
                 components: {:?}",
                template
                    .components
                    .iter()
                    .map(|c| (&c.name, &c.import_source))
                    .collect::<Vec<_>>()
            );
        }
    }
);

// TRACKED GAP: a namespaced component tag (`<widgets.WidgetFromStar>`, reached
// via `import * as widgets from './widgets'`) does NOT resolve to its carrier's
// props. The tag is recorded in the template-component analysis with
// `import_source = None` (no link to a carrier), and hover at the tag returns the
// namespace import binding (`import { widgets } from './widgets'`), not the
// component's props. Supporting it requires resolving a dotted component tag
// through its namespace binding to the member's carrier — an IDE-codegen +
// analysis change, not an import-resolver change. This test asserts the DESIRED
// behavior (props resolve) so it FAILS today and PASSES once namespaced tags are
// supported; it is discriminating, not a stub.
#[ignore = "tracked gap: namespaced component tag <ns.Comp> does not resolve to the member carrier's props (analysis records import_source=None)"]
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

// TRACKED GAP (tgo-only): tgo's pull-diagnostics for a Verter-generated carrier
// `.tsx` do NOT resolve tsconfig `paths` aliases, so a `@/`-aliased import
// surfaces `TS2307 Cannot find module '@/...'` in the merged diagnostics —
// even though tgo HOVER on the same carrier resolves `@/` correctly, and
// tsserver's diagnostics DO resolve it. The carrier-diagnostics program tgo
// builds is missing the tsconfig path-mapping the hover request has. This test
// asserts the DESIRED behavior (no TS2307 for the `@/` module) so it FAILS
// today on tgo and PASSES once tgo's carrier-diagnostics program is configured
// with the project's path mappings; it is discriminating, not a stub.
#[ignore = "tracked gap: tgo carrier-diagnostics program does not resolve tsconfig @/ path aliases (emits TS2307 where hover resolves)"]
#[tokio::test(flavor = "multi_thread")]
async fn tgo_carrier_diagnostics_resolve_path_alias_tsgo() {
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
        .wait_until_ready(&uri, "{{ count }}", 3, "count")
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
            "tgo carrier diagnostics must resolve the `@/components` alias \
             (no TS2307); got: {diags:?}"
        );
    }
    session.shutdown().await;
}
