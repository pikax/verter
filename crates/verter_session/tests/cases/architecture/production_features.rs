use super::*;

#[test]
fn compile_batch_options_has_no_thread_field() {
    use syn::parse_file;

    let src = read_workspace_file("crates/verter_session/src/host_compile.rs");
    let parsed = parse_file(&src).expect("parse host_compile.rs via syn");

    let (found_struct, banned_present) =
        compile_batch_options_banned_thread_fields(&parsed, "CompileBatchOptions");

    // Positive precondition: the guard actually SAW the struct (a rename
    // or move would silently pass otherwise).
    assert!(
        found_struct,
        "compile_batch_options_has_no_thread_field: `pub struct CompileBatchOptions` not found in \
         host_compile.rs — did it move or get renamed? The guard must scan the real surface type."
    );

    assert!(
        banned_present.is_empty(),
        "CompileBatchOptions must carry NO per-call thread/concurrency knob, but found field(s): \
         {banned_present:?}. Worker count is fixed at host construction via \
         HostConfig::host_cpu_threads; a per-call thread cap is a B7-scoped concept \
         (CpuConcurrencySemaphore), not an option on this struct."
    );
}

/// Discriminator self-test: the predicate MUST flag a `threads` field if
/// one is re-added. Drives the pure core against a synthetic struct
/// source carrying the banned field, proving the guard above is not
/// vacuous.
#[test]
fn compile_batch_options_guard_catches_readded_thread_field() {
    use syn::parse_file;

    let synthetic = "pub struct CompileBatchOptions { \
                     pub priority: Option<Priority>, \
                     pub default_mode: Option<CompileCacheMode>, \
                     pub threads: Option<usize>, \
                     }";
    let parsed = parse_file(synthetic).expect("parse synthetic struct");
    let (found_struct, banned_present) =
        compile_batch_options_banned_thread_fields(&parsed, "CompileBatchOptions");
    assert!(found_struct, "self-test must find the synthetic struct");
    assert_eq!(
        banned_present,
        vec!["threads".to_string()],
        "the guard predicate must flag a re-added `threads` field — otherwise the \
         real guard is vacuous"
    );
}

/// Self-test for the strengthened `carrier_production_code` extractor (PLS2 fix
/// F): it must match the robust Stage-1 strippers — (1) scan production code
/// AFTER an inline `#[cfg(test)]` item (blank the test item in place, never
/// truncate at the first marker) and (2) NOT false-positive on a token inside a
/// `/* */` block comment. Both halves FAIL against the weak split-once +
/// `//`-only extractor.
#[test]
fn carrier_production_code_scans_post_cfg_test_and_strips_block_comments() {
    // (1) Production code AFTER an inline cfg-test module is still scanned: the
    //     forbidden token in `fn prod` survives, while the token inside the
    //     cfg-test block is excluded. The weak split-once extractor truncates
    //     at the leading `#[cfg(test)]`, losing `fn prod` entirely.
    let post_cfg = "#[cfg(test)]\nmod t { let inert = execute_read(); }\n\
                    fn prod() { let hit = intern_node(); }";
    let scanned = carrier_production_code(post_cfg);
    assert!(
        scanned.contains("intern_node"),
        "production code AFTER a cfg-test item must still be scanned"
    );
    assert!(
        !scanned.contains("execute_read"),
        "a token inside the cfg-test block must be excluded"
    );

    // (2) A token inside a `/* */` block comment must NOT be a false positive.
    //     The weak `//`-only stripper leaves the block-comment token intact.
    let block_comment = "fn prod() { /* execute_read happens elsewhere */ let ok = 1; }";
    assert!(
        !carrier_production_code(block_comment).contains("execute_read"),
        "a token inside a `/* */` block comment must not be a false positive"
    );

    // Anti-vacuity: a forbidden token in genuine production code IS still seen.
    assert!(
        carrier_production_code("fn prod() { let _ = execute_read(); }").contains("execute_read"),
        "a production token must still be detected by the strengthened extractor"
    );

    // (3) A `'"'` char literal must NOT open string mode. Pre-fix the lone `"`
    //     inside the char literal opened a phantom string that disabled
    //     comment-stripping to end of input, so a token inside the following
    //     `//` comment leaked into the scan (false positive).
    let char_quote = "let _q = '\"'; // intern_node\nfn prod() { let hit = execute_read(); }";
    let scanned = carrier_production_code(char_quote);
    assert!(
        !scanned.contains("intern_node"),
        "a `//` comment after a `'\"'` char literal must still be stripped"
    );
    assert!(
        scanned.contains("execute_read"),
        "real code after a `'\"'` char literal and comment must still be scanned"
    );

    // (4) A lifetime (`'a`) is NOT a char literal and must pass through
    //     untouched — a naive char arm that scanned to the next `'` would mask
    //     the real code that follows.
    assert!(
        carrier_production_code("fn f<'a>(x: &'a str) { let _ = execute_read(); }")
            .contains("execute_read"),
        "a lifetime must not be mistaken for a char literal that masks later code"
    );
}

/// Self-test for [`crate_visible_producer_fn_names`] + the entry-surface guard:
/// the sole `pub(crate)` entry passes; an INJECTED second crate-visible producer
/// — `pub(crate)` OR `pub(in crate)`, FREE or ASSOCIATED (inside an INHERENT
/// `impl`) — reddens; reverting greens. A `pub(in crate::…)`-subtree /
/// `pub(super)` restricted entry is NOT crate-visible; an item whose
/// `#[cfg(...)]` ENTAILS test (`#[cfg(test)]`, `#[cfg(all(test, …))]`) is
/// excluded, while a PRODUCTION-satisfiable `#[cfg(any(test, …))]`,
/// `#[cfg(not(test))]`, or bare `#[cfg(debug_assertions)]` item is COUNTED.
/// The producer seal guards count a `test-support`-gated item as test code and
/// nothing else: `any(test, feature = "test-support")` is excluded, while
/// another feature, `debug_assertions`, `not(test)` and `cfg_attr` stay
/// counted; a `pub use` naming a builder is reported, a `pub use` of a
/// differently named seam is not.
#[test]
fn producer_seal_test_support_classifier_discriminates() {
    let gated = |cfg: &str| -> bool {
        let src = format!("{cfg}\npub(crate) fn seam() {{}}\n");
        crate_visible_producer_fn_names(&src).is_empty()
    };
    assert!(gated("#[cfg(test)]"));
    assert!(gated("#[cfg(feature = \"test-support\")]"));
    assert!(gated("#[cfg(any(test, feature = \"test-support\"))]"));
    assert!(gated("#[cfg(all(unix, feature = \"test-support\"))]"));
    assert!(!gated("#[cfg(any(test, feature = \"x\"))]"));
    assert!(!gated("#[cfg(any(test, debug_assertions))]"));
    assert!(!gated(
        "#[cfg(any(feature = \"test-support\", feature = \"x\"))]"
    ));
    assert!(!gated("#[cfg(not(test))]"));
    assert!(!gated(
        "#[cfg_attr(feature = \"test-support\", allow(dead_code))]"
    ));
    assert!(structural_builder_reexport_violation(
        "pub use macro_arg_producer::{lower_type_expr_structural, Other};",
        "lower_type_expr_structural"
    ));
    assert!(!structural_builder_reexport_violation(
        "pub use macro_arg_producer::{lower_type_expr_structural_for_tests, Other};",
        "lower_type_expr_structural"
    ));
    let gated_reexport = "mod infer_binder_names;\nmod macro_arg_producer;\n\
        pub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};\n\
        #[cfg(any(test, feature = \"test-support\"))]\n\
        pub(crate) use macro_arg_producer::lower_type_expr_structural_for_tests;\n";
    let violations = mod_rs_reexport_shape_violations(gated_reexport);
    assert!(
        !violations
            .iter()
            .any(|v| v.contains("lower_type_expr_structural_for_tests")),
        "a test-support-gated mod.rs re-export is test wiring: {violations:?}"
    );
    let ungated_reexport = "mod infer_binder_names;\nmod macro_arg_producer;\n\
        pub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};\n\
        pub(crate) use macro_arg_producer::lower_type_expr_structural_for_tests;\n";
    assert!(
        !mod_rs_reexport_shape_violations(ungated_reexport).is_empty(),
        "an ungated extra mod.rs re-export must stay a violation"
    );
}

#[test]
fn component_meta_hot_paths_self_test_inline_cfg_test_build_is_ignored() {
    // The guard is production-only: a `ScopeShadowing` build inside an inline
    // `#[cfg(test)]` module within a production `meta_resolve` file is NOT a
    // production construction and must be ignored.
    let planted = r#"
        fn production_path(query_engine: &mut ComponentMetaQueryEngine, scope: &str) {
            let _ = query_engine.scope_shadowing_for_scope(scope);
        }

        #[cfg(test)]
        mod tests {
            fn builds_directly_in_a_test() {
                let _ =
                    crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(None);
            }
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
        planted,
    );
    // A scanner that strips only `*_tests.rs` FILES flags this inline test (1);
    // an inline-`#[cfg(test)]`-aware scanner ignores it (0).
    assert!(
        v.is_empty(),
        "a `ScopeShadowing` build inside an inline `#[cfg(test)]` module must be \
         ignored (the guard is production-only); got: {v:?}"
    );
}

#[test]
fn component_meta_hot_paths_self_test_cfg_not_test_and_any_test_fire() {
    // PRODUCTION-SATISFIABLE cfgs are NOT test-only. A `#[cfg(not(test))]` item
    // compiles into every non-test build, and `#[cfg(any(test, feature = "x"))]`
    // compiles whenever the feature is on (a non-test config satisfies it). A
    // token-search-for-`test` classifier wrongly skips both (the token `test`
    // appears in the predicate); the entailment classifier (`attrs_test_gate`)
    // scans them. Both builds MUST fire.
    let planted = r#"
        #[cfg(not(test))]
        fn f() {
            let _ = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(None);
        }

        #[cfg(any(test, feature = "x"))]
        fn g() {
            let _ = crate::resolver_core::scope_shadowing::ScopeShadowing::empty();
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
        planted,
    );
    // A token-search classifier skips both production-satisfiable cfgs (0); the
    // entailment classifier scans both (2).
    assert_eq!(
        v.len(),
        2,
        "`#[cfg(not(test))]` and `#[cfg(any(test, feature = \"x\"))]` are \
         PRODUCTION-satisfiable, so direct builds under them MUST fire; got: {v:?}"
    );
    assert!(v.iter().any(|x| x.method == "from_scope_payload"));
    assert!(v.iter().any(|x| x.method == "empty"));
}

#[test]
fn component_meta_hot_paths_self_test_genuinely_test_only_cfg_ignored() {
    // Companion negative control: a cfg that genuinely ENTAILS test
    // (`#[cfg(test)]`, `#[cfg(all(test, unix))]` — every satisfying config has
    // `test = true`) is test-only and is IGNORED. Pins the entailment classifier
    // against the OPPOSITE failure (scanning every cfg).
    let planted = r#"
        #[cfg(test)]
        fn t() {
            let _ = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(None);
        }

        #[cfg(all(test, unix))]
        fn u() {
            let _ = crate::resolver_core::scope_shadowing::ScopeShadowing::empty();
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
        planted,
    );
    assert!(
        v.is_empty(),
        "`#[cfg(test)]` and `#[cfg(all(test, unix))]` ENTAIL test and must be \
         ignored (the guard is production-only); got: {v:?}"
    );
}

#[test]
fn component_meta_hot_paths_self_test_cfg_test_impl_block_is_ignored() {
    // The cfg-test gate applies to the ENCLOSING `impl` block, not just the
    // method: a `#[cfg(test)] impl Foo { fn t() { ScopeShadowing::.. } }` is
    // test-only and its inner builds must be IGNORED. Without impl-level cfg
    // depth the inner method (which carries no cfg of its own) is scanned — a
    // false positive.
    let planted = r#"
        struct Foo;

        #[cfg(test)]
        impl Foo {
            fn t() {
                let _ =
                    crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(None);
            }
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
        planted,
    );
    // No impl-level cfg depth scans the inner method (1); impl-level cfg gating
    // ignores it (0).
    assert!(
        v.is_empty(),
        "a build inside a `#[cfg(test)] impl` block is test-only and must be \
         ignored; got: {v:?}"
    );
}

#[test]
fn component_meta_hot_paths_self_test_cfg_test_item_initializer_is_ignored() {
    // A `#[cfg(test)]` `const`/`static` item is test-only; its initializer (here
    // a closure body) compiles only under test, so a `ScopeShadowing` build
    // inside it must be IGNORED. Without const/static cfg depth the initializer
    // is scanned — a false positive.
    let planted = r#"
        #[cfg(test)]
        const SAMPLE_CTOR: fn() = || {
            let _ = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(None);
        };

        #[cfg(test)]
        static SAMPLE_BUILDER: fn() = || {
            let _ = crate::resolver_core::scope_shadowing::ScopeShadowing::empty();
        };
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
        planted,
    );
    // No const/static cfg depth scans both initializers (2); item-level cfg
    // gating ignores them (0).
    assert!(
        v.is_empty(),
        "builds inside `#[cfg(test)]` `const`/`static` initializers are test-only \
         and must be ignored; got: {v:?}"
    );
}
