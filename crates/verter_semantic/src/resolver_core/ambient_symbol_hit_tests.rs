use super::*;
use crate::resolver_core::project_stable_key::compute_hash16;

#[test]
fn equality_is_field_wise() {
    let project = ProjectStableKey::Configured(compute_hash16(b"proj"));
    let a = AmbientSymbolHit {
        project,
        canonical_id: Arc::from("verter-ambient:vue-3.6-rc.3:global.d.ts"),
        virtual_id: Arc::from("C0abc-global.d.ts"),
        lib_order: 3,
    };
    let b = a.clone();
    assert_eq!(a, b);

    let mut c = a.clone();
    c.lib_order = 4;
    assert_ne!(a, c);
}

#[test]
fn distinct_projects_are_distinct_hits() {
    let hit_a = AmbientSymbolHit {
        project: ProjectStableKey::Configured(compute_hash16(b"a")),
        canonical_id: Arc::from("id"),
        virtual_id: Arc::from("vid"),
        lib_order: 0,
    };
    let hit_b = AmbientSymbolHit {
        project: ProjectStableKey::Configured(compute_hash16(b"b")),
        ..hit_a.clone()
    };
    assert_ne!(hit_a, hit_b);
}
