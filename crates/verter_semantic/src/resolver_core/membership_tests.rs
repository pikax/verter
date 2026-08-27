use super::*;
use crate::resolver_core::normalized_glob::NormalizedGlob;

fn glob(pattern: &str) -> CompiledGlob {
    CompiledGlob::new(NormalizedGlob::new(pattern))
}

fn spec(files: &[&str], include: &[&str], exclude: &[&str]) -> StaticMembershipSpec {
    StaticMembershipSpec {
        files: files.iter().map(|f| CanonicalPath::new(f)).collect(),
        include: include.iter().map(|g| glob(g)).collect(),
        exclude: exclude.iter().map(|g| glob(g)).collect::<Vec<_>>().into(),
    }
}

// ── StaticMembershipSpec::matches ──

#[test]
fn files_are_immune_to_exclude() {
    let s = spec(
        &["/proj/node_modules/pinned.ts"],
        &["/proj/**/*.ts"],
        &["/proj/node_modules/**"],
    );
    assert!(s.matches(&CanonicalPath::new("/proj/node_modules/pinned.ts")));
}

#[test]
fn include_minus_exclude() {
    let s = spec(&[], &["/proj/**/*.ts"], &["/proj/node_modules/**"]);
    assert!(s.matches(&CanonicalPath::new("/proj/src/main.ts")));
    assert!(!s.matches(&CanonicalPath::new("/proj/node_modules/dep/index.ts")));
}

#[test]
fn empty_include_matches_nothing_except_files() {
    let s = spec(&["/proj/only.ts"], &[], &[]);
    assert!(s.matches(&CanonicalPath::new("/proj/only.ts")));
    assert!(!s.matches(&CanonicalPath::new("/proj/other.ts")));
}

#[test]
fn not_included_short_circuits_before_exclude_check() {
    // A path outside `include` is rejected regardless of exclude.
    let s = spec(&[], &["/proj/src/**/*.ts"], &[]);
    assert!(!s.matches(&CanonicalPath::new("/proj/tests/main.ts")));
}

// ── ConfiguredMembership::contains ──

#[test]
fn contains_hits_materialized_set_first() {
    let s = spec(&[], &["/proj/**/*.ts"], &["/proj/node_modules/**"]);
    let mut materialized = FxHashSet::default();
    // Deliberately materialize a path the spec would otherwise reject, to
    // prove the positive-cache hit short-circuits spec evaluation.
    materialized.insert(CanonicalPath::new("/proj/node_modules/walked.ts"));
    let m = ConfiguredMembership {
        spec: s,
        materialized_files: materialized,
    };
    assert!(m.contains(&CanonicalPath::new("/proj/node_modules/walked.ts")));
}

#[test]
fn contains_falls_through_to_spec_on_miss() {
    let s = spec(&[], &["/proj/**/*.ts"], &["/proj/node_modules/**"]);
    let m = ConfiguredMembership {
        spec: s,
        materialized_files: FxHashSet::default(),
    };
    // Created after the walk: absent from materialized_files, but the spec
    // still accepts it.
    assert!(m.contains(&CanonicalPath::new("/proj/src/new_file.ts")));
    assert!(!m.contains(&CanonicalPath::new("/proj/node_modules/dep.ts")));
}

// ── ConfiguredMembership::directly_includes ──

#[test]
fn directly_includes_is_authoritative_when_materialized_is_populated() {
    let s = spec(&[], &["/proj/**/*.ts"], &[]);
    let mut materialized = FxHashSet::default();
    materialized.insert(CanonicalPath::new("/proj/walked.ts"));
    let m = ConfiguredMembership {
        spec: s,
        materialized_files: materialized,
    };
    // The spec would match this path, but the walk never produced it — a
    // populated materialized set is authoritative, unlike `contains`.
    assert!(!m.directly_includes(&CanonicalPath::new("/proj/never_walked.ts")));
    assert!(m.directly_includes(&CanonicalPath::new("/proj/walked.ts")));
}

#[test]
fn directly_includes_falls_back_to_spec_when_materialized_is_empty() {
    let s = spec(&[], &["/proj/**/*.ts"], &[]);
    let m = ConfiguredMembership {
        spec: s,
        materialized_files: FxHashSet::default(),
    };
    assert!(m.directly_includes(&CanonicalPath::new("/proj/anything.ts")));
}

// ── typescript_default_excludes ──

#[test]
fn default_excludes_reject_node_modules_bower_jspm() {
    let root = CanonicalPath::new("/proj-default-excludes-a");
    let excludes = typescript_default_excludes(&root);
    assert!(excludes.iter().any(|g| g.matches(&CanonicalPath::new(
        "/proj-default-excludes-a/node_modules/x.ts"
    ))));
    assert!(excludes.iter().any(|g| g.matches(&CanonicalPath::new(
        "/proj-default-excludes-a/bower_components/x.ts"
    ))));
    assert!(excludes.iter().any(|g| g.matches(&CanonicalPath::new(
        "/proj-default-excludes-a/jspm_packages/x.ts"
    ))));
    assert!(!excludes
        .iter()
        .any(|g| g.matches(&CanonicalPath::new("/proj-default-excludes-a/src/main.ts"))));
}

#[test]
fn default_excludes_are_memoized_per_root() {
    let root = CanonicalPath::new("/proj-default-excludes-b");
    let first = typescript_default_excludes(&root);
    let second = typescript_default_excludes(&root);
    assert!(
        Arc::ptr_eq(&first, &second),
        "same root must reuse the memoized allocation"
    );
}
