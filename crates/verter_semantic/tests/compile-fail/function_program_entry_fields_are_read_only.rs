//! A served function program entry is read-only: code outside its owning
//! crate cannot rewrite a field of an entry an index handed out.

use verter_session_query::function_program::FunctionProgramEntry;

fn rewrite(mut entry: FunctionProgramEntry) -> FunctionProgramEntry {
    entry.flow_body_exact_hash = None;
    entry
}

fn main() {
    let _ = rewrite;
}
