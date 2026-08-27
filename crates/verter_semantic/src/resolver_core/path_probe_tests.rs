use super::*;

#[test]
fn variants_are_pairwise_distinct() {
    let variants = [
        PathProbe::File,
        PathProbe::Directory,
        PathProbe::Absent,
        PathProbe::Inaccessible,
        PathProbe::Unknown,
    ];
    for (i, a) in variants.iter().enumerate() {
        for (j, b) in variants.iter().enumerate() {
            assert_eq!(i == j, a == b, "{a:?} vs {b:?}");
        }
    }
}

#[test]
fn error_tolerant_states_are_not_absence() {
    // The type-level distinction this enum exists to preserve: a permission
    // error or a transient failure must never collapse into `Absent`.
    assert_ne!(PathProbe::Inaccessible, PathProbe::Absent);
    assert_ne!(PathProbe::Unknown, PathProbe::Absent);
}

#[test]
fn orders_as_declared() {
    assert!(PathProbe::File < PathProbe::Directory);
    assert!(PathProbe::Directory < PathProbe::Absent);
    assert!(PathProbe::Absent < PathProbe::Inaccessible);
    assert!(PathProbe::Inaccessible < PathProbe::Unknown);
}
