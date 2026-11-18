//! Typed product lookup: every product the Vue projection backend can
//! produce is reachable through `product::<P>()` and equals what the
//! backend's own accessor returns for the same admitted parse.

use std::fmt::Debug;

use verter_compiler::framework_common::vue_projection_backend::{
    CallerAndSetupPropsContract, PropsProjection, VueProductInput, VueProjectionProduct,
};
use verter_compiler::framework_common::{SetupProjectionRefusal, VueProjectionBackend};

use super::{registered_artifact, KITCHEN_SINK};

const CANONICAL_ID: &str = "file:///kitchen-sink.vue";

/// The typed lookup of `P` equals the accessor's result `direct`, success
/// or refusal alike.
fn assert_product_matches_accessor<P>(
    direct: Result<P, SetupProjectionRefusal>,
    input: &VueProductInput<'_>,
) where
    P: VueProjectionProduct + PartialEq + Debug,
{
    assert_eq!(VueProjectionBackend.product::<P>(input), direct);
}

#[test]
fn every_product_lookup_equals_its_accessor_on_the_kitchen_sink() {
    let artifact = registered_artifact(CANONICAL_ID, KITCHEN_SINK);
    let input = VueProductInput {
        source: KITCHEN_SINK,
        artifact: &artifact,
        canonical_id: CANONICAL_ID,
        custom_elements: None,
    };
    let backend = VueProjectionBackend;
    let (source, id) = (KITCHEN_SINK, CANONICAL_ID);

    assert_product_matches_accessor(backend.script_projection(source, &artifact), &input);
    assert_product_matches_accessor(backend.options_projection(source, &artifact), &input);
    assert_product_matches_accessor(backend.attribute_operations(source, &artifact, id), &input);
    assert_product_matches_accessor(backend.component_uses(source, &artifact, id), &input);
    assert_product_matches_accessor(
        backend.component_resolution(source, &artifact, id, None),
        &input,
    );
    assert_product_matches_accessor(backend.event_transport(source, &artifact, id), &input);
    assert_product_matches_accessor(backend.advanced_generic_uses(source, &artifact, id), &input);
    assert_product_matches_accessor(backend.props(source, &artifact, id), &input);
    assert_product_matches_accessor(backend.binding_views(source, &artifact), &input);
    assert_product_matches_accessor(backend.binding_usage(source, &artifact), &input);
    assert_product_matches_accessor(backend.public_type_dependencies(source, &artifact), &input);
    assert_product_matches_accessor(backend.public_constructor(source, &artifact), &input);
}

#[test]
fn component_resolution_lookup_reads_the_selected_custom_elements() {
    let artifact = registered_artifact(CANONICAL_ID, KITCHEN_SINK);
    let custom_elements = vec!["x-".to_string()];
    let input = VueProductInput {
        source: KITCHEN_SINK,
        artifact: &artifact,
        canonical_id: CANONICAL_ID,
        custom_elements: Some(&custom_elements),
    };
    assert_product_matches_accessor(
        VueProjectionBackend.component_resolution(
            KITCHEN_SINK,
            &artifact,
            CANONICAL_ID,
            Some(&custom_elements),
        ),
        &input,
    );
}

#[test]
fn product_lookup_refuses_source_the_artifact_does_not_carry() {
    let artifact = registered_artifact(CANONICAL_ID, KITCHEN_SINK);
    let input = VueProductInput {
        source: "<script setup></script>",
        artifact: &artifact,
        canonical_id: CANONICAL_ID,
        custom_elements: None,
    };
    let refusal = VueProjectionBackend
        .product::<(PropsProjection, CallerAndSetupPropsContract)>(&input)
        .unwrap_err();
    assert_eq!(refusal, SetupProjectionRefusal::MissingParse);
}
