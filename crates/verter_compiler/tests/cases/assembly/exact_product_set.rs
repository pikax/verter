//! `publish` returns exactly the requested product set — never an extra
//! virtual artifact, never a missing mapping product, never a default-
//! constructed map standing in for "not requested".

use verter_compiler::assembly::{ArtifactContribution, FragmentDialect, ProductPlan};
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest, ProductKind,
    RuntimeProductRequest, VueCompileRequest,
};

fn request(products: Vec<CompileProduct>) -> CompileRequest {
    CompileRequest::new(
        products,
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("test request constructs")
}

fn contribution(kind: ProductKind) -> ArtifactContribution<'static> {
    ArtifactContribution {
        kind,
        fragments: Vec::new(),
        code: "export default {}".to_string(),
        emitted_imports: Vec::new(),
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    }
}

#[test]
fn runtime_client_request_publishes_exactly_that_artifact_never_ide_or_script() {
    let plan = ProductPlan::from_request(&request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]));
    let set =
        verter_compiler::assembly::publish(&plan, vec![contribution(ProductKind::RuntimeClient)])
            .expect("publish succeeds");
    assert_eq!(set.artifacts().len(), 1);
    assert!(set.artifact(ProductKind::RuntimeClient).is_some());
    assert!(set.artifact(ProductKind::IdeCompanion).is_none());
    assert!(set.artifact(ProductKind::Declarations).is_none());
}

#[test]
fn runtime_client_with_requested_runtime_map_publishes_it() {
    let plan = ProductPlan::from_request(&request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest {
            runtime_source_map: true,
            ..Default::default()
        },
    )]));
    let mut c = contribution(ProductKind::RuntimeClient);
    c.runtime_source_map = Some("{\"version\":3}".to_string());
    let set = verter_compiler::assembly::publish(&plan, vec![c]).expect("publish succeeds");
    assert!(set
        .artifact(ProductKind::RuntimeClient)
        .unwrap()
        .runtime_source_map()
        .is_some());
}

#[test]
fn runtime_client_without_requested_runtime_map_is_a_true_none() {
    let plan = ProductPlan::from_request(&request(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]));
    let set =
        verter_compiler::assembly::publish(&plan, vec![contribution(ProductKind::RuntimeClient)])
            .expect("publish succeeds");
    assert!(
        set.artifact(ProductKind::RuntimeClient)
            .unwrap()
            .runtime_source_map()
            .is_none(),
        "an unrequested runtime map must be None, never a default-constructed empty map"
    );
}

#[test]
fn ide_companion_request_without_its_projection_map_refuses_the_whole_publish() {
    let plan = ProductPlan::from_request(&request(vec![CompileProduct::IdeCompanion(
        IdeProductRequest::default(),
    )]));
    let err =
        verter_compiler::assembly::publish(&plan, vec![contribution(ProductKind::IdeCompanion)])
            .unwrap_err();
    assert_eq!(
        err,
        verter_compiler::assembly::AssemblyRefusal::MissingRequiredSourceProjectionMap {
            kind: ProductKind::IdeCompanion
        }
    );
}

#[test]
fn ide_companion_request_with_its_projection_map_publishes_both_atomically() {
    let plan = ProductPlan::from_request(&request(vec![CompileProduct::IdeCompanion(
        IdeProductRequest::default(),
    )]));
    let mut c = contribution(ProductKind::IdeCompanion);
    c.source_projection_map = Some("{\"version\":3}".to_string());
    let set = verter_compiler::assembly::publish(&plan, vec![c]).expect("publish succeeds");
    let artifact = set.artifact(ProductKind::IdeCompanion).unwrap();
    assert!(artifact.source_projection_map().is_some());
}

#[test]
fn multi_product_request_publishes_exactly_those_products_and_nothing_else() {
    let plan = ProductPlan::from_request(&request(vec![
        CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
        CompileProduct::Declarations(Default::default()),
    ]));
    let set = verter_compiler::assembly::publish(
        &plan,
        vec![
            contribution(ProductKind::RuntimeClient),
            contribution(ProductKind::Declarations),
        ],
    )
    .expect("publish succeeds");
    assert_eq!(set.artifacts().len(), 2);
    assert!(set.artifact(ProductKind::RuntimeClient).is_some());
    assert!(set.artifact(ProductKind::Declarations).is_some());
    assert!(set.artifact(ProductKind::IdeCompanion).is_none());
    assert!(set.artifact(ProductKind::PublicApi).is_none());
    assert!(set.artifact(ProductKind::Analysis).is_none());
}
