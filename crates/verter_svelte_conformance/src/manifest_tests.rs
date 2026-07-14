use super::*;
use crate::model::{
    ElementRegion, Quoting, SelectorKind, SelectorValueRepresentation, StructuralKind, Target,
    TemplateValueRepresentation,
};

/// The full candidate product of the nine factors.
fn full_product() -> u64 {
    factor_cardinalities()
        .iter()
        .map(|&c| u64::from(c))
        .product()
}

// ---------------------------------------------------------------------------
// Spec shape
// ---------------------------------------------------------------------------

/// The spec pins the nine level counts, global 3-wise strength, and exactly
/// the two strengthened interaction groups at their factor indices.
#[test]
fn coverage_spec_matches_the_declared_design() {
    let spec = coverage_spec();
    assert_eq!(spec.cardinalities, [6, 7, 4, 3, 3, 6, 2, 5, 3]);
    assert_eq!(spec.global_strength, 3);
    assert_eq!(spec.interaction_groups.len(), 2);

    let five_wise = &spec.interaction_groups[0];
    assert_eq!(five_wise.factors, vec![1, 3, 4, 5, 8]);
    assert_eq!(five_wise.strength, 5);

    let four_wise = &spec.interaction_groups[1];
    assert_eq!(four_wise.factors, vec![0, 2, 7, 8]);
    assert_eq!(four_wise.strength, 4);

    assert_eq!(full_product(), 272_160, "raw candidate space");
    assert_eq!(SCHEMA_VERSION, 4);
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

/// Two independent builds produce byte-identical hashes, slugs, and proof
/// renderings (no map-order or other nondeterminism anywhere).
#[test]
fn manifest_builds_are_deterministic() {
    let first = build_manifest();
    let second = build_manifest();
    assert_eq!(first.manifest_hash(), second.manifest_hash());
    assert_eq!(first.all_slugs(), second.all_slugs());
    assert_eq!(first.proof().render(), second.proof().render());
    assert!(
        first.manifest_hash().starts_with("fnv1a64-"),
        "hash format: {}",
        first.manifest_hash()
    );
    assert_eq!(first.manifest_hash().len(), "fnv1a64-".len() + 16);

    // The singleton serves the same content.
    assert_eq!(manifest().manifest_hash(), first.manifest_hash());
}

// ---------------------------------------------------------------------------
// Coverage reality
// ---------------------------------------------------------------------------

/// The manifest's own selection independently re-verifies against the spec,
/// and the selection is a real compression of the full product.
#[test]
fn selection_verifies_and_compresses_the_space() {
    let manifest = manifest();
    let classified: Vec<ClassifiedRow<FACTOR_COUNT>> = manifest
        .cases()
        .iter()
        .map(|case| ClassifiedRow {
            row: case.row,
            partition: case.disposition.partition(),
        })
        .collect();
    let proof =
        verify(manifest.spec(), &classified, classify_row).expect("manifest coverage is complete");
    assert_eq!(proof.render(), manifest.proof().render());

    let cases = manifest.cases().len() as u64;
    assert!(cases >= 100, "a real mixed-strength selection, got {cases}");
    assert!(
        cases * 100 <= full_product(),
        "at least 100x compression: {cases} cases vs {} rows",
        full_product()
    );

    // Dropping any single case breaks coverage (the reverse-delete already
    // minimized the selection) — spot-check the first, middle, and last.
    for index in [0, manifest.cases().len() / 2, manifest.cases().len() - 1] {
        let mut pruned = classified.clone();
        pruned.remove(index);
        assert!(
            verify(manifest.spec(), &pruned, classify_row).is_err(),
            "case {index} is load-bearing"
        );
    }
}

/// Every case is a non-Invalid row whose stored fields agree with the model,
/// and the two backends are always both present.
#[test]
fn cases_expose_the_stable_typed_contract() {
    let manifest = manifest();
    for case in manifest.cases() {
        assert_eq!(
            RowLevels::decode(case.row),
            Some(case.levels),
            "{}",
            case.slug
        );
        assert_eq!(case.levels.encode(), case.row, "{}", case.slug);
        assert_eq!(case.slug, slug(&case.levels), "slug authority");
        assert_eq!(case.expected_outcome, case.levels.outcome, "{}", case.slug);
        assert_eq!(
            case.backends,
            [CompileTarget::Client, CompileTarget::Server],
            "full backend expansion for {}",
            case.slug
        );
        assert_eq!(
            case.compile_options,
            compile_options(&case.levels),
            "compile-options authority"
        );
        assert_eq!(
            case.disposition,
            classify(&case.levels),
            "classify authority"
        );
        assert!(
            !matches!(case.disposition, Disposition::Invalid(_)),
            "invalid rows are never selected: {}",
            case.slug
        );
        let source = case.render_source();
        assert!(source.contains("<style>"), "renderable: {}", case.slug);
    }

    // Slug index round-trips and rejects unknown slugs.
    let first = &manifest.cases()[0];
    assert_eq!(
        manifest.case_for_slug(&first.slug).map(|c| c.row),
        Some(first.row)
    );
    assert_eq!(manifest.case_for_slug("no-such-slug"), None);
    assert_eq!(manifest.all_slugs().len(), manifest.cases().len());
}

/// Every declared refusal and oracle partition owns at least one selected
/// case; Supported cases dominate; no Invalid case exists.
#[test]
fn refusal_and_oracle_partitions_each_own_a_case() {
    let manifest = manifest();
    for &kind in RefusalKind::ALL {
        assert!(
            manifest
                .cases()
                .iter()
                .any(|case| case.disposition == Disposition::Refused(kind)),
            "refusal partition {kind:?} needs a focus case"
        );
    }
    for &kind in DiagnosticKind::ALL {
        assert!(
            manifest
                .cases()
                .iter()
                .any(|case| case.disposition == Disposition::OracleRejected(kind)),
            "oracle partition {kind:?} needs a focus case"
        );
    }
    let supported = manifest
        .cases()
        .iter()
        .filter(|case| case.disposition == Disposition::Supported)
        .count();
    assert!(
        supported * 2 > manifest.cases().len(),
        "supported cases dominate the selection"
    );
}

/// The full-space inventories tally every partition and sum to the product;
/// every declared kind is listed (zero rows would be a broken constraint).
#[test]
fn full_space_inventories_are_exhaustive() {
    let manifest = manifest();
    let refused: u64 = manifest.refused_inventory().iter().map(|&(_, n)| n).sum();
    let oracle: u64 = manifest
        .oracle_rejected_inventory()
        .iter()
        .map(|&(_, n)| n)
        .sum();
    let invalid: u64 = manifest.invalid_inventory().iter().map(|&(_, n)| n).sum();
    assert_eq!(
        manifest.supported_row_count() + refused + oracle + invalid,
        full_product(),
        "every row lands in exactly one partition"
    );
    assert!(manifest.supported_row_count() > 0);

    assert_eq!(manifest.refused_inventory().len(), RefusalKind::ALL.len());
    assert_eq!(
        manifest.oracle_rejected_inventory().len(),
        DiagnosticKind::ALL.len()
    );
    assert_eq!(
        manifest.invalid_inventory().len(),
        ConstraintKind::ALL.len()
    );
    for &(kind, count) in manifest.refused_inventory() {
        assert!(count > 0, "declared refusal {kind:?} never fires");
    }
    for &(kind, count) in manifest.oracle_rejected_inventory() {
        assert!(count > 0, "declared diagnostic {kind:?} never fires");
    }
    for &(kind, count) in manifest.invalid_inventory() {
        assert!(count > 0, "declared constraint {kind:?} never fires");
    }
}

/// The manifest hash pins the COMPLETE full-space classification stream, not
/// just the per-partition counts: a COUNT-PRESERVING reshuffle (swapping the
/// `ConstraintKind`s of two invalid rows) leaves every inventory identical —
/// so inventories alone could never discriminate it — yet MUST change the
/// classification digest and therefore the manifest hash.
#[test]
fn manifest_hash_pins_the_full_space_classification_stream() {
    let spec = coverage_spec();
    let baseline = full_space_facts(&spec, classify);

    // Locate two Invalid rows of DIFFERENT constraint kinds.
    let mut first: Option<(RowLevels, ConstraintKind)> = None;
    let mut second: Option<(RowLevels, ConstraintKind)> = None;
    for_each_row(&spec, |levels| {
        if second.is_some() {
            return;
        }
        if let Disposition::Invalid(kind) = classify(&levels) {
            match first {
                None => first = Some((levels, kind)),
                Some((_, first_kind)) if kind != first_kind && second.is_none() => {
                    second = Some((levels, kind));
                }
                Some(_) => {}
            }
        }
    });
    let (row_a, kind_a) = first.expect("an Invalid row exists");
    let (row_b, kind_b) = second.expect("a second Invalid kind exists");

    // The count-preserving reshuffle: row_a and row_b trade kinds.
    let reshuffled = full_space_facts(&spec, |levels| {
        if *levels == row_a {
            Disposition::Invalid(kind_b)
        } else if *levels == row_b {
            Disposition::Invalid(kind_a)
        } else {
            classify(levels)
        }
    });

    // Every inventory is IDENTICAL — the reshuffle is invisible to counts.
    assert_eq!(baseline.supported_rows, reshuffled.supported_rows);
    assert_eq!(baseline.refused, reshuffled.refused);
    assert_eq!(baseline.oracle_rejected, reshuffled.oracle_rejected);
    assert_eq!(
        baseline.invalid, reshuffled.invalid,
        "the reshuffle must preserve per-kind counts (otherwise this test \
         would not discriminate the stream digest from the inventories)"
    );

    // The classification stream digest moved…
    assert_ne!(
        baseline.classification_digest, reshuffled.classification_digest,
        "a count-preserving kind reshuffle must change the classification digest"
    );

    // …and it is load-bearing in the manifest hash.
    let manifest = manifest();
    let hash_baseline = content_hash(
        &spec,
        manifest.cases(),
        manifest.families(),
        manifest.proof(),
        &baseline,
    );
    let hash_reshuffled = content_hash(
        &spec,
        manifest.cases(),
        manifest.families(),
        manifest.proof(),
        &reshuffled,
    );
    assert_eq!(
        hash_baseline,
        manifest.manifest_hash(),
        "content_hash over the live facts IS the published manifest hash"
    );
    assert_ne!(
        hash_baseline, hash_reshuffled,
        "the manifest hash must pin the full-space classification"
    );
}

// ---------------------------------------------------------------------------
// Refusal-partition structural exactness (classification tripwire)
// ---------------------------------------------------------------------------

/// The structural cause of the `<slot>`-region twin-equivalence tripwire,
/// stated independently of `classify`'s rule order: the subject element lives
/// in a `<slot>` fallback region.
fn legacy_slot_cause(levels: &RowLevels) -> bool {
    levels.region == ElementRegion::LegacySlot
}

/// The structural cause of the
/// [`DiagnosticKind::CssNestingSelectorInvalidPlacement`] partition: a
/// nesting selector placed outside any parent rule (the three parentless
/// structural shapes).
fn parentless_nesting_cause(levels: &RowLevels) -> bool {
    levels.selector_kind == SelectorKind::Nesting
        && matches!(
            levels.structural,
            StructuralKind::Plain | StructuralKind::Pruning | StructuralKind::Combinator
        )
}

/// Scan the full candidate space under `classify_levels` and return the first
/// structural-exactness violation (ascending ordinal order); `None` when the
/// oracle partition is EXACTLY the row set of its structural cause, NO row
/// lands in the (uninhabited) refusal partition, and every `<slot>`-region row
/// classifies IDENTICALLY to its static-element twin.
fn refusal_exactness_violation(
    spec: &CoverageSpec<FACTOR_COUNT>,
    classify_levels: impl Fn(&RowLevels) -> Disposition,
) -> Option<String> {
    let mut violation: Option<String> = None;
    for_each_row(spec, |levels| {
        if violation.is_some() {
            return;
        }
        let disposition = classify_levels(&levels);
        let nesting = parentless_nesting_cause(&levels);

        // Forward: the refusal partition is UNINHABITED (no row may be spirited
        // into it), and an oracle partition member carries EXACTLY its
        // structural cause.
        match disposition {
            Disposition::Refused(kind) => match kind {},
            Disposition::OracleRejected(kind) => {
                if kind != DiagnosticKind::CssNestingSelectorInvalidPlacement || !nesting {
                    violation = Some(format!(
                        "OracleRejected({kind:?}) row without the parentless-nesting structural \
                         cause: {levels:?}"
                    ));
                    return;
                }
            }
            Disposition::Supported | Disposition::Invalid(_) => {}
        }

        // Backward: a structural-cause row not intercepted by an earlier
        // carrier/coherence constraint (Invalid) lands in EXACTLY its
        // partition — never Supported, never another partition.
        if !matches!(disposition, Disposition::Invalid(_))
            && nesting
            && disposition
                != Disposition::OracleRejected(DiagnosticKind::CssNestingSelectorInvalidPlacement)
        {
            violation = Some(format!(
                "structural-cause row classified {disposition:?}, expected the \
                 parentless-nesting oracle partition: {levels:?}"
            ));
            return;
        }

        // Twin equivalence: a `<slot>`-region row classifies IDENTICALLY to its
        // static-element twin — the fallback region carries NO classification
        // weight of its own (no region-conditioned constraint may split the
        // partitions), unless another typed factor independently causes an
        // official reject / invalid constraint (which then binds BOTH twins).
        if legacy_slot_cause(&levels) {
            let twin = RowLevels {
                region: ElementRegion::StaticElement,
                ..levels
            };
            let twin_disposition = classify_levels(&twin);
            if disposition != twin_disposition {
                violation = Some(format!(
                    "slot-region row {disposition:?} diverges from its static-element twin \
                     {twin_disposition:?}: {levels:?}"
                ));
            }
        }
    });
    violation
}

/// CLASSIFICATION TRIPWIRE (structural exactness): the refusal partition is
/// UNINHABITED (`RefusalKind` has no variants — no covering cell may be
/// spirited into a refusal), the oracle partition equals EXACTLY the row set
/// of its single structural cause
/// (`OracleRejected(CssNestingSelectorInvalidPlacement)` ⇔ a parentless
/// nesting selector), and every `<slot>`-region row classifies IDENTICALLY to
/// its static-element twin. A reclassification in EITHER direction (absorbing
/// hard Supported cells into the oracle partition, silently claiming support
/// for an oracle-rejected cell, or splitting the slot twins) fails here — a
/// full manifest regeneration cannot mask it, because the causes are pinned
/// against the raw row levels, not against `classify`'s own output.
#[test]
fn refused_and_oracle_partitions_are_exactly_their_structural_cause() {
    let spec = coverage_spec();

    // The exactness map is written against the CURRENT closed vocabularies; a
    // new kind must extend the causes and this map together.
    assert_eq!(RefusalKind::ALL.len(), 0, "extend the exactness map");
    assert_eq!(DiagnosticKind::ALL.len(), 1, "extend the exactness map");

    assert_eq!(refusal_exactness_violation(&spec, classify), None);

    // Discrimination controls: a SINGLE-row reclassification in any
    // direction must break exactness.
    let mut oracle_row: Option<RowLevels> = None;
    let mut supported_row: Option<RowLevels> = None;
    let mut supported_slot_row: Option<RowLevels> = None;
    for_each_row(&spec, |levels| match classify(&levels) {
        Disposition::OracleRejected(_) if oracle_row.is_none() => oracle_row = Some(levels),
        Disposition::Supported => {
            if supported_row.is_none() {
                supported_row = Some(levels);
            }
            if supported_slot_row.is_none() && levels.region == ElementRegion::LegacySlot {
                supported_slot_row = Some(levels);
            }
        }
        _ => {}
    });
    let oracle_row = oracle_row.expect("an OracleRejected row exists");
    let supported_row = supported_row.expect("a Supported row exists");
    let supported_slot_row = supported_slot_row.expect("a Supported slot-region row exists");

    let reclassify = |from: RowLevels, to: Disposition| {
        move |levels: &RowLevels| {
            if *levels == from {
                to
            } else {
                classify(levels)
            }
        }
    };
    assert!(
        refusal_exactness_violation(
            &spec,
            reclassify(
                supported_row,
                Disposition::OracleRejected(DiagnosticKind::CssNestingSelectorInvalidPlacement),
            ),
        )
        .is_some(),
        "a Supported row absorbed into the oracle partition must break exactness"
    );
    assert!(
        refusal_exactness_violation(&spec, reclassify(oracle_row, Disposition::Supported))
            .is_some(),
        "an oracle-rejected cell reclassified Supported must break exactness"
    );
    // The slot-twin control: re-splitting a Supported slot-region row away
    // from its static-element twin must break the twin equivalence.
    assert!(
        refusal_exactness_violation(
            &spec,
            reclassify(
                supported_slot_row,
                Disposition::Invalid(ConstraintKind::MaybeNeedsUncertainSource),
            ),
        )
        .is_some(),
        "a slot-region row reclassified away from its twin must break exactness"
    );
}

#[test]
fn full_space_partition_counts_are_pinned_literals() {
    let manifest = manifest();
    assert_eq!(
        manifest.supported_row_count(),
        38_348,
        "Supported full-space rows (the former 6,458 slot-region refusal rows \
         reclassified Supported)"
    );
    assert_eq!(
        manifest.refused_inventory(),
        &[][..],
        "the refusal partition is uninhabited"
    );
    assert_eq!(
        manifest.oracle_rejected_inventory(),
        &[(DiagnosticKind::CssNestingSelectorInvalidPlacement, 4_320)][..],
        "OracleRejected full-space rows, per kind"
    );
    let invalid: u64 = manifest.invalid_inventory().iter().map(|&(_, n)| n).sum();
    assert_eq!(invalid, 229_492, "Invalid full-space rows");
    assert_eq!(
        38_348 + 4_320 + 229_492,
        full_product(),
        "the pinned literals tile the whole candidate space"
    );
}

/// `classify` on the manifest is the same authority as the free function and
/// maps out-of-range rows to Invalid.
#[test]
fn classify_row_is_the_sole_total_authority() {
    let manifest = manifest();
    let row = manifest.cases()[0].row;
    assert_eq!(manifest.classify(row), classify_row(row));

    let out_of_range = Row([u16::MAX; FACTOR_COUNT]);
    assert_eq!(classify_row(out_of_range), Partition::Invalid);
}

/// Families ride the manifest unchanged from the model data.
#[test]
fn families_are_exposed_on_the_manifest() {
    let manifest = manifest();
    assert_eq!(manifest.families(), &semantic_value_families()[..]);
    assert!(manifest.families().len() >= 4);
}

/// Concrete strengthened-interaction spot checks: the declared 5-wise and
/// 4-wise Supported tuples each surface in some selected case.
#[test]
fn strengthened_interactions_surface_in_selected_cases() {
    let manifest = manifest();

    // 5-wise {template, target, quoting, region, outcome}: the corpus-
    // canonical decimal-entity quoted class match on a static element.
    assert!(
        manifest.cases().iter().any(|case| {
            case.levels.template_value == TemplateValueRepresentation::HtmlDecimalEntity
                && case.levels.target == Target::Class
                && case.levels.quoting == Quoting::Quoted
                && case.levels.region == ElementRegion::StaticElement
                && case.levels.outcome == MatchOutcome::Match
        }),
        "the canonical 5-wise entity tuple is selected"
    );

    // 4-wise {selector kind, selector representation, structural, outcome}:
    // a nesting selector inside a parent rule that certainly does not match.
    assert!(
        manifest.cases().iter().any(|case| {
            case.levels.selector_kind == SelectorKind::Nesting
                && case.levels.selector_value == SelectorValueRepresentation::Literal
                && case.levels.structural == StructuralKind::Nested
                && case.levels.outcome == MatchOutcome::NoMatch
        }),
        "the nesting 4-wise tuple is selected"
    );
}
