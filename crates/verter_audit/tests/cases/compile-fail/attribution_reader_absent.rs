// Negative control for the no-semantic-authority claim.
//
// The attribution counter table and every accessor that can produce a
// number live behind the non-default `attribution` feature. This fixture
// is compiled WITHOUT that feature (the configuration every production
// build and the canonical gate use) and reaches for the reader path.
//
// It MUST NOT compile. If it ever does, a production build can resolve a
// path from a counter value back into a branch — which is exactly the
// property the substrate exists to make impossible.

fn main() {
    // The whole reader surface: snapshot / snapshot_all / read / reset.
    let rows = verter_audit::attribution::snapshot();
    let all = verter_audit::attribution::snapshot_all();
    let one = verter_audit::attribution::read(verter_audit::attribution::WorkSite::ContentHash);
    verter_audit::attribution::reset();

    // The shape a semantic authority would take: branching on a counter.
    if one.calls > 0 {
        println!("{} {}", rows.len(), all.len());
    }
}
