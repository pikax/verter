use verter_semantic::analysis::framework_facts::svelte::SvelteScriptFacts;
use verter_session::framework::script_facts::PartialScriptFacts;

fn consume_as_complete(facts: &SvelteScriptFacts) {
    let _ = facts.syntax().props_calls();
}

fn main() {
    fn escape_partial(partial: &PartialScriptFacts<SvelteScriptFacts>) {
        consume_as_complete(partial.observed());
    }

    let _ = escape_partial;
}
