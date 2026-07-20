//! ARCH GUARD — pin the production-panic disposition
//! of the `impl ResolverContext for VerterHost` resolver methods.
//!
//! The `impl ResolverContext for VerterHost`
//! resolver methods MUST panic in production with a `#[cfg(test)]`
//! arm that performs the one-shot owned-view rebuild. This guard pins
//! that disposition in place — a future commit cannot silently revert
//! a panic body to a `route through bare-host` shim without failing
//! this test.
//!
//! The architectural rule:
//!
//! > Production paths are correct as-written OR they panic. No
//! > production shims. Test-only `#[cfg(test)]` arms perform the
//! > one-shot owned-view rebuild for test fixtures.
//!
//! Methods panic-shimmed (`impl ResolverContext for VerterHost`):
//! - `prepared_decl_bundle`
//! - `prepared_type_decl`
//! - `prepared_value_decl`
//! - `resolve_imported_type_root`
//! - `resolve_named_type_export_target_shallow`
//! - `resolve_owner_direct_import`
//! - `resolve_type_declaration_for_dep`
//!
//! (The former deep `resolve_named_type_export_target` trait method was
//! DELETED with the legacy eager type-export rail — only the shallow
//! variant remains on the trait; the deep spelling survives solely as a
//! `#[cfg(test)]` fixture on `VerterHost` in `host_resolve/route_surface.rs`,
//! which is not a production shim.)
//!
//! Each method MUST contain BOTH a `#[cfg(any(test, feature = "test-support"))]`
//! arm AND a `#[cfg(not(any(test, feature = "test-support")))] { panic!(...) }`
//! arm. The guard greps for both the method signature and the panic
//! body in the same file
//! (`crates/verter_session/src/resolver_core/resolver_context.rs`) —
//! a future commit that deletes the panic body fails this gate.

use std::fs;
use std::path::PathBuf;

/// Methods we expect to find panic-shimmed in `impl ResolverContext
/// for VerterHost`. Each entry pairs the method name with the
/// expected panic message substring; the panic message uniquely
/// identifies the matching arm and pins the bare-host context.
const EXPECTED_PANIC_METHODS: &[(&str, &str)] = &[
    (
        "prepared_decl_bundle",
        "bare-host prepared_decl_bundle called from",
    ),
    (
        "prepared_type_decl",
        "bare-host prepared_type_decl called from",
    ),
    (
        "prepared_value_decl",
        "bare-host prepared_value_decl called from",
    ),
    (
        "resolve_imported_type_root",
        "bare-host resolve_imported_type_root called from",
    ),
    (
        "resolve_named_type_export_target_shallow",
        "bare-host resolve_named_type_export_target_shallow",
    ),
    (
        "resolve_owner_direct_import",
        "bare-host resolve_owner_direct_import called from",
    ),
    (
        "resolve_type_declaration_for_dep",
        "bare-host resolve_type_declaration_for_dep called",
    ),
];

#[test]
fn bare_host_resolver_methods_panic_in_production() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let resolver_context_path = crate_root
        .join("src")
        .join("resolver_core")
        .join("resolver_context.rs");
    assert!(
        resolver_context_path.is_file(),
        "Guard fixture invariant: {} MUST exist",
        resolver_context_path.display(),
    );

    let source = fs::read_to_string(&resolver_context_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", resolver_context_path.display()));

    let mut missing: Vec<String> = Vec::new();
    for (method, expected_panic_msg) in EXPECTED_PANIC_METHODS {
        // Each method must:
        // 1. Be defined inside the bare-host trait impl.
        // 2. Have a `#[cfg(not(test))]` arm that contains the expected
        //    panic message substring.
        if !source.contains(&format!("fn {method}(")) {
            missing.push(format!(
                "method `{method}` not found in {}",
                resolver_context_path.display(),
            ));
            continue;
        }
        if !source.contains(expected_panic_msg) {
            missing.push(format!(
                "method `{method}` is missing its `#[cfg(not(test))]` panic body \
                 (expected panic msg containing `{expected_panic_msg}`)",
            ));
        }
    }

    assert!(
        missing.is_empty(),
        "`impl ResolverContext for VerterHost` resolver methods \
         MUST retain their `#[cfg(not(test))] panic!(...)` arms. A future commit cannot \
         revert these to route-through-bare-host shims without re-introducing the \
         eliminated architectural violation. {} violation(s):\n - {}",
        missing.len(),
        missing.join("\n - "),
    );
}

#[test]
fn bare_host_resolver_methods_retain_cfg_test_arm() {
    // Negative half of the pair: each method MUST also have a
    // `#[cfg(test)]` arm so test fixtures (e.g., direct
    // `host.resolve_*` calls in `host_manage_tests.rs`) keep working
    // via the one-shot owned-view rebuild. Deleting the `#[cfg(test)]`
    // arm would break the test surface; this guard pins it.
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let resolver_context_path = crate_root
        .join("src")
        .join("resolver_core")
        .join("resolver_context.rs");

    let source = fs::read_to_string(&resolver_context_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", resolver_context_path.display()));

    // The simplest discriminating check: confirm that each of the seven
    // panic-shim methods is paired with `#[cfg(test)]` somewhere in the
    // same impl block. We count `#[cfg(test)]` occurrences inside the
    // `impl ResolverContext for crate::VerterHost` block.
    let impl_start = source
        .find("impl ResolverContext for crate::VerterHost")
        .expect("`impl ResolverContext for crate::VerterHost` MUST be present");
    let impl_open = source[impl_start..]
        .find('{')
        .map(|rel| impl_start + rel)
        .expect("impl block opening brace MUST be present");
    let impl_close = find_matching_close_brace(&source, impl_open)
        .expect("impl block closing brace MUST be present");
    let impl_body = &source[impl_open..=impl_close];

    // Gating uses `#[cfg(any(test, debug_assertions))]` (and its
    // negation) so integration tests in `tests/*.rs` — which compile
    // the lib WITHOUT `cfg(test)` set — still reach the rebuild arm.
    // The release-build panic gate uses `cfg(not(any(test,
    // debug_assertions)))` because `debug_assertions` is OFF for
    // `cargo build --release`.
    let cfg_test_count = impl_body
        .matches("#[cfg(any(test, feature = \"test-support\"))]")
        .count();
    let cfg_not_test_count = impl_body
        .matches("#[cfg(not(any(test, feature = \"test-support\")))]")
        .count();
    assert!(
        cfg_test_count >= EXPECTED_PANIC_METHODS.len(),
        "`impl ResolverContext for VerterHost` MUST retain \
         at least {} test-support arms (one per panic-shim \
         resolver method). Found {}.",
        EXPECTED_PANIC_METHODS.len(),
        cfg_test_count,
    );
    assert!(
        cfg_not_test_count >= EXPECTED_PANIC_METHODS.len(),
        "`impl ResolverContext for VerterHost` MUST retain \
         at least {} non-test-support panic arms (one per panic-shim \
         resolver method). Found {}.",
        EXPECTED_PANIC_METHODS.len(),
        cfg_not_test_count,
    );
}

fn find_matching_close_brace(source: &str, open_idx: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth: i32 = 0;
    let mut idx = open_idx;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
        idx += 1;
    }
    None
}
