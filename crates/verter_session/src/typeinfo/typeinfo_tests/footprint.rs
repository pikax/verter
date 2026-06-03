//! @ai-generated - Typeinfo request-footprint contracts for synthetic
//! component-shaped fixtures.

use super::support::*;

#[test]
#[ignore = "typeinfo currently fails to attach a request footprint under the audit-passive-observer footprint-attachment pipeline; the contract is that every audited typeinfo request must produce a footprint when footprint_capture=true on HostConfig. Keep as the future footprint-attachment-on-named-symbol contract once the footprint-attachment pipeline is wired into this resolver path."]
fn typeinfo_footprint_is_attached_for_named_symbol_request() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/component-types.ts", COMPONENT_TYPES);

    let (_expr, record) = resolve_expr(
        &host,
        "/fixtures/component-types.ts",
        "PrimitiveSurface",
        &[],
        ProjectionMode::Expanded,
    );

    record
        .footprint
        .as_ref()
        .expect("typeinfo requests must attach a footprint when footprint_capture=true");
}

#[test]
#[ignore = "typeinfo currently records the scratch/owner footprint but does not attribute projected imported indexed-access members precisely; keep as the future demand-boundary contract"]
fn typeinfo_footprint_reports_requested_import_and_excludes_unprojected_branch() {
    let host = make_host_with_footprint();
    upsert_ts(&host, "/fixtures/needed.ts", FOOTPRINT_NEEDED);
    upsert_ts(&host, "/fixtures/unused.ts", FOOTPRINT_UNUSED);
    upsert_ts(&host, "/fixtures/owner.ts", FOOTPRINT_OWNER);

    let record = host
        .evaluate_type_expression_with_audit(EvaluateTypeExpressionRequest {
            scope: "/fixtures/owner.ts".to_string(),
            expression: "Surface['keep']".to_string(),
            extra_imports: Vec::new(),
            mode: ProjectionMode::Expanded,
            cacheable: false,
        })
        .audit()
        .clone();
    let footprint = record
        .footprint
        .as_ref()
        .expect("typeinfo host requests with footprint_capture=true must attach a footprint");
    let declared: Vec<String> = footprint
        .declared_dependency_files()
        .iter()
        .map(|path| path.to_string())
        .collect();

    assert!(
        declared.iter().any(|path| path == "/fixtures/needed.ts"),
        "projecting Surface['keep'] must touch NeededPayload; got {declared:?}"
    );
    assert!(
        !declared.iter().any(|path| path == "/fixtures/unused.ts"),
        "projecting Surface['keep'] must not touch skip?: UnusedPayload; got {declared:?}"
    );
}
