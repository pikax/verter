use verter_session::semantic_query::{
    ObjectProjectionAlternative, PositiveAlternativeEvidence,
};

fn positive_evidence_cannot_claim_exact_domain(evidence: PositiveAlternativeEvidence<'_>) {
    let _ = evidence.exact_keyof();
}

fn ordinary_alternative_cannot_claim_absence(alternative: &ObjectProjectionAlternative) {
    let _ = alternative.lookup(
        &verter_session::semantic_query::PropertyKey::identifier("missing"),
    );
}

fn main() {}
