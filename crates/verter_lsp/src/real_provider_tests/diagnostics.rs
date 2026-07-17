//! Real-provider diagnostics round-trip tests (tsserver + TSGO).
//!
//! These exercise the provider's own `get_diagnostics` contract against a real
//! backend process. They cover two things:
//!
//! - DEBT: a LIVE provider diagnostics round-trip for a type error (the deleted
//!   `test_e2e_tsgo_diagnostics_for_type_error` left this uncovered).
//! - GAP-2: tsserver-family diagnostics parity — the SYNTACTIC (parse-error) and
//!   SUGGESTION (unused-symbol hint) passes are now merged with the semantic set,
//!   so the tsserver-family providers reach parity with the native TS experience
//!   (and with TSGO's pull-diagnostics model, which already returns the full set).
//!   Reverting the GAP-2 merge to semantic-only makes the suggestion/syntactic
//!   assertions fail under tsserver (discriminating).
//!
//! Each test drives the REAL provider directly via `session.provider()` /
//! `session.open_in_provider()`. When the backend binary is unavailable the
//! session builder returns `None` and the generated test returns early — but when
//! the binary IS present the assertions are fail-closed (no vacuous skip past a
//! materialized provider).

use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString};
use verter_type_runtime::protocol::TypeDiagnosticSeverity;

use crate::test_harness::{real_provider_test, RealProviderTestSession};

fn lsp_diagnostic_code(diagnostic: &Diagnostic) -> Option<String> {
    diagnostic.code.as_ref().map(|code| match code {
        NumberOrString::Number(value) => value.to_string(),
        NumberOrString::String(value) => value.clone(),
    })
}

fn assert_framework_file_has_no_error_diagnostics(label: &str, diagnostics: &[Diagnostic]) {
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Some(DiagnosticSeverity::ERROR))
        .map(|diagnostic| {
            format!(
                "{}:{}",
                lsp_diagnostic_code(diagnostic).unwrap_or_default(),
                diagnostic.message
            )
        })
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "valid {label} must be error-clean through the public merged diagnostic path; got {errors:?}"
    );
}

// ---------------------------------------------------------------------------
// Vue's carrier-owned JSX environment must win over inferred ambient React.
// ---------------------------------------------------------------------------

real_provider_test!(
    diagnostics_vue_class_uses_official_jsx_without_tsconfig_override,
    fixture = "vue-parity",
    async fn run(session) {
        // The fixture intentionally has `jsx: preserve` and NO
        // `jsxImportSource`. The generated provider carrier must select Vue's
        // official JSX runtime per file; changing this valid `class` to React's
        // `className` would hide the exact production regression this pins.
        let ts_uri = session
            .open_fixture_file("src/diagnostics/JsxEnvironmentTs.vue")
            .await;
        let js_uri = session
            .open_fixture_file("src/diagnostics/JsxEnvironmentJs.vue")
            .await;

        assert!(
            session
                .wait_until_ready(&ts_uri, "{{ doubled }}", 4, "doubled")
                .await,
            "the TypeScript Vue carrier must materialize a typed template program before an empty diagnostic set can be accepted"
        );
        assert!(
            session
                .wait_until_ready(&js_uri, "{{ doubled }}", 4, "doubled")
                .await,
            "the JavaScript Vue carrier must materialize a typed template program before an empty diagnostic set can be accepted"
        );

        let ts_diagnostics = session.merged_diagnostics(&ts_uri).await;
        let js_diagnostics = session.merged_diagnostics(&js_uri).await;
        assert_framework_file_has_no_error_diagnostics("TypeScript Vue SFC", &ts_diagnostics);
        assert_framework_file_has_no_error_diagnostics("JavaScript Vue SFC", &js_diagnostics);
    }
);

// Proves Svelte's carrier-owned JSX environment for both TypeScript and
// JavaScript SFCs through the public diagnostic path.
real_provider_test!(
    diagnostics_svelte_class_uses_official_jsx_without_tsconfig_override,
    fixture = "svelte-parity",
    async fn run(session) {
        // These fixtures deliberately have no `jsxImportSource`. A valid Svelte
        // `class` attribute must resolve through Svelte's official element
        // types, without ambient React JSX types leaking into the carrier.
        let ts_uri = session
            .open_fixture_file("src/diagnostics/JsxEnvironmentTs.svelte")
            .await;
        let js_uri = session
            .open_fixture_file("src/diagnostics/JsxEnvironmentJs.svelte")
            .await;

        assert!(
            session
                .wait_until_ready(&ts_uri, "{doubled}", 1, "doubled")
                .await,
            "the TypeScript Svelte carrier must materialize a typed markup program before an empty diagnostic set can be accepted"
        );
        assert!(
            session
                .wait_until_ready(&js_uri, "{doubled}", 1, "doubled")
                .await,
            "the JavaScript Svelte carrier must materialize a typed markup program before an empty diagnostic set can be accepted"
        );

        for (label, uri) in [
            ("TypeScript Svelte SFC", &ts_uri),
            ("JavaScript Svelte SFC", &js_uri),
        ] {
            let diagnostics = session.merged_diagnostics(uri).await;
            assert_framework_file_has_no_error_diagnostics(label, &diagnostics);
            assert!(
                diagnostics.iter().all(|diagnostic| {
                    lsp_diagnostic_code(diagnostic).as_deref() != Some("7026")
                        && !diagnostic.message.contains("JSX.IntrinsicElements")
                        && !diagnostic.message.contains("React")
                }),
                "valid {label} markup must not leak an ambient React JSX environment or TS7026; got {diagnostics:?}"
            );
        }
    }
);

// ---------------------------------------------------------------------------
// Slot-content isolation must retain component prop checking.
// ---------------------------------------------------------------------------

real_provider_test!(
    diagnostics_vue_invalid_component_prop_remains_type_checked,
    fixture = "vue-parity",
    async fn run(session) {
        // Materialize the imported child carrier first. The real provider test
        // bypasses the workspace scanner, so an unopened framework dependency
        // is intentionally not fabricated by the harness.
        let child_uri = session
            .open_fixture_file("src/diagnostics/BadPropChild.vue")
            .await;
        session.ensure_synced(&child_uri).await;
        let uri = session
            .open_fixture_file("src/diagnostics/BadPropParent.vue")
            .await;

        // This positive control prevents the clean JSX-environment assertion
        // above from passing through a suppressed or disconnected diagnostic
        // channel. The child component requires `count: number`, while the
        // parent deliberately supplies a string.
        let diagnostics = session.merged_diagnostics(&uri).await;
        let assignability = diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.severity == Some(DiagnosticSeverity::ERROR)
                    && lsp_diagnostic_code(diagnostic).as_deref() == Some("2322")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            assignability.len(),
            1,
            "the deliberately invalid component prop must produce exactly one mapped assignability error; got {diagnostics:?}"
        );

        let prop_start = session.find_position(&uri, ":count", 0);
        let prop_end = session.find_position(
            &uri,
            ":count=\"'not-a-number'\"",
            ":count=\"'not-a-number'\"".len(),
        );
        assert!(
            assignability[0].range.start < prop_end
                && assignability[0].range.end > prop_start,
            "the component prop error must map onto the authored `:count` binding; got {:?}",
            assignability[0].range
        );
    }
);

// Exercises Svelte public-component prop checking and source mapping against
// both real TypeScript providers.
real_provider_test!(
    diagnostics_svelte_invalid_component_prop_remains_type_checked,
    fixture = "svelte-parity",
    async fn run(session) {
        // The real-provider harness has no workspace scanner, so materialize the
        // imported framework dependency before opening its consumer.
        let child_uri = session
            .open_fixture_file("src/diagnostics/BadPropChild.svelte")
            .await;
        session.ensure_synced(&child_uri).await;
        let uri = session
            .open_fixture_file("src/diagnostics/BadPropParent.svelte")
            .await;

        let child_state = session.provider_sync_state(&child_uri);
        let parent_state = session.provider_sync_state(&uri);
        let parent_provider_diagnostics = match parent_state
            .as_ref()
            .and_then(|state| state.ide_path.as_deref())
        {
            Some(path) => session.provider().get_diagnostics(path).await.unwrap_or_default(),
            None => Vec::new(),
        };
        let diagnostics = session.merged_diagnostics(&uri).await;
        let assignability = diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.severity == Some(DiagnosticSeverity::ERROR)
                    && lsp_diagnostic_code(diagnostic).as_deref() == Some("2322")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            assignability.len(),
            1,
            "the invalid Svelte child prop must produce exactly one mapped assignability error; public={diagnostics:?}; provider={parent_provider_diagnostics:?}; child_state={child_state:?}; parent_state={parent_state:?}"
        );

        let prop_start = session.find_position(&uri, "count={wrongCount}", 0);
        let prop_end = session.find_position(
            &uri,
            "count={wrongCount}",
            "count={wrongCount}".len(),
        );
        assert!(
            assignability[0].range.start < prop_end
                && assignability[0].range.end > prop_start,
            "the Svelte child-prop error must map onto the authored `count={{wrongCount}}` binding; got {:?}",
            assignability[0].range
        );
    }
);

// ---------------------------------------------------------------------------
// Authored-script unused diagnostics must survive the carrier source map.
// ---------------------------------------------------------------------------

real_provider_test!(
    diagnostics_vue_unused_script_binding_excludes_template_and_css_uses,
    fixture = "vue-parity",
    async fn run(session) {
        let uri = session
            .open_fixture_file("src/diagnostics/UnusedBindings.vue")
            .await;

        assert!(
            session
                .wait_until_ready(&uri, "templateOnly\"", 4, "templateOnly")
                .await,
            "the Vue carrier must materialize its typed template program before diagnostics are asserted"
        );

        session.ensure_synced(&uri).await;
        let provider_path = session
            .provider_sync_state(&uri)
            .and_then(|state| state.ide_path)
            .expect("a ready Vue carrier must have a committed IDE provider path");
        let provider_diagnostics = diagnostics_until_nonempty(session, &provider_path).await;
        let diagnostics = session.merged_diagnostics(&uri).await;
        assert_framework_file_has_no_error_diagnostics(
            "Vue SFC with script-, template-, and CSS-owned bindings",
            &diagnostics,
        );

        let unused = diagnostics
            .iter()
            .filter(|diagnostic| lsp_diagnostic_code(diagnostic).as_deref() == Some("6133"))
            .collect::<Vec<_>>();
        assert_eq!(
            unused.len(),
            1,
            "exactly the authored script-only binding must be reported unused; public={diagnostics:?}; provider={provider_diagnostics:?}"
        );

        let expected_start = session.find_position(&uri, "trulyUnused", 0);
        let expected_end = session.find_position(&uri, "trulyUnused", "trulyUnused".len());
        assert_eq!(
            unused[0].range.start, expected_start,
            "the unused diagnostic must start at the authored identifier"
        );
        assert_eq!(
            unused[0].range.end, expected_end,
            "the unused diagnostic must end at the authored identifier"
        );
        assert!(
            unused[0]
                .tags
                .as_ref()
                .is_some_and(|tags| tags.contains(&DiagnosticTag::UNNECESSARY)),
            "the unused diagnostic must retain TypeScript's Unnecessary tag; got {:?}",
            unused[0].tags
        );

        for live_binding in ["templateOnly", "cssOnly"] {
            let live_start = session.find_position(&uri, live_binding, 0);
            assert!(
                unused.iter().all(|diagnostic| {
                    diagnostic.range.end <= live_start || diagnostic.range.start > live_start
                }),
                "`{live_binding}` is live through the carrier projection and must not be reported unused; got {diagnostics:?}"
            );
        }
    }
);

real_provider_test!(
    diagnostics_svelte_unused_script_binding_excludes_markup_and_style_directive_uses,
    fixture = "svelte-parity",
    async fn run(session) {
        let uri = session
            .open_fixture_file("src/diagnostics/UnusedBindings.svelte")
            .await;
        session.ensure_synced(&uri).await;

        let provider_path = session
            .provider_sync_state(&uri)
            .and_then(|state| state.ide_path)
            .expect("a synced Svelte carrier must have a committed IDE provider path");
        let provider_diagnostics = diagnostics_until_nonempty(session, &provider_path).await;
        let diagnostics = session.merged_diagnostics(&uri).await;
        assert_framework_file_has_no_error_diagnostics(
            "Svelte SFC with script-, markup-, and style-directive-owned bindings",
            &diagnostics,
        );

        let unused = diagnostics
            .iter()
            .filter(|diagnostic| lsp_diagnostic_code(diagnostic).as_deref() == Some("6133"))
            .collect::<Vec<_>>();
        assert_eq!(
            unused.len(),
            1,
            "exactly the authored Svelte script-only binding must be reported unused; public={diagnostics:?}; provider={provider_diagnostics:?}"
        );

        let expected_start = session.find_position(&uri, "trulyUnused", 0);
        let expected_end = session.find_position(&uri, "trulyUnused", "trulyUnused".len());
        assert_eq!(unused[0].range.start, expected_start);
        assert_eq!(unused[0].range.end, expected_end);
        assert!(
            unused[0]
                .tags
                .as_ref()
                .is_some_and(|tags| tags.contains(&DiagnosticTag::UNNECESSARY)),
            "the Svelte unused diagnostic must retain TypeScript's Unnecessary tag; got {:?}",
            unused[0].tags
        );

        for live_binding in ["templateOnly", "styleDirectiveOnly"] {
            let live_start = session.find_position(&uri, live_binding, 0);
            assert!(
                unused.iter().all(|diagnostic| {
                    diagnostic.range.end <= live_start || diagnostic.range.start > live_start
                }),
                "`{live_binding}` is live through the Svelte markup projection and must not be reported unused; got {diagnostics:?}"
            );
        }
    }
);

/// Pull diagnostics for an open provider file, retrying briefly while the
/// inferred project warms up (a cold tsserver/TSGO project can return an empty
/// set on the first request before the program is built).
async fn diagnostics_until_nonempty(
    session: &RealProviderTestSession,
    provider_path: &str,
) -> Vec<verter_type_runtime::protocol::TypeDiagnostic> {
    let mut last = Vec::new();
    for attempt in 0..8 {
        match session.provider().get_diagnostics(provider_path).await {
            Ok(diags) if !diags.is_empty() => return diags,
            Ok(diags) => last = diags,
            Err(_) => {}
        }
        if attempt < 7 {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
    }
    last
}

// ---------------------------------------------------------------------------
// DEBT: a type error produces an Error diagnostic (both providers)
// ---------------------------------------------------------------------------

real_provider_test!(
    diagnostics_type_error_round_trip,
    fixture = "single-project",
    async fn run(session) {
        // A clean semantic type error: assigning a string to a `number`.
        let source = "export const broken: number = \"not a number\";\n";
        let path = session
            .open_in_provider("src/__diag_type_error.tsx", source)
            .await;

        let diags = diagnostics_until_nonempty(session, &path).await;

        assert!(
            !diags.is_empty(),
            "a real provider must produce a diagnostic for a type error (DEBT round-trip); got none"
        );
        assert!(
            diags
                .iter()
                .any(|d| matches!(d.severity, TypeDiagnosticSeverity::Error)),
            "the type error must surface as an Error-severity diagnostic, got: {diags:?}"
        );
        // TS2322 = "Type 'X' is not assignable to type 'Y'". Assert by code when
        // present (codes are stable across tsserver/TSGO); fall back to the
        // message text when a provider omits the numeric code.
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("2322")
                || d.message.contains("not assignable")),
            "the diagnostic must be the assignability error (2322), got: {diags:?}"
        );
    }
);

// ---------------------------------------------------------------------------
// GAP-2: the SUGGESTION pass (unused symbol) is merged in (both providers)
// ---------------------------------------------------------------------------

real_provider_test!(
    diagnostics_includes_suggestion_unused_symbol,
    fixture = "single-project",
    async fn run(session) {
        // A locally-declared symbol that is never read. tsserver/TSGO surface this
        // as a SUGGESTION-category diagnostic (unused-symbol hint, code 6133 family).
        // The pre-GAP-2 semantic-only tsserver path dropped it entirely.
        let source = "function unusedHelper() {\n  const neverRead = 42;\n  return 1;\n}\nexport { unusedHelper };\n";
        let path = session
            .open_in_provider("src/__diag_suggestion.tsx", source)
            .await;

        let diags = diagnostics_until_nonempty(session, &path).await;

        // A suggestion diagnostic carries Hint severity (tsserver "suggestion"
        // category → `TypeDiagnosticSeverity::Hint`) OR the unused-declaration
        // code (6133 / 6196 family). Either proves the suggestion pass was merged.
        let has_suggestion = diags.iter().any(|d| {
            matches!(d.severity, TypeDiagnosticSeverity::Hint)
                || matches!(
                    d.code.as_deref(),
                    Some("6133") | Some("6196") | Some("6138")
                )
                || d.message.contains("is declared but its value is never read")
                || d.message.contains("never used")
        });
        assert!(
            has_suggestion,
            "the unused-symbol SUGGESTION diagnostic must be present (GAP-2 merge); got: {diags:?}"
        );

        // ISSUE-3: the unused-symbol diagnostic must carry the `Unnecessary` tag
        // (tsserver `reportsUnnecessary` / TSGO native `tags`), so the editor
        // fades it. Without the tag a `.vue` unused import does NOT gray out.
        // Identify the unused-symbol diagnostic by its 6133-family code / message
        // (a non-6133 control diagnostic must NOT carry the tag).
        let unused = diags.iter().find(|d| {
            matches!(d.code.as_deref(), Some("6133") | Some("6196") | Some("6138"))
                || d.message.contains("is declared but its value is never read")
                || d.message.contains("never used")
        });
        // The `const neverRead = 42` is an unused local: a real provider MUST
        // surface the 6133-family unused-symbol diagnostic. Assert its presence
        // UNCONDITIONALLY so the tag assertion below can never be skipped (a
        // missing 6133 must fail the test, not pass it vacuously).
        let unused = unused.unwrap_or_else(|| {
            panic!("the unused `neverRead` local must surface a 6133-family diagnostic, got: {diags:?}")
        });
        assert!(
            unused
                .tags
                .contains(&verter_type_runtime::protocol::TypeDiagnosticTag::Unnecessary),
            "the unused-symbol diagnostic must carry the Unnecessary tag, got: {:?}",
            unused.tags
        );
        // Negative: no NON-unused diagnostic should spuriously carry Unnecessary.
        for d in &diags {
            let is_unused = matches!(d.code.as_deref(), Some("6133") | Some("6196") | Some("6138"))
                || d.message.contains("is declared but its value is never read")
                || d.message.contains("never used");
            if !is_unused {
                assert!(
                    !d.tags
                        .contains(&verter_type_runtime::protocol::TypeDiagnosticTag::Unnecessary),
                    "a non-unused diagnostic must NOT carry the Unnecessary tag, got: {d:?}"
                );
            }
        }
    }
);

// ---------------------------------------------------------------------------
// ISSUE-3: a `@deprecated` symbol usage carries the Deprecated tag (both providers)
// ---------------------------------------------------------------------------

real_provider_test!(
    diagnostics_deprecated_symbol_carries_deprecated_tag,
    fixture = "single-project",
    async fn run(session) {
        // A `@deprecated` function whose USE surfaces a deprecation diagnostic.
        // tsserver flags it via `reportsDeprecated`; TSGO via the native LSP tag 2.
        // Both must normalize onto the `Deprecated` carrier tag (strikethrough).
        let source = "/** @deprecated use newApi instead */\nexport function oldApi() {}\noldApi();\n";
        let path = session
            .open_in_provider("src/__diag_deprecated.tsx", source)
            .await;

        let diags = diagnostics_until_nonempty(session, &path).await;

        // The deprecation diagnostic is the one carrying the Deprecated tag (its
        // message text varies across provider/TS versions, so identify it by tag).
        let deprecated = diags
            .iter()
            .find(|d| {
                d.tags
                    .contains(&verter_type_runtime::protocol::TypeDiagnosticTag::Deprecated)
            });
        assert!(
            deprecated.is_some(),
            "a `@deprecated` symbol usage must surface a diagnostic carrying the \
             Deprecated tag (strikethrough), got: {diags:?}"
        );
        // Negative: the Deprecated tag must be the ONLY tag on a pure-deprecation
        // diagnostic (a deprecated-but-used symbol is not also "unnecessary").
        let dep = deprecated.unwrap();
        assert!(
            !dep.tags
                .contains(&verter_type_runtime::protocol::TypeDiagnosticTag::Unnecessary),
            "a used (non-unused) deprecated symbol must NOT carry the Unnecessary tag, got: {:?}",
            dep.tags
        );
    }
);

// ---------------------------------------------------------------------------
// GAP-2: the SYNTACTIC pass (parse error) is merged in (both providers)
// ---------------------------------------------------------------------------

real_provider_test!(
    diagnostics_includes_syntactic_parse_error,
    fixture = "single-project",
    async fn run(session) {
        // A pure parse error: a missing closing brace. tsserver surfaces this via
        // the SYNTACTIC pass (`syntacticDiagnosticsSync`); a semantic-only path
        // would miss it. TSGO's pull model returns it natively.
        let source = "export function brokenSyntax() {\n  return 1;\n";
        let path = session
            .open_in_provider("src/__diag_syntax.tsx", source)
            .await;

        let diags = diagnostics_until_nonempty(session, &path).await;

        assert!(
            !diags.is_empty(),
            "a parse error must produce a diagnostic via the syntactic pass (GAP-2); got none"
        );
        assert!(
            diags
                .iter()
                .any(|d| matches!(d.severity, TypeDiagnosticSeverity::Error)),
            "the parse error must surface as an Error-severity diagnostic, got: {diags:?}"
        );
    }
);

// ---------------------------------------------------------------------------
// ISSUE-7: an unused `<script setup>` local surfaces TS6133 on its decl range
// ---------------------------------------------------------------------------
//
// These open the EXACT IDE-codegen TSX shape the Vue script-setup lowering
// produces, directly in the provider, and prove the unused-binding liveness
// fix end to end against a real backend:
//
//  - The POST-fix unused shape OMITS the binding from the `___VERTER___unwrapped`
//    object AND the destructure block entirely. The original `const foo` is then
//    its sole occurrence and never value-read, so TS6133 fires at its decl range.
//  - The control shape keeps `foo` value-read (`foo: foo as unknown as typeof
//    foo`, the shape used for a binding that IS used somewhere) → NO TS6133.
//  - A regression guard pins the ROOT CAUSE: the retired type-only entry
//    (`foo: undefined as unknown as typeof foo`) does NOT fire TS6133 on the
//    source decl, because `typeof foo` is itself a use of `foo` — so that shape
//    silently dropped the diagnostic (it landed instead on the unmapped
//    destructure copy and collapsed to line 1). The omission shape is the only
//    one that lands TS6133 on the source decl.
//
// Vacuous-skip aware: the generated test returns early when the backend binary
// is unavailable (no `node_modules`); when present, assertions are fail-closed.

real_provider_test!(
    vue_unused_script_setup_local_omitted_from_unwrap_flags_6133,
    fixture = "single-project",
    async fn run(session) {
        // Faithful reduction of the IDE script-setup lowering for an unused
        // top-level `const foo = 1` (used in neither template nor script). The
        // binding is OMITTED from the unwrapped object + destructure block, so
        // `const foo` is genuinely unused and TS6133 fires at its declaration.
        // `___VERTER___unwrapped` is kept live by `void` (the all-omitted case).
        let source = "\
export function ___VERTER___TemplateBindingFN() {
const foo = 1
const ___VERTER___unwrapped = ___VERTER___shallowUnwrapRef({});
void ___VERTER___unwrapped;
return {};
}
declare function ___VERTER___shallowUnwrapRef<T>(o: T): T;
";
        let path = session
            .open_in_provider("src/__diag_vue_unused_local.tsx", source)
            .await;

        let diags = diagnostics_until_nonempty(session, &path).await;

        let foo_decl = source.find("const foo").expect("decl present") as u32;
        let foo_decl_end = foo_decl + "const foo = 1".len() as u32;

        let unused_foo = diags.iter().find(|d| {
            (matches!(d.code.as_deref(), Some("6133"))
                || d.message.contains("is declared but its value is never read")
                || d.message.contains("never used"))
                // The diagnostic must land on the `foo` decl, not elsewhere.
                && d.start < foo_decl_end
                && d.end > foo_decl
        });
        assert!(
            unused_foo.is_some(),
            "omitting the unused binding from the unwrap surface must surface TS6133 \
             at the `const foo` decl range; got: {diags:?}"
        );
        // The keep-alive temp must NOT itself be flagged.
        let unused_temp = diags.iter().any(|d| {
            matches!(d.code.as_deref(), Some("6133"))
                && d.message.contains("___VERTER___unwrapped")
        });
        assert!(
            !unused_temp,
            "the `void ___VERTER___unwrapped` keep-alive must prevent a spurious \
             TS6133 on the temp; got: {diags:?}"
        );
    }
);

real_provider_test!(
    vue_value_read_unwrap_does_not_flag_used_local,
    fixture = "single-project",
    async fn run(session) {
        // Control: the value-read unwrap entry (`foo: foo as ...`, the shape used
        // for a binding that IS used somewhere) keeps `foo` live, so no unused
        // diagnostic for `foo` is produced — proving the fix discriminates.
        let source = "\
export function ___VERTER___TemplateBindingFN() {
const foo = 1
const ___VERTER___unwrapped = { foo: foo as unknown as typeof foo };
void ___VERTER___unwrapped;
return {};
}
";
        let path = session
            .open_in_provider("src/__diag_vue_used_local.tsx", source)
            .await;

        // Allow the project to warm; we expect NO unused-foo diagnostic.
        let diags = diagnostics_until_nonempty(session, &path).await;

        let foo_decl = source.find("const foo").expect("decl present") as u32;
        let foo_decl_end = foo_decl + "const foo = 1".len() as u32;
        let unused_foo = diags.iter().any(|d| {
            matches!(d.code.as_deref(), Some("6133"))
                && d.start < foo_decl_end
                && d.end > foo_decl
        });
        assert!(
            !unused_foo,
            "value-read unwrap entry must keep `foo` live (no TS6133 on its decl); got: {diags:?}"
        );
    }
);

real_provider_test!(
    vue_type_only_unwrap_does_not_flag_source_decl_regression,
    fixture = "single-project",
    async fn run(session) {
        // ROOT-CAUSE regression guard: the RETIRED type-only entry
        // (`foo: undefined as unknown as typeof foo`) does NOT fire TS6133 on the
        // SOURCE `const foo`, because `typeof foo` is a type-query REFERENCE to
        // `foo` that keeps the decl live. This is exactly why that shape failed:
        // the diagnostic never landed on the source decl. If a future change
        // re-introduces the `typeof foo` keep-alive for an unused binding, this
        // test FAILS — proving the omission shape is load-bearing.
        let source = "\
export function ___VERTER___TemplateBindingFN() {
const foo = 1
const ___VERTER___unwrapped = { foo: undefined as unknown as typeof foo };
void ___VERTER___unwrapped;
return {};
}
";
        let path = session
            .open_in_provider("src/__diag_vue_typeonly_local.tsx", source)
            .await;

        let diags = diagnostics_until_nonempty(session, &path).await;

        let foo_decl = source.find("const foo").expect("decl present") as u32;
        let foo_decl_end = foo_decl + "const foo = 1".len() as u32;
        let flags_source_decl = diags.iter().any(|d| {
            matches!(d.code.as_deref(), Some("6133"))
                && d.start < foo_decl_end
                && d.end > foo_decl
        });
        assert!(
            !flags_source_decl,
            "the retired `typeof foo` keep-alive must NOT flag the source decl \
             (it keeps `foo` live) — this is the bug the omission shape fixes; got: {diags:?}"
        );
    }
);

// ---------------------------------------------------------------------------
// K4: a diagnostic's `relatedInformation` ("see declaration here") survives the
// provider parse (both backends)
// ---------------------------------------------------------------------------
//
// An interface with a duplicate member of a CONFLICTING type
// (`x: number; x: string`) produces TS2717 ("Subsequent property declarations
// must have the same type") carrying a `relatedInformation` span that points at
// the OTHER `x` declaration in the SAME file ("'x' was also declared here").
//
// Per-backend wire reality (both verified to emit the related span):
//  - tsserver always includes the `relatedInformation` array (each span has its
//    own `file`) on its diagnostic response — no client-capability gate.
//  - TSGO (LSP) only attaches `Diagnostic.relatedInformation` when the client
//    advertises `publishDiagnostics.relatedInformation` (the same silent-degrade
//    class as the tag/completion capabilities) — now advertised in
//    `build_client_capabilities`. WITHOUT that capability tsgo strips the related
//    spans entirely (the pre-fix tree), so this test is discriminating: pre-fix
//    the carrier `related_information` was always empty under both backends (no
//    parser read it; tsgo additionally never sent it).
//
// The related span resolves to a REAL same-file byte offset on the SECOND `x`
// (not a [0,0] degenerate / line-0 packed fallback). Vacuous-skip aware: the
// generated test returns early when the backend binary is unavailable; when
// present the assertions are fail-closed.

real_provider_test!(
    diagnostics_carry_related_information_for_duplicate_member,
    fixture = "single-project",
    async fn run(session) {
        // `x` declared twice with conflicting types → TS2717 with a related span
        // pointing back at the first `x` declaration in the same file.
        let source = "interface Dup {\n  x: number;\n  x: string;\n}\nexport type { Dup };\n";
        let path = session
            .open_in_provider("src/__diag_related_dupmember.tsx", source)
            .await;

        let diags = diagnostics_until_nonempty(session, &path).await;

        // Identify the diagnostic carrying related information by its presence
        // (the exact code is TS2717 on both backends, but key on the related span
        // so the test stays robust to per-backend code drift).
        let with_related = diags
            .iter()
            .find(|d| !d.related_information.is_empty())
            .unwrap_or_else(|| {
                panic!(
                    "the duplicate-member diagnostic must carry a relatedInformation \
                     span (K4); got: {diags:?}"
                )
            });

        let ri = &with_related.related_information[0];
        assert!(
            !ri.message.is_empty(),
            "the related span must carry its message, got: {ri:?}"
        );
        // The related path is the same opened file (each provider spells it its
        // own way — match on the file basename to stay portable).
        assert!(
            ri.path.contains("__diag_related_dupmember"),
            "the same-file related span must point at the opened file, got path: {}",
            ri.path
        );
        // Real byte offset, never the [0,0] degenerate / line-0 packed fallback.
        assert_ne!(
            (ri.start, ri.end),
            (0, 0),
            "the same-file related span must resolve to a real byte range, got: {ri:?}"
        );
        assert!(
            ri.end > ri.start,
            "the related span must be a non-empty range, got: {ri:?}"
        );
        // The related byte range must slice one of the `x` member identifiers in
        // the source (it points at the OTHER `x` declaration) — proving the
        // same-file conversion produced a genuine offset, not a sentinel.
        let span = source.get(ri.start as usize..ri.end as usize);
        assert_eq!(
            span,
            Some("x"),
            "the related byte range must slice the `x` member identifier, got: {span:?} \
             from {ri:?}"
        );
    }
);
