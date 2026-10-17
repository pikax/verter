//! Overload applicability over a large overload group: the call answers
//! the checker's overload, and its declaration-lowering work grows
//! linearly with the group — each overload's signature is re-borrowed from
//! the one statement that declares it, never by re-lowering the group.

use super::differential_harness_tests::{Matrix, Read, STRICT};

/// `size` string-literal overloads, then a number overload, called with a
/// number.
fn overload_group(size: usize) -> String {
    let mut source = String::new();
    for i in 0..size {
        source.push_str(&format!("declare function f(x: \"s{i}\"): \"s\";\n"));
    }
    source.push_str("declare function f(x: number): \"n\";\nconst r = f(1);\n");
    source
}

#[test]
fn overload_applicability_lowers_each_overload_statement_a_bounded_number_of_times() {
    const SIZES: [usize; 3] = [128, 256, 512];
    let mut lowered = Vec::new();
    for size in SIZES {
        let source = overload_group(size);
        let matrix = Matrix::new(&source).settings(&[STRICT]);
        let rows = [(Read::Type("typeof r"), vec![r#""n""#])];
        let host = matrix.host(&rows);
        let before = host.provenance().snapshot().decl_statements_lowered;
        let verdict = &matrix.verdicts_on(&host, &rows)[0][0];
        assert!(
            verdict.matched,
            "size={size}: the number overload answers, got {}",
            verdict.lane
        );
        lowered.push(host.provenance().snapshot().decl_statements_lowered - before);
    }
    for (pair, sizes) in lowered.windows(2).zip(SIZES.windows(2)) {
        assert!(
            pair[0] >= sizes[0] as u64,
            "every overload statement lowers: {lowered:?} at sizes {SIZES:?}"
        );
        assert!(
            pair[1] <= 2 * pair[0] + 16,
            "declaration lowering must double at most when the overloads double: \
             {lowered:?} at sizes {SIZES:?}"
        );
    }
}
