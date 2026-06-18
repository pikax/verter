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

use verter_type_runtime::protocol::TypeDiagnosticSeverity;

use crate::test_harness::{real_provider_test, RealProviderTestSession};

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
