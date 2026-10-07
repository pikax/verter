use super::*;

#[test]
fn origin_edge_dep_signatures_are_not_an_invalidation_source() {
    let violations = origin_fence_reconstruction_violations();
    assert!(
        violations.is_empty(),
        "Architecture guard `origin_edge_dep_signatures_are_not_an_invalidation_source`\n\
             violations: production source reconstructs a CompletionFence from DerivationStore\n\
             origin edges.\n\n\
             INVARIANT:\n  \
             Origin-edge dep signatures are not an invalidation source.\n  \
             No production code may reconstruct a CompletionFence from DerivationStore\n  \
             origin edges.\n\n\
             Origin edges are bounded best-effort provenance for the audit origin-graph\n\
             trace; the FIFO `edge_budget` evicts the oldest buckets, so an `edge_dep_signature`\n\
             is NOT load-bearing for invalidation. The load-bearing record is the memo entry's\n\
             own `ReadSetSignature` carrier, validated strictly on every warm read via\n\
             `ReadSetSignature::validate_with_self_roots`.\n\
             Do not reintroduce `origins_with_fence` or fold an `edge_dep_signature` into a\n\
             `CompletionFence`.\n\n\
             Violations:\n  {}",
        violations
            .iter()
            .map(|(rel, lineno, line)| format!("{rel}:{lineno}: {}", line.trim()))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}
