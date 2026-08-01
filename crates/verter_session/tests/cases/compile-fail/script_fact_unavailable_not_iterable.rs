use verter_session::framework::script_facts::UnavailableScriptFacts;

fn main() {
    fn iterate(unavailable: UnavailableScriptFacts) {
        for _fact in unavailable {}
    }
    let _ = iterate;
}
