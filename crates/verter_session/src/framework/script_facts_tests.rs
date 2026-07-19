//! Unit tests for the resolved-validation script-fact seam: the
//! content-addressed candidate store, the fact-rail-validated resolved-fact
//! store, the strict-same-generation gate, and the two admission rails the
//! entry-point folds together (a fenced import-route serve and a
//! fact-signature overflow).

use super::*;
use crate::cache_runtime::SignatureAdmission;
use crate::resolver_core::{FactVersionRef, PermissiveStoreView, StoreView, StoreViewCompatToken};
use verter_semantic::analysis::framework_facts::FrameworkScriptCandidates;

fn candidate_key(canonical: &str, content: [u8; 16]) -> CandidateSlotKey {
    CandidateSlotKey {
        canonical: Arc::from(canonical),
        content_hash: content,
        parse_env_hash: [0u8; 16],
        parser_version: crate::file_artifact_store::LEGACY_PARSER_VERSION,
        file_language_id: crate::file_artifact_store::FileArtifactKey::derived_file_language_id(
            canonical,
        ),
        provider_id: FrameworkAdapterId::new("fixture-fw"),
        provider_version: 1,
    }
}

fn fixture_candidates() -> FrameworkScriptCandidates {
    FrameworkScriptCandidates {
        adapter_id: FrameworkAdapterId::new("fixture-fw"),
        provider_version: 1,
        stable_hash: [1u8; 16],
        payload: Arc::new(()),
    }
}

#[test]
fn candidate_store_is_content_addressed_hit_and_version_miss() {
    let store = FrameworkScriptCandidateStore::new();
    let key = candidate_key("/a.ts", [3u8; 16]);
    store.insert(key.clone(), fixture_candidates());
    // Same key ⇒ hit.
    assert!(store.get(&key).is_some());
    // A content edit (different content_hash) ⇒ a DIFFERENT key ⇒ miss.
    let edited = candidate_key("/a.ts", [4u8; 16]);
    assert!(store.get(&edited).is_none());
    // A provider upgrade (different provider_version) ⇒ miss.
    let upgraded = CandidateSlotKey {
        provider_version: 2,
        ..key.clone()
    };
    assert!(store.get(&upgraded).is_none());
}

#[test]
fn carrier_parser_v3_candidate_is_rejected_by_v4_key() {
    const PREVIOUS_CARRIER_PARSER_VERSION: u32 = 3;
    assert_eq!(
        crate::file_artifact_store::LEGACY_PARSER_VERSION,
        PREVIOUS_CARRIER_PARSER_VERSION + 1
    );

    let store = FrameworkScriptCandidateStore::new();
    let current = candidate_key("/Fixture.vue", [3u8; 16]);
    let stale = CandidateSlotKey {
        parser_version: PREVIOUS_CARRIER_PARSER_VERSION,
        ..current.clone()
    };
    store.insert(stale.clone(), fixture_candidates());

    assert!(store.get(&stale).is_some(), "the planted v3 row exists");
    assert!(
        store.get(&current).is_none(),
        "the owner-exact v4 key rejects the v3 carrier candidate"
    );

    store.insert(current.clone(), fixture_candidates());
    assert!(
        store.get(&current).is_some(),
        "a v4 candidate roundtrips under the current key"
    );
}

fn fixture_payload() -> Arc<dyn FrameworkScriptFactPayload> {
    Arc::new(fixtures::FixtureFactPayload {
        resolved_specifier: "@corp/fixture-fw".to_string(),
    })
}

fn resolved_fact_key(canonical: &str) -> ResolvedFactKey {
    ResolvedFactKey {
        canonical: Arc::from(canonical),
        provider_id: FrameworkAdapterId::new("fixture-fw"),
        provider_version: 1,
        consumed_capability_bits: [0u8; 16],
        project_identity: [0u8; 16],
        resolve_env_hash: [0u8; 16],
    }
}

/// A view that REJECTS every fact — discriminates the fact-rail gate.
struct RejectingView;
impl StoreView for RejectingView {
    fn compat_token(&self) -> StoreViewCompatToken {
        StoreViewCompatToken {
            epoch: 0,
            session: None,
            validity_fingerprint: 0,
        }
    }
    fn validates(&self, _fact: &FactVersionRef) -> bool {
        false
    }
}

#[test]
fn resolved_fact_warm_read_requires_same_generation_and_fact_rail() {
    let store = FrameworkScriptFactStore::new();
    let key = resolved_fact_key("/a.ts");
    // Admit a Cacheable entry carrying a tracked cross-file fact at gen 5.
    let cross_file = FactVersionRef::FileWholeHash {
        canonical_id: "/node_modules/fixture-fw/index.d.ts".to_string(),
        hash: [9u8; 16],
    };
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(
        vec![cross_file].into_boxed_slice(),
    )));
    store.publish_if_cacheable(key.clone(), fixture_payload(), &admission, 5);

    // Same gen + permissive view ⇒ warm hit.
    assert!(store.get_with_view(&key, &PermissiveStoreView, 5).is_some());
    // Generation bump ⇒ miss (strict same-generation gate).
    assert!(store.get_with_view(&key, &PermissiveStoreView, 6).is_none());
    // Right gen but a view that rejects the tracked fact ⇒ miss (fact rail).
    assert!(store.get_with_view(&key, &RejectingView, 5).is_none());
}

#[test]
fn overflowed_admission_never_warms_the_store_return_only() {
    let store = FrameworkScriptFactStore::new();
    let key = resolved_fact_key("/a.ts");
    // An overflowed (NonCacheable) admission: the value is returned to the
    // caller but the store is NOT warmed (the no-poison invariant).
    let admission =
        SignatureAdmission::from_finalise(crate::resolver_core::FactReadSetFinalise::Overflow);
    let stored = store.publish_if_cacheable(key.clone(), fixture_payload(), &admission, 5);
    // The computed value is handed back...
    assert!(
        verter_semantic::analysis::framework_facts::downcast_fact_payload::<
            fixtures::FixtureFactPayload,
        >(Arc::clone(&stored.payload))
        .is_some()
    );
    // ...but the store stays empty — the overflowed result was NOT warmed.
    assert!(store.is_empty());
    assert!(store.get_with_view(&key, &PermissiveStoreView, 5).is_none());
}

#[test]
fn cacheable_admission_warms_exactly_one_entry() {
    let store = FrameworkScriptFactStore::new();
    let key = resolved_fact_key("/a.ts");
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::empty());
    store.publish_if_cacheable(key, fixture_payload(), &admission, 5);
    assert_eq!(store.len(), 1, "a Cacheable admission warms one entry");
}

use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_language::FileLanguage;

fn host_with_files() -> std::sync::Arc<VerterHost> {
    let host = std::sync::Arc::new(VerterHost::new_standalone(HostConfig::default()));
    // The framework package file (its canonical contains the package dir
    // the fixture provider's resolved-validation checks for).
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/node_modules/fixture-fw/index.ts".to_string(),
            source: Arc::from("export const marker = 1;"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert package file");
    // The consumer file importing the package by the GATED specifier.
    let _ = host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/Consumer.ts".to_string(),
            source: Arc::from(
                "import { marker } from './node_modules/fixture-fw/index';\nexport const x = marker;",
            ),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert consumer file");
    host
}

#[test]
fn fixture_provider_resolves_validates_and_caches_end_to_end() {
    let host = host_with_files();
    let registration = fixtures::import_gated_capability_free_fixture_registration();
    // First resolve: cold compute through capture → active-set selection →
    // resolved-validation → content-addressed cache.
    let facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
        &host,
        &registration,
        "/proj/Consumer.ts",
    )
    .expect("the consumer imports the framework package, so facts resolve");
    assert_eq!(facts.resolved_specifier, "./node_modules/fixture-fw/index");
    // The content-addressed candidate slot was filled.
    assert!(
        !host.framework_script_caches().candidates.is_empty(),
        "the candidate slot is filled on the cold capture"
    );
    // The resolved-fact slot warmed (a Cacheable admission).
    assert!(
        !host.framework_script_caches().facts.is_empty(),
        "the resolved fact warmed the store (Cacheable admission)"
    );
    // The cached entry's read-set ROOTS the payload against the owner's
    // IMPORT-ROUTE surface (not just whole-hashes) — a re-route that leaves
    // file contents unchanged must still invalidate. (If the ImportRoute
    // fact were dropped, this assertion fails.)
    let stored = host
        .framework_script_caches()
        .facts
        .only_entry()
        .expect("exactly one resolved fact is cached");
    let has_import_route_fact = stored.read_set_signature.facts.iter().any(|f| {
        matches!(
            f,
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                ..
            } if canonical_id == "/proj/Consumer.ts"
        )
    });
    assert!(
        has_import_route_fact,
        "the cached resolved fact must observe the owner's ImportRoute \
             derived fact so a re-route invalidates it"
    );
    // Second resolve: warm hit returns the same typed payload.
    let facts2 = resolve_script_facts::<fixtures::FixtureFactPayload>(
        &host,
        &registration,
        "/proj/Consumer.ts",
    )
    .expect("warm hit");
    assert_eq!(facts2.resolved_specifier, facts.resolved_specifier);
}

/// Inner-cache fenced-serve poison — the resolved-fact store must
/// REFUSE publication when a FENCED (ReturnOnly, `store_published == false`)
/// serve is consumed while resolving the owner's import route
/// (`resolve_snapshot_imports`, which reaches `ensure_indexed_ready_serve`).
/// The resolved import targets feed both the `fact_key` AND the validation,
/// so a fenced serve derives STALE targets while the payload's fact signature
/// validates against the live view. The import resolution runs BEFORE the
/// `provider.validate` fact tracer, so it needs its OWN traced scope; the
/// standalone entry-point (`request_ctx = None`) has NO enclosing tracer, so
/// this is the sole refusal covering it.
///
/// DISCRIMINATING: the unfenced control publishes the facts entry; the fenced
/// request must NOT (`facts.is_empty()`) while STILL serving the value to its
/// caller (ReturnOnly). RED-pre (`import_non_cacheable` / `validate_non_cacheable`
/// not consulted) the fenced facts entry LANDS and a later `get_with_view`
/// stale-serves it.
#[test]
fn fenced_import_serve_refuses_script_facts_publication() {
    use std::sync::atomic::Ordering;

    // Control — an UNFENCED resolve publishes the facts entry.
    let control = host_with_files();
    let registration = fixtures::import_gated_capability_free_fixture_registration();
    let facts_c = resolve_script_facts::<fixtures::FixtureFactPayload>(
        &control,
        &registration,
        "/proj/Consumer.ts",
    )
    .expect("control: the consumer imports the framework package, so facts resolve");
    assert_eq!(
        facts_c.resolved_specifier,
        "./node_modules/fixture-fw/index"
    );
    assert!(
        !control.framework_script_caches().facts.is_empty(),
        "control: an unfenced resolve warms the facts store (fixture invariant — otherwise \
             the fenced assertion is vacuous)",
    );

    // Fenced — every `ensure_indexed_ready_serve` (the owner's import-route
    // resolution rides one) is fenced at a STABLE generation, so the resolved
    // import targets derive from a served-without-publication basis.
    let host = host_with_files();
    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(true, Ordering::Relaxed);
    let facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
        &host,
        &registration,
        "/proj/Consumer.ts",
    );
    host.test_force
        .force_indexed_ready_serve_fence_for_tests
        .store(false, Ordering::Relaxed);

    // The value is still SERVED to THIS caller (ReturnOnly).
    assert!(
        facts.is_some(),
        "a fenced import-route serve still serves the caller (ReturnOnly)",
    );
    assert_eq!(
        facts.unwrap().resolved_specifier,
        "./node_modules/fixture-fw/index"
    );
    // ...but the facts entry is NOT published (poison refused).
    assert!(
        host.framework_script_caches().facts.is_empty(),
        "POISON: a fenced import-route serve consumed during script-fact resolution admitted \
             the facts entry — `import_non_cacheable` / `validate_non_cacheable` must refuse the \
             `publish_if_cacheable` admission, else a later `get_with_view` stale-serves the \
             poisoned facts (and the executor's Svelte-surface entry re-reads them)",
    );
}

/// The IMPORT-ROUTE resolution's cacheability tracer must refuse publication on
/// its OWN fact-signature OVERFLOW — the second, independent non-admission
/// condition alongside the fenced-serve rail the test above covers.
///
/// The facts entry's `ReadSetSignature` is built from the SIBLING
/// `provider.validate` tracer's finalised set, never from the import tracer's,
/// so an overflow seen only by the import tracer has nowhere else to surface: it
/// must fold into the import tracer's CACHEABILITY verdict, or it is dropped on
/// the floor and a compute whose curated signature provably does not cover
/// everything it read warms the store.
///
/// DISCRIMINATING — and the STICKY overflow knob canNOT discriminate here.
/// Arming the sticky knob overflows the sibling validation tracer too, whose
/// `SignatureAdmission::from_finalise` refuses publication INDEPENDENTLY; the
/// test would then pass even with the import tracer's overflow dropped. The
/// ONE-SHOT knob is armed FOR THE NAMED import-route scope and claimed by that
/// scope alone, leaving the validation tracer cacheable, so the ONLY thing that
/// can refuse the write is the boundary under test.
///
/// Four assertions pin exactly that:
///   0. the overflow was claimed BY the import-route scope — the attribution
///      check. A one-shot that was merely "consumed somewhere" proves nothing
///      about WHICH boundary overflowed;
///   1. the payload is still SERVED to the caller (ReturnOnly, never a refusal);
///   2. the resolved-fact store is NOT warmed — the load-bearing one;
///   3. the sibling VALIDATION tracer stayed CACHEABLE. Only a
///      signature-CONSUMING `install_fact_tracer` emits the overflow audit event
///      + host counter (the cacheability path peeks overflow without emitting),
///        so a ZERO counter says no signature-consuming boundary overflowed — i.e.
///        `provider.validate` finalised `Ok` and its `SignatureAdmission` was
///        Cacheable. That is what makes (2) attributable to the import tracer
///        ALONE rather than to a second, independent refusal.
///
/// Reverting the import boundary to a raw tracer whose finalise is discarded
/// makes `import_non_cacheable` false and the entry publishes: (2) fails.
#[test]
fn import_route_tracer_overflow_refuses_script_facts_publication() {
    use std::sync::atomic::Ordering;

    let over_cap = crate::resolver_core::FACT_SIGNATURE_CAP + 1;
    let registration = fixtures::import_gated_capability_free_fixture_registration();

    // Control — with no knob armed the fixture PUBLISHES, so the refusal below
    // is not vacuous.
    let control = host_with_files();
    let control_facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
        &control,
        &registration,
        "/proj/Consumer.ts",
    )
    .expect("control: the consumer imports the framework package, so facts resolve");
    assert_eq!(
        control_facts.resolved_specifier,
        "./node_modules/fixture-fw/index"
    );
    assert!(
        !control.framework_script_caches().facts.is_empty(),
        "control: an unarmed resolve warms the facts store (fixture invariant — otherwise \
             the overflow assertion is vacuous)",
    );

    // Overflowed — ONLY the NAMED import-route resolution scope observes above the
    // cap. The target is an identity, not a position: whatever else the flow opens,
    // before or after, this scope is the one that overflows.
    let host = host_with_files();
    crate::host_test_force::arm_fact_tracer_overflow_once(
        TracerScope::ScriptFactsImportRoute,
        over_cap,
    );
    let facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
        &host,
        &registration,
        "/proj/Consumer.ts",
    );

    // (0) The overflow was applied to the SCOPE UNDER TEST. Asserting only that the
    // one-shot was consumed would leave the boundary unattributed — the exact hole a
    // positional knob hides behind.
    assert_eq!(
        crate::host_test_force::fact_tracer_overflow_claimed_by(),
        Some(TracerScope::ScriptFactsImportRoute),
        "the forced overflow must be claimed BY the import-route scope — the boundary under \
             test. Any other claimant (or none) means the assertions below characterise a \
             different scope",
    );
    assert_eq!(
        crate::host_test_force::peek_fact_tracer_overflow_once(),
        None,
        "fixture invariant: the one-shot overflow knob must be CLAIMED inside the entry-point \
             (otherwise nothing overflowed and the assertions below are vacuous)",
    );

    // (1) The payload is still SERVED to this caller (ReturnOnly).
    assert!(
        facts.is_some(),
        "an overflowed import-route tracer still serves the caller (ReturnOnly): refusal is \
             CACHE-ONLY",
    );
    assert_eq!(
        facts.unwrap().resolved_specifier,
        "./node_modules/fixture-fw/index"
    );

    // (2) ...but the facts entry is NOT published. THE load-bearing assertion.
    assert!(
        host.framework_script_caches().facts.is_empty(),
        "POISON: an import-route resolution whose fact-signature OVERFLOWED admitted the \
             facts entry. The entry's signature comes from the SIBLING validate tracer, so the \
             import tracer's overflow has no other place to surface — it must fold into that \
             boundary's cacheability verdict, else a compute whose curated signature provably \
             does not cover everything it read warms the store",
    );

    // (3) The sibling VALIDATION tracer stayed CACHEABLE, so (2) is attributable
    // to the IMPORT tracer alone. Only a signature-CONSUMING `install_fact_tracer`
    // emits the overflow audit event + bumps this counter (the cacheability path
    // peeks overflow without emitting), so a ZERO counter says NO
    // signature-consuming boundary overflowed — `provider.validate` finalised `Ok`
    // and its `SignatureAdmission` was Cacheable, i.e. it did NOT refuse the write
    // independently. (The STICKY knob would overflow it too and this would read 1
    // — the reason the sticky knob cannot discriminate this boundary.)
    assert_eq!(
        host.signature_overflow_at_install.load(Ordering::Relaxed),
        0,
        "the one-shot knob must overflow ONLY the import-route tracer; a non-zero \
             signature-overflow counter means a signature-CONSUMING boundary overflowed — the \
             sibling `provider.validate` tracer — whose `SignatureAdmission` refusal would \
             refuse publication independently, making the assertion above non-discriminating",
    );
}

/// The one-shot overflow knob is claimed by scope IDENTITY, never by scope ORDER.
///
/// An ORDER-keyed one-shot ("the next tracer scope entered on this thread takes
/// it") is silently RETARGETED by any tracer scope that opens earlier — a scope
/// added UPSTREAM by an unrelated change, or simply an enclosing scope in a
/// different caller. The test above would then keep passing while overflowing a
/// completely different boundary, so it would characterise nothing.
///
/// This test reproduces exactly that hazard and pins the targeting against it: an
/// unrelated cacheability scope is opened AFTER arming and BEFORE the entry-point.
/// It is the "next scope entered", so an order-keyed knob hands it the count.
///
///   * `unrelated_non_cacheable` — the upstream scope's own verdict. Order-keyed:
///     it swallows `FACT_SIGNATURE_CAP + 1` synthetic observations and reports
///     `true`. Identity-keyed: it is UNNAMED, claims nothing, and reports `false`.
///   * the one-shot stays ARMED across that scope, and is then claimed by the
///     import-route scope inside the entry-point — the attribution rail proves it.
///   * the entry-point still refuses publication, i.e. the overflow landed on the
///     RIGHT scope even though a foreign scope ran first.
///
/// Reverting the claim to "next scope entered" fails the first two assertions
/// (the upstream scope non-cacheable, the one-shot disarmed before the
/// entry-point) and then the third (the entry-point's tracers see zero forced
/// observations, so the facts entry PUBLISHES).
#[test]
fn overflow_knob_targets_the_named_scope_not_the_next_scope_entered() {
    let over_cap = crate::resolver_core::FACT_SIGNATURE_CAP + 1;
    let registration = fixtures::import_gated_capability_free_fixture_registration();
    let host = host_with_files();

    crate::host_test_force::arm_fact_tracer_overflow_once(
        TracerScope::ScriptFactsImportRoute,
        over_cap,
    );

    // The silent-retarget hazard: an UNRELATED tracer scope opens first. Under an
    // order-keyed one-shot this scope consumes the count and overflows itself.
    let ((), unrelated_non_cacheable) =
        crate::fact_signature_helpers::with_cacheability_scope(&host, |_probe| ());
    assert!(
        !unrelated_non_cacheable,
        "an UNRELATED tracer scope that merely happens to open first must NOT claim a one-shot \
         armed for another scope. It did — so the knob is positional, and any scope added \
         upstream of a boundary under test silently retargets its overflow while the test stays \
         green",
    );
    assert_eq!(
        crate::host_test_force::fact_tracer_overflow_claimed_by(),
        None,
        "no scope may claim the one-shot before the NAMED target is entered",
    );
    assert_eq!(
        crate::host_test_force::peek_fact_tracer_overflow_once(),
        Some((TracerScope::ScriptFactsImportRoute, over_cap)),
        "the one-shot must survive an unrelated upstream scope intact, still armed for its \
         intended claimant",
    );

    // The named target then claims it inside the entry-point, exactly as it would
    // with no upstream scope at all.
    let facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
        &host,
        &registration,
        "/proj/Consumer.ts",
    );
    assert_eq!(
        crate::host_test_force::fact_tracer_overflow_claimed_by(),
        Some(TracerScope::ScriptFactsImportRoute),
        "the overflow must land on the NAMED import-route scope even though a foreign scope ran \
         first",
    );
    assert!(
        facts.is_some(),
        "the refusal stays CACHE-ONLY: the payload is still served (ReturnOnly)",
    );
    assert!(
        host.framework_script_caches().facts.is_empty(),
        "POISON: the import-route scope's overflow did not refuse the publication. The count was \
         claimed by the unrelated upstream scope instead, so the boundary under test never \
         overflowed",
    );
}

#[test]
fn capability_off_refuses_through_the_real_host() {
    // The real host's capability snapshot is empty, so the
    // capability-REQUIRING fixture provider refuses — even though the
    // import resolves to the framework package.
    let host = host_with_files();
    let registration = fixtures::import_gated_fixture_registration();
    let facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
        &host,
        &registration,
        "/proj/Consumer.ts",
    );
    assert!(
        facts.is_none(),
        "the consumed capability bit is OFF on the real host, so the \
             provider refuses to emit resolved facts"
    );
    // The refusal does NOT warm the resolved-fact store.
    assert!(host.framework_script_caches().facts.is_empty());
}

#[test]
fn no_provider_registration_is_zero_cost_none() {
    // A registration with no provider answers None without touching the
    // host's parse/analysis at all (the steady-state zero-cost path).
    let host = host_with_files();
    let registration = fixtures::import_gated_capability_free_fixture_registration();
    let empty_registration = FrameworkRegistration {
        script_fact_providers: Vec::new(),
        ..registration
    };
    let facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
        &host,
        &empty_registration,
        "/proj/Consumer.ts",
    );
    assert!(facts.is_none());
    assert!(host.framework_script_caches().candidates.is_empty());
}

#[test]
fn gate_miss_file_does_not_import_specifier_resolves_none() {
    // A file that does NOT import the gated specifier selects no provider
    // (the gate misses), so no facts resolve.
    let host = std::sync::Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/proj/Unrelated.ts".to_string(),
            source: Arc::from("export const y = 1;"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert unrelated file");
    let registration = fixtures::import_gated_capability_free_fixture_registration();
    let facts = resolve_script_facts::<fixtures::FixtureFactPayload>(
        &host,
        &registration,
        "/proj/Unrelated.ts",
    );
    assert!(facts.is_none(), "the gate misses — no provider is active");
}
