use verter_session_query::facts::fact_set::FactSet;
use verter_type_engine::project_semantic_dispatch::memo::ComponentMetaEvidence;

fn some_facts() -> FactSet {
    unimplemented!()
}

fn fake_evidence() -> ComponentMetaEvidence {
    ComponentMetaEvidence {
        facts: some_facts(),
        generation: 0,
        external_fingerprint: 0,
    }
}

fn main() {}
