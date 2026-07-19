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
        // assertions below instead of vacuously skipping.
        if !session
            .wait_until_ready(&uri, "{{ pingMsg }}", 6, "pingMsg")
            .await
        {
            eprintln!("SKIP global_component_tag_typed_in_setup_arm: provider never warmed up");
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
        {
            eprintln!("SKIP global_component_tag_typed_in_options_arm: provider never warmed up");
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
        {
            eprintln!("SKIP global_component_unknown_tag_fails_closed: provider never warmed up");
            return;
        }

        // A diagnostic IS present at the unknown tag (fail-closed `unknown`
        // element — TS2604-class), proving the error was not traded for a
        // silent `any`.
        let diags = session.merged_diagnostics(&uri).await;
        let tag_line = session.find_position(&uri, "<TotallyUnknownComp", 3).line;
        let has_tag_error = diags.iter().any(|d| {
            d.severity == Some(DiagnosticSeverity::ERROR) && d.range.start.line == tag_line
        });
        assert!(
            has_tag_error,
            "unknown tag must carry a fail-closed diagnostic, got: {diags:?}"
        );

        // Definition on the unknown tag is REQUIRED to be empty (there is no
        // real declaration to land on — an `unknown`-typed const has none).
        let tag_pos = session.find_position(&uri, "<TotallyUnknownComp", 3);
        let defs = session.definition_locations(&uri, tag_pos).await;
        assert!(
            defs.is_empty(),
            "unknown tag definition must stay empty (fail-closed), got: {defs:?}"
        );

        // Hover must not claim a concrete component type for the tag.
        if let Some(hover) = session.hover_text(&uri, tag_pos).await {
            assert!(
                !hover.contains("$props") && !hover.contains("DefineComponent"),
                "unknown tag hover must not present a component type, got: {hover}"
            );
        }
    }
);
