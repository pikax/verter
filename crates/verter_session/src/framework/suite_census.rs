//! Each documented suite invocation, proven to select a non-vacuous suite.
//!
//! `cargo test`'s filter is a plain substring, and these suites are gated —
//! `#[cfg(test)]`, and one of them additionally on a cargo feature. So an
//! invocation whose module is ABSENT (commented out, renamed, gated off, or
//! deleted) reports `running 0 tests`, then `test result: ok`, then exit 0. A
//! suite that reports success while executing nothing is a gate-bypass, so
//! each documented filter must also select a test that FAILS in that case.
//!
//! Such a test cannot live inside the suite it protects — deleting the module
//! would delete the check along with it. It lives here instead, and its name
//! carries the suite's documented filter substring, so the filter still
//! selects it once the suite module is gone.
//!
//! That placement leaves the reverse hole: deleting this module too, in the
//! same adjacent edit, would restore the vacuous green. The registration is
//! therefore MUTUAL and compile-enforced rather than conventional — this module
//! reads each suite's own `CENSUS_WITNESS_PATH` constant, and each suite calls
//! [`covers`] from a test of its own. Removing any ONE of the four `mod`
//! declarations is a build error, not a filter that quietly matches less.
//!
//! Each row is bound to the suite that OWNS it, not merely to a location: the
//! constant is the full path of that suite's own witness test, this module
//! requires a test of exactly that path to exist in the listing, and the module
//! it counts is DERIVED from it. Location checks alone prove only a location
//! class, so a suite emptied and repointed at another direct sibling with tests
//! of its own would otherwise clear its floor on that sibling's tests.
//! Removing all four at once is not decidable from inside this binary; it is
//! the general execution-attestation problem, and no in-binary check is
//! pretended here.
//!
//! The count is INDEPENDENTLY DISCOVERED, never a hand-maintained list of
//! expected test names: the census re-execs this same binary with
//! `--list --format=terse` and counts the tests it reports under the suite's
//! own module path. The listing must also contain the census test itself, so
//! an empty, failed, or unparsed listing cannot read as a pass.

use std::process::{Command, Stdio};

// Each suite's module path is read from the SUITE, not written down here, so
// this module cannot compile once a suite it censuses is gone. The suites in
// turn consume [`covers`], so neither side's `mod` declaration can be removed
// without a build error — the registration is a compile-time dependency, not a
// convention two hand-written lists are expected to keep.
use super::framework_product_surface_tests::CENSUS_WITNESS_PATH as PRODUCT_SURFACE_WITNESS;
use super::svelte_batch_route_tests::CENSUS_WITNESS_PATH as BATCH_ROUTE_WITNESS;
#[cfg(feature = "transport-authoritative")]
use super::transport_route_equivalence_tests::CENSUS_WITNESS_PATH as TRANSPORT_WITNESS;

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

/// The suite paths are USED as counting prefixes, so each must be a unique
/// suite identity. A suite owns its own constant, so without this a suite that
/// widened it — to an ancestor module, say — would count its siblings' tests,
/// or the census's own, as its own and clear its floor while carrying nothing.
///
/// Each property is asserted separately so the failure names which one broke.
#[track_caller]
fn assert_suite_paths_are_distinct_identities() {
    let census = census_module();
    let prefixes: Vec<String> = CENSUSED_WITNESSES
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
    for witness in CENSUSED_WITNESSES {
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
#[cfg(feature = "transport-authoritative")]
const CENSUSED_WITNESSES: &[&str] = &[
    PRODUCT_SURFACE_WITNESS,
    BATCH_ROUTE_WITNESS,
    TRANSPORT_WITNESS,
];
/// Without the feature the transport suite is not compiled in, and neither is
/// its census test — the documented and separately recorded vacuous case.
#[cfg(not(feature = "transport-authoritative"))]
const CENSUSED_WITNESSES: &[&str] = &[PRODUCT_SURFACE_WITNESS, BATCH_ROUTE_WITNESS];

/// Whether this census carries a test for the suite at `module_path`.
///
/// Each suite asserts its own membership from INSIDE itself. That call is what
/// closes the second half of the mutual dependency: deleting this module breaks
/// every suite's compile, exactly as deleting a suite breaks this one's.
pub(crate) fn covers(module_path: &str) -> bool {
    CENSUSED_WITNESSES.contains(&module_path)
}

/// Every test this binary reports for ITSELF, by full path.
///
/// The child is given ONLY `--list --format=terse`. Passing along the parent's
/// own arguments would let the caller's filter decide what the census can see,
/// which is exactly the property being measured.
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
         not the suite that registered this census row — either the suite was emptied, or its \
         constant points at a module that does not own this witness"
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
        PRODUCT_SURFACE_WITNESS,
        23,
        "the_framework_product_surface_suite_is_present_and_non_vacuous",
    );
}

#[test]
fn the_svelte_batch_route_suite_is_present_and_non_vacuous() {
    assert_suite_is_not_vacuous(
        "svelte_batch_route",
        BATCH_ROUTE_WITNESS,
        8,
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
        TRANSPORT_WITNESS,
        15,
        "the_transport_route_equivalence_suite_is_present_and_non_vacuous",
    );
}
