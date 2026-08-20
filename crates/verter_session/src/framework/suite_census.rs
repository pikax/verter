//! Each documented suite invocation, proven to select a non-vacuous suite.
//!
//! Libtest's filter is a substring. These suites are gated (`#[cfg(test)]`,
//! one also on a cargo feature), so an absent module reports
//! `running 0 tests`, then `test result: ok`, then exit 0. That is a
//! gate-bypass: each documented filter must also select a test that FAILS
//! in that case. Read the `running N tests` line, never the exit code.
//!
//! The check cannot live inside the suite it protects. Registration is
//! mutual and compile-enforced: this module names each suite's witness
//! test as an ITEM, and each suite calls [`covers`] from that test.
//! Removing any one of the four `mod` declarations is a build error.
//! Identity is the function item ([`witness_identity`]), not a string
//! path a suite could hand over. The counted module is derived from that
//! item, so a suite cannot clear its floor on a sibling's tests.
//!
//! Deleting all four `mod`s in one edit is anchored outside this suite:
//! `framework::script_facts` consumes [`counts_tests_in`]. That does not
//! close execution-attestation — a binary cannot attest a universe it
//! was never given. Counts come from re-execing this binary with
//! `--list --format=terse`; the listing must contain the census test
//! itself so an empty/failed listing cannot read as a pass.

use std::process::{Command, Stdio};

/// Compiler path of the item `witness` refers to. Takes `&F`, not `fn()`:
/// a pointer erases every function to `fn()` and loses identity.
pub(crate) fn witness_identity<F>(_witness: &F) -> &'static str {
    std::any::type_name::<F>()
}

// Each suite's identity comes from an item NAMED here, so this module cannot
// compile once a suite it censuses is gone. The suites in turn consume
// [`covers`], so neither side's `mod` declaration can be removed without a build
// error — the registration is a compile-time dependency, not a convention two
// hand-written lists are expected to keep.
fn product_surface_witness() -> &'static str {
    witness_identity(
        &super::framework_product_surface_tests::this_suite_is_registered_with_the_census,
    )
}

fn batch_route_witness() -> &'static str {
    witness_identity(&super::svelte_batch_route_tests::this_suite_is_registered_with_the_census)
}

#[cfg(feature = "transport-authoritative")]
fn transport_witness() -> &'static str {
    witness_identity(
        &super::transport_route_equivalence_tests::this_suite_is_registered_with_the_census,
    )
}

/// This module's own compiler-derived path.
const CENSUS_RAW_PATH: &str = module_path!();

/// Turn a compiler `module_path!()` into the prefix libtest reports.
///
/// libtest names a test by its path WITHOUT the crate segment, so the crate
/// segment is stripped — taken from this module's own path rather than an
/// environment variable, so the two cannot disagree — and the trailing `::` is
/// added, making the result a prefix that can only match inside that module.
fn libtest_module_prefix(raw_module_path: &str) -> String {
    let (krate, _) = CENSUS_RAW_PATH
        .split_once("::")
        .expect("the census is not at the crate root");
    let inside = raw_module_path
        .strip_prefix(krate)
        .and_then(|rest| rest.strip_prefix("::"))
        .unwrap_or_else(|| panic!("`{raw_module_path}` is not a module path inside `{krate}`"));
    format!("{inside}::")
}

/// This module's own path, the prefix of every census test's reported name.
fn census_module() -> String {
    libtest_module_prefix(CENSUS_RAW_PATH)
}

/// The module a witness path belongs to.
fn witness_module(raw_witness_path: &str) -> &str {
    raw_witness_path
        .rsplit_once("::")
        .unwrap_or_else(|| {
            panic!("`{raw_witness_path}` is a bare name, not a path to a witness test")
        })
        .0
}

/// A witness path as libtest reports it: the crate segment stripped, and no
/// trailing `::` — this is a whole test name, not a module prefix.
fn libtest_witness_path(raw_witness_path: &str) -> String {
    let prefix = libtest_module_prefix(witness_module(raw_witness_path));
    let (_, test_name) = raw_witness_path
        .rsplit_once("::")
        .expect("checked by witness_module");
    format!("{prefix}{test_name}")
}

/// Counting prefixes must be unique suite identities, derived from the
/// witness item. An ancestor-module witness would steal siblings' tests.
#[track_caller]
fn assert_suite_paths_are_distinct_identities() {
    let census = census_module();
    let witnesses = censused_witnesses();
    let prefixes: Vec<String> = witnesses
        .iter()
        .map(|raw| libtest_module_prefix(witness_module(raw)))
        .collect();

    // (0) Each suite must be this census's DIRECT SIBLING: same parent module,
    //     exactly one further segment, and that segment non-empty. Without
    //     this, the checks below only relate the four paths to each other — so
    //     a suite retargeted at a DISJOINT module elsewhere in the crate is
    //     non-prefixing with all of them and clears its floor on that module's
    //     tests while carrying none of its own.
    let (census_parent, _) = CENSUS_RAW_PATH
        .rsplit_once("::")
        .expect("the census is not at the crate root");
    for witness in &witnesses {
        let raw = witness_module(witness);
        let segment = raw
            .strip_prefix(census_parent)
            .and_then(|rest| rest.strip_prefix("::"))
            .unwrap_or_else(|| {
                panic!(
                    "`{raw}` is not a child of `{census_parent}`, the module this census lives \
                     in, so it is not one of the sibling suites this census can speak for"
                )
            });
        assert!(
            !segment.is_empty() && !segment.contains("::"),
            "`{raw}` is `{census_parent}::{segment}`, which is not a DIRECT sibling of the census \
             — a suite identity is exactly one segment beyond the shared parent"
        );
    }

    for (index, left) in prefixes.iter().enumerate() {
        // (1) No suite path may be a prefix of the CENSUS's path: such a suite
        //     would count the census's own tests toward its floor.
        assert!(
            !census.starts_with(left.as_str()),
            "`{left}` is a prefix of the census's own module path `{census}`, so it is an \
             ancestor rather than a suite identity and would count census tests as its own"
        );
        // (2) …nor may the census's path be a prefix of a suite's, which would
        //     mean a suite had been declared inside the census.
        assert!(
            !left.starts_with(census.as_str()),
            "`{left}` sits inside the census's own module `{census}`, so it cannot be an \
             independent suite"
        );
        // (3) The suite paths must be PAIRWISE non-prefixing: one suite must
        //     never be able to count another's tests.
        for right in prefixes.iter().skip(index + 1) {
            assert!(
                !left.starts_with(right.as_str()) && !right.starts_with(left.as_str()),
                "`{left}` and `{right}` are prefix-related, so at least one of them counts the \
                 other's tests toward its own floor"
            );
        }
    }
}

/// The suites this census carries a test for.
///
/// Not a list of strings: each entry is the compiler's answer for an item this
/// module NAMES, so the list cannot name a suite that does not exist.
#[cfg(feature = "transport-authoritative")]
fn censused_witnesses() -> Vec<&'static str> {
    vec![
        product_surface_witness(),
        batch_route_witness(),
        transport_witness(),
    ]
}
/// Without the feature the transport suite is not compiled in, and neither is
/// its census test — the documented and separately recorded vacuous case.
#[cfg(not(feature = "transport-authoritative"))]
fn censused_witnesses() -> Vec<&'static str> {
    vec![product_surface_witness(), batch_route_witness()]
}

/// Whether this census carries a test for the suite that owns `witness`.
/// Mutual: deleting this module breaks every suite's compile.
pub(crate) fn covers<F>(witness: &F) -> bool {
    censused_witnesses().contains(&witness_identity(witness))
}

/// Whether any census row would count tests in `raw_module_path`. Prefix
/// match; outside modules use this as the compile-time anchor.
pub(crate) fn counts_tests_in(raw_module_path: &str) -> bool {
    censused_witnesses().into_iter().any(|witness| {
        let counted = witness_module(witness);
        raw_module_path == counted || raw_module_path.starts_with(&format!("{counted}::"))
    })
}

/// Tests this binary reports for itself. Child gets only
/// `--list --format=terse` — the parent's filter must not decide the census.
fn listed_test_paths() -> Vec<String> {
    let exe = std::env::current_exe().expect("the running test binary has a path");
    let output = Command::new(&exe)
        .args(["--list", "--format=terse"])
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "cannot re-exec {} to list its own tests: {error}",
                exe.display()
            )
        });
    assert!(
        output.status.success(),
        "listing this binary's own tests exited {:?}:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let listing = String::from_utf8_lossy(&output.stdout).into_owned();
    listing
        .lines()
        .filter_map(|line| {
            line.strip_suffix(": test")
                .or_else(|| line.strip_suffix(": benchmark"))
        })
        .map(str::to_string)
        .collect()
}

/// Assert one documented invocation selects a suite that actually exists.
///
/// `floor` is the number of tests recorded for the suite in the evidence
/// index; the assertion is `>=` so adding a test to the suite does not fail
/// the census, while removing the suite — or emptying it — does.
#[track_caller]
fn assert_suite_is_not_vacuous(suite: &str, raw_witness_path: &str, floor: usize, own_test: &str) {
    // The counting prefixes are validated BEFORE anything is counted with
    // them: a widened prefix makes every number below meaningless.
    assert_suite_paths_are_distinct_identities();
    let census = census_module();
    // The counted prefix is DERIVED from the suite's witness test, not given
    // alongside it, so the two cannot point at different modules.
    let witness = libtest_witness_path(raw_witness_path);
    let module_path = libtest_module_prefix(witness_module(raw_witness_path));

    let listed = listed_test_paths();
    assert!(
        !listed.is_empty(),
        "{suite}: this binary reported NO tests at all, so the census observed nothing and its \
         count below would be meaningless"
    );
    // SELF-WITNESS: the listing demonstrably describes THIS binary. Without it
    // a listing that parsed to nothing would report zero for every suite and
    // read as an ordinary absence rather than a broken census.
    let own_path = format!("{census}{own_test}");
    assert!(
        listed.contains(&own_path),
        "{suite}: the census test `{own_path}` is missing from this binary's own listing of {} \
         tests, so the discovery mechanism is not observing this binary",
        listed.len()
    );

    // SUITE WITNESS: the module being counted must contain the very test that
    // named it. Location checks alone prove only a location CLASS — a suite
    // retargeted at another direct sibling with tests of its own satisfies
    // every one of them. This binds the row to the suite that owns it: only
    // that suite defines a test with this path.
    assert!(
        listed.contains(&witness),
        "{suite}: no test named `{witness}` exists in this binary, so the module being counted is \
         not the suite that registered this census row — either the suite was emptied, or the \
         witness item this row names resolves into a module that does not declare it as a test"
    );

    let counted: Vec<&String> = listed
        .iter()
        .filter(|path| path.starts_with(module_path.as_str()))
        .collect();
    // A suite's total must be its OWN tests. The census's tests carry the
    // documented filter substrings, so a suite that swallowed them would clear
    // its floor on the very tests that exist to detect its absence.
    let census_tests_counted: Vec<&&String> = counted
        .iter()
        .filter(|path| path.starts_with(census.as_str()))
        .collect();
    assert!(
        census_tests_counted.is_empty(),
        "{suite}: {} census test(s) fall inside this suite's own count under `{module_path}`, so \
         the count is not this suite's: {census_tests_counted:?}",
        census_tests_counted.len()
    );

    let observed = counted.len();
    assert!(
        observed >= floor,
        "{suite}: this binary carries {observed} test(s) under `{module_path}`, below the \
         recorded floor of {floor}. A documented invocation naming this suite therefore executes \
         less than the evidence index records; at zero it is the vacuous case this census \
         exists to catch — `running 0 tests`, `test result: ok`, exit 0, proving nothing."
    );
}

#[test]
fn the_framework_product_surface_suite_is_present_and_non_vacuous() {
    assert_suite_is_not_vacuous(
        "framework_product_surface",
        product_surface_witness(),
        23,
        "the_framework_product_surface_suite_is_present_and_non_vacuous",
    );
}

#[test]
fn the_svelte_batch_route_suite_is_present_and_non_vacuous() {
    assert_suite_is_not_vacuous(
        "svelte_batch_route",
        batch_route_witness(),
        11,
        "the_svelte_batch_route_suite_is_present_and_non_vacuous",
    );
}

/// Gated exactly like the suite it censuses: without the feature neither
/// exists, which is the documented and separately recorded vacuous case.
#[cfg(feature = "transport-authoritative")]
#[test]
fn the_transport_route_equivalence_suite_is_present_and_non_vacuous() {
    assert_suite_is_not_vacuous(
        "transport_route_equivalence",
        transport_witness(),
        21,
        "the_transport_route_equivalence_suite_is_present_and_non_vacuous",
    );
}
