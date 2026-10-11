use verter_type_engine::project_semantic_dispatch::memo::ComponentMetaEvidence;

fn fake_evidence() -> ComponentMetaEvidence {
    ComponentMetaEvidence {
        facts: std::sync::Arc::from([]),
        generation: 0,
        external_fingerprint: 0,
    }
}

fn main() {}
