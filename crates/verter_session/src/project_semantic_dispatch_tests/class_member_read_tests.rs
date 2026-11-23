//! A class member read is an owned read: its first stage names the reading
//! class without reading it, each class it names is delivered by its holder,
//! and the read keeps the inheritance path it followed between deliveries.
//! Stepped by hand it answers, names and observes exactly what the in-place
//! driver does.

use super::carrier_head_resolution_tests::{host, upsert_ts};
use super::ProjectSemanticDispatch;
use verter_session_query::facts::fact_cache::AggregateBasisSeed;
use verter_session_query::facts::fact_read_set::{FactReadSet, FactReadSetFinalise};
use verter_session_query::facts::fact_set::FactSet;
use verter_type_engine::project_semantic_dispatch::build::ClassMemberTraceForTests;

const FILE: &str = "/classes.ts";
const SOURCE: &str = "\
export class Own { a = 'own' as const; b = 1; }
export class B { a: string = 's'; other = 1; }
export class M extends B { }
export class D extends M { }
export class G<T> { v!: T; w = 0; }
export class GD extends G<string> { }
export class C1 extends C2 { }
export class C2 extends C1 { }
function mix<T>(base: T) { return base; }
export class V extends mix(B) { }
export class Lone { other = 1; }
";

/// The facts a traced closure observed, and whether they may be cached.
fn observed(reads: FactReadSet) -> (bool, FactSet) {
    match reads.finalise() {
        FactReadSetFinalise::Ok(facts) => (true, facts),
        FactReadSetFinalise::NonCacheable(facts) => (false, facts),
        FactReadSetFinalise::MutationUnstable => {
            panic!("the fixed fixture cannot have a torn observation")
        }
    }
}

/// A member read of `name` on the fixture class `class` through `dispatch`,
/// stepped or in place, and what it observed.
fn traced(
    host: &crate::VerterHost,
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    class: &str,
    name: &str,
    stepped: bool,
) -> (ClassMemberTraceForTests, (bool, FactSet)) {
    let (trace, reads) = host.with_fact_tracer(AggregateBasisSeed::Unvouched, || {
        dispatch.trace_class_member_read_for_tests(FILE, class, &[], name, stepped)
    });
    (trace, observed(reads))
}

/// The member read stepped by hand answers, names and observes exactly what
/// `class_member_source`'s in-place driver does, over the same classes: an
/// own member in one delivery; an inherited member followed up a chain to
/// the class declaring it; a generic base applied to its clause's argument;
/// a circular chain, which stops at the class already followed; a value-only
/// base, which names no class; and a class with neither the member nor an
/// `extends` clause, which answers nothing.
#[test]
fn stepped_class_member_reads_preserve_in_place_answers_order_and_observations() {
    for (class, name, path, answers) in [
        ("Own", "a", &["Own"][..], Some(true)),
        ("D", "a", &["D", "M", "B"][..], Some(true)),
        ("D", "other", &["D", "M", "B"][..], Some(true)),
        ("GD", "v", &["GD", "G"][..], Some(true)),
        ("C1", "a", &["C1", "C2"][..], Some(true)),
        ("V", "a", &["V"][..], None),
        ("Lone", "a", &["Lone"][..], Some(false)),
    ] {
        let host = host();
        upsert_ts(&host, FILE, SOURCE);
        host.ensure_indexed_ready(FILE)
            .expect("the fixture declarations are indexed");
        let fixture = ProjectSemanticDispatch::new(&host);
        let _construction = fixture.enter_construction_scope();
        fixture.graph().invalidate_all();
        let stepped_dispatch = ProjectSemanticDispatch::new(&host);
        let stepped = traced(&host, &stepped_dispatch, class, name, true);
        // The same fixture with an independently cold memo.
        fixture.graph().invalidate_all();
        let in_place_dispatch = ProjectSemanticDispatch::new(&host);
        let in_place = traced(&host, &in_place_dispatch, class, name, false);
        assert_eq!(stepped, in_place, "{class}.{name}");

        let (trace, _) = &stepped;
        let named: Vec<&str> = trace
            .requested
            .iter()
            .map(|(file, named)| {
                assert_eq!(file, FILE, "{class}.{name}: every class is the fixture's");
                named.as_str()
            })
            .collect();
        assert_eq!(named, path, "{class}.{name}: the classes the read named");
        if let Some(answers) = answers {
            assert_eq!(
                trace.source.is_some(),
                answers,
                "{class}.{name}: where the member reads from"
            );
        }
    }
}

/// The read's first stage only names the class it reads: it lowers, interns
/// and observes nothing of the class until its holder delivers that class,
/// while the delivered read of the same member does all three.
#[test]
fn a_class_member_read_is_requested_not_settled_in_place() {
    let host = host();
    upsert_ts(&host, FILE, SOURCE);
    host.ensure_indexed_ready(FILE)
        .expect("the fixture declarations are indexed");
    let dispatch = ProjectSemanticDispatch::new(&host);

    let nodes = dispatch.graph().node_slot_count();
    let (named, reads) = host.with_fact_tracer(AggregateBasisSeed::Unvouched, || {
        dispatch.begin_class_member_read_for_tests(FILE, "D", "a")
    });
    assert_eq!(
        named,
        Some((FILE.to_owned(), "D".to_owned())),
        "the read names the reading class first"
    );
    assert_eq!(
        dispatch.graph().node_slot_count(),
        nodes,
        "naming the class interns nothing"
    );
    assert!(
        observed(reads).1.is_empty(),
        "naming the class observes nothing"
    );

    let (trace, reads) = host.with_fact_tracer(AggregateBasisSeed::Unvouched, || {
        dispatch.trace_class_member_read_for_tests(FILE, "D", &[], "a", true)
    });
    assert!(trace.source.is_some(), "the delivered read answers");
    assert!(
        dispatch.graph().node_slot_count() > nodes,
        "delivering the classes interns their member positions"
    );
    assert!(
        !observed(reads).1.is_empty(),
        "delivering the classes observes them"
    );
}
