//! P2.A regression — the dispatch's cold-publish → warm-hit boundary
//! preserves path-precise `Parse(...)` facts observed during the cold
//! build.
//!
//! ## Discrimination contract
//!
//! Pre-fix the dispatch's `install_fact_tracer` wrapper discarded the
//! traced `Arc<[FactVersionRef]>` in the `Ok` arm with a `_`-prefix
//! underscore binding. `warm_publish_one` then derived the
//! `MemoEntry.fact_dep_signature` from
//! `crate::component_meta_materialize::fact_signature_from_fence` —
//! a legacy bridge that only converts `DepVersion::WholeHash` entries
//! to `FactVersionRef::FileWholeHash`. Every `Parse(...)`,
//! `ResolveImports(...)`, and `RouteSurface(...)` observation made
//! during the cold build was silently dropped before the memo entry
//! landed in the warm cache.
//!
//! Post-fix the `Ok` arm sets
//! `output.fact_dep_signature = Some(fact_dep_signature)` on
//! `QueryBuildOutput`; `execute_cooperative_slow` destructures the
//! field; `warm_publish_one` stores it verbatim on the published
//! `MemoEntry`. The warm-hit fast path (`try_warm_hit_fast_path`)
//! bubbles the stored signature into the active outer tracer via
//! `bubble_fact_signature_via_tls` — delivering the full path-precise
//! observation set to every outer cold-compute scope that depends on
//! the cached value.
//!
//! ## Driver shape
//!
//! 1. T1 (cold path): arm the dispatch's test-only Parse-fact injection
//!    slot via `for_tests::dispatch_inject_parse_fact_for_tests`. The
//!    next cold build observes the injected `Parse(...)` fact onto the
//!    tracer cell `install_fact_tracer` pushes for the inner build. On
//!    `Ok`, the signature carries the Parse fact verbatim. The fix
//!    threads it onto `QueryBuildOutput.fact_dep_signature`; the memo
//!    publishes it on `MemoEntry.fact_dep_signature`.
//! 2. T2 (warm path): drop the injection guard so the next dispatch
//!    does NOT re-observe the fact organically. Open an outer
//!    `with_fact_tracer` scope, re-issue the same dispatch key — the
//!    warm-hit fast path fires, bubbles `MemoEntry.fact_dep_signature`
//!    into the outer tracer. The outer tracer's finalised signature
//!    MUST contain the Parse fact.
//!
//! Reverting any one of these three sites makes the test fail:
//! - `project_semantic_dispatch::mod.rs::execute` Ok arm setting
//!   `output.fact_dep_signature`.
//! - `semantic_query_memo::mod.rs::execute_cooperative_slow`
//!   destructuring the new field.
//! - `semantic_query_memo::mod.rs::warm_publish_one` preferring the
//!   traced signature over the legacy fence bridge.

use std::sync::Arc;
use std::sync::Mutex;

use verter_semantic::facts::registry::InternedName;
use verter_semantic::facts::{FactKey, FactLane, SymbolSpace};

use verter_session::for_tests::{
    dispatch_execute_type_node_for_tests, dispatch_inject_parse_fact_for_tests,
    install_fact_tracer_for_tests,
};
use verter_session::resolver_core::{FactReadSetFinalise, FactVersionRef, ParseFactRef};
use verter_session::semantic_query::{ResolveDeclKey, ScopeId, SemanticQueryKey};
use verter_session::{CompileErrorPolicy, FileKind, HostConfig, UpsertRequest, VerterHost};

// Serialise this test against any concurrent test that arms the same
// process-global Parse-fact injection slot.
static MUTEX: Mutex<()> = Mutex::new(());

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
}

/// Construct a `Parse(...)` fact the warm-hit path will both bubble
/// AND validate.
///
/// The semantic-graph warm-read path validates a `MemoEntry`'s carrier
/// strictly BEFORE bubbling its fact rail — a stale entry must not
/// pollute the outer tracer. So a fact that survives the cold-publish →
/// warm-hit transition must also *validate* against the live store
/// view, otherwise the entry is (correctly) rejected and never bubbles.
///
/// This fact is keyed on the real tracked file `/w/types.ts` with a
/// member name that file does NOT export (`InjectedExport`) and the
/// zero-hash sentinel: `StoreView::validates_parse_domain` accepts an
/// absent parse fact whose observed hash is the zero sentinel
/// ("consistent absence"). The fact is therefore a genuine traced
/// observation that the warm-hit validator accepts — exercising the
/// cold-publish → warm-hit → outer-tracer thread without tripping the
/// validate-before-bubble gate.
fn injected_parse_fact() -> FactVersionRef {
    FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/w/types.ts".to_string(),
        key: FactKey::Export {
            name: InternedName::from("InjectedExport"),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: [0u8; 16],
    })
}

#[test]
fn dispatch_warm_hit_bubbles_traced_parse_fact_into_outer_tracer() {
    let _serial = MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let host = host();

    // Seed a tiny TS file so the dispatch has a real ResolveDecl key
    // to chew on. The cold build resolves `Foo` from this file; the
    // test-only injection hook fires the Parse fact onto the active
    // tracer cell inside the dispatch's `install_fact_tracer` scope
    // before the inner build runs.
    upsert_ts(&host, "/w/types.ts", "export type Foo = { x: number }");

    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from("/w/types.ts"),
            local_scope: None,
        },
        name: Arc::from("Foo"),
    });

    let want = injected_parse_fact();

    // T1 — cold path. Arm the injection slot, dispatch once.
    // The cold build observes the injected Parse fact onto the
    // install_fact_tracer cell; on Ok, the dispatch threads the
    // signature onto QueryBuildOutput.fact_dep_signature, and
    // execute_cooperative_slow → warm_publish_one persists it on
    // MemoEntry.fact_dep_signature.
    let cold_result = {
        let _inject_guard = dispatch_inject_parse_fact_for_tests(want.clone());
        dispatch_execute_type_node_for_tests(&host, key.clone())
    };
    // Discriminating side-check: the cold dispatch returned a real
    // result. Catches a regression where future refactoring turns the
    // dispatch into an unconditional short-circuit (which would mean
    // the inner build never ran and the injection hook never fired).
    use verter_session::semantic_query::QueryResult;
    assert!(
        matches!(cold_result, QueryResult::Value(_)),
        "cold dispatch must return Value (the inner build must have run); got {cold_result:?}"
    );

    // T2 — warm path. Install an OUTER fact tracer and re-issue the
    // same dispatch key. The warm-hit fast path bubbles the stored
    // MemoEntry.fact_dep_signature into the outer tracer's TLS cell
    // via `bubble_fact_signature_via_tls`. The outer tracer's
    // finalised signature MUST contain the injected Parse fact.
    let ((), warm_finalise) = install_fact_tracer_for_tests(&host, || {
        // The injection slot is dropped by the cold-path guard above;
        // the warm dispatch does NOT re-observe the Parse fact
        // organically. Any presence in the outer tracer's finalised
        // signature comes from the warm-hit bubble, not from a fresh
        // cold compute.
        let _warm = dispatch_execute_type_node_for_tests(&host, key.clone());
    });

    match warm_finalise {
        FactReadSetFinalise::Ok(sig) => {
            assert!(
                sig.iter().any(|f| f == &want),
                "P2.A regression: outer tracer's finalised signature MUST contain the \
                 injected Parse fact bubbled from the warm MemoEntry. \
                 Pre-fix: `execute()`'s Ok arm discarded the traced \
                 `fact_dep_signature` with `_`-prefix binding; \
                 `warm_publish_one` derived MemoEntry.fact_dep_signature \
                 from the legacy DepSignature via `fact_signature_from_fence`, \
                 which only carries FileWholeHash facts; the Parse fact \
                 silently dropped before reaching the warm cache. Post-fix: \
                 the traced signature threads through QueryBuildOutput → \
                 warm_publish_one → MemoEntry.fact_dep_signature → \
                 warm-hit bubble → outer tracer.\n\
                 \n\
                 got signature: {sig:?}\n\
                 expected to contain: {want:?}"
            );
        }
        FactReadSetFinalise::Overflow => {
            panic!(
                "outer tracer overflowed — test setup error (injected only one fact, \
                 should not overflow FACT_SIGNATURE_CAP=1024)"
            );
        }
    }
}
