//! Fact-tracer cacheability and file-source-env observation suites that
//! drive the tracer through a live session host.

use verter_session_query::facts::{
    fact_cache::FactVersionRef,
    fact_read_set::{FactReadSetFinalise, FACT_PAGE_WIDTH},
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
            FactReadSetFinalise::MutationUnstable => {
                panic!("no domain moves in this fixture")
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

/// The tracer-CACHEABILITY entry ([`install_fact_tracer_cacheability`]) refuses
/// for a non-cacheable read and never for the NUMBER of facts a compute read: a
/// wide observation set is paged into a complete signature, so width cannot be
/// a refusal reason at any admission boundary.
mod tracer_cacheability_tests {
    use super::*;
    use crate::{HostConfig, VerterHost};

    /// Fan `count` distinct whole-hash observations into every active tracer.
    fn observe_wide(count: usize) {
        for index in 0..count {
            verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                FactVersionRef::FileWholeHash {
                    canonical_id: format!("/wide/{index:05}.ts"),
                    hash: [(index & 0xff) as u8; 16],
                },
            );
        }
    }

    /// DISCRIMINATING: a compute that read more facts than one evidence page —
    /// and consumed no non-cacheable read — is CACHEABLE, and its signature
    /// keeps every fact it read.
    #[test]
    fn a_wide_compute_stays_cacheable_and_keeps_every_fact() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let width = FACT_PAGE_WIDTH + 1;

        let (value, finalise) = install_fact_tracer(
            &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(&host),
            || {
                observe_wide(width);
                7u32
            },
        );
        assert_eq!(value, 7, "the traced value flows to the caller verbatim");
        let FactReadSetFinalise::Ok(facts) = finalise else {
            panic!("a wide compute finalises into its complete signature, got {finalise:?}");
        };
        let signature = verter_session_query::facts::fact_cache::ReadSetSignature::new(facts);
        assert_eq!(
            signature.entry_count(),
            width,
            "every observed fact survives"
        );

        let (value, non_cacheable) = install_fact_tracer_cacheability(
            &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(&host),
            || {
                observe_wide(width);
                7u32
            },
        );
        assert_eq!(value, 7, "the traced value flows to the caller verbatim");
        assert!(
            !non_cacheable,
            "width is never a refusal: the cacheability verdict of a wide compute that \
             consumed no non-cacheable read is CACHEABLE",
        );
    }

    /// Anti-vacuity: the verdict is not a constant. An ordinary compute is
    /// CACHEABLE, and the same compute under the armed refusal knob is NOT.
    #[test]
    fn cacheability_verdict_refuses_only_a_non_cacheable_read() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let (value, non_cacheable) = install_fact_tracer_cacheability(
            &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(&host),
            || 7u32,
        );
        assert_eq!(value, 7);
        assert!(
            !non_cacheable,
            "an ordinary compute stays CACHEABLE — the verdict must not be an \
             unconditional refusal",
        );

        host.test_force
            .engine
            .force_fact_tracer_non_cacheable_read
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let (value, non_cacheable) = install_fact_tracer_cacheability(
            &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(&host),
            || 7u32,
        );
        host.test_force
            .engine
            .force_fact_tracer_non_cacheable_read
            .store(false, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(value, 7, "a refused value still flows to the caller");
        assert!(non_cacheable, "a non-cacheable read refuses admission");
    }
}
