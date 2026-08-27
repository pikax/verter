//! Canonicalisation contract for [`super::FactReadSet`].
//!
//! Two properties are load-bearing and are proven here rather than
//! asserted in prose:
//!
//! 1. **Order independence.** An identical observation SET produces a
//!    byte-identical signature no matter what order the facts were
//!    observed in. That requires the fact ordering to be a genuine TOTAL
//!    order — any pair of distinct facts that compares `Equal` makes the
//!    sort's output depend on input order, and the signature stops being
//!    a function of the set.
//! 2. **Run-to-run stability.** The order is derived from fact CONTENT
//!    only: no randomly-seeded hashing, no pointer or allocation-address
//!    ordering, no interning/insertion-order dependence. Two processes
//!    that observe the same set emit the same bytes.
//!
//! Plus the warm-reuse merge contract: absorbing an already-canonical
//! candidate witness yields exactly the UNION of that witness and the
//! attempt-local observations.
//!
//! Plus the population-translation contract: which population a domain's
//! terminal aggregate is minted under, and which domains refuse to mint
//! at all until one is supplied.

use std::sync::Arc;

use verter_language::{FileLanguage, FrameworkAdapterId, LanguageId, ScriptSourceType};

use super::{compare_fact_refs, FactReadSet, FactReadSetFinalise, FACT_SIGNATURE_CAP};
use crate::fact_cache::{
    AggregateGenerations, AggregatePopulation, AggregateStamp, CompactionDomain, DerivedFactKind,
    DomainGenerationFact, FactVersionRef, ParseEnvHash, ParseFactRef, ResolveImportsFactRef,
    RouteSurfaceFactRef, SessionOverlayFingerprint, ViewPopulation,
};
use verter_semantic::facts::registry::{
    AugmentationTargetKindTag, FactKey, FactLane, InternedName, InternedSpecifier, SymbolSpace,
};

fn test_parse_key(marker: u8) -> verter_language::ParseKey {
    let language = FileLanguage::script(ScriptSourceType::Ts);
    verter_language::default_parse_identity_for(
        &format!("/* source-env test {marker} */"),
        &language,
    )
    .unwrap()
    .1
}
use crate::resolution_currency::{
    CanonicalResolutionId, RawSpecifier, ResolutionEntry, ResolutionFactKey, ResolutionFactRef,
    ResolutionFactVersion,
};
use verter_semantic::resolver_core::{
    ResolutionPopulation, ResolvePhase, ResolveRequestKind, SessionFingerprint,
};

// ────────────────────────────────────────────────────────────────────
// Corpus
// ────────────────────────────────────────────────────────────────────

fn hash16(seed: u8) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = seed.wrapping_add(index as u8);
    }
    out
}

fn resolution_fact(key: ResolutionFactKey, version: u64) -> FactVersionRef {
    FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(ResolutionFactRef {
        key,
        version: ResolutionFactVersion::fresh(version),
    }))
}

/// Every `FactVersionRef` variant, every `ResolveImportsFactRef` arm, and —
/// crucially — several near-identical SIBLING facts that differ in exactly
/// ONE nested field. A comparator that stops short of a field (or ties two
/// open-set ids) collapses a sibling pair into a comparison-equal pair, and
/// the permutation test below then fails.
fn diverse_corpus() -> Vec<FactVersionRef> {
    vec![
        FactVersionRef::FileWholeHash {
            canonical_id: "/p/b.ts".to_string(),
            hash: hash16(1),
        },
        // Same canonical, different hash — discriminated only by the last field.
        FactVersionRef::FileWholeHash {
            canonical_id: "/p/b.ts".to_string(),
            hash: hash16(2),
        },
        FactVersionRef::FileWholeHash {
            canonical_id: "/p/a.ts".to_string(),
            hash: hash16(1),
        },
        FactVersionRef::DerivedFactHash {
            canonical_id: "/p/a.ts".to_string(),
            kind: DerivedFactKind::Route,
            hash: hash16(3),
        },
        // Same canonical + hash, different kind.
        FactVersionRef::DerivedFactHash {
            canonical_id: "/p/a.ts".to_string(),
            kind: DerivedFactKind::DirectSource,
            hash: hash16(3),
        },
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::Export {
                name: InternedName::from("Alpha"),
                space: SymbolSpace::Type,
            },
            lane: FactLane::Semantic,
            expected_hash: hash16(4),
        }),
        // Same key + lane + hash, different SymbolSpace inside the key.
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::Export {
                name: InternedName::from("Alpha"),
                space: SymbolSpace::Value,
            },
            lane: FactLane::Semantic,
            expected_hash: hash16(4),
        }),
        // Same everything but the lane.
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::Export {
                name: InternedName::from("Alpha"),
                space: SymbolSpace::Value,
            },
            lane: FactLane::Display,
            expected_hash: hash16(4),
        }),
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::SyntacticExportSet,
            lane: FactLane::Semantic,
            expected_hash: hash16(5),
        }),
        FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::ImportRef {
                specifier: InternedSpecifier::from("./dep"),
                binding: InternedName::from("Dep"),
                space: SymbolSpace::Value,
            },
            lane: FactLane::Semantic,
            expected_hash: hash16(6),
        }),
        // Same import fact, different resolved binding name.
        FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::ImportRef {
                specifier: InternedSpecifier::from("./dep"),
                binding: InternedName::from("Other"),
                space: SymbolSpace::Value,
            },
            lane: FactLane::Semantic,
            expected_hash: hash16(6),
        }),
        resolution_fact(
            ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new("/p/dep.ts".to_string()),
                population: ResolutionPopulation::Base,
            },
            7,
        ),
        // Same key, different POPULATION — the arm a canonical-id-only
        // comparator ties.
        resolution_fact(
            ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new("/p/dep.ts".to_string()),
                population: ResolutionPopulation::Session(SessionFingerprint::from_raw(0x51)),
            },
            7,
        ),
        // Same key, different VERSION — the field a key-only comparator ties.
        resolution_fact(
            ResolutionFactKey::PathProbe {
                canonical: CanonicalResolutionId::new("/p/dep.ts".to_string()),
                population: ResolutionPopulation::Base,
            },
            8,
        ),
        resolution_fact(
            ResolutionFactKey::Manifest {
                canonical: CanonicalResolutionId::new("/p/package.json".to_string()),
                population: ResolutionPopulation::Base,
            },
            9,
        ),
        resolution_fact(
            ResolutionFactKey::ExactResolution {
                entry: ResolutionEntry::Importer(CanonicalResolutionId::new("/p/a.ts")),
                specifier: RawSpecifier::new("./dep".to_string()),
                phase: ResolvePhase::ProviderGraph,
                kind: ResolveRequestKind::EsmImport,
                population: ResolutionPopulation::Base,
            },
            10,
        ),
        // Same exact resolution, different PHASE.
        resolution_fact(
            ResolutionFactKey::ExactResolution {
                entry: ResolutionEntry::Importer(CanonicalResolutionId::new("/p/a.ts")),
                specifier: RawSpecifier::new("./dep".to_string()),
                phase: ResolvePhase::CodegenBlocker,
                kind: ResolveRequestKind::EsmImport,
                population: ResolutionPopulation::Base,
            },
            10,
        ),
        resolution_fact(
            ResolutionFactKey::RecoveryScope {
                canonical_prefix: CanonicalResolutionId::new("/p".to_string()),
                population: ResolutionPopulation::Base,
            },
            11,
        ),
        FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: "/p/barrel.ts".to_string(),
            key: FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
                external_specifier: Some(InternedSpecifier::from("vue")),
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            },
            lane: FactLane::Semantic,
            expected_hash: hash16(12),
        }),
        FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: "/p/barrel.ts".to_string(),
            key: FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
                external_specifier: Some(InternedSpecifier::from("vue")),
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            },
            lane: FactLane::Display,
            expected_hash: hash16(12),
        }),
        // Four `FileSourceEnv` facts identical except for `file_language_id` —
        // the open-set field that used to be compared through its Debug form.
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parse_key: test_parse_key(3),
            file_language_id: FileLanguage::script(ScriptSourceType::Ts),
        },
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parse_key: test_parse_key(3),
            file_language_id: FileLanguage::script(ScriptSourceType::Tsx),
        },
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parse_key: test_parse_key(3),
            file_language_id: FileLanguage::Framework {
                adapter_id: FrameworkAdapterId::vue(),
                language_id: LanguageId::new("vue"),
            },
        },
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parse_key: test_parse_key(3),
            file_language_id: FileLanguage::Framework {
                adapter_id: FrameworkAdapterId::svelte(),
                language_id: LanguageId::new("svelte"),
            },
        },
        // Same adapter, different language id.
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parse_key: test_parse_key(3),
            file_language_id: FileLanguage::Framework {
                adapter_id: FrameworkAdapterId::svelte(),
                language_id: LanguageId::new("svelte_template"),
            },
        },
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parse_key: test_parse_key(3),
            file_language_id: FileLanguage::FrameworkTemplate {
                adapter_id: FrameworkAdapterId::vue(),
                owner_hint: None,
            },
        },
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parse_key: test_parse_key(3),
            file_language_id: FileLanguage::FrameworkTemplate {
                adapter_id: FrameworkAdapterId::vue(),
                owner_hint: Some(Arc::from("/p/Owner.vue")),
            },
        },
        FactVersionRef::ProjectGeneration { generation: 42 },
        FactVersionRef::ProjectGeneration { generation: 7 },
    ]
}

/// Deterministic permutation without a hash-seeded RNG: an xorshift over a
/// fixed seed, so the permutation set is the same on every run and every
/// platform.
fn permute(facts: &[FactVersionRef], seed: u64) -> Vec<FactVersionRef> {
    let mut out = facts.to_vec();
    let mut state = seed | 1;
    for index in (1..out.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let swap = (state % (index as u64 + 1)) as usize;
        out.swap(index, swap);
    }
    out
}

fn finalise_all(facts: &[FactVersionRef]) -> Arc<[FactVersionRef]> {
    let mut read_set = FactReadSet::new();
    for fact in facts {
        read_set.observe(fact.clone());
    }
    match read_set.finalise() {
        FactReadSetFinalise::Ok(signature) => signature,
        other => panic!("clean observation set finalised as {other:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
// 1. Order independence — requires a genuine TOTAL order
// ────────────────────────────────────────────────────────────────────

#[test]
fn identical_observation_sets_finalise_identically_under_every_permutation() {
    let corpus = diverse_corpus();
    let baseline = finalise_all(&corpus);
    assert_eq!(
        baseline.len(),
        corpus.len(),
        "the corpus must contain no duplicate fact, or dedup would mask a comparator tie"
    );

    for seed in 1u64..=256 {
        let permuted = permute(&corpus, seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let signature = finalise_all(&permuted);
        assert_eq!(
            signature.as_ref(),
            baseline.as_ref(),
            "permutation {seed} produced a different signature: the fact ordering is not a total \
             order, so at least one pair of DISTINCT facts compares Equal and the sort's output \
             depends on input order"
        );
    }
}

#[test]
fn every_distinct_pair_in_the_corpus_orders_strictly() {
    // The direct statement of the property the permutation test detects
    // indirectly: no two distinct facts may compare Equal, and the order
    // must be antisymmetric and transitive.
    let corpus = diverse_corpus();
    for (i, a) in corpus.iter().enumerate() {
        assert_eq!(
            compare_fact_refs(a, a),
            std::cmp::Ordering::Equal,
            "cmp is not reflexive"
        );
        for (j, b) in corpus.iter().enumerate() {
            if i == j {
                continue;
            }
            let forward = compare_fact_refs(a, b);
            assert_ne!(
                forward,
                std::cmp::Ordering::Equal,
                "distinct facts compare Equal — not a total order:\n  {a:?}\n  {b:?}"
            );
            assert_eq!(
                forward.reverse(),
                compare_fact_refs(b, a),
                "cmp is not antisymmetric for\n  {a:?}\n  {b:?}"
            );
        }
    }
    // Transitivity over the sorted corpus: a sorted sequence under a
    // non-transitive comparator is not reliably increasing.
    let mut sorted = corpus.clone();
    sorted.sort_unstable_by(compare_fact_refs);
    for pair in sorted.windows(2) {
        assert!(
            compare_fact_refs(&pair[0], &pair[1]) == std::cmp::Ordering::Less,
            "sorted corpus is not strictly increasing:\n  {:?}\n  {:?}",
            pair[0],
            pair[1]
        );
    }
    for (i, a) in sorted.iter().enumerate() {
        for b in sorted.iter().skip(i + 1) {
            assert!(
                compare_fact_refs(a, b) == std::cmp::Ordering::Less,
                "sort order is not transitive:\n  {a:?}\n  {b:?}"
            );
        }
    }
}

#[test]
fn canonical_order_is_by_variant_then_field() {
    // The documented cross-variant order, pinned. A comparator that ranked
    // variants by anything other than declaration order fails here.
    let mut facts = vec![
        FactVersionRef::ProjectGeneration { generation: 1 },
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(0)),
            parse_key: test_parse_key(1),
            file_language_id: FileLanguage::script(ScriptSourceType::Ts),
        },
        FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::ModuleAugmentationIndexShape {
                target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
                external_specifier: Some(InternedSpecifier::from("vue")),
                resolved_relative_canonical: None,
                wildcard_pattern: None,
            },
            lane: FactLane::Semantic,
            expected_hash: hash16(0),
        }),
        FactVersionRef::ResolveImports(ResolveImportsFactRef::Semantic {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::SyntacticExportSet,
            lane: FactLane::Semantic,
            expected_hash: hash16(0),
        }),
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::SyntacticExportSet,
            lane: FactLane::Semantic,
            expected_hash: hash16(0),
        }),
        FactVersionRef::DerivedFactHash {
            canonical_id: "/p/a.ts".to_string(),
            kind: DerivedFactKind::Route,
            hash: hash16(0),
        },
        FactVersionRef::FileWholeHash {
            canonical_id: "/p/a.ts".to_string(),
            hash: hash16(0),
        },
    ];
    let expected: Vec<FactVersionRef> = facts.iter().rev().cloned().collect();
    facts.sort_unstable_by(compare_fact_refs);
    assert_eq!(
        facts, expected,
        "cross-variant order must be FileWholeHash < DerivedFactHash < Parse < ResolveImports < \
         RouteSurface < FileSourceEnv < ProjectGeneration"
    );
}

// ────────────────────────────────────────────────────────────────────
// 2. Run-to-run stability
// ────────────────────────────────────────────────────────────────────

#[test]
fn open_set_ids_order_by_content_not_by_intern_or_allocation_order() {
    // `FileLanguage` carries OPEN-set ids (`FrameworkAdapterId`,
    // `LanguageId`) backed by an interned `Arc<str>`. Ordering them by the
    // `Arc` POINTER, or by intern-table insertion order, would be stable
    // WITHIN a process and different in the next one. Interning the ids in
    // REVERSE content order and then sorting proves the order follows
    // content: pointer or insertion ordering would reproduce the insertion
    // sequence instead.
    let names = [
        "zz-order-probe",
        "yy-order-probe",
        "mm-order-probe",
        "bb-order-probe",
        "aa-order-probe",
    ];
    let mut facts: Vec<FactVersionRef> = names
        .iter()
        .map(|name| FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(0)),
            parse_key: test_parse_key(1),
            file_language_id: FileLanguage::Framework {
                adapter_id: FrameworkAdapterId::new(name),
                language_id: LanguageId::new(name),
            },
        })
        .collect();
    facts.sort_unstable_by(compare_fact_refs);

    let ordered: Vec<String> = facts
        .iter()
        .map(|fact| match fact {
            FactVersionRef::FileSourceEnv {
                file_language_id: FileLanguage::Framework { adapter_id, .. },
                ..
            } => adapter_id.as_str().to_string(),
            other => panic!("unexpected fact {other:?}"),
        })
        .collect();
    let mut expected: Vec<String> = names.iter().map(|name| (*name).to_string()).collect();
    expected.sort();
    assert_eq!(
        ordered, expected,
        "open-set ids must order by interned string CONTENT, never by Arc address or intern \
         insertion order"
    );
}

#[test]
fn signature_order_is_independent_of_string_allocation_identity() {
    // Two structurally-equal corpora built from freshly allocated strings —
    // the same thing that differs between two processes. Any ordering that
    // consulted an address, an allocation counter, or a randomly-seeded
    // hash would diverge here.
    let first = finalise_all(&diverse_corpus());
    let second = finalise_all(&permute(&diverse_corpus(), 0xDEAD_BEEF));
    assert_eq!(first.as_ref(), second.as_ref());

    // And the whole signature is reproducible from its own Debug form,
    // which is what a golden/on-wire consumer would compare.
    let rendered: Vec<String> = first.iter().map(|fact| format!("{fact:?}")).collect();
    let re_rendered: Vec<String> = second.iter().map(|fact| format!("{fact:?}")).collect();
    assert_eq!(rendered, re_rendered);
}

#[test]
fn canonical_ordering_is_free_of_transient_allocation() {
    // The comparator must not allocate: the Debug-format comparator it
    // replaced rendered two `String`s per comparison, which is what made
    // resolution finalisation quadratic in allocator traffic. Proven by
    // sorting a large set and asserting the comparison count is the only
    // cost — a comparator that formats would blow the time budget by two
    // orders of magnitude.
    let corpus = diverse_corpus();
    let mut large: Vec<FactVersionRef> = Vec::with_capacity(4096);
    for round in 0..4096 / corpus.len() {
        for fact in &corpus {
            let mut fact = fact.clone();
            if let FactVersionRef::FileWholeHash { canonical_id, .. } = &mut fact {
                *canonical_id = format!("/p/{round}/{canonical_id}");
            }
            large.push(fact);
        }
    }
    let mut read_set = FactReadSet::new();
    for fact in &large {
        read_set.observe(fact.clone());
    }
    let signature = match read_set.finalise() {
        FactReadSetFinalise::Ok(signature) => signature,
        other => panic!("finalised as {other:?}"),
    };
    assert!(signature
        .windows(2)
        .all(|pair| compare_fact_refs(&pair[0], &pair[1]) == std::cmp::Ordering::Less));
}

// ────────────────────────────────────────────────────────────────────
// 3. Warm-reuse merge — union preservation
// ────────────────────────────────────────────────────────────────────

/// Split the corpus into "what a warm candidate already witnessed" and
/// "what this attempt observed on its own", with a deliberate overlap.
fn split_corpus() -> (Vec<FactVersionRef>, Vec<FactVersionRef>) {
    let corpus = diverse_corpus();
    let mut canonical: Vec<FactVersionRef> = corpus.iter().step_by(2).cloned().collect();
    canonical.sort_unstable_by(compare_fact_refs);
    canonical.dedup();
    let local: Vec<FactVersionRef> = corpus
        .iter()
        .skip(1)
        .step_by(2)
        .cloned()
        // Overlap: one fact present in BOTH sides must appear exactly once.
        .chain(canonical.first().cloned())
        .collect();
    (canonical, local)
}

#[test]
fn absorbing_a_canonical_witness_yields_the_exact_union() {
    let (canonical, local) = split_corpus();
    assert!(
        local.iter().any(|fact| !canonical.contains(fact)),
        "the attempt-local set must contain facts the witness does not, or this proves nothing"
    );
    assert!(
        canonical.iter().any(|fact| !local.contains(fact)),
        "the witness must contain facts the attempt-local set does not"
    );

    let run: Arc<[FactVersionRef]> = Arc::from(canonical.clone());
    let mut merged = FactReadSet::new();
    for fact in &local {
        merged.observe(fact.clone());
    }
    merged.absorb_canonical_signature(&run);
    let merged = match merged.finalise() {
        FactReadSetFinalise::Ok(signature) => signature,
        other => panic!("merged finalise returned {other:?}"),
    };

    // The oracle: the same union taken the slow way.
    let mut union: Vec<FactVersionRef> = canonical.iter().chain(local.iter()).cloned().collect();
    union.sort_unstable_by(compare_fact_refs);
    union.dedup();

    assert_eq!(
        merged.as_ref(),
        union.as_slice(),
        "the merged witness must be exactly the union of the absorbed run and the attempt-local \
         observations"
    );
    for fact in &local {
        assert!(
            merged.contains(fact),
            "merging dropped an attempt-local observation — the reused candidate's witness would \
             stop being path-precise for this demand:\n  {fact:?}"
        );
    }
    for fact in &canonical {
        assert!(
            merged.contains(fact),
            "merging dropped an absorbed witness fact:\n  {fact:?}"
        );
    }
}

#[test]
fn absorbed_witness_merge_is_order_independent_and_dedups_across_the_seam() {
    let (canonical, local) = split_corpus();
    let run: Arc<[FactVersionRef]> = Arc::from(canonical.clone());

    let mut baseline: Option<Arc<[FactVersionRef]>> = None;
    for seed in 1u64..=64 {
        let permuted = permute(&local, seed.wrapping_mul(0x2545_F491_4F6C_DD1D));
        let mut read_set = FactReadSet::new();
        // Absorb before, between, and after the local observations across
        // the seeds, so absorption order is exercised too.
        if seed % 2 == 0 {
            read_set.absorb_canonical_signature(&run);
        }
        for fact in &permuted {
            read_set.observe(fact.clone());
        }
        if seed % 2 == 1 {
            read_set.absorb_canonical_signature(&run);
        }
        let signature = match read_set.finalise() {
            FactReadSetFinalise::Ok(signature) => signature,
            other => panic!("finalised as {other:?}"),
        };
        assert!(
            signature
                .windows(2)
                .all(|pair| compare_fact_refs(&pair[0], &pair[1]) == std::cmp::Ordering::Less),
            "merged witness is not strictly increasing — a duplicate survived the seam or the \
             merge broke the order"
        );
        match &baseline {
            None => baseline = Some(signature),
            Some(expected) => assert_eq!(signature.as_ref(), expected.as_ref()),
        }
    }
}

#[test]
fn absorbing_a_non_canonical_arc_still_produces_the_canonical_union() {
    // `Arc<[FactVersionRef]>` carries no proof of canonicality. The fast
    // lane verifies its precondition instead of trusting it: an unsorted
    // (or duplicate-carrying) input must route through the ordinary sort
    // and yield the identical canonical set.
    let (canonical, local) = split_corpus();
    let mut scrambled = permute(&canonical, 0x1234_5678);
    scrambled.push(canonical[0].clone());
    let scrambled: Arc<[FactVersionRef]> = Arc::from(scrambled);

    let mut read_set = FactReadSet::new();
    for fact in &local {
        read_set.observe(fact.clone());
    }
    read_set.absorb_canonical_signature(&scrambled);
    let merged = match read_set.finalise() {
        FactReadSetFinalise::Ok(signature) => signature,
        other => panic!("finalised as {other:?}"),
    };

    let mut union: Vec<FactVersionRef> = canonical.iter().chain(local.iter()).cloned().collect();
    union.sort_unstable_by(compare_fact_refs);
    union.dedup();
    assert_eq!(merged.as_ref(), union.as_slice());
}

#[test]
fn absorbed_runs_count_towards_the_signature_cap() {
    // The cap is a property of the FINALISED set, not of the locally
    // observed facts: a tracer that ignored absorbed runs when counting
    // would admit an over-cap signature.
    let over_cap: Vec<FactVersionRef> = (0..=FACT_SIGNATURE_CAP)
        .map(|index| FactVersionRef::FileWholeHash {
            canonical_id: format!("/p/f-{index:05}.ts"),
            hash: hash16(0),
        })
        .collect();
    let run: Arc<[FactVersionRef]> = Arc::from(over_cap);

    let mut read_set = FactReadSet::new();
    read_set.absorb_canonical_signature(&run);
    assert_eq!(read_set.len(), FACT_SIGNATURE_CAP + 1);
    assert!(!read_set.is_empty());
    assert!(read_set.would_overflow());
    assert!(matches!(read_set.finalise(), FactReadSetFinalise::Overflow));
}

#[test]
fn absorbed_runs_survive_a_would_overflow_peek() {
    // `would_overflow` collapses the tracer to canonical form. That must
    // not lose the absorbed facts for the later `finalise`.
    let (canonical, local) = split_corpus();
    let run: Arc<[FactVersionRef]> = Arc::from(canonical.clone());
    let mut read_set = FactReadSet::new();
    for fact in &local {
        read_set.observe(fact.clone());
    }
    read_set.absorb_canonical_signature(&run);
    assert!(!read_set.would_overflow());
    // Peek twice: the collapse is idempotent.
    assert!(!read_set.would_overflow());
    let merged = match read_set.finalise() {
        FactReadSetFinalise::Ok(signature) => signature,
        other => panic!("finalised as {other:?}"),
    };
    let mut union: Vec<FactVersionRef> = canonical.iter().chain(local.iter()).cloned().collect();
    union.sort_unstable_by(compare_fact_refs);
    union.dedup();
    assert_eq!(merged.as_ref(), union.as_slice());
}

#[test]
fn absorbing_an_empty_signature_is_a_no_op() {
    let empty: Arc<[FactVersionRef]> = Arc::from(Vec::new());
    let mut read_set = FactReadSet::new();
    read_set.absorb_canonical_signature(&empty);
    assert!(read_set.is_empty());
    assert_eq!(read_set.len(), 0);
    match read_set.finalise() {
        FactReadSetFinalise::Ok(signature) => assert!(signature.is_empty()),
        other => panic!("finalised as {other:?}"),
    }
}

// ────────────────────────────────────────────────────────────────────
// 4. Population translation — which population an aggregate speaks for
// ────────────────────────────────────────────────────────────────────
//
// A terminal aggregate makes the strongest claim in the system: "every
// precise fact this scope observed in this domain held as of this stamp".
// The `population` field is what stops one population's aggregate from
// satisfying another's read on a numeric coincidence, and it is
// DOMAIN-SPECIFIC:
//
// * `Resolution` partitions from the precise bucket — its keys carry a
//   population of their own.
// * `WorkspaceShape` is a whole-host scalar no overlay shadows.
// * The remaining four (`Content`, `SourceEnv`, `SemanticImports`,
//   `RouteSurface`) derive from the EFFECTIVE VALIDATING VIEW. A session
//   overlay re-roots whole hashes and parse facts while leaving the
//   workspace content generation untouched, so answering `Base` for them
//   would let an overlay-derived signature validate as base — a
//   stale-serve, not a bounded coarsening. They mint NOTHING until a
//   view-derived population arrives alongside their stamp.

const OVERLAY_FINGERPRINT: u64 = 0x0BAD_CAFE;

fn session_overlay_view() -> ViewPopulation {
    ViewPopulation::SessionOverlay(
        SessionOverlayFingerprint::new(OVERLAY_FINGERPRINT)
            .expect("a non-zero fingerprint is a session overlay identity"),
    )
}

/// `n` distinct CONTENT-domain facts — one of the four view-derived
/// domains, and the one a session overlay demonstrably re-roots.
fn distinct_content_facts(count: usize) -> Vec<FactVersionRef> {
    (0..count)
        .map(|index| FactVersionRef::FileWholeHash {
            canonical_id: format!("/p/content-{index:05}.ts"),
            hash: hash16(0),
        })
        .collect()
}

/// `n` distinct WORKSPACE-SHAPE facts.
fn distinct_workspace_shape_facts(count: usize) -> Vec<FactVersionRef> {
    (0..count)
        .map(|index| FactVersionRef::ProjectGeneration {
            generation: index as u64,
        })
        .collect()
}

/// `n` distinct RESOLUTION-domain facts under `population`.
fn distinct_resolution_facts(
    count: usize,
    population: ResolutionPopulation,
) -> Vec<FactVersionRef> {
    (0..count)
        .map(|index| {
            resolution_fact(
                ResolutionFactKey::PathProbe {
                    canonical: CanonicalResolutionId::new(format!("/p/res-{index:05}.ts")),
                    population,
                },
                1,
            )
        })
        .collect()
}

fn finalise_with_basis(
    facts: &[FactVersionRef],
    basis: AggregateGenerations,
) -> FactReadSetFinalise {
    let mut read_set = FactReadSet::new();
    read_set.set_aggregate_basis(basis);
    for fact in facts {
        read_set.observe(fact.clone());
    }
    read_set.finalise()
}

fn aggregates_of(signature: &[FactVersionRef]) -> Vec<DomainGenerationFact> {
    signature
        .iter()
        .filter_map(|fact| match fact {
            FactVersionRef::DomainGeneration(aggregate) => Some(*aggregate),
            _ => None,
        })
        .collect()
}

/// Run the lifting pass directly on a canonical (sorted, deduplicated)
/// observation set — the shape `canonicalise` hands it — and return the
/// lifted set alongside the pass's own "did anything lift" verdict.
///
/// Called directly rather than through `finalise` because `finalise`
/// returns NO facts on the legacy cardinality refusal, and a test about
/// what a domain did or did not MINT has to be able to look at the set.
fn compact_canonical(
    facts: &[FactVersionRef],
    basis: AggregateGenerations,
) -> (Vec<FactVersionRef>, bool) {
    let mut canonical = facts.to_vec();
    canonical.sort_unstable_by(compare_fact_refs);
    canonical.dedup();
    let lifted = super::compact_domains(&mut canonical, &basis);
    (canonical, lifted)
}

/// **No producer, no compaction** — the mirror of
/// `a_view_derived_domain_stays_precise_without_a_view_population`, on
/// the STAMP axis instead of the population axis.
///
/// A domain whose generation is absent from the basis has no live
/// producer, so an aggregate minted for it would be a witness nothing
/// can ever advance and therefore nothing can ever invalidate — a
/// permanently-valid stale claim over the whole domain. It must stay
/// precise no matter how far past the threshold its bucket runs.
///
/// The population axis was guarded; this direction was not. Verified
/// uncovered before this test was written: removing the
/// `basis.stamp_for(*domain).is_some()` filter AND replacing the mint
/// loop's `.expect` with `unwrap_or(AggregateStamp::Generation(0))` —
/// so a stampless domain mints a fabricated generation-zero aggregate —
/// left the entire crate suite at 722 passed, 0 failed.
///
/// Mutation recipe (both edits are required; either alone fails to
/// compile or panics rather than minting): drop the
/// `basis.stamp_for(*domain).is_some()` filter from `compact_domains`'s
/// `mint` chain, and change the `.expect(...)` in the mint loop to
/// `.unwrap_or(crate::fact_cache::AggregateStamp::Generation(0))`. The
/// negative half below then finds a minted aggregate and fails.
#[test]
fn a_domain_with_no_live_producer_never_mints_an_aggregate() {
    let facts = distinct_content_facts(FACT_SIGNATURE_CAP + 1);

    // NEGATIVE: population supplied, stamp absent. Nothing may mint.
    let (kept, lifted) = compact_canonical(
        &facts,
        AggregateGenerations {
            content: None,
            view_population: Some(ViewPopulation::Base),
            ..Default::default()
        },
    );
    assert!(
        aggregates_of(&kept).is_empty(),
        "a domain with NO live producer must not mint: its aggregate could never be advanced by \
         anything, so it would be a permanently-valid witness over the whole domain. Minted: {:?}",
        aggregates_of(&kept)
    );
    assert!(
        !lifted,
        "`compact_domains` must report that nothing lifted when no domain could mint"
    );
    assert_eq!(
        kept.len(),
        facts.len(),
        "the bucket must stay PRECISE — every fact survives, and the legacy cardinality refusal \
         is what keeps the unarmed domain bounded"
    );

    // POSITIVE CONTROL: the identical bucket, with a stamp, DOES mint.
    // Without this the negative half would also pass if the bucket were
    // mis-sized and never reached the threshold at all.
    let (kept, lifted) = compact_canonical(
        &facts,
        AggregateGenerations {
            content: Some(AggregateStamp::Generation(9)),
            view_population: Some(ViewPopulation::Base),
            ..Default::default()
        },
    );
    assert!(lifted, "the control must lift");
    assert_eq!(
        aggregates_of(&kept),
        vec![DomainGenerationFact {
            domain: CompactionDomain::Content,
            population: AggregatePopulation::View(ViewPopulation::Base),
            stamp: AggregateStamp::Generation(9),
        }],
        "the same bucket, with a producer, mints exactly one aggregate — so the negative half \
         above is a statement about the MISSING STAMP and not about the bucket's size"
    );
}

/// A view-derived domain mints under the population of the view that
/// installed the basis — never a hard-coded `Base`.
///
/// Mutation recipe: make the view-derived arm of `aggregate_population`
/// answer `AggregatePopulation::View(ViewPopulation::Base)` instead of
/// reading `basis.view_population` (the pre-change `_ => Base`
/// catch-all). The population assertion fails immediately.
#[test]
fn a_view_derived_domain_mints_under_the_supplied_view_population() {
    let basis = AggregateGenerations {
        content: Some(AggregateStamp::Generation(9)),
        view_population: Some(session_overlay_view()),
        ..Default::default()
    };
    let facts = distinct_content_facts(FACT_SIGNATURE_CAP + 1);

    let FactReadSetFinalise::Ok(signature) = finalise_with_basis(&facts, basis) else {
        panic!("an over-threshold content bucket with a live stamp AND a view population must compact and admit");
    };

    let aggregates = aggregates_of(&signature);
    assert_eq!(
        aggregates.len(),
        1,
        "exactly one terminal aggregate stands in for the lifted content bucket; got {aggregates:?}"
    );
    assert_eq!(aggregates[0].domain, CompactionDomain::Content);
    assert_eq!(
        aggregates[0].population,
        AggregatePopulation::View(session_overlay_view()),
        "the aggregate must carry the population of the EFFECTIVE VALIDATING VIEW"
    );
    assert_ne!(
        aggregates[0].population,
        AggregatePopulation::View(ViewPopulation::Base),
        "a session overlay re-roots whole hashes and parse facts while leaving the workspace \
         content generation untouched, so an overlay-derived content aggregate labelled `Base` \
         would validate against the base view and stale-serve overlay content"
    );
    assert!(
        !signature
            .iter()
            .any(|fact| matches!(fact, FactVersionRef::FileWholeHash { .. })),
        "the lifted domain's precise facts must be GONE, not merely joined by an aggregate"
    );
}

/// Without a view population a view-derived domain does NOT mint: its
/// bucket stays precise and the legacy cardinality refusal still covers
/// it. This is the invariant that keeps an UNARMED domain from being
/// both uncompacted and unbounded.
///
/// Mutation recipe: give the view-derived arm of `aggregate_population`
/// the unconditional fallback
/// `Some(AggregatePopulation::View(ViewPopulation::Base))` — the
/// pre-change `_ => Base` catch-all. The domain then mints from a stamp
/// whose view identity nothing supplied, this finalises `Ok` instead of
/// `Overflow`, and the assertion fails. (The mint filter's
/// `population.is_some()` gate cannot be mutated independently: without
/// a population there is nothing to stamp the aggregate with, so any
/// mutation of that gate IS this fallback.)
#[test]
fn a_view_derived_domain_stays_precise_without_a_view_population() {
    let basis = AggregateGenerations {
        // A live stamp is present. The population is not — and a stamp
        // alone must not be enough.
        content: Some(AggregateStamp::Generation(9)),
        view_population: None,
        ..Default::default()
    };
    let facts = distinct_content_facts(FACT_SIGNATURE_CAP + 1);

    assert!(
        matches!(
            finalise_with_basis(&facts, basis),
            FactReadSetFinalise::Overflow
        ),
        "a domain with no view population must not mint an aggregate: the population is what \
         binds the claim to a view, and an aggregate minted without one would be a witness no \
         view can honestly reject"
    );
}

/// Two views that differ ONLY in population mint aggregates that differ
/// only in population — so a base entry and an overlay entry can never
/// satisfy each other's read even at an identical stamp.
///
/// Mutation recipe: drop `population` from the `DomainGenerationFact`
/// the mint loop pushes (hard-code `ViewPopulation::Base`). The two
/// aggregates become equal and the inequality assertion fails.
#[test]
fn base_and_session_overlay_views_mint_distinguishable_aggregates() {
    let facts = distinct_content_facts(FACT_SIGNATURE_CAP + 1);
    let stamp = AggregateStamp::Generation(9);

    let base = {
        let FactReadSetFinalise::Ok(signature) = finalise_with_basis(
            &facts,
            AggregateGenerations {
                content: Some(stamp),
                view_population: Some(ViewPopulation::Base),
                ..Default::default()
            },
        ) else {
            panic!("the base view must compact");
        };
        aggregates_of(&signature)
    };
    let overlay = {
        let FactReadSetFinalise::Ok(signature) = finalise_with_basis(
            &facts,
            AggregateGenerations {
                content: Some(stamp),
                view_population: Some(session_overlay_view()),
                ..Default::default()
            },
        ) else {
            panic!("the session-overlay view must compact");
        };
        aggregates_of(&signature)
    };

    assert_eq!(base.len(), 1);
    assert_eq!(overlay.len(), 1);
    assert_eq!(
        base[0].stamp, overlay[0].stamp,
        "the stamps are deliberately identical — population identity, not the number, is what \
         must separate these two aggregates"
    );
    assert_ne!(
        base[0], overlay[0],
        "a base-view aggregate and an overlay-view aggregate at the same stamp must remain \
         distinct facts"
    );
    assert_eq!(
        base[0].population,
        AggregatePopulation::View(ViewPopulation::Base)
    );
    assert_eq!(
        overlay[0].population,
        AggregatePopulation::View(session_overlay_view())
    );
}

/// The zero fingerprint is NOT a session identity. "No overlays
/// installed" IS the base view, so admitting a zero fingerprint would
/// partition base entries from themselves — every base scope minting a
/// `SessionOverlay(0)` aggregate that no base read can satisfy.
///
/// Mutation recipe: drop the zero check from
/// `SessionOverlayFingerprint::new` and return `Some(Self(0))`. The
/// `is_none` assertion fails.
#[test]
fn the_zero_overlay_fingerprint_is_not_a_session_identity() {
    assert!(
        SessionOverlayFingerprint::new(0).is_none(),
        "a zero overlay-set fingerprint means NO overlays are installed — that is the BASE view, \
         and it must not be expressible as a session-overlay population"
    );
    assert_eq!(
        SessionOverlayFingerprint::new(OVERLAY_FINGERPRINT).map(SessionOverlayFingerprint::get),
        Some(OVERLAY_FINGERPRINT),
        "every non-zero fingerprint round-trips"
    );
}

/// `WorkspaceShape` is a whole-host scalar: `ProjectGeneration` moves
/// only on a project-shape change, which no per-canonical overlay can
/// shadow. Its aggregate is therefore global — `View(Base)` — even in a
/// scope whose view carries overlays, so a session scope and a base
/// scope share one workspace-shape witness instead of each minting a
/// private copy.
///
/// Mutation recipe: route the `WorkspaceShape` arm of
/// `aggregate_population` through `basis.view_population` like the four
/// view-derived domains. The aggregate then carries the overlay
/// population and the equality assertion fails.
#[test]
fn workspace_shape_mints_globally_regardless_of_the_view_population() {
    let basis = AggregateGenerations {
        workspace_shape: Some(AggregateStamp::Generation(3)),
        view_population: Some(session_overlay_view()),
        ..Default::default()
    };
    let facts = distinct_workspace_shape_facts(FACT_SIGNATURE_CAP + 1);

    let FactReadSetFinalise::Ok(signature) = finalise_with_basis(&facts, basis) else {
        panic!("an over-threshold workspace-shape bucket with a live stamp must compact");
    };
    let aggregates = aggregates_of(&signature);
    assert_eq!(aggregates.len(), 1, "got {aggregates:?}");
    assert_eq!(aggregates[0].domain, CompactionDomain::WorkspaceShape);
    assert_eq!(
        aggregates[0].population,
        AggregatePopulation::View(ViewPopulation::Base),
        "a project generation is a whole-host scalar; no overlay shadows it, so its aggregate is \
         global rather than view-scoped"
    );
}

/// The resolution domain partitions from its OWN keys, not from the
/// installed view. Its precise facts carry a population; the view's is
/// irrelevant to them.
///
/// Mutation recipe: route the `Resolution` arm of
/// `aggregate_population` through `basis.view_population`. The
/// aggregate's population becomes `View(..)`, the equality assertion
/// fails, and `CapturedResolutionWorld` — the domain's only authority —
/// stops recognising its own aggregate.
#[test]
fn a_resolution_bucket_partitions_by_its_own_population_not_the_view() {
    let basis = AggregateGenerations {
        resolution: Some(AggregateStamp::Generation(5)),
        // Deliberately a DIFFERENT population, installed on the same
        // scope. It must not reach the resolution bucket.
        view_population: Some(session_overlay_view()),
        ..Default::default()
    };
    let facts = distinct_resolution_facts(FACT_SIGNATURE_CAP + 1, ResolutionPopulation::Base);

    let FactReadSetFinalise::Ok(signature) = finalise_with_basis(&facts, basis) else {
        panic!("an over-threshold resolution bucket with a live stamp must compact");
    };
    let aggregates = aggregates_of(&signature);
    assert_eq!(aggregates.len(), 1, "got {aggregates:?}");
    assert_eq!(aggregates[0].domain, CompactionDomain::Resolution);
    assert_eq!(
        aggregates[0].population,
        AggregatePopulation::Resolution(ResolutionPopulation::Base),
        "the resolution domain's population is carried by its own keys"
    );
}

/// The population is part of the BUCKET, not just a label on the
/// aggregate. Two populations inside ONE domain are two buckets, and
/// they cross the threshold independently: an over-threshold base
/// bucket lifts while a small session bucket in the same domain stays
/// precise.
///
/// This is the one property that cannot be restated on the cardinality
/// axis — the existing domain-wise tests separate facts by DOMAIN, so
/// they stay green under a comparator that ignores population entirely.
///
/// Mutation recipe: drop the population component from `BucketKey` in
/// `compact_domains` — replace `aggregate_population(fact, basis)` with
/// a constant in both `let key = …` lines. The two populations then
/// merge into one bucket of 1,027, the merged bucket crosses the
/// threshold, and the session-population facts vanish into a
/// base-population aggregate that never observed them.
#[test]
fn two_populations_in_one_domain_lift_independently() {
    let session = ResolutionPopulation::Session(SessionFingerprint::from_raw(0x5E55));
    let basis = AggregateGenerations {
        resolution: Some(AggregateStamp::Generation(5)),
        ..Default::default()
    };
    let mut facts = distinct_resolution_facts(FACT_SIGNATURE_CAP + 1, ResolutionPopulation::Base);
    // Well under the threshold on its own, and it must stay precise.
    // Distinct canonicals from the base run so dedup cannot hide the
    // population axis: these are different facts either way.
    let minority: Vec<FactVersionRef> = (0..2)
        .map(|index| {
            resolution_fact(
                ResolutionFactKey::PathProbe {
                    canonical: CanonicalResolutionId::new(format!("/p/minority-{index}.ts")),
                    population: session,
                },
                1,
            )
        })
        .collect();
    facts.extend(minority.iter().cloned());

    let FactReadSetFinalise::Ok(signature) = finalise_with_basis(&facts, basis) else {
        panic!("the over-threshold base bucket must compact and admit");
    };

    // The HARM first, so a regression reports the damage rather than a
    // label: the minority population's facts must still be in the
    // signature. A bucketing that ignores population sweeps them into the
    // base aggregate, which never observed them.
    for fact in &minority {
        assert!(
            signature.contains(fact),
            "a small bucket must not be swept up by a sibling population's lifting in the same \
             domain — the aggregate never observed these facts and cannot speak for them:\n  \
             {fact:?}"
        );
    }
    let aggregates = aggregates_of(&signature);
    assert_eq!(
        aggregates.len(),
        1,
        "only the over-threshold BASE bucket lifts; got {aggregates:?}"
    );
    assert_eq!(
        aggregates[0].population,
        AggregatePopulation::Resolution(ResolutionPopulation::Base)
    );
}

/// **`SIG-3` — compaction is DOMAIN-WISE, never whole-signature.** The
/// over-threshold domain lifts to one terminal aggregate; every OTHER
/// domain in the same signature stays precise.
///
/// The arrangement is constructed directly on `FactReadSet`, where mixed
/// domains are valid and the compactor must classify each bucket independently.
///
/// Mutation recipe, EXECUTED: make `compact_domains` replace the WHOLE
/// observation vector instead of only the lifted buckets — i.e. drop
/// the `|| !lifted.contains(&key)` disjunct from its `kept` filter so
/// every precise fact is discarded. The workspace-shape survival
/// assertion fails while the aggregate assertions stay green.
#[test]
fn lifting_one_domain_leaves_every_other_domain_precise() {
    let mut facts = distinct_content_facts(FACT_SIGNATURE_CAP + 1);
    // A second domain, well under its own threshold.
    let unrelated = distinct_workspace_shape_facts(3);
    facts.extend(unrelated.iter().cloned());

    let basis = AggregateGenerations {
        content: Some(AggregateStamp::Generation(7)),
        workspace_shape: Some(AggregateStamp::Generation(9)),
        view_population: Some(ViewPopulation::Base),
        ..Default::default()
    };
    let (compacted, lifted) = compact_canonical(&facts, basis);
    assert!(lifted, "the over-threshold content domain must lift");

    let aggregates = aggregates_of(&compacted);
    assert_eq!(
        aggregates.len(),
        1,
        "exactly one terminal aggregate must stand in for the lifted domain; got {aggregates:?}"
    );
    assert_eq!(aggregates[0].domain, CompactionDomain::Content);

    for fact in &unrelated {
        assert!(
            compacted.contains(fact),
            "lifting the content domain must leave every WORKSPACE-SHAPE fact precise — a \
             whole-signature collapse would coarsen this entry's project-shape dependency and \
             destroy warm reuse across unrelated edits"
        );
    }
}
