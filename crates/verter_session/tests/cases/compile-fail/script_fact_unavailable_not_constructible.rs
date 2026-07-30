use verter_session::framework::script_facts::{
    ScriptFactUnavailableReason, UnavailableScriptFacts,
};

fn main() {
    let _ = UnavailableScriptFacts::new(
        ScriptFactUnavailableReason::ValidationProducedNoFacts,
    );
}
