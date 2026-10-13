//! Provider-free flow corpus coverage and full-size construction growth.

use verter_compiler::flow_check::growth;
use verter_compiler::flow_check::ide_fixtures::{self, SfcFixture};

/// Chain depth 40 and the JavaScript setup are the expensive provider checks.
/// A chain sharing its round trip with smaller fixtures used the whole debug
/// budget, so each chain and each hover fixture is its own test. Smaller
/// fixtures share a test in threes. Together the partitions are the whole suite.
fn is_heavy_semantic(name: &str) -> bool {
    name.contains("40") || name.contains("JavaScript")
}

fn is_contextual(name: &str) -> bool {
    name.contains("Contextual")
}

pub(crate) fn semantic_partitions() -> Vec<Vec<SfcFixture>> {
    let mut heavy = Vec::new();
    let mut contextual = Vec::new();
    let mut light = Vec::new();
    for fixture in ide_fixtures::semantic_suite() {
        if is_heavy_semantic(&fixture.name) {
            heavy.push(fixture);
        } else if is_contextual(&fixture.name) {
            contextual.push(fixture);
        } else {
            light.push(fixture);
        }
    }
    let mut partitions: Vec<Vec<SfcFixture>> =
        heavy.into_iter().map(|fixture| vec![fixture]).collect();
    for fixture in contextual {
        partitions.push(vec![fixture]);
    }
    let mut bucket = Vec::new();
    for fixture in light {
        bucket.push(fixture);
        if bucket.len() == 3 {
            partitions.push(std::mem::take(&mut bucket));
        }
    }
    if !bucket.is_empty() {
        partitions.push(bucket);
    }
    partitions
}

#[test]
fn semantic_partitions_cover_the_exact_suite() {
    let partitions = semantic_partitions();
    assert_eq!(
        partitions.len(),
        SEMANTIC_PARTITION_COUNT,
        "one provider test per partition; add a test when the suite grows"
    );
    for partition in &partitions {
        let heavy = partition
            .iter()
            .filter(|fixture| is_heavy_semantic(&fixture.name))
            .count();
        let contextual = partition
            .iter()
            .filter(|fixture| is_contextual(&fixture.name))
            .count();
        assert!(heavy <= 1, "a partition carries at most one chain");
        assert!(partition.len() <= 3, "a partition stays a short round trip");
        assert!(
            contextual == 0 || partition.len() == 1,
            "a hover fixture is its own round trip"
        );
        if heavy == 1 {
            assert_eq!(partition.len(), 1, "a chain is its own round trip");
        }
    }
    let mut covered: Vec<_> = partitions
        .iter()
        .flat_map(|partition| partition.iter().map(|fixture| fixture.name.clone()))
        .collect();
    let mut expected: Vec<_> = ide_fixtures::semantic_suite()
        .into_iter()
        .map(|fixture| fixture.name)
        .collect();
    covered.sort();
    expected.sort();
    assert_eq!(covered, expected);
    assert!(covered.iter().any(|name| is_heavy_semantic(name)));
}

/// One provider test per `semantic_partitions()` index. The cover test pins
/// this length; add an entry when that assertion asks for one.
const SEMANTIC_PARTITION_COUNT: usize = 27;

/// Full-size linearity is an operation count beside the live sweep of the
/// same sizes. Path-replay rejection stays on the compiler oracle.
#[test]
fn reference_matrix_growth_is_linear() {
    growth::assert_reference_matrix_growth();
}

/// Full-size linearity is an operation count on the production emitter,
/// beside the live sweep of the same sizes.
#[test]
fn production_matrix_growth_is_linear() {
    growth::assert_production_matrix_growth();
}
