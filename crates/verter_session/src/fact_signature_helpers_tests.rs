//! `#[cfg(test)]` coverage for the [`crate::fact_signature_helpers`] fact-based
//! validation substrate — the `ReadSetSignature` unit tests, the source-env
//! observation tests, and the tracer-CACHEABILITY boundary tests. Extracted to a
//! sibling `_tests.rs` (excluded from the oversize-files guard) so the production
//! module stays under the line cap. The module is a descendant of
//! `fact_signature_helpers`, so `super::` reaches its private items.

use super::*;

#[cfg(test)]
mod read_set_signature_unit_tests {
    use super::*;
    use crate::resolver_core::DerivedFactKind;

    fn fact_filewhole(canon: &str, byte: u8) -> FactVersionRef {
        FactVersionRef::FileWholeHash {
            canonical_id: canon.to_string(),
            hash: [byte; 16],
        }
    }

    fn fact_derived(canon: &str, byte: u8) -> FactVersionRef {
        FactVersionRef::DerivedFactHash {
            canonical_id: canon.to_string(),
            kind: DerivedFactKind::Route,
            hash: [byte; 16],
        }
    }

    fn fact_parse(canon: &str, byte: u8) -> FactVersionRef {
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: canon.to_string(),
            key: FactKey::SyntacticExportSet,
            lane: FactLane::Semantic,
            expected_hash: [byte; 16],
        })
    }

    #[test]
    fn read_set_signature_empty_validates_vacuously_via_facts_path() {
        // Empty carrier: facts empty. `validate_fact_signature`
        // returns true on empty input.
        let sig = ReadSetSignature::empty();
        assert!(!sig.is_overflow(), "empty carrier must NOT be overflow");
        assert_eq!(sig.facts.len(), 0, "empty carrier carries no facts");
        // Don't assert validate without ctx — empty carrier's
        // `validate` short-circuits via empty fact list. Tested
        // separately in integration with a `ResolverContext` stub.
    }

    #[test]
    fn read_set_signature_overflow_validate_returns_false() {
        let sig = ReadSetSignature::overflow();
        assert!(sig.is_overflow(), "overflow carrier must report overflow");
        // We can't trivially construct a ResolverContext here, but
        // the overflow short-circuit doesn't even call ctx — it
        // returns false directly. Integration tests cover the live
        // `validate(ctx)` call.
    }

    #[test]
    fn read_set_signature_canonical_ids_deduplicates_facts() {
        // facts mention /a.ts twice + /b.ts once. The canonical set
        // must collapse the duplicate /a.ts to one entry.
        let facts: Arc<[FactVersionRef]> = Arc::from(vec![
            fact_filewhole("/a.ts", 1),
            fact_parse("/a.ts", 9),
            fact_filewhole("/b.ts", 2),
        ]);
        let sig = ReadSetSignature::new(facts);
        let canons: Vec<String> = sig
            .canonical_ids()
            .iter()
            .map(|a| a.as_ref().to_string())
            .collect();
        assert_eq!(
            canons.len(),
            2,
            "duplicate /a.ts across facts must collapse to one entry"
        );
        assert!(canons.contains(&"/a.ts".to_string()));
        assert!(canons.contains(&"/b.ts".to_string()));
    }

    #[test]
    fn read_set_signature_canonical_ids_covers_all_fact_variants() {
        let facts: Arc<[FactVersionRef]> = Arc::from(vec![
            fact_filewhole("/wholehash.ts", 1),
            fact_derived("/derived.ts", 2),
            fact_parse("/parse.ts", 3),
            FactVersionRef::ResolveImports(crate::resolver_core::ResolveImportsFactRef {
                canonical_id: "/resolve.ts".to_string(),
                key: FactKey::SyntacticExportSet,
                lane: FactLane::Semantic,
                expected_hash: [0u8; 16],
            }),
            FactVersionRef::RouteSurface(crate::resolver_core::RouteSurfaceFactRef {
                canonical_id: "/route.ts".to_string(),
                key: FactKey::SyntacticExportSet,
                lane: FactLane::Semantic,
                expected_hash: [0u8; 16],
            }),
        ]);
        let sig = ReadSetSignature::new(facts);
        let canons: Vec<String> = sig
            .canonical_ids()
            .iter()
            .map(|a| a.as_ref().to_string())
            .collect();
        assert!(
            canons.contains(&"/wholehash.ts".to_string()),
            "FileWholeHash canonical must surface"
        );
        assert!(
            canons.contains(&"/derived.ts".to_string()),
            "DerivedFactHash canonical must surface"
        );
        assert!(
            canons.contains(&"/parse.ts".to_string()),
            "Parse canonical must surface"
        );
        assert!(
            canons.contains(&"/resolve.ts".to_string()),
            "ResolveImports canonical must surface"
        );
        assert!(
            canons.contains(&"/route.ts".to_string()),
            "RouteSurface canonical must surface"
        );
        assert_eq!(canons.len(), 5, "all 5 distinct canonicals must be present");
    }

    #[test]
    fn read_set_signature_canonical_ids_skips_project_generation_fact() {
        // A `ProjectGeneration` fact references no canonical — it must
        // contribute nothing to the reverse-index canonical set, while
        // the sibling `FileWholeHash` fact still surfaces.
        let facts: Arc<[FactVersionRef]> = Arc::from(vec![
            FactVersionRef::ProjectGeneration { generation: 7 },
            fact_filewhole("/only.ts", 1),
        ]);
        let sig = ReadSetSignature::new(facts);
        let canons: Vec<String> = sig
            .canonical_ids()
            .iter()
            .map(|a| a.as_ref().to_string())
            .collect();
        assert_eq!(
            canons,
            vec!["/only.ts".to_string()],
            "ProjectGeneration contributes no canonical; only /only.ts surfaces"
        );
    }

    #[test]
    fn read_set_signature_new_is_fact_only() {
        let facts: Arc<[FactVersionRef]> = Arc::from(vec![fact_filewhole("/a.ts", 1)]);
        let sig = ReadSetSignature::new(Arc::clone(&facts));
        assert_eq!(sig.facts.len(), 1);
        assert!(!sig.overflowed);
        assert!(
            Arc::ptr_eq(&sig.facts, &facts),
            "new() stores facts verbatim"
        );
        let canons = sig.canonical_ids();
        assert_eq!(canons.len(), 1);
        assert_eq!(canons[0].as_ref(), "/a.ts");
    }
}

#[cfg(test)]
mod file_source_env_observation_tests {
    use super::*;
    use crate::file_artifact_store::FileArtifactKey;
    use crate::locator_identity::ParseEnvHash;
    use crate::resolver_core::FactReadSetFinalise;
    use crate::{HostConfig, VerterHost};
    use std::sync::Arc as StdArc;

    /// The reverse index registers a `(canonical → entry)` mapping for
    /// every canonical the fact rail names — a `FileSourceEnv`
    /// contributor fact must contribute its contributor canonical.
    #[test]
    fn canonical_ids_includes_file_source_env_contributor() {
        let fact = FactVersionRef::FileSourceEnv {
            canonical_id: "/contrib.d.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash([3u8; 16]),
            parser_version: 2,
            file_language_id: FileArtifactKey::derived_file_language_id("/contrib.d.ts"),
        };
        let sig = ReadSetSignature::new(StdArc::from(vec![fact]));
        let canons = sig.canonical_ids();
        assert_eq!(canons.len(), 1, "one contributor canonical expected");
        assert_eq!(canons[0].as_ref(), "/contrib.d.ts");
    }

    /// The observation API sources `parser_version` / `file_language_id`
    /// from the exact artifact key the read used — never re-derived from
    /// the canonical/path at the call site — while the `parse_env_hash`
    /// dimension is the canonical's LIVE per-canonical parse env (the
    /// same dimension the contributor `LowerLocator` key folds), NEVER
    /// the key's `parse_env_hash` slot (a base key carries the zero
    /// sentinel there, not an env identity). The planted key carries a
    /// non-current parser version, a language row derived from a
    /// DIFFERENT path than the key's canonical, and a non-live
    /// `parse_env_hash`, so any re-derivation (or a key-copied env)
    /// would produce different field values and fail the assertions.
    #[test]
    fn observe_file_source_env_from_artifact_key_builds_fact_from_key_identity() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let key = FileArtifactKey {
            canonical: StdArc::from("/dep.ts"),
            content_hash: [7u8; 16],
            parse_env_hash: [3u8; 16],
            parser_version: 9,
            file_language_id: FileArtifactKey::derived_file_language_id("/dep.vue"),
        };
        let live_parse_env =
            ParseEnvHash::from_env_hash(host.host_view_env_hashes_for("/dep.ts").parse_env_hash);
        assert_ne!(
            live_parse_env,
            ParseEnvHash::from_env_hash([3u8; 16]),
            "the planted key env must differ from the live env so the assertions \
             below discriminate live sourcing from a key copy"
        );
        let (returned, read_set) =
            host.with_fact_tracer(|| observe_file_source_env_from_artifact_key(&host, Some(&key)));
        let expected = FactVersionRef::FileSourceEnv {
            canonical_id: "/dep.ts".to_string(),
            parse_env_hash: live_parse_env,
            parser_version: 9,
            file_language_id: FileArtifactKey::derived_file_language_id("/dep.vue"),
        };
        assert_eq!(
            returned.as_ref(),
            Some(&expected),
            "the returned fact must carry the key's parser-version/language identity \
             and the canonical's LIVE parse-env dimension"
        );
        let facts = match read_set.finalise() {
            FactReadSetFinalise::Ok(facts) => facts,
            FactReadSetFinalise::Overflow => panic!("one fact cannot overflow the signature cap"),
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
        let (returned, read_set) =
            host.with_fact_tracer(|| observe_file_source_env_from_artifact_key(&host, None));
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
#[cfg(test)]
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
            .force_fact_tracer_overflow_observations
            .store(OVER_CAP, std::sync::atomic::Ordering::Relaxed);

        // The raw 3-tuple entry: overflow lands in `finalise`, and the
        // non-cacheable-read bit stays FALSE (no fenced serve / lease miss ran).
        let (value, finalise, non_cacheable_read_observed) = install_fact_tracer(&host, || 7u32);
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
        let (value, non_cacheable) = install_fact_tracer_cacheability(&host, || 7u32);
        assert_eq!(value, 7, "the traced value flows to the caller verbatim");
        assert!(
            non_cacheable,
            "OVERFLOW MUST REFUSE: an observation set above FACT_SIGNATURE_CAP can be rooted \
             by NO signature, so a warm read could never revalidate the entry — the \
             cacheability verdict must fold `FactReadSetFinalise::Overflow` in as a second, \
             INDEPENDENT non-admission condition alongside the non-cacheable-read rail",
        );

        host.test_force
            .force_fact_tracer_overflow_observations
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Anti-vacuity: with the knob UNARMED an ordinary compute is CACHEABLE, so the
    /// verdict above is not a constant `true`.
    #[test]
    fn cacheability_verdict_is_false_for_an_ordinary_compute() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let (value, non_cacheable) = install_fact_tracer_cacheability(&host, || 7u32);
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
            .force_fact_tracer_overflow_observations
            .store(OVER_CAP, Ordering::Relaxed);

        // The signature-CONSUMING boundary (it finalises and roots its entry on the
        // finalised set) with TWO nested cacheability scopes inside it — the shape
        // the producer rewiring creates.
        let (_v, finalise, _nc) = install_fact_tracer(&host, || {
            let (inner, inner_non_cacheable) = install_fact_tracer_cacheability(&host, || {
                let (deepest, deepest_non_cacheable) =
                    install_fact_tracer_cacheability(&host, || 1u32);
                assert!(
                    deepest_non_cacheable,
                    "fixture invariant: the innermost cacheability scope must OVERFLOW (else \
                     the counter assertion is vacuous)",
                );
                deepest
            });
            assert!(
                inner_non_cacheable,
                "fixture invariant: the enclosing cacheability scope must ALSO overflow (the \
                 inner scope's observations fan outward into it)",
            );
            inner
        });
        assert!(
            matches!(finalise, FactReadSetFinalise::Overflow),
            "fixture invariant: the outermost signature-consuming tracer must overflow too",
        );

        host.test_force
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
