//! Style v-bind compile inputs derive from the request's SINGLE source
//! snapshot.
//!
//! `style_v_bind_vars` is codegen-affecting compile input: the IDE
//! script wrapper emits a `void(name);` keep-alive for every setup
//! binding referenced only from style `v-bind()` expressions, so TS
//! does not flag it unused. The vars must come from the same coherent
//! source snapshot the compiled bytes and the cache key derive from.
//! Reading them from an independent analysis-snapshot lookup races the
//! scheduler's Source→Analysis commit window (source committed at the
//! node generation, analysis not yet): the racing compile observed
//! `None`, defaulted to EMPTY v-bind vars, and PUBLISHED the wrong
//! bytes warm under a fully-valid session key. The publish fence cannot
//! catch it — the source never moved — and nothing invalidates the slot
//! when the analysis commit lands.

use std::sync::Arc;

use crate::types::{
    BlockOverrideEntry, BlockOverrideRequest, CompileProfile, HostConfig, UpsertRequest,
};
use crate::VerterHost;

const CANONICAL: &str = "/proj/Themed.vue";

/// `themeColor` is referenced ONLY from the style block's `v-bind()` —
/// never from the script body, never from the template — so its
/// `void(themeColor);` keep-alive exists in the IDE TSX output iff the
/// compile input actually carried the style v-bind vars. (The VDOM
/// `_useCssVars` injection extracts its vars independently inside style
/// codegen, so only the IDE output discriminates the compile input.)
const SOURCE: &str = "<script setup lang=\"ts\">\nconst themeColor = 'red'\n</script>\n<template><div>x</div></template>\n<style>div { color: v-bind(themeColor); }</style>";

const VBIND_KEEPALIVE: &str = "void(themeColor);";

fn make_host() -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

fn upsert(host: &VerterHost) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(SOURCE),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(CANONICAL)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

/// The LSP's IDE-target profile: TSX codegen, the consumer of
/// `CompileInput.style_v_bind_vars`.
fn ide_profile() -> CompileProfile {
    CompileProfile {
        target: crate::CompileTarget::IDE,
        ..CompileProfile::default()
    }
}

/// Drive the IDE-only compile and report whether it was served warm.
///
/// Subject is the published TSX. The identity asks for the IDE product
/// only; warm/cold is read from the slot predicate, not a runtime
/// response this identity does not produce.
fn compile(host: &VerterHost) -> bool {
    let served_warm = host.compile_slot_is_warm(CANONICAL, &ide_profile());
    host.ensure_ide_compiled(CANONICAL, &ide_profile())
        .expect("compile must serve");
    served_warm
}

/// The published session slot's TSX — the LSP-facing surface the
/// keep-alive lands on. `None` when no admitted slot exists.
fn published_tsx(host: &VerterHost) -> Option<Arc<str>> {
    host.get_ide(CANONICAL, &ide_profile()).map(|r| r.code)
}

/// A compile running inside the Source→Analysis commit window must
/// carry the source snapshot's style v-bind vars, not an empty default
/// read off the absent analysis snapshot.
///
/// Discrimination: pre-fix the compile input read `style_v_bind_vars`
/// from an independent `try_get_analysis` lookup; with the analysis
/// slot empty it defaulted to NO vars, the published TSX lost the
/// `void(themeColor);` keep-alive, and — the source being unmoved — the
/// session publish admitted the wrong bytes warm. Post-fix the vars
/// derive from the request's single source snapshot
/// (`HostSourceData.parse.style_analyses`), so the window is
/// unobservable in the compiled output.
#[test]
fn compile_inside_the_analysis_commit_window_carries_the_snapshot_style_vbind_vars() {
    let host = make_host();
    upsert(&host);

    // Model the commit window deterministically: source committed at
    // the node generation, analysis slot empty.
    host.scheduler.test_clear_analysis(CANONICAL);
    assert!(
        host.scheduler.try_get_source(CANONICAL).is_some(),
        "window sanity: the source snapshot must still be served",
    );
    assert!(
        host.scheduler.try_get_analysis(CANONICAL).is_none(),
        "window sanity: the analysis snapshot must be absent",
    );

    let raced_served_warm = compile(&host);
    assert!(!raced_served_warm, "first compile must be cold");
    let raced_tsx = published_tsx(&host)
        .expect("the snapshot-coherent raced compile must publish its session slot");
    // THE PIN: the compile input's style v-bind vars come from the same
    // source snapshot as the compiled bytes, so the keep-alive survives
    // the analysis-absent window.
    assert!(
        raced_tsx.contains(VBIND_KEEPALIVE),
        "a compile racing the analysis commit must carry the snapshot's \
         style v-bind vars — empty vars compiled here would publish warm \
         under an unmoved key with no invalidation when the analysis lands",
    );

    // Snapshot-coherence equivalence: the same content compiled
    // quiescent (analysis present) is byte-identical — the commit
    // window must be unobservable in the compiled output.
    let quiescent_host = make_host();
    upsert(&quiescent_host);
    assert!(
        quiescent_host
            .scheduler
            .try_get_analysis(CANONICAL)
            .is_some(),
        "equivalence sanity: the quiescent host's analysis must be present",
    );
    let _ = compile(&quiescent_host);
    let quiescent_tsx = published_tsx(&quiescent_host).expect("the quiescent compile must publish");
    assert_eq!(
        raced_tsx, quiescent_tsx,
        "the analysis commit window must be unobservable in the compiled bytes",
    );

    // No over-decline: the raced compile's inputs are coherent with its
    // unmoved source, so the session publish must land and serve the
    // next request warm with the SAME bytes.
    let warm_served_warm = compile(&host);
    assert!(
        warm_served_warm,
        "the snapshot-coherent raced compile must still publish its \
         session slot — coherence is restored by reading the one \
         snapshot, not by declining publication",
    );
    let warm_tsx = published_tsx(&host).expect("warm slot must keep serving the TSX");
    assert_eq!(warm_tsx, raced_tsx, "warm TSX must be byte-identical");
}

/// No-over-decline negative control: a quiescent compile (analysis
/// present) publishes warm with the real v-bind vars. Also pins the
/// `void(themeColor);` marker itself — if the keep-alive emission ever
/// moves, this control fails alongside the window test, flagging a
/// stale marker rather than a regression.
#[test]
fn quiescent_compile_with_analysis_present_publishes_the_vbind_vars_warm() {
    let host = make_host();
    upsert(&host);
    assert!(
        host.scheduler.try_get_analysis(CANONICAL).is_some(),
        "control sanity: the analysis snapshot must be present",
    );

    let cold_served_warm = compile(&host);
    assert!(!cold_served_warm, "first compile must be cold");
    let cold_tsx = published_tsx(&host).expect("the quiescent compile must publish");
    assert!(
        cold_tsx.contains(VBIND_KEEPALIVE),
        "the quiescent compile must emit the v-bind keep-alive",
    );

    let warm_served_warm = compile(&host);
    assert!(warm_served_warm, "the quiescent entry must serve warm");
    let warm_tsx = published_tsx(&host).expect("warm slot must keep serving the TSX");
    assert_eq!(warm_tsx, cold_tsx, "warm TSX must be byte-identical");
}

#[test]
fn supplied_style_vbind_vars_are_hydrated_for_the_compile_profile() {
    let host = make_host();
    let source = "<script setup lang=\"ts\">\nconst suppliedOnly = 'blue'\n</script>\n<template><div>x</div></template>\n<style lang=\"customcss\">authored preprocessing input</style>";
    let update = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(source),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(CANONICAL)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert must request SCSS preprocessing");
    let request = update
        .preprocessor_requests
        .iter()
        .find(|request| request.lang == "customcss")
        .expect("the style must have one captured preprocessing request");
    let profile = ide_profile();
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id,
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry::supplied_for_test(
                request,
                "div { color: v-bind(suppliedOnly); }",
            )],
        })
        .expect("the supplied CSS must be admitted for the IDE profile");

    let source_snapshot = host
        .scheduler
        .try_get_source(CANONICAL)
        .expect("source snapshot must remain live");
    let source_data = source_snapshot
        .downcast_data::<crate::host_executor::HostSourceData>()
        .expect("source snapshot must carry host data");
    let hydrated = host.capture_compiler_style_content(
        CANONICAL,
        &source_data.parse.style_analyses,
        crate::block_content::SuppliedBlockScope::Profile(&profile),
    );
    assert!(hydrated.usage_complete);
    assert_eq!(hydrated.analyses.len(), 1);
    assert_eq!(hydrated.v_bind_vars, ["suppliedOnly"]);
    assert!(
        hydrated.analyses[0].v_binds.is_empty() && hydrated.analyses[0].css.is_none(),
        "compiler-only usage roots must not publish foreign-space spans"
    );

    let cold_served_warm = compile(&host);
    assert!(!cold_served_warm);
    let tsx = host
        .get_ide(CANONICAL, &profile)
        .expect("the supplied-style compile must publish IDE output")
        .code;
    assert!(
        tsx.contains("void(suppliedOnly);"),
        "style usage must be scanned from the profile-selected supplied CSS"
    );
}

/// Hydrate the compile-input style capture for one SFC source under the
/// typed canonical-request route's scope: no override was admitted, so the
/// unprofiled bucket is empty and the registered carrier source is the sole
/// content authority, which is the ordinary inline-`<style>` case.
fn capture_inline(source: &str) -> (Vec<String>, bool) {
    let host = make_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(source),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(CANONICAL)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
    capture(&host, crate::block_content::SuppliedBlockScope::Unprofiled)
}

/// Admit preprocessor output for the file's single style block and hydrate
/// the compile-input capture from it.
fn capture_supplied(script: &str, produced_css: &str) -> (Vec<String>, bool) {
    let host = make_host();
    let source = format!(
        "<script setup lang=\"ts\">\n{script}\n</script>\n<template><div>x</div></template>\n<style lang=\"customcss\">authored preprocessing input</style>"
    );
    let update = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(source.as_str()),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(CANONICAL)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert must request external preprocessing");
    let request = update
        .preprocessor_requests
        .iter()
        .find(|request| request.lang == "customcss")
        .expect("the style must have one captured preprocessing request");
    let profile = ide_profile();
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: update.canonical_id,
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry::supplied_for_test(request, produced_css)],
        })
        .expect("the supplied CSS must be admitted for the IDE profile");
    capture(
        &host,
        crate::block_content::SuppliedBlockScope::Profile(&profile),
    )
}

/// The compile-input style capture, reduced to what the liveness consumer
/// reads: the published usage names and whether they are an exhaustive set.
fn capture(
    host: &VerterHost,
    scope: crate::block_content::SuppliedBlockScope<'_>,
) -> (Vec<String>, bool) {
    let source_snapshot = host
        .scheduler
        .try_get_source(CANONICAL)
        .expect("source snapshot must remain live");
    let source_data = source_snapshot
        .downcast_data::<crate::host_executor::HostSourceData>()
        .expect("source snapshot must carry host data");
    let captured =
        host.capture_compiler_style_content(CANONICAL, &source_data.parse.style_analyses, scope);
    (captured.v_bind_vars, captured.usage_complete)
}

/// A style block whose surface reaches past the bytes this parse read never
/// reports a complete `v-bind()` inventory, on EITHER host route.
///
/// The consumer publishes "this binding is unused" — and the IDE demotes it to
/// a TS6133 — from a name's ABSENCE from this inventory, so the inventory has
/// to be exhaustive or say that it is not. The recorded `v-bind()` list cannot
/// say it: an `@import` swallowed inside a recovery window mints no inclusion,
/// no binding and no error, so a block that pulls in a whole other stylesheet
/// is indistinguishable from a self-contained one. Both host routes answered
/// from that list alone and published a complete surface for such a block.
#[test]
fn a_style_block_reaching_past_its_own_bytes_reports_an_incomplete_surface() {
    let script = "const tone = 'red'";
    // No inclusion in these bytes, and the `v-bind()` sits in a rule the
    // parse read cleanly, so the published name proves the bindings resolved
    // and the only thing left that can withhold completeness is the parse's
    // own record that it discarded input. Both halves are load-bearing: a
    // block whose bindings failed to resolve also reports incomplete, and
    // would not discriminate the check under test.
    let recovered = capture_inline(&format!(
        "<script setup lang=\"ts\">
{script}
</script>
<style>.a {{ color: v-bind(tone); }}
.b {{ content: \"unterminated
</style>"
    ));
    assert_eq!(
        recovered.0,
        ["tone"],
        "the recovered parse still publishes the rule it read: {recovered:?}"
    );
    assert!(
        !recovered.1,
        "a parse that skipped input cannot claim an exhaustive surface: {recovered:?}"
    );

    // The other half of the same question, with the bindings equally clean:
    // an inclusion names bytes this parse never saw.
    let included = capture_inline(&format!(
        "<script setup lang=\"ts\">
{script}
</script>
<style>@import \"theme.css\";
.a {{ color: v-bind(tone); }}</style>"
    ));
    assert_eq!(included.0, ["tone"], "{included:?}");
    assert!(
        !included.1,
        "an inclusion names foreign bytes: {included:?}"
    );

    let self_contained = capture_inline(&format!(
        "<script setup lang=\"ts\">\n{script}\n</script>\n<style>.a {{ color: v-bind(tone); }}</style>"
    ));
    assert!(
        self_contained.1,
        "a self-contained block still declares an exhaustive surface: {self_contained:?}"
    );
    assert_eq!(self_contained.0, ["tone"]);

    // Preprocessor output travels the other host route and must answer the
    // same question the same way.
    let supplied_import = capture_supplied(
        script,
        "@import \"theme.css\";\ndiv { color: v-bind(tone); }",
    );
    assert!(
        !supplied_import.1,
        "an inclusion in preprocessor output still names foreign bytes: {supplied_import:?}"
    );
    assert!(supplied_import.0.contains(&"tone".to_string()));
}

/// A block whose selected content is unavailable has not been surveyed and
/// therefore cannot publish an exhaustive empty usage inventory.
#[test]
fn unavailable_style_content_reports_an_incomplete_surface() {
    let captured = capture_inline(
        "<script setup lang=\"ts\">\nconst tone = 'red'\n</script>\n\
         <style lang=\"customcss\">.a { color: v-bind(tone); }</style>",
    );

    assert!(
        captured.0.is_empty(),
        "unavailable bytes cannot publish observed bindings: {captured:?}"
    );
    assert!(
        !captured.1,
        "unsurveyed bytes cannot publish an exhaustive empty inventory: {captured:?}"
    );
}

/// Preprocessed style bytes publish the same free identifier ROOTS every other
/// route publishes.
///
/// The liveness consumer matches a script binding's NAME against this list, so
/// publishing the whole expression for `v-bind(theme.primary)` records a name
/// no binding ever has and leaves `theme` looking unused — the same
/// wrong-unused direction, reached through the other host route.
#[test]
fn supplied_style_vbind_member_expressions_publish_their_root_binding() {
    let captured = capture_supplied(
        "const theme = { primary: 'red' }",
        "div { color: v-bind(theme.primary); }",
    );
    assert_eq!(
        captured.0,
        ["theme"],
        "the published usage is the expression's free roots, not its text"
    );
    assert!(captured.1, "{captured:?}");
}
