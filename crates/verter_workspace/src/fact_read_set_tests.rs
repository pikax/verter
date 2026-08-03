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

use std::sync::Arc;

use verter_language::{FileLanguage, FrameworkAdapterId, LanguageId, ScriptSourceType};

use super::{compare_fact_refs, FactReadSet, FactReadSetFinalise, FACT_SIGNATURE_CAP};
use crate::fact_cache::{
    DerivedFactKind, FactVersionRef, ParseEnvHash, ParseFactRef, ResolveImportsFactRef,
    RouteSurfaceFactRef,
};
use crate::fact_registry::{FactKey, FactLane, InternedName, InternedSpecifier, SymbolSpace};
use crate::resolution_currency::{
    CanonicalResolutionId, RawSpecifier, ResolutionEntry, ResolutionFactKey, ResolutionFactRef,
    ResolutionFactVersion, ResolutionPopulation, SessionFingerprint,
};
use crate::types::{ResolvePhase, ResolveRequestKind};

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
                population: ResolutionPopulation::Session(SessionFingerprint::fresh(0x51)),
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
            key: FactKey::EffectiveExportSet,
            lane: FactLane::Semantic,
            expected_hash: hash16(12),
        }),
        FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: "/p/barrel.ts".to_string(),
            key: FactKey::EffectiveExportSet,
            lane: FactLane::Display,
            expected_hash: hash16(12),
        }),
        // Four `FileSourceEnv` facts identical except for `file_language_id` —
        // the open-set field that used to be compared through its Debug form.
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parser_version: 3,
            file_language_id: FileLanguage::script(ScriptSourceType::Ts),
        },
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parser_version: 3,
            file_language_id: FileLanguage::script(ScriptSourceType::Tsx),
        },
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parser_version: 3,
            file_language_id: FileLanguage::Framework {
                adapter_id: FrameworkAdapterId::vue(),
                language_id: LanguageId::new("vue"),
            },
        },
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parser_version: 3,
            file_language_id: FileLanguage::Framework {
                adapter_id: FrameworkAdapterId::svelte(),
                language_id: LanguageId::new("svelte"),
            },
        },
        // Same adapter, different language id.
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parser_version: 3,
            file_language_id: FileLanguage::Framework {
                adapter_id: FrameworkAdapterId::svelte(),
                language_id: LanguageId::new("svelte_template"),
            },
        },
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parser_version: 3,
            file_language_id: FileLanguage::FrameworkTemplate {
                adapter_id: FrameworkAdapterId::vue(),
                owner_hint: None,
            },
        },
        FactVersionRef::FileSourceEnv {
            canonical_id: "/p/a.ts".to_string(),
            parse_env_hash: ParseEnvHash::from_env_hash(hash16(13)),
            parser_version: 3,
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
            parser_version: 1,
            file_language_id: FileLanguage::script(ScriptSourceType::Ts),
        },
        FactVersionRef::RouteSurface(RouteSurfaceFactRef {
            canonical_id: "/p/a.ts".to_string(),
            key: FactKey::EffectiveExportSet,
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
            parser_version: 1,
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
