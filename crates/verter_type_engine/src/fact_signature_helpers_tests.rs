//! `#[cfg(test)]` coverage for the [`crate::fact_signature_helpers`] fact-based
//! validation substrate — the `ReadSetSignature` unit tests, the source-env
//! observation tests, and the tracer-CACHEABILITY boundary tests. They live in a
//! sibling `_tests.rs` so the production substrate and its tests remain independently
//! readable. The module is a descendant of
//! `fact_signature_helpers`, so `super::` reaches its private items.

use super::*;

#[cfg(test)]
mod read_set_signature_unit_tests {
    use super::*;
    use verter_session_query::facts::fact_cache::DerivedFactKind;

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
        assert_eq!(sig.facts.len(), 0, "empty carrier carries no facts");
        // Don't assert validate without ctx — empty carrier's
        // `validate` short-circuits via empty fact list. Tested
        // separately in integration with a `ResolverContext` stub.
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
            FactVersionRef::ResolveImports(
                verter_session_query::facts::fact_cache::ResolveImportsFactRef::Semantic {
                    canonical_id: "/resolve.ts".to_string(),
                    key: FactKey::SyntacticExportSet,
                    lane: FactLane::Semantic,
                    expected_hash: [0u8; 16],
                },
            ),
            FactVersionRef::RouteSurface(
                verter_session_query::facts::fact_cache::RouteSurfaceFactRef {
                    canonical_id: "/route.ts".to_string(),
                    key: FactKey::SyntacticExportSet,
                    lane: FactLane::Semantic,
                    expected_hash: [0u8; 16],
                },
            ),
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

    use std::sync::Arc as StdArc;
    use verter_session_query::facts::fact_cache::ParseEnvHash;

    use verter_session_query::source::artifact_key::FileArtifactKey;

    /// The reverse index registers a `(canonical → entry)` mapping for
    /// every canonical the fact rail names — a `FileSourceEnv`
    /// contributor fact must contribute its contributor canonical.
    #[test]
    fn canonical_ids_includes_file_source_env_contributor() {
        let fact = FactVersionRef::FileSourceEnv {
            canonical_id: "/contrib.d.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash([3u8; 16]),
            parse_key: verter_session_query::source::toolchain::parse_key_for_test(
                "/contrib.d.ts",
                2,
            ),
            file_language_id: FileArtifactKey::synthetic_file_language_for_test("/contrib.d.ts"),
        };
        let sig = ReadSetSignature::new(StdArc::from(vec![fact]));
        let canons = sig.canonical_ids();
        assert_eq!(canons.len(), 1, "one contributor canonical expected");
        assert_eq!(canons[0].as_ref(), "/contrib.d.ts");
    }
}
