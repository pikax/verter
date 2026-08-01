use verter_session::framework::script_facts::UnavailableScriptFacts;

fn main() {
    fn dereference(unavailable: &UnavailableScriptFacts) {
        let _facts = &**unavailable;
    }
    let _ = dereference;
}
