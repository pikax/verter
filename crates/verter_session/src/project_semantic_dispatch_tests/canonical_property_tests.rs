//! Canonical keys across declaration contributors.

use super::differential_harness_tests::{Matrix, Read, STRICT};

#[test]
fn merged_interface_dual_spellings_publish_one_checker_key() {
    for (source, key) in [
        (
            r#"interface A { 1: string } interface A { "1": string }"#,
            "1",
        ),
        (
            r#"interface A { "1": string } interface A { 1: string }"#,
            r#""1""#,
        ),
    ] {
        let rows = [
            (Read::Type("keyof A"), vec![key]),
            (Read::Type("A[1]"), vec!["string"]),
        ];
        let matrix = Matrix::new(source).settings(&[STRICT]);
        for verdict in matrix.verdicts(&rows).iter().flatten() {
            assert!(verdict.matched, "{source}: {}", verdict.lane);
        }
    }
}
