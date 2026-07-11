use super::*;

fn mk_spec<const N: usize>(
    cardinalities: [u16; N],
    global_strength: u8,
    interaction_groups: Vec<InteractionGroup>,
) -> CoverageSpec<N> {
    CoverageSpec {
        cardinalities,
        global_strength,
        interaction_groups,
    }
}

fn supported<const N: usize>(rows: &[[u16; N]]) -> Vec<ClassifiedRow<N>> {
    rows.iter()
        .map(|&levels| ClassifiedRow {
            row: Row(levels),
            partition: Partition::Supported,
        })
        .collect()
}

/// T1: a genuinely pairwise-complete array over four binary factors must be
/// rejected at `global_strength = 3`, naming the first missing 3-factor tuple.
#[test]
fn t01_pairwise_array_fails_strength_three_verify() {
    let rows = supported(&[
        [0, 0, 0, 0],
        [0, 1, 1, 1],
        [1, 0, 1, 1],
        [1, 1, 0, 1],
        [1, 1, 1, 0],
    ]);
    // Control: the array really is pairwise-complete, so it is the missing
    // strength, not a broken fixture, that trips the strength-3 check.
    let pairwise = mk_spec::<4>([2, 2, 2, 2], 2, vec![]);
    assert!(verify(&pairwise, &rows, |_| Partition::Supported).is_ok());

    let three_way = mk_spec::<4>([2, 2, 2, 2], 3, vec![]);
    let err = verify(&three_way, &rows, |_| Partition::Supported).unwrap_err();
    assert_eq!(
        err,
        CoverageError::Uncovered(UncoveredInteraction {
            obligation: Obligation::Global,
            factors: vec![0, 1, 2],
            levels: vec![0, 0, 1],
        })
    );
}

/// T2: dropping the sole cover of one strengthened-group tuple is reported as
/// exactly that `Group(i)` tuple — global and focus coverage survive the removal.
#[test]
fn t02_removing_sole_group_cover_names_exact_group_tuple() {
    let spec = mk_spec::<2>(
        [2, 2],
        1,
        vec![InteractionGroup {
            factors: vec![0, 1],
            strength: 2,
        }],
    );
    let classify = |_: Row<2>| Partition::Supported;
    let array = generate(&spec, classify).expect("generate");
    assert!(verify(&spec, &array.rows, classify).is_ok());

    let sole_cover = Row([1, 1]);
    assert!(
        array.rows.iter().any(|r| r.row == sole_cover),
        "the full array must carry the sole cover of group tuple (1,1)"
    );
    let pruned: Vec<ClassifiedRow<2>> = array
        .rows
        .iter()
        .copied()
        .filter(|r| r.row != sole_cover)
        .collect();
    assert_eq!(pruned.len(), array.rows.len() - 1);

    let err = verify(&spec, &pruned, classify).unwrap_err();
    assert_eq!(
        err,
        CoverageError::Uncovered(UncoveredInteraction {
            obligation: Obligation::Group(0),
            factors: vec![0, 1],
            levels: vec![1, 1],
        })
    );
}

/// T3: `Invalid` rows are never selected and can never credit an obligation,
/// even when the caller labels them `Supported`; tuples carried only by
/// `Invalid` candidates are not required at all.
#[test]
fn t03_invalid_rows_cover_nothing() {
    let spec = mk_spec::<2>([2, 2], 2, vec![]);
    let classify = |r: Row<2>| {
        if r.0 == [1, 1] {
            Partition::Invalid
        } else {
            Partition::Supported
        }
    };

    let array = generate(&spec, classify).expect("generate");
    assert_eq!(
        array.rows.iter().map(|r| r.row).collect::<Vec<_>>(),
        vec![Row([0, 0]), Row([0, 1]), Row([1, 0])]
    );
    assert!(array
        .rows
        .iter()
        .all(|r| r.partition == Partition::Supported));
    assert!(
        !array.rows.iter().any(|r| r.row == Row([1, 1])),
        "an Invalid candidate must never be selected"
    );

    // A caller cannot smuggle credit through an Invalid row by relabelling it.
    let cheating = vec![
        ClassifiedRow {
            row: Row([0, 0]),
            partition: Partition::Supported,
        },
        ClassifiedRow {
            row: Row([0, 1]),
            partition: Partition::Supported,
        },
        ClassifiedRow {
            row: Row([1, 1]),
            partition: Partition::Supported, // classify says Invalid
        },
    ];
    let err = verify(&spec, &cheating, classify).unwrap_err();
    assert_eq!(
        err,
        CoverageError::Uncovered(UncoveredInteraction {
            obligation: Obligation::Global,
            factors: vec![0, 1],
            levels: vec![1, 0],
        })
    );

    // (1,1) itself is not required: its only carrier is Invalid.
    let honest = supported(&[[0, 0], [0, 1], [1, 0]]);
    assert!(verify(&spec, &honest, classify).is_ok());
}

/// T4: a Refused row never credits a Supported obligation, and `generate`
/// dedicates at least one selected row to every refusal/oracle partition.
#[test]
fn t04_refused_row_never_credits_supported_obligation() {
    let spec = mk_spec::<2>([2, 2], 1, vec![]);
    let classify = |r: Row<2>| match r.0 {
        [1, 1] => Partition::Refused(RefusalKind(7)),
        [0, 1] => Partition::OracleRejected(DiagnosticKind(3)),
        _ => Partition::Supported,
    };

    // f0=1 is Supported-satisfiable (via (1,0)); carrying it only on a Refused
    // row must leave it uncovered.
    let refused_only = vec![
        ClassifiedRow {
            row: Row([0, 0]),
            partition: Partition::Supported,
        },
        ClassifiedRow {
            row: Row([1, 1]),
            partition: Partition::Refused(RefusalKind(7)),
        },
    ];
    let err = verify(&spec, &refused_only, classify).unwrap_err();
    assert_eq!(
        err,
        CoverageError::Uncovered(UncoveredInteraction {
            obligation: Obligation::Global,
            factors: vec![0],
            levels: vec![1],
        })
    );

    // With the Supported carrier restored, the remaining hole is the missing
    // OracleRejected partition (levels encode the partition: [1, diagnostic id]).
    let missing_oracle = vec![
        ClassifiedRow {
            row: Row([0, 0]),
            partition: Partition::Supported,
        },
        ClassifiedRow {
            row: Row([1, 0]),
            partition: Partition::Supported,
        },
        ClassifiedRow {
            row: Row([1, 1]),
            partition: Partition::Refused(RefusalKind(7)),
        },
    ];
    let err = verify(&spec, &missing_oracle, classify).unwrap_err();
    assert_eq!(
        err,
        CoverageError::Uncovered(UncoveredInteraction {
            obligation: Obligation::RefusalPartition,
            factors: vec![],
            levels: vec![1, 3],
        })
    );

    let array = generate(&spec, classify).expect("generate");
    assert!(array.rows.contains(&ClassifiedRow {
        row: Row([1, 1]),
        partition: Partition::Refused(RefusalKind(7)),
    }));
    assert!(array.rows.contains(&ClassifiedRow {
        row: Row([0, 1]),
        partition: Partition::OracleRejected(DiagnosticKind(3)),
    }));
    assert_eq!(array.rows.len(), 4);
}

/// T5: `generate` is a pure function of (spec, classify): byte-identical rows
/// and proof rendering across runs, rows strictly ascending by ordinal tuple.
#[test]
fn t05_generate_is_deterministic_and_rows_ascend() {
    let spec = mk_spec::<4>(
        [3, 2, 3, 2],
        2,
        vec![InteractionGroup {
            factors: vec![0, 1, 2],
            strength: 3,
        }],
    );
    let classify = |r: Row<4>| {
        if r.0[0] + r.0[3] == 2 {
            Partition::Invalid
        } else if r.0[2] == 2 && r.0[1] == 1 {
            Partition::Refused(RefusalKind(5))
        } else {
            Partition::Supported
        }
    };

    let a = generate(&spec, classify).expect("generate a");
    let b = generate(&spec, classify).expect("generate b");
    assert!(!a.rows.is_empty());
    assert!(!a.proof.render().is_empty());
    assert_eq!(a.rows, b.rows);
    assert_eq!(a.proof, b.proof);
    assert_eq!(a.proof.render(), b.proof.render());
    assert!(
        a.rows.windows(2).all(|w| w[0].row < w[1].row),
        "rows must be strictly ascending (and therefore duplicate-free)"
    );
}

/// T6: a genuine greedy tie resolves to the smallest ordinal row. All four
/// Supported candidates open with identical gain (6); picking the largest
/// instead would end at rows (0,1,1),(1,0,1),(1,1,0).
#[test]
fn t06_greedy_tie_selects_smallest_ordinal_row() {
    let spec = mk_spec::<3>([2, 2, 2], 1, vec![]);
    let classify = |r: Row<3>| {
        if (r.0[0] + r.0[1] + r.0[2]).is_multiple_of(2) {
            Partition::Supported
        } else {
            Partition::Invalid
        }
    };
    let array = generate(&spec, classify).expect("generate");
    let rows: Vec<Row<3>> = array.rows.iter().map(|r| r.row).collect();
    assert_eq!(rows, vec![Row([0, 0, 0]), Row([0, 1, 1]), Row([1, 0, 1])]);
    assert!(
        !rows.contains(&Row([1, 1, 0])),
        "the largest-ordinal tie candidate must lose the first pick"
    );
}

/// T7: the proof rendering is byte-pinned and repeatable.
#[test]
fn t07_proof_render_is_byte_pinned() {
    let spec = mk_spec::<2>(
        [2, 2],
        2,
        vec![InteractionGroup {
            factors: vec![0, 1],
            strength: 1,
        }],
    );
    let classify = |r: Row<2>| {
        if r.0 == [1, 1] {
            Partition::Refused(RefusalKind(2))
        } else {
            Partition::Supported
        }
    };
    let expected = "covering-array proof\n\
                    candidates: 4\n\
                    selected-rows: 4\n\
                    global: required=3 covered=3\n\
                    group[0]: required=4 covered=4\n\
                    focus-cells: 4\n\
                    refusal-partitions: 1\n";

    let array = generate(&spec, classify).expect("generate");
    assert_eq!(array.proof.render(), expected);
    let again = generate(&spec, classify).expect("generate again");
    assert_eq!(again.proof.render(), expected);

    // A different spec must not render to the same bytes.
    let other = generate(&mk_spec::<2>([2, 2], 1, vec![]), |_: Row<2>| {
        Partition::Supported
    })
    .expect("other");
    assert_ne!(other.proof.render(), expected);
}

/// T8: an uncovered global tuple is named with its exact factors and levels.
#[test]
fn t08_uncovered_names_exact_factors_and_levels() {
    let spec = mk_spec::<3>([2, 2, 2], 2, vec![]);
    // Full product minus both carriers of (f1,f2) = (1,0).
    let rows = supported(&[
        [0, 0, 0],
        [0, 0, 1],
        [0, 1, 1],
        [1, 0, 0],
        [1, 0, 1],
        [1, 1, 1],
    ]);
    let err = verify(&spec, &rows, |_| Partition::Supported).unwrap_err();
    assert_eq!(
        err,
        CoverageError::Uncovered(UncoveredInteraction {
            obligation: Obligation::Global,
            factors: vec![1, 2],
            levels: vec![1, 0],
        })
    );

    // Restoring one carrier closes the hole.
    let mut fixed = rows.clone();
    fixed.push(ClassifiedRow {
        row: Row([0, 1, 0]),
        partition: Partition::Supported,
    });
    assert!(verify(&spec, &fixed, |_| Partition::Supported).is_ok());
}

/// T9: a candidate space above the ceiling errors out before any
/// classification runs; below the ceiling generation proceeds.
#[test]
fn t09_candidate_ceiling_exceeded_errors_without_classify() {
    let spec = mk_spec::<5>([15, 15, 15, 15, 15], 1, vec![]);
    let Err(err) = generate(&spec, |_: Row<5>| -> Partition {
        panic!("classify must not run when the ceiling is exceeded")
    }) else {
        panic!("expected CandidateCeilingExceeded, got a covering array");
    };
    assert_eq!(
        err,
        CoveringError::CandidateCeilingExceeded {
            candidates: 759_375,
            ceiling: 500_000,
        }
    );

    let below = mk_spec::<4>([15, 15, 15, 15], 1, vec![]);
    assert!(generate(&below, |_| Partition::Supported).is_ok());
}

/// T9b: a candidate space BELOW the candidate ceiling whose OBLIGATION
/// universe exceeds the obligation ceiling errors out typed before any
/// classification runs — generation fails loudly instead of exploding the
/// slot allocation (high-strength/high-cardinality specs blow up the
/// interaction universe long before the row count does).
#[test]
fn t09b_obligation_ceiling_exceeded_errors_without_classify() {
    // 2^18 = 262,144 candidates (below CANDIDATE_CEILING) but strength-9
    // global coverage demands C(18,9)·2^9 + 18·2 = 24,893,476 slots.
    let spec = mk_spec::<18>([2; 18], 9, vec![]);
    let Err(err) = generate(&spec, |_: Row<18>| -> Partition {
        panic!("classify must not run when the obligation ceiling is exceeded")
    }) else {
        panic!("expected ObligationCeilingExceeded, got a covering array");
    };
    assert_eq!(
        err,
        CoveringError::ObligationCeilingExceeded {
            obligations: 24_893_476,
            ceiling: OBLIGATION_CEILING,
        }
    );

    // `verify` enforces the same bound (typed as an invalid spec, mirroring
    // its candidate-ceiling treatment), without classifying.
    let verify_err = verify(&spec, &[], |_: Row<18>| -> Partition {
        panic!("classify must not run when the obligation ceiling is exceeded")
    })
    .unwrap_err();
    assert!(
        matches!(verify_err, CoverageError::InvalidSpec(ref msg) if msg.contains("OBLIGATION_CEILING")),
        "verify must reject an over-ceiling obligation universe: {verify_err:?}"
    );

    // The bound is the SLOT universe, not the factor count: the same binary
    // factor shape at global strength 3 stays far below the ceiling.
    let below = mk_spec::<12>([2; 12], 3, vec![]);
    assert!(generate(&below, |_| Partition::Supported).is_ok());
}

/// T9c: the obligation-universe accounting saturates instead of overflowing —
/// a spec whose subset count alone is astronomically large (C(200, 100)
/// overflows u64 by hundreds of digits) still returns the typed error rather
/// than panicking, wrapping, or attempting enumeration.
#[test]
fn t09c_obligation_accounting_saturates_on_astronomical_specs() {
    // 200 binary factors would exceed the candidate ceiling too, so pin the
    // obligation path specifically: cardinality 1 keeps the candidate count at
    // exactly 1 while C(200, 100) subsets (each of slot size 1) explode.
    let spec = mk_spec::<200>([1; 200], 100, vec![]);
    let Err(err) = generate(&spec, |_: Row<200>| -> Partition {
        panic!("classify must not run when the obligation ceiling is exceeded")
    }) else {
        panic!("expected ObligationCeilingExceeded, got a covering array");
    };
    match err {
        CoveringError::ObligationCeilingExceeded {
            obligations,
            ceiling,
        } => {
            assert!(
                obligations > ceiling,
                "the reported obligation count must exceed the ceiling"
            );
            assert_eq!(ceiling, OBLIGATION_CEILING);
        }
        other => panic!("expected ObligationCeilingExceeded, got {other:?}"),
    }
}

/// T10: a CSS-manifest-shaped spec (9 mixed factors, strength-5 and strength-4
/// groups, refusal/oracle/invalid partitions) generates, self-verifies, and
/// compresses far below the full product.
#[test]
fn t10_css_shaped_spec_compresses_and_self_verifies() {
    let spec = CoverageSpec::<9> {
        cardinalities: [3, 2, 4, 2, 3, 2, 5, 2, 2],
        global_strength: 2,
        interaction_groups: vec![
            InteractionGroup {
                factors: vec![0, 1, 2, 3, 4],
                strength: 5,
            },
            InteractionGroup {
                factors: vec![5, 6, 7, 8],
                strength: 4,
            },
        ],
    };
    let classify = |r: Row<9>| {
        let v = &r.0;
        if (v[0] + v[2] + v[4]) % 7 == 6 {
            Partition::Invalid
        } else if v[6] == 4 && v[8] == 1 {
            Partition::Refused(RefusalKind(1))
        } else if v[0] == 2 && v[5] == 1 {
            Partition::Refused(RefusalKind(2))
        } else if v[2] == 3 && v[3] == 1 {
            Partition::OracleRejected(DiagnosticKind(9))
        } else {
            Partition::Supported
        }
    };

    let product: usize = spec.cardinalities.iter().map(|&c| usize::from(c)).product();
    assert_eq!(product, 5760);

    let array = generate(&spec, classify).expect("generate");
    assert!(!array.rows.is_empty());
    assert!(
        array.rows.len() * 10 < product,
        "selected {} of {} candidates — no real compression",
        array.rows.len(),
        product
    );

    let proof = verify(&spec, &array.rows, classify).expect("independent verify");
    assert_eq!(proof, array.proof);

    for partition in [
        Partition::Refused(RefusalKind(1)),
        Partition::Refused(RefusalKind(2)),
        Partition::OracleRejected(DiagnosticKind(9)),
    ] {
        assert!(
            array.rows.iter().any(|r| r.partition == partition),
            "missing selected row for partition {partition:?}"
        );
    }
    assert!(array
        .rows
        .iter()
        .all(|r| classify(r.row) != Partition::Invalid));
    assert!(array.rows.windows(2).all(|w| w[0].row < w[1].row));
    println!(
        "t10 compression: {} selected rows / {} candidates",
        array.rows.len(),
        product
    );
}

/// T11: malformed specs are rejected by both entry points before any
/// classification runs.
#[test]
fn t11_invalid_specs_rejected_by_generate_and_verify() {
    fn boom(_: Row<2>) -> Partition {
        panic!("classify must not run for an invalid spec")
    }
    let group = |factors: Vec<u8>, strength: u8| InteractionGroup { factors, strength };
    let cases: Vec<(CoverageSpec<2>, &str)> = vec![
        (mk_spec::<2>([0, 2], 1, vec![]), "zero cardinality"),
        (mk_spec::<2>([2, 2], 0, vec![]), "global_strength zero"),
        (mk_spec::<2>([2, 2], 3, vec![]), "global_strength above N"),
        (
            mk_spec::<2>([2, 2], 1, vec![group(vec![1, 0], 1)]),
            "non-ascending group factors",
        ),
        (
            mk_spec::<2>([2, 2], 1, vec![group(vec![1, 1], 1)]),
            "duplicate group factors",
        ),
        (
            mk_spec::<2>([2, 2], 1, vec![group(vec![0, 2], 1)]),
            "group factor out of range",
        ),
        (
            mk_spec::<2>([2, 2], 1, vec![group(vec![0, 1], 0)]),
            "group strength zero",
        ),
        (
            mk_spec::<2>([2, 2], 1, vec![group(vec![0, 1], 3)]),
            "group strength above factor count",
        ),
        (
            mk_spec::<2>([2, 2], 1, vec![group(vec![], 1)]),
            "empty group",
        ),
    ];
    for (spec, what) in &cases {
        assert!(
            matches!(generate(spec, boom), Err(CoveringError::InvalidSpec(_))),
            "generate must reject: {what}"
        );
        assert!(
            matches!(verify(spec, &[], boom), Err(CoverageError::InvalidSpec(_))),
            "verify must reject: {what}"
        );
    }

    // Control: the well-formed variant is accepted.
    let good = mk_spec::<2>([2, 2], 1, vec![group(vec![0, 1], 2)]);
    assert!(generate(&good, |_| Partition::Supported).is_ok());
}

/// T12: `verify` on `generate`'s output reproduces the embedded proof exactly,
/// and rows outside the candidate universe credit nothing.
#[test]
fn t12_verify_matches_generate_and_out_of_range_rows_credit_nothing() {
    let spec = mk_spec::<2>([2, 2], 1, vec![]);
    let classify = |r: Row<2>| match r.0 {
        [1, 1] => Partition::Refused(RefusalKind(7)),
        [0, 1] => Partition::OracleRejected(DiagnosticKind(3)),
        _ => Partition::Supported,
    };
    let array = generate(&spec, classify).expect("generate");
    assert_eq!(array.rows.len(), 4);
    let proof = verify(&spec, &array.rows, classify).expect("verify");
    assert_eq!(proof, array.proof);
    assert_eq!(proof.render(), array.proof.render());

    // Replacing the (1,0) carrier with an out-of-range row leaves f0=1 uncovered:
    // a row outside the universe cannot credit anything.
    let mut swapped: Vec<ClassifiedRow<2>> = array
        .rows
        .iter()
        .copied()
        .filter(|r| r.row != Row([1, 0]))
        .collect();
    swapped.push(ClassifiedRow {
        row: Row([9, 0]),
        partition: Partition::Supported,
    });
    let err = verify(&spec, &swapped, classify).unwrap_err();
    assert_eq!(
        err,
        CoverageError::Uncovered(UncoveredInteraction {
            obligation: Obligation::Global,
            factors: vec![0],
            levels: vec![1],
        })
    );

    // ...but an out-of-range row alongside a complete array is inert, not fatal.
    let mut padded = array.rows.clone();
    padded.push(ClassifiedRow {
        row: Row([9, 0]),
        partition: Partition::Supported,
    });
    assert!(verify(&spec, &padded, classify).is_ok());
}
