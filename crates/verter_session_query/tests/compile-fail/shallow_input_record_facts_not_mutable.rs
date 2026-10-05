// A published observation's facts are read-only: it derefs to them for
// reading only, so changed facts must be finalized as a new observation.
use verter_session_query::inputs::shallow::ShallowInputRecord;

fn mutate(record: &mut ShallowInputRecord) {
    record.exports.clear();
}

fn main() {}
