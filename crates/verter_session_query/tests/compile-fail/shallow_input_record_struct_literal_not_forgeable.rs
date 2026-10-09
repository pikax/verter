// A published shallow observation is built only by the finalizer, which
// assigns its observation id. A literal choosing the id for some facts
// does not compile.
use verter_session_query::inputs::shallow::{ShallowInputAssembly, ShallowInputRecord};

fn forge(facts: ShallowInputAssembly) -> ShallowInputRecord {
    ShallowInputRecord {
        observation_id: 7,
        facts,
    }
}

fn main() {}
