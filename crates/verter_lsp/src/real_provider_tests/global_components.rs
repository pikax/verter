//! Globally-registered component typing in the template surface (tsserver + TSGO).
//!
//! A component registered ONLY through the project's `GlobalComponents`
//! augmentation (`src/global-components.d.ts` registers `GlobalCountComp`) — no
//! local import — must type at its template tag through the synthesized
//! fallback const in EVERY script arm:
//!
//! * `<script setup>` (`GlobalTagSetup.vue`) and Options API
//!   (`GlobalTagOptions.vue`), each using the PascalCase tag, the kebab-case
//!   tag (rewritten to the Pascal const in the IDE TSX), and the
//!   `<component :is="'GlobalCountComp'">` string form;
//! * tag hover presents the component binding (never `const X: any` /
//!   `const X: unknown`);
//! * `:count` prop hover types `number`, and a mistyped `:count="'mistyped'"`
//!   produces a REAL type diagnostic (the `any` cascade is gone);
//! * go-to-definition on the tag lands in a REAL file (the component carrier or
//!   the registering `global-components.d.ts`), never empty, never a virtual
//!   suffix;
//! * negative control (`GlobalTagUnknown.vue`): an unregistered tag stays
//!   FAIL-CLOSED — a diagnostic is present and definition is EMPTY — never a
//!   silent `any`.

use super::super::test_harness::real_provider_test;
use tower_lsp_server::ls_types::*;

/// Materialise `@verter/types` under the single-project fixture's
/// `node_modules`, exactly as the production server's background init does
/// (`materialize_verter_types`). The harness does not run background init, and
/// the fallback consts' `GlobalComponentType` helper must resolve from disk on
/// BOTH provider surfaces (tsgo's `--api` checker resolves modules only from
/// the real filesystem).
fn materialize_fixture_verter_types() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../packages/vue-vscode/e2e/fixtures/single-project/node_modules/@verter/types",
        );
        std::fs::create_dir_all(&root).expect("create fixture @verter/types dir");
        std::fs::write(
            root.join("index.d.ts"),
            verter_session::VERTER_TYPES_STANDALONE_DTS,
        )
        .expect("write fixture @verter/types dts");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"@verter/types","types":"index.d.ts"}"#,
        )
        .expect("write fixture @verter/types manifest");
    });
}

/// Retry hover at a position until it returns content (provider warm-up).
async fn hover_with_retry(
    session: &crate::test_harness::RealProviderTestSession,
    uri: &Uri,
    position: Position,
) -> Option<String> {
    for attempt in 0..8 {
        session.ensure_synced(uri).await;
        if let Some(text) = session.hover_text(uri, position).await {
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
        if attempt < 7 {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
    }
    None
}

/// Assert one consumer file's global-component surface: typed tag hover
/// (Pascal + kebab), typed `:count` prop, real-file definition, and the
/// mistyped-prop diagnostic.
async fn assert_global_component_surface(
    session: &crate::test_harness::RealProviderTestSession,
    uri: &Uri,
    arm: &str,
) {
    // --- PascalCase tag hover: the fallback const resolves the component. ---
    let tag_pos = session.find_position(uri, "<GlobalCountComp :count=\"42\"", 3);
    let hover = hover_with_retry(session, uri, tag_pos)
        .await
        .unwrap_or_else(|| panic!("{arm}: hover on the PascalCase global tag must answer"));
    assert!(
        hover.contains("GlobalCountComp"),
        "{arm}: tag hover must present the component binding, got: {hover}"
    );
    assert!(
        !hover.contains("GlobalCountComp: any") && !hover.contains("GlobalCountComp: unknown"),
        "{arm}: tag hover must not degrade to any/unknown, got: {hover}"
    );

    // --- Kebab tag: hover resolves the SAME Pascal const (the rewritten JSX
    // identifier), its `:count` types number, and definition lands real. ---
    let kebab_pos = session.find_position(uri, "<global-count-comp", 3);
    let kebab_hover = hover_with_retry(session, uri, kebab_pos)
        .await
        .unwrap_or_else(|| panic!("{arm}: hover on the kebab global tag must answer"));
    assert!(
        kebab_hover.contains("GlobalCountComp"),
        "{arm}: kebab tag hover must resolve the Pascal component binding, got: {kebab_hover}"
    );
    assert!(
        !kebab_hover.contains("GlobalCountComp: any")
            && !kebab_hover.contains("GlobalCountComp: unknown"),
        "{arm}: kebab tag hover must not degrade to any/unknown, got: {kebab_hover}"
    );

    // --- Kebab tag TAIL: the LAST column of the authored tag name must map
    // (the whole-name overwrite left authored columns past the Pascal length
    // in a dead zone — hover answered at offset 3 but not at the tail). ---
    let kebab_tail_pos =
        session.find_position(uri, "<global-count-comp", "<global-count-comp".len() - 1);
    let kebab_tail_hover = hover_with_retry(session, uri, kebab_tail_pos)
        .await
        .unwrap_or_else(|| {
            panic!("{arm}: hover at the LAST column of the kebab tag name must answer (tail dead-zone)")
        });
    assert!(
        kebab_tail_hover.contains("GlobalCountComp"),
        "{arm}: tail-column hover must resolve the same component binding, got: {kebab_tail_hover}"
    );
    let kebab_tail_defs = session.definition_locations(uri, kebab_tail_pos).await;
    assert!(
        !kebab_tail_defs.is_empty(),
        "{arm}: definition at the LAST column of the kebab tag name must not be empty"
    );
    let kebab_count_pos = session.find_position(uri, ":count=\"7\"", 2);
    let kebab_count_hover = hover_with_retry(session, uri, kebab_count_pos)
        .await
        .unwrap_or_else(|| panic!("{arm}: hover on the kebab :count prop must answer"));
    assert!(
        kebab_count_hover.contains("number"),
        "{arm}: kebab-tag :count prop hover must type number, got: {kebab_count_hover}"
    );
    let kebab_defs = session.definition_locations(uri, kebab_pos).await;
    assert!(
        !kebab_defs.is_empty(),
        "{arm}: definition on the kebab global tag must not be empty"
    );
    for def in &kebab_defs {
        let path = def.uri.as_str();
        assert!(
            path.ends_with("GlobalCountComp.vue") || path.ends_with("global-components.d.ts"),
            "{arm}: kebab tag definition must land in the component or its registration, got: {path}"
        );
    }

    // --- `:count` prop types number (the prop cascade is resolved). ---
    let count_attr_pos = session.find_position(uri, ":count=\"42\"", 2);
    let count_hover = hover_with_retry(session, uri, count_attr_pos)
        .await
        .unwrap_or_else(|| panic!("{arm}: hover on the :count prop must answer"));
    assert!(
        count_hover.contains("number"),
        "{arm}: :count prop hover must type number, got: {count_hover}"
    );

    // --- `<component :is="'GlobalCountComp'">` string form types the prop. ---
    let is_count_pos = session.find_position(uri, ":count=\"9\"", 2);
    let is_count_hover = hover_with_retry(session, uri, is_count_pos)
        .await
        .unwrap_or_else(|| panic!("{arm}: hover on the :is-element :count prop must answer"));
    assert!(
        is_count_hover.contains("number"),
        "{arm}: the <component :is> string form must type :count as number, got: {is_count_hover}"
    );

    // --- Tag definition lands in a REAL file (never empty, no virtual suffix). ---
    let defs = session.definition_locations(uri, tag_pos).await;
    assert!(
        !defs.is_empty(),
        "{arm}: definition on the global tag must not be empty"
    );
    for def in &defs {
        let path = def.uri.as_str();
        assert!(
            path.ends_with("GlobalCountComp.vue") || path.ends_with("global-components.d.ts"),
            "{arm}: tag definition must land in the component or its registration, got: {path}"
        );
        assert!(
            !path.contains(".vue.tsx") && !path.contains(".vue.verter") && !path.contains(".d.vue"),
            "{arm}: tag definition must not leak a virtual carrier suffix, got: {path}"
        );
    }

    // --- Mistyped `:count="'mistyped'"` produces a REAL type diagnostic. ---
    let diags = session.merged_diagnostics(uri).await;
    let mistype_line = session.find_position(uri, ":count=\"'mistyped'\"", 2).line;
    let has_count_error = diags.iter().any(|d| {
        d.severity == Some(DiagnosticSeverity::ERROR) && d.range.start.line == mistype_line
    });
    assert!(
        has_count_error,
        "{arm}: the mistyped :count must carry a type diagnostic on its line \
         (the any-cascade would swallow it), got: {diags:?}"
    );
}

real_provider_test!(
    global_component_tag_typed_in_setup_arm,
    fixture = "single-project",
    async fn run(session) {
        materialize_fixture_verter_types();
        let uri = session.open_fixture_file("src/GlobalTagSetup.vue").await;
        let comp_uri = session.open_fixture_file("src/GlobalCountComp.vue").await;
        session.ensure_synced(&comp_uri).await;
        session.ensure_synced(&uri).await;

        // Warm-up on a stable local binding so a regression FAILS the
        // assertions below instead of vacuously skipping. Under require-mode
        // the skip itself is a hard failure (non-vacuity gate).
        if !session
            .wait_until_ready(&uri, "{{ pingMsg }}", 6, "pingMsg")
            .await
            && session.allow_warmup_skip("global_component_tag_typed_in_setup_arm")
        {
            return;
        }

        assert_global_component_surface(session, &uri, "setup").await;
    }
);

real_provider_test!(
    global_component_tag_typed_in_options_arm,
    fixture = "single-project",
    async fn run(session) {
        materialize_fixture_verter_types();
        let uri = session.open_fixture_file("src/GlobalTagOptions.vue").await;
        let comp_uri = session.open_fixture_file("src/GlobalCountComp.vue").await;
        session.ensure_synced(&comp_uri).await;
        session.ensure_synced(&uri).await;

        // Warm-up through a sibling setup file (the options file has no local
        // setup binding to complete on); the provider session is shared.
        let warm_uri = session.open_fixture_file("src/GlobalTagSetup.vue").await;
        if !session
            .wait_until_ready(&warm_uri, "{{ pingMsg }}", 6, "pingMsg")
            .await
            && session.allow_warmup_skip("global_component_tag_typed_in_options_arm")
        {
            return;
        }

        assert_global_component_surface(session, &uri, "options").await;

        // --- The event cascade resolves on the Options arm too (probe #8): the
        // `@ping` handler param types from the component's emit payload. ---
        let param_pos = session.find_position(&uri, "void evtPayload", "void ".len());
        let param_hover = hover_with_retry(session, &uri, param_pos)
            .await
            .expect("options: hover on the @ping handler param must answer");
        assert!(
            param_hover.contains("pingCode") || param_hover.contains("pingCount"),
            "options: the @ping param must type from the emit payload, got: {param_hover}"
        );
    }
);

real_provider_test!(
    global_component_unknown_tag_fails_closed,
    fixture = "single-project",
    async fn run(session) {
        materialize_fixture_verter_types();
        let uri = session.open_fixture_file("src/GlobalTagUnknown.vue").await;
        session.ensure_synced(&uri).await;

        if !session
            .wait_until_ready(&uri, "{{ anchorReady }}", 6, "anchorReady")
            .await
            && session.allow_warmup_skip("global_component_unknown_tag_fails_closed")
        {
            return;
        }

        // A diagnostic IS present at the unknown tag (fail-closed `unknown`
        // element — TS2604-class), proving the error was not traded for a
        // silent `any`.
        let diags = session.merged_diagnostics(&uri).await;
        let tag_line = session.find_position(&uri, "<TotallyUnknownComp", 3).line;
        let tag_line_errors: Vec<_> = diags
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR) && d.range.start.line == tag_line
            })
            .collect();
        assert!(
            !tag_line_errors.is_empty(),
            "unknown tag must carry a fail-closed diagnostic, got: {diags:?}"
        );

        // DISCRIMINATOR against the pre-fallback revert: without the emitted
        // const the tag is an UNRESOLVED IDENTIFIER — TS2304 ("Cannot find
        // name") / TS2552 — and hover renders `const TotallyUnknownComp: any`.
        // The fail-closed design instead produces the TS2604-class JSX-element
        // diagnostic from an `unknown`-typed const. Assert the diagnostic is
        // NOT the unresolved-identifier class.
        let code_num = |d: &Diagnostic| match &d.code {
            Some(NumberOrString::Number(n)) => Some(*n),
            Some(NumberOrString::String(s)) => s.parse::<i32>().ok(),
            None => None,
        };
        for d in &tag_line_errors {
            let code = code_num(d);
            assert!(
                code != Some(2304) && code != Some(2552),
                "unknown tag diagnostic must be the fail-closed JSX-element class, \
                 NOT unresolved-identifier TS2304/TS2552 (the pre-fallback revert), got: {d:?}"
            );
        }
        assert!(
            tag_line_errors.iter().any(|d| code_num(d) == Some(2604)),
            "unknown tag must carry the fail-closed TS2604 JSX-element diagnostic, got: {tag_line_errors:?}"
        );

        // Definition on the unknown tag is REQUIRED to be empty (there is no
        // real declaration to land on — an `unknown`-typed const has none).
        let tag_pos = session.find_position(&uri, "<TotallyUnknownComp", 3);
        let defs = session.definition_locations(&uri, tag_pos).await;
        assert!(
            defs.is_empty(),
            "unknown tag definition must stay empty (fail-closed), got: {defs:?}"
        );

        // Hover must render the fail-closed `unknown` const — NEVER `any` (the
        // pre-fallback revert's `const TotallyUnknownComp: any`), and never a
        // concrete component type.
        let hover = hover_with_retry(session, &uri, tag_pos).await;
        if let Some(hover) = hover {
            assert!(
                !hover.contains(": any"),
                "unknown tag hover must not render the silent-any revert form, got: {hover}"
            );
            assert!(
                !hover.contains("$props") && !hover.contains("DefineComponent"),
                "unknown tag hover must not present a component type, got: {hover}"
            );
            assert!(
                hover.contains("unknown") || hover.contains("GlobalComponentType"),
                "unknown tag hover must render the fail-closed unknown const \
                 (directly or through its GlobalComponentType alias), got: {hover}"
            );
        }
    }
);

real_provider_test!(
    custom_element_tag_stays_fail_open,
    fixture = "single-project",
    async fn run(session) {
        materialize_fixture_verter_types();
        let uri = session.open_fixture_file("src/CustomElementTag.vue").await;
        let comp_uri = session.open_fixture_file("src/GlobalCountComp.vue").await;
        session.ensure_synced(&comp_uri).await;
        session.ensure_synced(&uri).await;

        if !session
            .wait_until_ready(&uri, "{{ ceReady }}", 6, "ceReady")
            .await
            && session.allow_warmup_skip("custom_element_tag_stays_fail_open")
        {
            return;
        }

        // --- The web-component tag must carry NO diagnostic (fail-open, the
        // pre-fallback behavior of an authored custom-element tag). The
        // regression class: the kebab rewrite + fail-closed `unknown` const
        // produced a false TS2604 on every unregistered web-component tag. ---
        let diags = session.merged_diagnostics(&uri).await;
        let ce_line = session.find_position(&uri, "<x-status-badge", 3).line;
        let ce_line_errors: Vec<_> = diags
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR) && d.range.start.line == ce_line
            })
            .collect();
        assert!(
            ce_line_errors.is_empty(),
            "a custom-element tag must stay FAIL-OPEN (no TS2604-class diagnostic), got: {ce_line_errors:?}"
        );

        // Hover on the tag must not claim the fail-closed `unknown` const.
        let ce_pos = session.find_position(&uri, "<x-status-badge", 3);
        if let Some(hover) = session.hover_text(&uri, ce_pos).await {
            assert!(
                !hover.contains(": unknown"),
                "custom-element tag hover must not present the fail-closed unknown const, got: {hover}"
            );
        }

        // --- The kebab-ONLY registered global in the same file (no Pascal
        // occurrence) still types through the fail-open kebab fallback const:
        // `:count` resolves `number`, tag hover resolves the component. ---
        let count_pos = session.find_position(&uri, ":count=\"7\"", 2);
        let count_hover = hover_with_retry(session, &uri, count_pos)
            .await
            .expect("kebab-only registered global: :count hover must answer");
        assert!(
            count_hover.contains("number"),
            "kebab-only registered global must keep typing :count as number, got: {count_hover}"
        );
        let kebab_pos = session.find_position(&uri, "<global-count-comp", 3);
        let kebab_hover = hover_with_retry(session, &uri, kebab_pos)
            .await
            .expect("kebab-only registered global: tag hover must answer");
        assert!(
            kebab_hover.contains("GlobalCountComp"),
            "kebab-only registered global tag must resolve its component, got: {kebab_hover}"
        );
        assert!(
            !kebab_hover.contains("GlobalCountComp: any")
                && !kebab_hover.contains("GlobalCountComp: unknown"),
            "kebab-only registered global tag must not degrade, got: {kebab_hover}"
        );
    }
);
