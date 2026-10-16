use std::sync::Arc;

use super::*;
use crate::project_semantic_dispatch::walk::DiagnosticError;
use crate::semantic_query::demand::{Demand, MaterializedPoint};

fn arm(name: &str, owner: &str) -> ShallowDiagnostic {
    ShallowDiagnostic::UnresolvedSurfaceArm {
        name: Arc::from(name),
        owner_canonical: Arc::from(owner),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
    }
}

#[test]
fn a_diagnostic_rail_is_charged_for_the_text_its_diagnostics_name() {
    let short: Arc<[ShallowDiagnostic]> = Arc::from(vec![arm("A", "/a.ts")]);
    let long: Arc<[ShallowDiagnostic]> = Arc::from(vec![arm(&"n".repeat(9_000), "/a.ts")]);
    let short_bytes = short.owned_heap_bytes(&mut AliasSet::new());
    let long_bytes = long.owned_heap_bytes(&mut AliasSet::new());
    // The former per-diagnostic proxy charged both 256 bytes.
    assert!(long_bytes >= short_bytes + 8_900);
}

#[test]
fn a_nested_error_and_its_declaration_are_charged() {
    let decl = DeclIdentity {
        canonical_id: Arc::from("/decl.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: Default::default(),
        decl_name: Arc::from("Decl"),
    };
    let message: Arc<str> = Arc::from("m".repeat(5_000));
    let diagnostic = ShallowDiagnostic::InstantiationError {
        decl,
        error: DiagnosticError::for_tests(QueryError::Other(Arc::clone(&message))),
    };
    assert!(diagnostic.owned_heap_bytes(&mut AliasSet::new()) >= 5_000);
}

#[test]
fn a_materialised_point_is_charged_for_its_path_text() {
    let path = ProjectionPath::from_segments([PathSegment::Member(PropertyKey::String(
        Arc::from("p".repeat(4_000)),
    ))]);
    let mut demand = Demand::identity();
    demand.projection.path = path;
    let set = MaterializedSet(Arc::from(vec![MaterializedPoint::new(demand)]));
    let empty = MaterializedSet::empty();
    assert!(
        set.owned_heap_bytes(&mut AliasSet::new())
            >= empty.owned_heap_bytes(&mut AliasSet::new()) + 4_000,
        "the former per-point proxy charged 96 bytes whatever the path"
    );
}

#[test]
fn one_charge_counts_a_shared_allocation_once() {
    let name: Arc<str> = Arc::from("s".repeat(3_000));
    let rail: Arc<[ShallowDiagnostic]> = Arc::from(vec![ShallowDiagnostic::UnresolvedSurfaceArm {
        name: Arc::clone(&name),
        owner_canonical: Arc::clone(&name),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
    }]);
    let bytes = rail.owned_heap_bytes(&mut AliasSet::new());
    assert!(bytes >= 3_000);
    assert!(bytes < 6_000, "one name aliased twice is one allocation");
}
