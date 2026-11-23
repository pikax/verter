//! A class member read is an owned read: it walks the inheritance path
//! holding the classes it followed, and every authored shape it lowers (a
//! declaring member's value, an `extends` arm) is a `LowerLocator` query it
//! names for its holder to answer whole. Stepped by hand, with the holder
//! answering each read, it answers and observes exactly what the in-place
//! driver does, and its own stages make no read the body census counts.

use super::carrier_head_resolution_tests::{host, upsert_ts};
use super::ProjectSemanticDispatch;
use verter_session_query::facts::fact_cache::AggregateBasisSeed;
use verter_session_query::facts::fact_read_set::{FactReadSet, FactReadSetFinalise};
use verter_session_query::facts::fact_set::FactSet;
use verter_type_engine::project_semantic_dispatch::build::{
    ClassMemberRequestForTests, ClassMemberTraceForTests,
};

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

/// A `LowerLocator` read, answered whole, of `class`'s `position`.
fn shape_read(class: &str, position: &'static str) -> ClassMemberRequestForTests {
    ClassMemberRequestForTests {
        family: "LowerLocator",
        shape: "whole",
        class: class.to_owned(),
        position,
    }
}

/// The member read stepped by hand, its holder answering each read it
/// names, answers, names and observes exactly what `class_member_source`'s
/// in-place driver does, over the same classes: an own member; an inherited
/// member followed up a chain to the class declaring it; a generic base
/// applied to its clause's argument; a circular chain, which stops at the
/// class already followed; a value-only base, whose `extends` arm names the
/// synthesized `V:extends` declaration, which is no class, so the read
/// answers with the arm; and a class with neither the member nor an
/// `extends` clause, which answers nothing.
///
/// Every read the walk names is a `LowerLocator` query answered whole, one
/// per `extends` arm and declaring member it lowers; held, the walk's own
/// stages make no read the body census counts, while in place the census
/// counts exactly those reads.
#[test]
fn stepped_class_member_reads_preserve_in_place_answers_order_and_observations() {
    let heritage = |class| shape_read(class, "heritage");
    for (class, name, first, exact, answers) in [
        ("Own", "a", vec![], false, true),
        ("D", "a", vec![heritage("D"), heritage("M")], false, true),
        (
            "D",
            "other",
            vec![heritage("D"), heritage("M")],
            false,
            true,
        ),
        ("GD", "v", vec![heritage("GD")], false, true),
        ("C1", "a", vec![heritage("C1"), heritage("C2")], true, true),
        ("V", "a", vec![heritage("V")], false, true),
        ("Lone", "a", vec![], true, false),
    ] {
        let host = host();
        upsert_ts(&host, FILE, SOURCE);
        host.ensure_indexed_ready(FILE)
            .expect("the fixture declarations are indexed");
        let fixture = ProjectSemanticDispatch::new(&host);
        let _construction = fixture.enter_construction_scope();
        fixture.graph().invalidate_all();
        let stepped_dispatch = ProjectSemanticDispatch::new(&host);
        let (mut stepped, stepped_facts) = traced(&host, &stepped_dispatch, class, name, true);
        // The same fixture with an independently cold memo.
        fixture.graph().invalidate_all();
        let in_place_dispatch = ProjectSemanticDispatch::new(&host);
        let (mut in_place, in_place_facts) = traced(&host, &in_place_dispatch, class, name, false);

        let held_in_place = std::mem::take(&mut stepped.made_in_place);
        let made_in_place = std::mem::take(&mut in_place.made_in_place);
        assert_eq!(stepped, in_place, "{class}.{name}");
        assert_eq!(
            stepped_facts, in_place_facts,
            "{class}.{name}: observed facts"
        );

        let requested = &stepped.requested;
        assert!(
            requested.iter().all(|read| read.family == "LowerLocator"
                && read.shape == "whole"
                && !read.class.is_empty()),
            "{class}.{name}: every read is a class's authored shape, answered whole: {requested:?}"
        );
        if exact {
            assert_eq!(
                requested, &first,
                "{class}.{name}: the reads the walk named"
            );
        } else {
            assert_eq!(
                &requested[..first.len().min(requested.len())],
                &first[..],
                "{class}.{name}: the reads the walk names first"
            );
        }
        assert_eq!(
            stepped.source.is_some(),
            answers,
            "{class}.{name}: where the member reads from"
        );
        assert_eq!(
            held_in_place,
            Vec::<String>::new(),
            "{class}.{name}: held, the walk makes no read of its own"
        );
        assert_eq!(
            made_in_place,
            vec!["LowerLocator".to_owned(); requested.len()],
            "{class}.{name}: in place, the census counts each read the walk names"
        );
    }
}

/// The read's first stage stands at the first read it names, the reading
/// class's `extends` arm (`D` declares no `a`), and makes none of it: no
/// read the body census counts. Its holder answering that read is what
/// lowers the arm.
#[test]
fn a_class_member_read_is_requested_not_settled_in_place() {
    let host = host();
    upsert_ts(&host, FILE, SOURCE);
    host.ensure_indexed_ready(FILE)
        .expect("the fixture declarations are indexed");
    let dispatch = ProjectSemanticDispatch::new(&host);

    let (first, made_in_place) = dispatch.begin_class_member_read_for_tests(FILE, "D", "a");
    assert_eq!(
        first,
        Some(shape_read("D", "heritage")),
        "the read names the reading class's extends arm, a LowerLocator query answered whole"
    );
    assert_eq!(
        made_in_place,
        Vec::<String>::new(),
        "naming the read makes no read of its own"
    );

    let (trace, _) = host.with_fact_tracer(AggregateBasisSeed::Unvouched, || {
        dispatch.trace_class_member_read_for_tests(FILE, "D", &[], "a", true)
    });
    assert!(
        trace.source.is_some(),
        "the answered reads answer the member"
    );
    assert_eq!(
        trace.requested.first(),
        Some(&shape_read("D", "heritage")),
        "the holder answers the read the first stage named"
    );
}
