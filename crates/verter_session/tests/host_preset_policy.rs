//! Host preset / resource-policy + query-profile drift pins (public API only).
//!
//! Covers the two query-profile fixes and the Batch analysis-scope boundary:
//!   - the live session query profile is now SOURCED from `HostConfig`
//!     (default `LspInteractive`, NOT a hardcoded `Build`);
//!   - `HostConfig::from_query_profile` sets BOTH the profile and the scope;
//!   - `HostConfig::batch_typecheck()` keys its analysis scope off the
//!     carrier-affecting `AnalysisScope::BUILD` bitset (incl. STYLE_VBIND /
//!     STYLE_SCOPED), NOT `QueryProfile::Build`'s recommended bits (which
//!     drop the style facts that feed carrier bytes — the profile-bits
//!     style-omission pitfall).
//!
//! Public-API only (`HostConfig` presets + `effective_scope` + the public
//! `host.query_profile()` accessor), so it lives as an integration test.

use verter_semantic::analysis::AnalysisScope;
use verter_semantic::profile::QueryProfile;
use verter_session::{HostConfig, VerterHost};

// ── query_profile flows from config (was hardcoded `Build`) ──

/// A host built with `default()` / `lsp_interactive()` reports the
/// INTERACTIVE profile — NOT `Build`. Reverting the `host_construction`
/// fix (hardcoded `QueryProfile::Build`) flips this to `Build` → RED.
#[test]
fn default_and_lsp_interactive_hosts_report_interactive_query_profile() {
    let default_host = VerterHost::new_standalone(HostConfig::default());
    assert_eq!(
        default_host.query_profile(),
        QueryProfile::LspInteractive,
        "default() host must report the interactive query profile, not the \
         hardcoded `Build` the drift bug left behind"
    );

    let lsp_host = VerterHost::new_standalone(HostConfig::lsp_interactive());
    assert_eq!(
        lsp_host.query_profile(),
        QueryProfile::LspInteractive,
        "lsp_interactive() host must report the interactive query profile"
    );
}

/// A host built with `batch_typecheck()` reports the `Build` profile.
#[test]
fn batch_typecheck_host_reports_build_query_profile() {
    let host = VerterHost::new_standalone(HostConfig::batch_typecheck());
    assert_eq!(
        host.query_profile(),
        QueryProfile::Build,
        "batch_typecheck() host must report the Build query profile"
    );
}

/// The config field itself carries the right defaults (the value the host
/// sources at construction).
#[test]
fn host_config_query_profile_field_defaults() {
    assert_eq!(
        HostConfig::default().query_profile,
        QueryProfile::LspInteractive,
        "HostConfig::default().query_profile must be LspInteractive"
    );
    assert_eq!(
        HostConfig::lsp_interactive().query_profile,
        QueryProfile::LspInteractive,
    );
    assert_eq!(
        HostConfig::batch_typecheck().query_profile,
        QueryProfile::Build,
    );
}

// ── from_query_profile sets BOTH profile and scope ──

/// `from_query_profile(p)` sets `query_profile == p` AND the recommended
/// `analysis_scope` for at least two distinct profiles. Reverting the fix
/// (only the scope set, profile left at the default) makes the
/// `query_profile` assertions RED.
#[test]
fn from_query_profile_sets_both_profile_and_scope() {
    for profile in [QueryProfile::Build, QueryProfile::LspInteractive] {
        let config = HostConfig::from_query_profile(profile);
        assert_eq!(
            config.query_profile, profile,
            "from_query_profile({profile:?}) must set the query_profile field to {profile:?}"
        );
        let expected_scope =
            AnalysisScope::from_bits_truncate(profile.recommended_analysis_scope_bits());
        assert_eq!(
            config.analysis_scope,
            Some(expected_scope),
            "from_query_profile({profile:?}) must set analysis_scope to the profile's \
             recommended bits"
        );
        // The two profiles produce DIFFERENT scopes, so this is not vacuous.
        assert_eq!(
            config.effective_scope(),
            expected_scope,
            "effective_scope() must reflect the from_query_profile scope"
        );
    }

    // Distinctness control — the two profiles really do differ in scope, so
    // the per-profile assertions above are discriminating.
    assert_ne!(
        HostConfig::from_query_profile(QueryProfile::Build).effective_scope(),
        HostConfig::from_query_profile(QueryProfile::LspInteractive).effective_scope(),
        "Build and LspInteractive must map to different recommended scopes"
    );
}

// ── Batch analysis-scope boundary (carrier-affecting facts) ──

/// `batch_typecheck()`'s effective analysis scope is EXACTLY the
/// carrier-affecting `AnalysisScope::BUILD` bitset, and INCLUDES the style
/// facts STYLE_VBIND + STYLE_SCOPED. Keeping these bits is deliberate policy
/// conservatism: it holds Batch on the byte-identical `stored_styles`
/// (parse-clone) analysis branch rather than the fresh artifact-rebuild
/// branch a narrowed scope would force.
///
/// This boundary test — together with
/// `batch_scope_is_not_the_build_profile_bits_which_omit_style` — is the
/// discriminator that flips RED when the batch scope is narrowed to drop a
/// BUILD fact. (The Full-vs-Batch carrier-parity test does NOT guard this
/// bit: the IDE carrier re-derives style v-bind from the parse, not from the
/// analysis scope, so it is invariant to STYLE_VBIND scope membership.)
#[test]
fn batch_typecheck_effective_scope_is_build_with_style_facts() {
    let scope = HostConfig::batch_typecheck().effective_scope();

    assert_eq!(
        scope,
        AnalysisScope::BUILD,
        "batch_typecheck() effective scope must be exactly AnalysisScope::BUILD"
    );

    // The carrier-affecting facts, named explicitly (the load-bearing pair
    // is STYLE_VBIND + STYLE_SCOPED — style v-bind feeds the generated
    // carrier bytes).
    for (flag, name) in [
        (AnalysisScope::IMPORTS, "IMPORTS"),
        (AnalysisScope::BINDINGS, "BINDINGS"),
        (AnalysisScope::MACROS, "MACROS"),
        (AnalysisScope::MACRO_TYPE_DEPS, "MACRO_TYPE_DEPS"),
        (AnalysisScope::EXPORT_SIGNATURES, "EXPORT_SIGNATURES"),
        (AnalysisScope::STYLE_VBIND, "STYLE_VBIND"),
        (AnalysisScope::STYLE_SCOPED, "STYLE_SCOPED"),
    ] {
        assert!(
            scope.contains(flag),
            "batch_typecheck() scope must include the carrier-affecting fact {name}"
        );
    }
}

/// The profile-bits style-omission pitfall, guarded directly:
/// `batch_typecheck()` must NOT key its scope off
/// `QueryProfile::Build.recommended_analysis_scope_bits()`, because that set
/// OMITS the style facts. This proves the batch scope was taken from
/// `AnalysisScope::BUILD`, not the `Build` profile bits.
#[test]
fn batch_scope_is_not_the_build_profile_bits_which_omit_style() {
    let batch_scope = HostConfig::batch_typecheck().effective_scope();
    let build_profile_scope =
        AnalysisScope::from_bits_truncate(QueryProfile::Build.recommended_analysis_scope_bits());

    // Precondition (pins the pitfall exists): the Build PROFILE bits really do
    // omit STYLE_VBIND / STYLE_SCOPED.
    assert!(
        !build_profile_scope.contains(AnalysisScope::STYLE_VBIND),
        "precondition: QueryProfile::Build recommended bits must omit STYLE_VBIND \
         (if this changes, the pitfall no longer exists and this guard must be revisited)"
    );

    assert!(
        batch_scope.contains(AnalysisScope::STYLE_VBIND),
        "batch_typecheck() scope must include STYLE_VBIND (carrier-affecting)"
    );
    assert_ne!(
        batch_scope, build_profile_scope,
        "batch_typecheck() scope must NOT equal the Build PROFILE's recommended bits \
         (those omit the style facts that feed carrier bytes — the profile-bits \
         style-omission pitfall)"
    );
}
