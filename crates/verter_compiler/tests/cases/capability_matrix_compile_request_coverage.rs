//! Drives `packages/framework-conformance-harness/evidence/
//! capability-matrix.tsv` directly: every row's `cell_id` must have a
//! registered verification closure below, exercised against the ONE
//! canonical `CompileRequest` construction/resolution authority.
//!
//! This is NOT a re-proof of every capability owner's runtime behavior
//! (parse recovery, VDOM/Vapor/SSR client-server topology, Svelte runtime
//! topology, and TS-provider reachability are proven elsewhere, by their
//! own owners — this file does not own framework lowering/codegen or
//! cross-route runtime equivalence). It verifies the ONE thing every
//! row's `target_disposition` implies at the `CompileRequest`
//! construction/resolution boundary this file DOES own:
//! - `supported` / `projection-required` rows whose axis is expressed as a
//!   distinct `CompileProduct` or `VueCompileRequest`/`SvelteCompileRequest`
//!   field construct a request successfully.
//! - `unsupported fail-closed` rows enforced by `CompileRequest::new`,
//!   `CompileRequest::resolve_vue_backend`, or `VueOptionAttempt`/
//!   `SvelteOptionAttempt::into_request` are proven to refuse, with the
//!   EXACT typed reason.
//! - Rows whose claim is genuinely outside `CompileRequest`'s own
//!   construction-time option surface (parse diagnostics, version pinning,
//!   TS-provider-mediated reachability, script-content-driven runtime
//!   behavior such as `SVELTE-ASYNC-EXPERIMENTAL`'s component-level async/
//!   boundary reachability) are EXPLICIT, documented exemptions — never a
//!   silent skip.
//!
//! Discriminating: `every_tsv_row_has_a_registered_verification` asserts the
//! TSV's row set is EXACTLY the registered table's key set — a new TSV row
//! added without a matching entry here fails the test, and a stale entry
//! whose TSV row was removed fails it too (no drift either direction).

use std::path::PathBuf;

use verter_compiler::compile_request::svelte::AdmittedSvelteCustomElementDescriptor;
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, CompileRequestError, DeclarationProductRequest,
    FrameworkCompileRequest, IdeProductRequest, PublicApiProductRequest, RuntimeProductRequest,
    SvelteCompileRequest, SvelteOptionAttempt, VueBackendRequest, VueCompileRequest,
    VueOptionAttempt,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate is <workspace>/crates/verter_compiler")
        .to_path_buf()
}

/// One parsed capability-matrix.tsv row (only the columns this guard reads).
struct MatrixRow {
    cell_id: String,
    target_disposition: String,
}

fn read_matrix_rows() -> Vec<MatrixRow> {
    let path = workspace_root()
        .join("packages/framework-conformance-harness/evidence/capability-matrix.tsv");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("capability-matrix.tsv must be readable at {path:?}: {e}"));
    let mut lines = raw.lines();
    let header = lines
        .next()
        .expect("capability-matrix.tsv must have a header row");
    let columns: Vec<&str> = header.split('\t').collect();
    let cell_id_idx = columns
        .iter()
        .position(|c| *c == "cell_id")
        .expect("capability-matrix.tsv header must have a cell_id column");
    let disposition_idx = columns
        .iter()
        .position(|c| *c == "target_disposition")
        .expect("capability-matrix.tsv header must have a target_disposition column");
    lines
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            MatrixRow {
                cell_id: fields[cell_id_idx].to_string(),
                target_disposition: fields[disposition_idx].to_string(),
            }
        })
        .collect()
}

/// Construct a lone-runtime-client request, the minimal always-constructible
/// baseline every "supported" probe below starts from.
fn base_vue_runtime_client() -> Vec<CompileProduct> {
    vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]
}

/// Assert a `CompileRequest::new` call succeeds; panics with the row id on
/// failure so a broken probe names the exact cell it belongs to.
fn assert_constructs(cell_id: &str, result: Result<CompileRequest, CompileRequestError>) {
    assert!(
        result.is_ok(),
        "{cell_id}: expected the request to construct, got {result:?}"
    );
}

/// Assert a `CompileRequest::new` call refuses with EXACTLY `expected`.
fn assert_refuses(
    cell_id: &str,
    result: Result<CompileRequest, CompileRequestError>,
    expected: CompileRequestError,
) {
    match result {
        Err(actual) => assert_eq!(
            actual, expected,
            "{cell_id}: expected refusal {expected:?}, got {actual:?}"
        ),
        Ok(_) => panic!("{cell_id}: expected refusal {expected:?}, request constructed instead"),
    }
}

/// Verify one capability-matrix cell. Returns `Ok(())` when the cell's
/// disposition is proven at the `CompileRequest` construction/resolution
/// boundary, or `Err(reason)` for a documented exemption (a claim this
/// boundary genuinely cannot express — never a silent skip: the caller
/// still records the reason and the exhaustiveness check below still fires
/// on an unregistered cell_id).
fn verify_cell(cell_id: &str) -> Result<(), &'static str> {
    match cell_id {
        // ── Parse diagnostics have no CompileRequest axis. ──
        "VUE-PARSE-LOCAL" | "SVELTE-PARSE-LOCAL" => Err(
            "parse-diagnostics reachability is its own product (ParseDiagnostics), \
                 not a CompileRequest product/profile axis",
        ),

        // ── Vue runtime client/server/backend. ──
        "VUE-VDOM-CLIENT" => {
            let request = CompileRequest::new(
                base_vue_runtime_client(),
                FrameworkCompileRequest::Vue(VueCompileRequest {
                    backend: VueBackendRequest::Vdom,
                    ..Default::default()
                }),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "VUE-VAPOR-CLIENT" => {
            let request = CompileRequest::new(
                base_vue_runtime_client(),
                FrameworkCompileRequest::Vue(VueCompileRequest {
                    backend: VueBackendRequest::Vapor,
                    ..Default::default()
                }),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "VUE-SSR" => {
            let request = CompileRequest::new(
                vec![CompileProduct::RuntimeServer(
                    RuntimeProductRequest::default(),
                )],
                FrameworkCompileRequest::Vue(VueCompileRequest {
                    backend: VueBackendRequest::Vdom,
                    ..Default::default()
                }),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "VUE-SSR-VAPOR-BACKEND" => {
            // Explicit force_vapor + an SSR product: refused at construction.
            let explicit = CompileRequest::new(
                vec![CompileProduct::RuntimeServer(
                    RuntimeProductRequest::default(),
                )],
                FrameworkCompileRequest::Vue(VueCompileRequest {
                    backend: VueBackendRequest::Vapor,
                    ..Default::default()
                }),
                None,
                None,
                None,
                false,
                false,
            );
            assert_refuses(
                cell_id,
                explicit,
                CompileRequestError::SsrVaporBackendUnsupported,
            );
            // Implicit half: an SSR product whose backend resolves to Vapor
            // via the source's own `<template vapor>` marker (not caught
            // until `resolve_vue_backend`, since construction runs before
            // parsing).
            let implicit = CompileRequest::new(
                vec![CompileProduct::RuntimeServer(
                    RuntimeProductRequest::default(),
                )],
                FrameworkCompileRequest::Vue(VueCompileRequest {
                    backend: VueBackendRequest::Inferred,
                    ..Default::default()
                }),
                None,
                None,
                None,
                false,
                false,
            )
            .expect("SSR + Inferred backend constructs (the marker is not known yet)");
            assert_eq!(
                implicit.resolve_vue_backend(true),
                Err(CompileRequestError::SsrVaporBackendUnsupported),
                "{cell_id}: an SSR request whose source marks <template vapor> must refuse \
                 at resolve_vue_backend, matching the explicit-backend refusal above"
            );
            Ok(())
        }
        "VUE-MACRO-LOCAL" | "VUE-MACRO-IMPORTED" => Err(
            "macro semantic resolution is a host-resolved input (VueExecutionInputs), \
             not a CompileRequest product/profile axis",
        ),
        "VUE-SCOPED-SLOTTED" => Err(
            "scoped/slotted CSS-variable semantics ride on block content + style-planner \
             input, not a distinct CompileRequest option",
        ),
        "VUE-CUSTOM-ELEMENT" => {
            let request = CompileRequest::new(
                base_vue_runtime_client(),
                FrameworkCompileRequest::Vue(VueCompileRequest {
                    is_custom_element: vec!["my-".to_string()],
                    script_custom_element: Some(true),
                    ..Default::default()
                }),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "VUE-TEMPLATE-OPTIONS" => {
            let request = CompileRequest::new(
                base_vue_runtime_client(),
                FrameworkCompileRequest::Vue(VueCompileRequest {
                    delimiters: Some(("[[".to_string(), "]]".to_string())),
                    comments: Some(true),
                    hoist_static: Some(true),
                    cache_handlers: Some(true),
                    ..Default::default()
                }),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "VUE-ASYNC-SETUP" => Err(
            "async setup / Suspense / hydration topology is script-content-driven codegen \
             behavior, not a CompileRequest construction option",
        ),
        "VUE-PUBLIC-API" => {
            let request = CompileRequest::new(
                vec![CompileProduct::PublicApi(PublicApiProductRequest::default())],
                FrameworkCompileRequest::Vue(VueCompileRequest::default()),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "VUE-TSC" => {
            let request = CompileRequest::new(
                vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
                FrameworkCompileRequest::Vue(VueCompileRequest::default()),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "VUE-DECLARATION" => {
            let request = CompileRequest::new(
                vec![CompileProduct::Declarations(
                    DeclarationProductRequest::default(),
                )],
                FrameworkCompileRequest::Vue(VueCompileRequest::default()),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "VUE-COMPAT-V2" => {
            let attempt = VueOptionAttempt {
                compat_config: Some(true),
                ..Default::default()
            };
            let err = attempt
                .into_request()
                .expect_err("compatConfig must refuse at VueOptionAttempt::into_request");
            match err {
                CompileRequestError::UnsupportedOption { option, .. } => assert_eq!(
                    option,
                    verter_compiler::compile_request::FrameworkOption::Vue(
                        verter_compiler::compile_request::VueOption::ParserOptionsCompatConfig
                    ),
                    "{cell_id}: wrong option named in the refusal"
                ),
                other => panic!("{cell_id}: expected UnsupportedOption, got {other:?}"),
            }
            Ok(())
        }
        "VUE-OTHER-VERSION" => Err(
            "compatibility-domain version pinning is a workspace/host-resolved fact, not a \
             CompileRequest construction option",
        ),

        // ── Svelte. ──
        "SVELTE-CLIENT-RUNES" | "SVELTE-CLIENT-LEGACY" => {
            let request = CompileRequest::new(
                vec![CompileProduct::RuntimeClient(
                    RuntimeProductRequest::default(),
                )],
                FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "SVELTE-SERVER-RUNES" | "SVELTE-SERVER-LEGACY" => {
            // Constructible today (no CompileRequest-level refusal — the
            // capability matrix's "unsupported today" is a downstream
            // carrier-execution gap, not a construction-time refusal); the
            // eventual Preview backend is a downstream runtime-codegen
            // concern, not a CompileRequest construction concern.
            let request = CompileRequest::new(
                vec![CompileProduct::RuntimeServer(
                    RuntimeProductRequest::default(),
                )],
                FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "SVELTE-COMPONENT" => {
            // The row's product_family lists RuntimeClient+RuntimeServer
            // together — a single request MAY carry both (an isomorphic
            // client+server compile of the same component), each a
            // distinct ProductKind so neither refuses the other.
            let request = CompileRequest::new(
                vec![
                    CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
                    CompileProduct::RuntimeServer(RuntimeProductRequest::default()),
                ],
                FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "SVELTE-MODULE" => {
            // Both `ModuleCompileOptions` fields this capability gates
            // (`generate`, `experimental.async`) must refuse at
            // `SvelteOptionAttempt::into_request`, naming the SAME
            // `SvelteModule` capability cell — the option itself admits
            // fine in isolation (`SvelteOption::class()` still classifies
            // both `SupportedCanonical`), but the module-compilation
            // product family they gate is `unsupported fail-closed`.
            let generate_err = SvelteOptionAttempt {
                generate_module: Some(true),
                ..Default::default()
            }
            .into_request()
            .expect_err("generate must refuse at SvelteOptionAttempt::into_request");
            match generate_err {
                CompileRequestError::UnsupportedOption { option, capability } => {
                    assert_eq!(
                        option,
                        verter_compiler::compile_request::FrameworkOption::Svelte(
                            verter_compiler::compile_request::SvelteOption::ModuleGenerate
                        ),
                        "{cell_id}: wrong option named in the generate refusal"
                    );
                    assert_eq!(
                        capability,
                        Some(verter_compiler::compile_request::CapabilityCell::SvelteModule),
                        "{cell_id}: generate refusal must name the SvelteModule capability cell"
                    );
                }
                other => panic!("{cell_id}: expected UnsupportedOption, got {other:?}"),
            }
            let async_err = SvelteOptionAttempt {
                experimental_async: Some(true),
                ..Default::default()
            }
            .into_request()
            .expect_err("experimental.async must refuse at SvelteOptionAttempt::into_request");
            match async_err {
                CompileRequestError::UnsupportedOption { option, capability } => {
                    assert_eq!(
                        option,
                        verter_compiler::compile_request::FrameworkOption::Svelte(
                            verter_compiler::compile_request::SvelteOption::ModuleExperimentalAsync
                        ),
                        "{cell_id}: wrong option named in the experimental.async refusal"
                    );
                    assert_eq!(
                        capability,
                        Some(verter_compiler::compile_request::CapabilityCell::SvelteModule),
                        "{cell_id}: experimental.async refusal must name the SvelteModule \
                         capability cell"
                    );
                }
                other => panic!("{cell_id}: expected UnsupportedOption, got {other:?}"),
            }
            Ok(())
        }
        "SVELTE-SEMANTIC-CORE" => Err(
            "snippets/blocks/effects/bindings/etc. are script-content-driven runtime \
             semantics, not CompileRequest construction options",
        ),
        "SVELTE-CUSTOM-ELEMENT" => {
            let request = CompileRequest::new(
                vec![CompileProduct::RuntimeClient(
                    RuntimeProductRequest::default(),
                )],
                FrameworkCompileRequest::Svelte(SvelteCompileRequest {
                    custom_element: Some(true),
                    custom_element_descriptor: Some(
                        AdmittedSvelteCustomElementDescriptor::default(),
                    ),
                    ..Default::default()
                }),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "SVELTE-ASYNC-EXPERIMENTAL" => Err(
            "this row's RuntimeClient+RuntimeServer/'experimental.async; boundaries; \
             hydration' axes name component-level async/boundary runtime reachability, a \
             script-content-driven runtime behavior fact like SVELTE-HYDRATION/SVELTE-\
             SEMANTIC-CORE — not a CompileRequest construction option. The identically-\
             named 'ModuleCompileOptions.experimental.async' compiler OPTION is a \
             different, ModuleJavaScript-product-family axis covered under SVELTE-MODULE \
             (it refuses there, unconditionally, because no module-compilation product is \
             claimed — independent of whether this row's runtime capability is ever built)",
        ),
        "SVELTE-HYDRATION" => Err(
            "hydration topology is a client/server pairing behavior of the RuntimeClient/\
             RuntimeServer products already covered by SVELTE-CLIENT-RUNES/SVELTE-SERVER-\
             RUNES, not a distinct CompileRequest option",
        ),
        "SVELTE-PUBLIC-API" => {
            let request = CompileRequest::new(
                vec![CompileProduct::PublicApi(PublicApiProductRequest::default())],
                FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "SVELTE-TSC" => {
            let request = CompileRequest::new(
                vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
                FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "SVELTE-DECLARATION" => {
            let request = CompileRequest::new(
                vec![CompileProduct::Declarations(
                    DeclarationProductRequest::default(),
                )],
                FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
                None,
                None,
                None,
                false,
                false,
            );
            assert_constructs(cell_id, request);
            Ok(())
        }
        "SVELTE-HMR" => {
            let attempt = SvelteOptionAttempt {
                hmr: Some(true),
                ..Default::default()
            };
            let err = attempt
                .into_request()
                .expect_err("hmr must refuse at SvelteOptionAttempt::into_request");
            match err {
                CompileRequestError::UnsupportedOption { option, .. } => assert_eq!(
                    option,
                    verter_compiler::compile_request::FrameworkOption::Svelte(
                        verter_compiler::compile_request::SvelteOption::CompileOptionsHmr
                    ),
                    "{cell_id}: wrong option named in the refusal"
                ),
                other => panic!("{cell_id}: expected UnsupportedOption, got {other:?}"),
            }
            Ok(())
        }
        "SVELTE-COMPAT-API4" => {
            let attempt = SvelteOptionAttempt {
                compatibility_component_api: Some(true),
                ..Default::default()
            };
            let err = attempt.into_request().expect_err(
                "compatibility.componentApi must refuse at SvelteOptionAttempt::into_request",
            );
            match err {
                CompileRequestError::UnsupportedOption { option, .. } => assert_eq!(
                    option,
                    verter_compiler::compile_request::FrameworkOption::Svelte(
                        verter_compiler::compile_request::SvelteOption::CompileOptionsCompatibilityComponentApi
                    ),
                    "{cell_id}: wrong option named in the refusal"
                ),
                other => panic!("{cell_id}: expected UnsupportedOption, got {other:?}"),
            }
            Ok(())
        }
        "SVELTE-OFFICIAL-AST" => Err(
            "modernAst is an official-oracle-only extra artifact (TestOnly class on \
             SvelteOption) — never a public product; there is nothing for a production \
             CompileRequest to construct or refuse",
        ),
        "SVELTE-OTHER-VERSION" => Err(
            "compatibility-domain version pinning is a workspace/host-resolved fact, not a \
             CompileRequest construction option",
        ),

        other => panic!(
            "unregistered capability-matrix cell_id '{other}' — add a verify_cell arm \
             (a construction/refusal probe or a documented exemption) before this guard \
             can pass"
        ),
    }
}

/// Every row's disposition proven or explicitly exempted; the exhaustive
/// `verify_cell` match's `other => panic!` arm is the actual coverage
/// enforcement — this test just drives it over the real committed rows.
#[test]
fn every_tsv_row_has_a_registered_verification() {
    let rows = read_matrix_rows();
    assert!(
        rows.len() >= 30,
        "capability-matrix.tsv parsed suspiciously few rows ({}); check the TSV path/format",
        rows.len()
    );
    let mut exempt = Vec::new();
    for row in &rows {
        match verify_cell(&row.cell_id) {
            Ok(()) => {}
            Err(reason) => exempt.push((row.cell_id.clone(), reason)),
        }
    }

    // CLOSED allowlist of every cell_id this file exempts from a
    // construction/refusal probe, plus the disposition each is expected to
    // carry. Adding a NEW exemption (or a disposition drifting under an
    // EXISTING one) requires touching this list explicitly — the exact
    // failure mode a silent, unaudited exemption would otherwise produce.
    // Every entry's reasoning is that the row's claim lives entirely
    // outside `CompileRequest`'s own construction-time option/product
    // surface (parse diagnostics, macro/TS-provider resolution, version
    // pinning, official-oracle-only artifacts, or script-content-driven
    // runtime behavior). There is NO `unsupported fail-closed` exemption —
    // `SVELTE-MODULE` is proven directly as a refusal (see its match arm
    // above); `SVELTE-ASYNC-EXPERIMENTAL` is `experimental`, not
    // `unsupported fail-closed`, and exempted because its axis is runtime
    // reachability, not a construction-time option.
    const EXPECTED_EXEMPTIONS: &[(&str, &str)] = &[
        ("VUE-PARSE-LOCAL", "supported"),
        ("VUE-MACRO-LOCAL", "supported"),
        ("VUE-MACRO-IMPORTED", "projection-required"),
        ("VUE-SCOPED-SLOTTED", "supported"),
        ("VUE-ASYNC-SETUP", "supported"),
        ("VUE-OTHER-VERSION", "version-incompatible"),
        ("SVELTE-PARSE-LOCAL", "supported"),
        ("SVELTE-SEMANTIC-CORE", "supported"),
        ("SVELTE-HYDRATION", "supported"),
        ("SVELTE-ASYNC-EXPERIMENTAL", "experimental"),
        ("SVELTE-OFFICIAL-AST", "not applicable"),
        ("SVELTE-OTHER-VERSION", "version-incompatible"),
    ];

    let exempt_ids: std::collections::BTreeSet<&str> =
        exempt.iter().map(|(id, _)| id.as_str()).collect();
    let expected_ids: std::collections::BTreeSet<&str> =
        EXPECTED_EXEMPTIONS.iter().map(|(id, _)| *id).collect();
    let unexpected: Vec<&&str> = exempt_ids.difference(&expected_ids).collect();
    assert!(
        unexpected.is_empty(),
        "these cell_ids are exempted but NOT on the allowlist — a new exemption \
         must be added to EXPECTED_EXEMPTIONS explicitly, never silently: {unexpected:?}"
    );
    let missing: Vec<&&str> = expected_ids.difference(&exempt_ids).collect();
    assert!(
        missing.is_empty(),
        "these allowlisted cell_ids are no longer exempted (now proven directly) — \
         remove them from EXPECTED_EXEMPTIONS: {missing:?}"
    );

    // Every exemption must carry the disposition the allowlist expects —
    // an `unsupported fail-closed` row silently exempted under a
    // `supported`-looking reason (or vice versa) is exactly the class of
    // bug this check exists to catch.
    for (cell_id, expected_disposition) in EXPECTED_EXEMPTIONS {
        let row = rows
            .iter()
            .find(|r| r.cell_id == *cell_id)
            .unwrap_or_else(|| panic!("{cell_id}: allowlisted but absent from the TSV"));
        assert_eq!(
            row.target_disposition, *expected_disposition,
            "{cell_id}: TSV disposition drifted from what this exemption allowlist expects — \
             re-audit whether the exemption is still correct"
        );
    }

    // NO `unsupported fail-closed` row may be exempted — every one
    // (currently: VUE-SSR-VAPOR-BACKEND, VUE-COMPAT-V2, SVELTE-MODULE,
    // SVELTE-HMR, SVELTE-COMPAT-API4) must be an exhaustively listed
    // construction/refusal probe above, never an `Err(reason)` arm.
    for row in &rows {
        if row.target_disposition == "unsupported fail-closed" {
            assert!(
                !exempt_ids.contains(row.cell_id.as_str()),
                "{}: an 'unsupported fail-closed' row must be proven as a refusal, \
                 not merely exempted",
                row.cell_id
            );
        }
    }
}

/// The `cell_id` a refusal quotes is the committed matrix's own
/// identifier, in both directions: a variant naming a row the matrix does
/// not have fails, and a matrix row no variant names fails too.
///
/// `CapabilityCell::cell_id` is the SINGLE owner of that mapping — a
/// refusal naming an unsupported capability quotes the matrix row a caller
/// can go read, and every transport renders it from there rather than
/// keeping its own copy. This is the pin that keeps the two in step; the
/// exhaustiveness itself is the `match`'s.
///
/// Mutation recipes:
/// - Change one `cell_id` arm (`SvelteHmr` to `"SVELTE-HOT-MODULE"`): this
///   reports the invented id and the unnamed row.
/// - Delete one entry from `ALL_CAPABILITY_CELLS`: the count assertion
///   fails before the set comparison can hide the gap.
/// - Point two variants at one id (`SvelteHmr` to `"SVELTE-MODULE"`): the
///   duplicate assertion reports it, and the unnamed row does too.
#[test]
fn cell_ids_match_the_committed_matrix() {
    use std::collections::BTreeSet;
    use verter_compiler::compile_request::ALL_CAPABILITY_CELLS;

    let committed: BTreeSet<String> = read_matrix_rows()
        .into_iter()
        .map(|row| row.cell_id)
        .collect();
    assert_eq!(
        committed.len(),
        ALL_CAPABILITY_CELLS.len(),
        "the matrix commits {} rows, {} variants are listed",
        committed.len(),
        ALL_CAPABILITY_CELLS.len()
    );

    let named: BTreeSet<String> = ALL_CAPABILITY_CELLS
        .iter()
        .map(|cell| cell.cell_id().to_string())
        .collect();
    assert_eq!(
        named.len(),
        ALL_CAPABILITY_CELLS.len(),
        "two variants name the same matrix cell id"
    );

    let invented: Vec<_> = named.difference(&committed).collect();
    assert!(
        invented.is_empty(),
        "variants name cell ids the matrix does not have: {invented:?}"
    );
    let unnamed: Vec<_> = committed.difference(&named).collect();
    assert!(
        unnamed.is_empty(),
        "matrix rows no variant names: {unnamed:?}"
    );
}
