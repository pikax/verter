use verter_semantic::analysis::framework_facts::{
    svelte::{SveltePropsCall, SvelteScriptFacts},
    NegativeEvidence,
};
use verter_session::framework::script_facts::PartialScriptFacts;

fn consume_authoritative_syntax<E>(evidence: &E)
where
    E: NegativeEvidence<Observation = SveltePropsCall>,
{
    let _ = evidence.observations();
}

fn main() {
    fn inspect_partial(partial: &PartialScriptFacts<SvelteScriptFacts>) {
        if let Some(syntax) = partial.exact_syntax() {
            consume_authoritative_syntax(syntax.props_calls());
        }
    }

    let _ = inspect_partial;
}
