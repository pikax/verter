// A published observation keeps the id it was finalized under.
use verter_session_query::inputs::shallow::ShallowInputRecord;

fn reassign(record: &mut ShallowInputRecord) {
    record.observation_id = 7;
}

fn main() {}
