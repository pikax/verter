use crate::project_semantic_dispatch::flow_return::*;
use crate::semantic_query::SemanticNodeId;
use std::sync::Arc;
use verter_session_query::flow::binding::FlowBindingRef as FlowProductSubject;

fn param(ordinal: u32) -> FlowProductSubject {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = crate::parse::Parser::new(
        &allocator,
        "function f(p0: unknown, p1: unknown) {}",
        oxc_span::SourceType::ts(),
    )
    .parse();
    let oxc_ast::ast::Statement::FunctionDeclaration(function) = &parsed.program.body[0] else {
        panic!("function fixture");
    };
    let source =
        verter_semantic::analysis::flow::FunctionBodySource::from_function(function).expect("body");
    let skeleton = verter_semantic::analysis::flow::build_function_body_skeleton(
        &source,
        parsed.program.source_text,
    );
    let name = skeleton
        .name_id(&format!("p{ordinal}"))
        .expect("parameter name");
    let binding = skeleton
        .bindings_named(name)
        .next()
        .expect("parameter binding");
    FlowProductSubject::Local(binding)
}

fn authored(ordinal: u32, path: &[&str]) -> verter_session_query::flow::slice::SliceNarrowSubject {
    verter_session_query::flow::slice::SliceNarrowSubject {
        root: verter_session_query::flow::slice::SliceNarrowRoot::Local {
            name: Arc::from(format!("p{ordinal}")),
            binding: param(ordinal),
        },
        path: Arc::from(
            path.iter()
                .map(|segment| Arc::from(*segment))
                .collect::<Vec<Arc<str>>>()
                .into_boxed_slice(),
        ),
    }
}

fn established(ordinal: u32, path: &[&str], node: u64) -> NarrowingLedgerEntry {
    NarrowingLedgerEntry::Established {
        root: param(ordinal),
        subject: authored(ordinal, path),
        node: SemanticNodeId(node),
    }
}

/// A window whose only movement RE-WRITES the value the position
/// already held still reports that fact. The union-of-facts guard
/// asks a disjunct "what did your edge prove", and an edge that
/// re-establishes a fact proved it; an overlay diff cannot see the
/// write at all, so the disjunct would drop out of `narrowed_in_all`
/// and the guard would publish a narrower type than any edge proves.
///
/// Mutation: skipping an establishment whose node equals the value
/// the position already held (the overlay-diff reading) empties the
/// reported window and fails this.
#[test]
fn a_re_established_fact_is_reported_as_the_windows_contribution() {
    let window = [established(0, &["v"], 7)];
    assert_eq!(
        standing_narrowings(&window),
        vec![(authored(0, &["v"]), SemanticNodeId(7))],
    );
}

/// Write ORDER is the report's order, and a position written twice
/// reports both writes: the union guard's per-disjunct fold keeps the
/// LAST write for a position, so collapsing the pair here (or
/// reporting them reversed) would hand it the wrong alternative.
#[test]
fn a_position_written_twice_reports_both_writes_in_order() {
    let window = [
        established(0, &["v"], 7),
        established(1, &[], 9),
        established(0, &["v"], 8),
    ];
    assert_eq!(
        standing_narrowings(&window),
        vec![
            (authored(0, &["v"]), SemanticNodeId(7)),
            (authored(1, &[]), SemanticNodeId(9)),
            (authored(0, &["v"]), SemanticNodeId(8)),
        ],
    );
}

/// A kill inside the window RETRACTS every fact the window
/// established under that root — a write to the binding invalidates
/// the facts about the value it replaced, and an edge does not prove
/// a fact it invalidated. Facts under OTHER roots survive, and a
/// later establishment under the killed root stands again.
///
/// Mutation: dropping the retraction (treating a kill as no
/// movement) reports the invalidated `v` fact and fails this.
#[test]
fn a_kill_retracts_only_the_facts_its_own_root_established() {
    let window = [
        established(0, &["v"], 7),
        established(1, &[], 9),
        NarrowingLedgerEntry::Cleared { root: param(0) },
        established(0, &["w"], 11),
    ];
    assert_eq!(
        standing_narrowings(&window),
        vec![
            (authored(1, &[]), SemanticNodeId(9)),
            (authored(0, &["w"]), SemanticNodeId(11)),
        ],
    );
}

/// A kill reports nothing about facts the window never established:
/// those belong to whichever outstanding mark still holds them, and
/// a window that claimed them would let one disjunct inherit an
/// enclosing scope's facts as its own contribution.
#[test]
fn a_kill_alone_reports_an_empty_contribution() {
    let window = [NarrowingLedgerEntry::Cleared { root: param(0) }];
    assert!(standing_narrowings(&window).is_empty());
    assert!(standing_narrowings(&[]).is_empty());
}
