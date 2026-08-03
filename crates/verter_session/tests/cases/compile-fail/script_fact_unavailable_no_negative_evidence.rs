use verter_semantic::analysis::framework_facts::{
    svelte::SveltePropsCall, NegativeEvidence,
};
use verter_session::framework::script_facts::UnavailableScriptFacts;

fn requires_authoritative_absence<E>(_evidence: &E)
where
    E: NegativeEvidence<Observation = SveltePropsCall>,
{
}

fn main() {
    fn consume(unavailable: &UnavailableScriptFacts) {
        requires_authoritative_absence(unavailable);
    }
    let _ = consume;
}
