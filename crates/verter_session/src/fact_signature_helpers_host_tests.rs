//! Fact-tracer cacheability and file-source-env observation suites that
//! drive the tracer through a live session host.

use verter_session_query::facts::{
    fact_cache::FactVersionRef,
    fact_read_set::{FactReadSetFinalise, FACT_SIGNATURE_CAP},
};

#[allow(unused_imports)]
use verter_type_engine::fact_signature_helpers::*;

mod file_source_env_observation_tests {
    use super::*;
    use crate::{HostConfig, VerterHost};
    use std::sync::Arc as StdArc;
    use verter_session_query::facts::fact_cache::ParseEnvHash;
    use verter_session_query::facts::fact_read_set::FactReadSetFinalise;
    use verter_session_query::source::artifact_key::FileArtifactKey;

    /// The observation API sources `parse_key` / `file_language_id`
    /// from the exact artifact key the read used — never re-derived from
    /// the canonical/path at the call site — while the `parse_env_hash`
    /// dimension is the canonical's LIVE per-canonical parse env (the
    /// same dimension the contributor `LowerLocator` key folds), NEVER
    /// the key's `parse_env_hash` slot (a base key carries the zero
    /// sentinel there, not an env identity). The planted key carries a
    /// non-current parse key, a language row derived from a
    /// DIFFERENT path than the key's canonical, and a non-live
    /// `parse_env_hash`, so any re-derivation (or a key-copied env)
    /// would produce different field values and fail the assertions.
    #[test]
    fn observe_file_source_env_from_artifact_key_builds_fact_from_key_identity() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let key = FileArtifactKey {
            parse_env_hash: [3u8; 16],
            file_language_id: FileArtifactKey::synthetic_file_language_for_test("/dep.vue"),
            ..FileArtifactKey::base_for_test(StdArc::from("/dep.ts"), [7u8; 16])
        };
        let live_parse_env =
            ParseEnvHash::from_env_hash(host.host_view_env_hashes_for("/dep.ts").parse_env_hash);
        assert_ne!(
            live_parse_env,
            ParseEnvHash::from_env_hash([3u8; 16]),
            "the planted key env must differ from the live env so the assertions \
             below discriminate live sourcing from a key copy"
        );
        let (returned, read_set) = host.with_fact_tracer(
            verter_session_query::facts::fact_cache::AggregateBasisSeed::Unvouched,
            || observe_file_source_env_from_artifact_key(&host, Some(&key)),
        );
        let expected = FactVersionRef::FileSourceEnv {
            canonical_id: "/dep.ts".to_string(),
            parse_env_hash: live_parse_env,
            parse_key: key.parse_key.clone(),
            file_language_id: FileArtifactKey::synthetic_file_language_for_test("/dep.vue"),
        };
        assert_eq!(
            returned.as_ref(),
            Some(&expected),
            "the returned fact must carry the key's parse-key/language identity \
             and the canonical's LIVE parse-env dimension"
        );
        let facts = match read_set.finalise() {
            FactReadSetFinalise::Ok(facts) => facts,
            FactReadSetFinalise::NonCacheable(_) => {
                panic!("the observed source-env fact is cacheable")
            }
            FactReadSetFinalise::Overflow | FactReadSetFinalise::MutationUnstable => {
                panic!("one fact overflows nothing and no domain moves in this fixture")
            }
        };
        assert_eq!(
            facts.as_ref(),
            &[expected],
            "the observation must land on the active tracer"
        );
    }

    /// A read that cannot supply the exact artifact key it used has no
    /// coherent source-env identity to observe: the API returns `None`
    /// (so the caller routes the result through `ReturnOnly`) and
    /// records nothing — never a fabricated default.
    #[test]
    fn observe_file_source_env_without_exact_key_returns_none_and_records_nothing() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let (returned, read_set) = host.with_fact_tracer(
            verter_session_query::facts::fact_cache::AggregateBasisSeed::Unvouched,
            || observe_file_source_env_from_artifact_key(&host, None),
        );
        assert!(
            returned.is_none(),
            "an unobservable source-env identity must surface as None, never a default"
        );
        assert!(
            read_set.is_empty(),
            "no observation may be recorded for an unobservable identity"
        );
    }
}

/// The tracer-CACHEABILITY entry ([`install_fact_tracer_cacheability`]) must fold
/// BOTH independent non-admission conditions into its single verdict bit: a
/// non-cacheable read AND a `FactReadSetFinalise::Overflow`.
///
/// An admission boundary whose entry signature is built from another source (the
/// carrier's `dep_signature`, the keyed canonical's observed hash) never inspects
/// the tracer's finalised set, so an `Overflow` seen only there would be dropped on
/// the floor and a rootless entry would warm the shared cache.
mod tracer_cacheability_tests {
    use super::*;
    use crate::{HostConfig, VerterHost};

    /// One synthetic observation above the per-signature cap.
    const OVER_CAP: usize = FACT_SIGNATURE_CAP + 1;

    /// DISCRIMINATING: a compute that consumed NO non-cacheable read but whose
    /// observation set OVERFLOWED is NON-CACHEABLE. The raw
    /// [`install_fact_tracer`] bit is `false` for it (it reports only the
    /// non-cacheable-read rail) — which is exactly the hole: a boundary reading
    /// that bit alone admits a rootless entry. The cacheability entry must report
    /// `true`.
    #[test]
    fn cacheability_verdict_folds_overflow_with_no_non_cacheable_read() {
        let host = VerterHost::new_standalone(HostConfig::default());
        host.test_force
            .engine
            .force_fact_tracer_overflow_observations
            .store(OVER_CAP, std::sync::atomic::Ordering::Relaxed);

        // The raw 3-tuple entry: overflow lands in `finalise`, and the
        // non-cacheable-read bit stays FALSE (no fenced serve / lease miss ran).
        let (value, finalise) = install_fact_tracer(
            &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(&host),
            || 7u32,
        );
        let non_cacheable_read_observed = matches!(&finalise, FactReadSetFinalise::NonCacheable(_));
        assert_eq!(value, 7, "the traced value flows to the caller verbatim");
        assert!(
            matches!(finalise, FactReadSetFinalise::Overflow),
            "fixture invariant: the forced observations must overflow the signature cap",
        );
        assert!(
            !non_cacheable_read_observed,
            "fixture invariant: no non-cacheable READ was consumed — so a boundary that \
             consults ONLY this bit would ADMIT the rootless entry (the hole under test)",
        );

        // The cacheability entry folds the overflow in — one verdict, two conditions.
        let (value, non_cacheable) = install_fact_tracer_cacheability(
            &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(&host),
            || 7u32,
        );
        assert_eq!(value, 7, "the traced value flows to the caller verbatim");
        assert!(
            non_cacheable,
            "OVERFLOW MUST REFUSE: an observation set above FACT_SIGNATURE_CAP can be rooted \
             by NO signature, so a warm read could never revalidate the entry — the \
             cacheability verdict must fold `FactReadSetFinalise::Overflow` in as a second, \
             INDEPENDENT non-admission condition alongside the non-cacheable-read rail",
        );

        host.test_force
            .engine
            .force_fact_tracer_overflow_observations
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Anti-vacuity: with the knob UNARMED an ordinary compute is CACHEABLE, so the
    /// verdict above is not a constant `true`.
    #[test]
    fn cacheability_verdict_is_false_for_an_ordinary_compute() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let (value, non_cacheable) = install_fact_tracer_cacheability(
            &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(&host),
            || 7u32,
        );
        assert_eq!(value, 7);
        assert!(
            !non_cacheable,
            "an ordinary compute (no non-cacheable read, no overflow) stays CACHEABLE — the \
             verdict must not be an unconditional refusal",
        );
    }

    /// AUDIT SEMANTICS: ONE overflowing compute emits ONE overflow audit event and
    /// bumps [`crate::VerterHost::signature_overflow_at_install`] exactly ONCE — no
    /// matter how many cacheability scopes nest inside it.
    ///
    /// Cacheability scopes now wrap whole producer computes, and they NEST (a
    /// component-meta cold compute's signature-consuming tracer encloses the
    /// shape-cache producers' scopes). An observation fans into EVERY active tracer,
    /// so an inner overflow overflows every enclosing cell too. If the cacheability
    /// path emitted on overflow, ONE overflowing compute would emit the event and
    /// bump the counter once PER NESTING LEVEL — silently multiplying the audit
    /// substrate's overflow counter and footprint. The overflow-only peek
    /// (`FactReadSet::would_overflow`) exists precisely so the emission stays owned
    /// by the ONE signature-CONSUMING boundary.
    ///
    /// DISCRIMINATING: the compute below runs TWO cacheability scopes nested inside
    /// one `install_fact_tracer`, all overflowing. Exactly one bump is correct.
    /// Routing the cacheability path back through the emitting `install_fact_tracer`
    /// yields 3.
    #[test]
    fn one_overflowing_compute_bumps_the_overflow_counter_exactly_once() {
        use std::sync::atomic::Ordering;

        let host = VerterHost::new_standalone(HostConfig::default());
        host.test_force
            .engine
            .force_fact_tracer_overflow_observations
            .store(OVER_CAP, Ordering::Relaxed);

        // The signature-CONSUMING boundary (it finalises and roots its entry on the
        // finalised set) with TWO nested cacheability scopes inside it — the shape
        // the producer rewiring creates.
        let (_v, finalise) = install_fact_tracer(
            &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(&host),
            || {
                let (inner, inner_non_cacheable) = install_fact_tracer_cacheability(
                    &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(
                        &host,
                    ),
                    || {
                        let (deepest, deepest_non_cacheable) = install_fact_tracer_cacheability(
                            &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(&host),
                            || 1u32,
                        );
                        assert!(
                    deepest_non_cacheable,
                    "fixture invariant: the innermost cacheability scope must OVERFLOW (else \
                     the counter assertion is vacuous)",
                );
                        deepest
                    },
                );
                assert!(
                    inner_non_cacheable,
                    "fixture invariant: the enclosing cacheability scope must ALSO overflow (the \
                 inner scope's observations fan outward into it)",
                );
                inner
            },
        );
        assert!(
            matches!(finalise, FactReadSetFinalise::Overflow),
            "fixture invariant: the outermost signature-consuming tracer must overflow too",
        );

        host.test_force
            .engine
            .force_fact_tracer_overflow_observations
            .store(0, Ordering::Relaxed);

        assert_eq!(
            host.signature_overflow_at_install.load(Ordering::Relaxed),
            1,
            "AUDIT REGRESSION: one overflowing compute bumped the signature-overflow counter \
             more than once. An observation fans into every active tracer, so an inner overflow \
             overflows each enclosing scope; only the ONE signature-CONSUMING boundary may emit \
             the audit event and bump the counter. A cacheability scope must PEEK overflow \
             (`FactReadSet::would_overflow`) — never finalise-and-emit — or nesting silently \
             multiplies the audit substrate's overflow counter and footprint",
        );
    }
}
