use verter_semantic::analysis::framework_facts::{
    svelte::{SveltePropsCall, SvelteScriptFacts},
    NegativeEvidence,
};
use verter_session::framework::script_facts::PartialScriptFacts;

fn requires_exact_props_calls<E>(evidence: &E)
where
    E: NegativeEvidence<Observation = SveltePropsCall>,
{
    let _ = evidence.observations();
}

fn inspect_partial(partial: &PartialScriptFacts<SvelteScriptFacts>) {
    let observations = partial.conservative_svelte_observations();
    let props_calls = observations.props_calls();
    requires_exact_props_calls(&props_calls);
}

fn main() {
    let _ = inspect_partial;
}
