//! The counter vocabulary. Stable names are the export contract.

use super::OwnerPhase;

/// Bumped when a counter is added, removed or renamed.
pub const WORK_LEDGER_SCHEMA_VERSION: u32 = 1;

/// Number of [`WorkKind`] variants.
pub const WORK_KIND_COUNT: usize = 49;

/// Counter family: the prefix of the stable name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorkFamily {
    Parse,
    Semantic,
    Structure,
    Style,
    Planning,
    Emission,
    Reuse,
    Memory,
    Concurrency,
}

/// One labelled counter. Declaration order is export order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum WorkKind {
    // parse
    ParseFullSourceScans,
    /// Keyed by grammar.
    ParseRegionScans,
    /// Keyed by grammar.
    ParseBytes,
    ParseExpressionAttempts,
    ParseAuthoritativeExpressionParses,
    ParseDownstreamReparses,
    ParseRawSourceCopyBytes,
    ParseSemanticNormalizationBytes,
    // semantic
    SemanticFactFamiliesDemanded,
    SemanticFactsProduced,
    SemanticFactReads,
    SemanticBindingLookups,
    SemanticDependencySets,
    SemanticDependencyEdges,
    SemanticProvenanceEntries,
    // structure
    StructureNodesMaterialized,
    StructureRegions,
    StructureTopologyNodes,
    StructureSourceSizedVisits,
    StructureRegionalVisits,
    StructureGraphVisits,
    // style
    StyleBlocks,
    StyleSelectorPlans,
    StyleIndexBuilds,
    StyleCandidateNodes,
    StylePredicateTests,
    StyleCombinatorHops,
    /// Keyed by match outcome.
    StyleMatchYesMaybeNo,
    StylePrunedRules,
    StyleWitnessesMaterialized,
    // planning
    PlanningTargetEntries,
    PlanningEffectNodes,
    PlanningEffectEdges,
    PlanningMultiTargetSharedPrerequisites,
    // emission
    EmissionSegments,
    EmissionSourceSliceBytes,
    EmissionGeneratedBytes,
    EmissionCopyBytes,
    EmissionAllocations,
    EmissionMapSegments,
    // reuse
    ReuseCandidates,
    ReuseValidated,
    ReuseRejectedByBasis,
    ReuseRecomputed,
    // memory
    /// Keyed by lifetime class.
    MemoryAllocatedByLifetime,
    /// Keyed by lifetime class.
    MemoryPeakByLifetime,
    /// Keyed by product.
    MemoryRetainedByProduct,
    // concurrency
    ConcurrencyTasksSpawned,
    ConcurrencyCancellationWaste,
}

impl WorkKind {
    /// Every variant, in declaration order.
    pub const ALL: [WorkKind; WORK_KIND_COUNT] = [
        WorkKind::ParseFullSourceScans,
        WorkKind::ParseRegionScans,
        WorkKind::ParseBytes,
        WorkKind::ParseExpressionAttempts,
        WorkKind::ParseAuthoritativeExpressionParses,
        WorkKind::ParseDownstreamReparses,
        WorkKind::ParseRawSourceCopyBytes,
        WorkKind::ParseSemanticNormalizationBytes,
        WorkKind::SemanticFactFamiliesDemanded,
        WorkKind::SemanticFactsProduced,
        WorkKind::SemanticFactReads,
        WorkKind::SemanticBindingLookups,
        WorkKind::SemanticDependencySets,
        WorkKind::SemanticDependencyEdges,
        WorkKind::SemanticProvenanceEntries,
        WorkKind::StructureNodesMaterialized,
        WorkKind::StructureRegions,
        WorkKind::StructureTopologyNodes,
        WorkKind::StructureSourceSizedVisits,
        WorkKind::StructureRegionalVisits,
        WorkKind::StructureGraphVisits,
        WorkKind::StyleBlocks,
        WorkKind::StyleSelectorPlans,
        WorkKind::StyleIndexBuilds,
        WorkKind::StyleCandidateNodes,
        WorkKind::StylePredicateTests,
        WorkKind::StyleCombinatorHops,
        WorkKind::StyleMatchYesMaybeNo,
        WorkKind::StylePrunedRules,
        WorkKind::StyleWitnessesMaterialized,
        WorkKind::PlanningTargetEntries,
        WorkKind::PlanningEffectNodes,
        WorkKind::PlanningEffectEdges,
        WorkKind::PlanningMultiTargetSharedPrerequisites,
        WorkKind::EmissionSegments,
        WorkKind::EmissionSourceSliceBytes,
        WorkKind::EmissionGeneratedBytes,
        WorkKind::EmissionCopyBytes,
        WorkKind::EmissionAllocations,
        WorkKind::EmissionMapSegments,
        WorkKind::ReuseCandidates,
        WorkKind::ReuseValidated,
        WorkKind::ReuseRejectedByBasis,
        WorkKind::ReuseRecomputed,
        WorkKind::MemoryAllocatedByLifetime,
        WorkKind::MemoryPeakByLifetime,
        WorkKind::MemoryRetainedByProduct,
        WorkKind::ConcurrencyTasksSpawned,
        WorkKind::ConcurrencyCancellationWaste,
    ];

    /// Stable dotted name, e.g. `parse.full_source_scans`.
    pub const fn name(self) -> &'static str {
        match self {
            WorkKind::ParseFullSourceScans => "parse.full_source_scans",
            WorkKind::ParseRegionScans => "parse.region_scans",
            WorkKind::ParseBytes => "parse.bytes",
            WorkKind::ParseExpressionAttempts => "parse.expression_attempts",
            WorkKind::ParseAuthoritativeExpressionParses => "parse.authoritative_expression_parses",
            WorkKind::ParseDownstreamReparses => "parse.downstream_reparses",
            WorkKind::ParseRawSourceCopyBytes => "parse.raw_source_copy_bytes",
            WorkKind::ParseSemanticNormalizationBytes => "parse.semantic_normalization_bytes",
            WorkKind::SemanticFactFamiliesDemanded => "semantic.fact_families_demanded",
            WorkKind::SemanticFactsProduced => "semantic.facts_produced",
            WorkKind::SemanticFactReads => "semantic.fact_reads",
            WorkKind::SemanticBindingLookups => "semantic.binding_lookups",
            WorkKind::SemanticDependencySets => "semantic.dependency_sets",
            WorkKind::SemanticDependencyEdges => "semantic.dependency_edges",
            WorkKind::SemanticProvenanceEntries => "semantic.provenance_entries",
            WorkKind::StructureNodesMaterialized => "structure.nodes_materialized",
            WorkKind::StructureRegions => "structure.regions",
            WorkKind::StructureTopologyNodes => "structure.topology_nodes",
            WorkKind::StructureSourceSizedVisits => "structure.source_sized_visits",
            WorkKind::StructureRegionalVisits => "structure.regional_visits",
            WorkKind::StructureGraphVisits => "structure.graph_visits",
            WorkKind::StyleBlocks => "style.blocks",
            WorkKind::StyleSelectorPlans => "style.selector_plans",
            WorkKind::StyleIndexBuilds => "style.index_builds",
            WorkKind::StyleCandidateNodes => "style.candidate_nodes",
            WorkKind::StylePredicateTests => "style.predicate_tests",
            WorkKind::StyleCombinatorHops => "style.combinator_hops",
            WorkKind::StyleMatchYesMaybeNo => "style.match_yes_maybe_no",
            WorkKind::StylePrunedRules => "style.pruned_rules",
            WorkKind::StyleWitnessesMaterialized => "style.witnesses_materialized",
            WorkKind::PlanningTargetEntries => "planning.target_entries",
            WorkKind::PlanningEffectNodes => "planning.effect_nodes",
            WorkKind::PlanningEffectEdges => "planning.effect_edges",
            WorkKind::PlanningMultiTargetSharedPrerequisites => {
                "planning.multi_target_shared_prerequisites"
            }
            WorkKind::EmissionSegments => "emission.segments",
            WorkKind::EmissionSourceSliceBytes => "emission.source_slice_bytes",
            WorkKind::EmissionGeneratedBytes => "emission.generated_bytes",
            WorkKind::EmissionCopyBytes => "emission.copy_bytes",
            WorkKind::EmissionAllocations => "emission.allocations",
            WorkKind::EmissionMapSegments => "emission.map_segments",
            WorkKind::ReuseCandidates => "reuse.candidates",
            WorkKind::ReuseValidated => "reuse.validated",
            WorkKind::ReuseRejectedByBasis => "reuse.rejected_by_basis",
            WorkKind::ReuseRecomputed => "reuse.recomputed",
            WorkKind::MemoryAllocatedByLifetime => "memory.allocated_by_lifetime",
            WorkKind::MemoryPeakByLifetime => "memory.peak_by_lifetime",
            WorkKind::MemoryRetainedByProduct => "memory.retained_by_product",
            WorkKind::ConcurrencyTasksSpawned => "concurrency.tasks_spawned",
            WorkKind::ConcurrencyCancellationWaste => "concurrency.cancellation_waste",
        }
    }
    pub const fn family(self) -> WorkFamily {
        match self {
            WorkKind::ParseFullSourceScans => WorkFamily::Parse,
            WorkKind::ParseRegionScans => WorkFamily::Parse,
            WorkKind::ParseBytes => WorkFamily::Parse,
            WorkKind::ParseExpressionAttempts => WorkFamily::Parse,
            WorkKind::ParseAuthoritativeExpressionParses => WorkFamily::Parse,
            WorkKind::ParseDownstreamReparses => WorkFamily::Parse,
            WorkKind::ParseRawSourceCopyBytes => WorkFamily::Parse,
            WorkKind::ParseSemanticNormalizationBytes => WorkFamily::Parse,
            WorkKind::SemanticFactFamiliesDemanded => WorkFamily::Semantic,
            WorkKind::SemanticFactsProduced => WorkFamily::Semantic,
            WorkKind::SemanticFactReads => WorkFamily::Semantic,
            WorkKind::SemanticBindingLookups => WorkFamily::Semantic,
            WorkKind::SemanticDependencySets => WorkFamily::Semantic,
            WorkKind::SemanticDependencyEdges => WorkFamily::Semantic,
            WorkKind::SemanticProvenanceEntries => WorkFamily::Semantic,
            WorkKind::StructureNodesMaterialized => WorkFamily::Structure,
            WorkKind::StructureRegions => WorkFamily::Structure,
            WorkKind::StructureTopologyNodes => WorkFamily::Structure,
            WorkKind::StructureSourceSizedVisits => WorkFamily::Structure,
            WorkKind::StructureRegionalVisits => WorkFamily::Structure,
            WorkKind::StructureGraphVisits => WorkFamily::Structure,
            WorkKind::StyleBlocks => WorkFamily::Style,
            WorkKind::StyleSelectorPlans => WorkFamily::Style,
            WorkKind::StyleIndexBuilds => WorkFamily::Style,
            WorkKind::StyleCandidateNodes => WorkFamily::Style,
            WorkKind::StylePredicateTests => WorkFamily::Style,
            WorkKind::StyleCombinatorHops => WorkFamily::Style,
            WorkKind::StyleMatchYesMaybeNo => WorkFamily::Style,
            WorkKind::StylePrunedRules => WorkFamily::Style,
            WorkKind::StyleWitnessesMaterialized => WorkFamily::Style,
            WorkKind::PlanningTargetEntries => WorkFamily::Planning,
            WorkKind::PlanningEffectNodes => WorkFamily::Planning,
            WorkKind::PlanningEffectEdges => WorkFamily::Planning,
            WorkKind::PlanningMultiTargetSharedPrerequisites => WorkFamily::Planning,
            WorkKind::EmissionSegments => WorkFamily::Emission,
            WorkKind::EmissionSourceSliceBytes => WorkFamily::Emission,
            WorkKind::EmissionGeneratedBytes => WorkFamily::Emission,
            WorkKind::EmissionCopyBytes => WorkFamily::Emission,
            WorkKind::EmissionAllocations => WorkFamily::Emission,
            WorkKind::EmissionMapSegments => WorkFamily::Emission,
            WorkKind::ReuseCandidates => WorkFamily::Reuse,
            WorkKind::ReuseValidated => WorkFamily::Reuse,
            WorkKind::ReuseRejectedByBasis => WorkFamily::Reuse,
            WorkKind::ReuseRecomputed => WorkFamily::Reuse,
            WorkKind::MemoryAllocatedByLifetime => WorkFamily::Memory,
            WorkKind::MemoryPeakByLifetime => WorkFamily::Memory,
            WorkKind::MemoryRetainedByProduct => WorkFamily::Memory,
            WorkKind::ConcurrencyTasksSpawned => WorkFamily::Concurrency,
            WorkKind::ConcurrencyCancellationWaste => WorkFamily::Concurrency,
        }
    }
    /// The phase this counter is charged to.
    pub const fn phase(self) -> OwnerPhase {
        match self.family() {
            WorkFamily::Parse => OwnerPhase::Parse,
            WorkFamily::Semantic => OwnerPhase::Semantic,
            WorkFamily::Structure => OwnerPhase::Structure,
            WorkFamily::Style => OwnerPhase::Style,
            WorkFamily::Planning => OwnerPhase::Planning,
            WorkFamily::Emission => OwnerPhase::Emission,
            WorkFamily::Reuse => OwnerPhase::Reuse,
            WorkFamily::Memory => OwnerPhase::Assembly,
            WorkFamily::Concurrency => OwnerPhase::Assembly,
        }
    }
    /// Whether this counter requires a non-`None` key.
    pub const fn is_keyed(self) -> bool {
        matches!(
            self,
            WorkKind::ParseRegionScans
                | WorkKind::ParseBytes
                | WorkKind::StyleMatchYesMaybeNo
                | WorkKind::MemoryAllocatedByLifetime
                | WorkKind::MemoryPeakByLifetime
                | WorkKind::MemoryRetainedByProduct
        )
    }
    /// One-line definition, exported with the schema.
    pub const fn definition(self) -> &'static str {
        match self {
            WorkKind::ParseFullSourceScans => {
                "Passes that walk the entire source text from start to end."
            }
            WorkKind::ParseRegionScans => {
                "Passes that walk one region of the source, keyed by the grammar that owns it."
            }
            WorkKind::ParseBytes => "Source bytes consumed by parsing, keyed by grammar.",
            WorkKind::ParseExpressionAttempts => {
                "Expression parse attempts, including ones that did not become authoritative."
            }
            WorkKind::ParseAuthoritativeExpressionParses => {
                "Expression parses whose result is the one every later stage reads."
            }
            WorkKind::ParseDownstreamReparses => {
                "Parses of text an earlier stage already parsed; zero for strict valid input."
            }
            WorkKind::ParseRawSourceCopyBytes => {
                "Source bytes copied into a separate buffer while parsing."
            }
            WorkKind::ParseSemanticNormalizationBytes => {
                "Bytes rewritten to normalise text for semantic analysis."
            }
            WorkKind::SemanticFactFamiliesDemanded => {
                "Semantic fact families the request demanded."
            }
            WorkKind::SemanticFactsProduced => "Semantic facts computed.",
            WorkKind::SemanticFactReads => "Reads of already computed semantic facts.",
            WorkKind::SemanticBindingLookups => "Binding lookups performed against scopes.",
            WorkKind::SemanticDependencySets => "Dependency sets built.",
            WorkKind::SemanticDependencyEdges => "Edges recorded across all dependency sets.",
            WorkKind::SemanticProvenanceEntries => "Provenance entries recorded for facts.",
            WorkKind::StructureNodesMaterialized => "Structure nodes built.",
            WorkKind::StructureRegions => "Source regions recorded by the structure.",
            WorkKind::StructureTopologyNodes => "Nodes in the template topology.",
            WorkKind::StructureSourceSizedVisits => {
                "Traversals whose cost grows with the whole source."
            }
            WorkKind::StructureRegionalVisits => "Traversals bounded by one region.",
            WorkKind::StructureGraphVisits => "Node visits while walking a structure graph.",
            WorkKind::StyleBlocks => "Style blocks processed.",
            WorkKind::StyleSelectorPlans => "Selector plans built.",
            WorkKind::StyleIndexBuilds => "Template indexes built for selector matching.",
            WorkKind::StyleCandidateNodes => "Template nodes considered as match candidates.",
            WorkKind::StylePredicateTests => "Selector predicate evaluations.",
            WorkKind::StyleCombinatorHops => "Combinator steps taken while matching.",
            WorkKind::StyleMatchYesMaybeNo => "Selector match results, keyed by outcome.",
            WorkKind::StylePrunedRules => "Rules removed before matching.",
            WorkKind::StyleWitnessesMaterialized => "Match witnesses built.",
            WorkKind::PlanningTargetEntries => "Target entries planned.",
            WorkKind::PlanningEffectNodes => "Nodes in the effect plan.",
            WorkKind::PlanningEffectEdges => "Edges in the effect plan.",
            WorkKind::PlanningMultiTargetSharedPrerequisites => {
                "Prerequisites shared by more than one planned target."
            }
            WorkKind::EmissionSegments => "Output segments emitted.",
            WorkKind::EmissionSourceSliceBytes => {
                "Bytes of source text copied verbatim into output."
            }
            WorkKind::EmissionGeneratedBytes => "Bytes of generated text written to output.",
            WorkKind::EmissionCopyBytes => "Bytes copied while assembling output.",
            WorkKind::EmissionAllocations => "Allocations made while emitting.",
            WorkKind::EmissionMapSegments => "Source map segments emitted.",
            WorkKind::ReuseCandidates => "Earlier results considered for reuse.",
            WorkKind::ReuseValidated => "Candidates accepted after validation.",
            WorkKind::ReuseRejectedByBasis => "Candidates rejected because their basis changed.",
            WorkKind::ReuseRecomputed => "Results computed again instead of reused.",
            WorkKind::MemoryAllocatedByLifetime => "Bytes allocated, keyed by lifetime class.",
            WorkKind::MemoryPeakByLifetime => "Peak bytes live at once, keyed by lifetime class.",
            WorkKind::MemoryRetainedByProduct => {
                "Bytes retained by a finished product, keyed by product."
            }
            WorkKind::ConcurrencyTasksSpawned => "Tasks started on other threads.",
            WorkKind::ConcurrencyCancellationWaste => {
                "Work completed before a cancellation took effect."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_lists_every_kind_once() {
        assert_eq!(WorkKind::ALL.len(), 49);
        for (index, kind) in WorkKind::ALL.iter().enumerate() {
            assert_eq!(*kind as usize, index);
        }
    }

    #[test]
    fn names_are_unique_and_prefixed_by_family() {
        let mut seen = HashSet::new();
        for kind in WorkKind::ALL {
            assert!(seen.insert(kind.name()), "duplicate {}", kind.name());
            let family = format!("{:?}", kind.family()).to_lowercase();
            assert!(
                kind.name().starts_with(&format!("{family}.")),
                "{} is not prefixed by {family}",
                kind.name()
            );
        }
    }

    #[test]
    fn every_kind_has_a_definition() {
        for kind in WorkKind::ALL {
            assert!(!kind.definition().is_empty(), "{}", kind.name());
        }
    }

    #[test]
    fn exactly_six_kinds_are_keyed() {
        assert_eq!(WorkKind::ALL.iter().filter(|k| k.is_keyed()).count(), 6);
    }

    #[test]
    fn memory_and_concurrency_are_charged_to_assembly() {
        assert_eq!(WorkKind::MemoryPeakByLifetime.phase(), OwnerPhase::Assembly);
        assert_eq!(
            WorkKind::ConcurrencyTasksSpawned.phase(),
            OwnerPhase::Assembly
        );
        assert_eq!(WorkKind::StyleBlocks.phase(), OwnerPhase::Style);
    }
}
