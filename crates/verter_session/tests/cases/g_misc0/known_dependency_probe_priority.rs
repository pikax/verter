//! The host's default known-file dependency probe order: carrier
//! MEMBERSHIP follows the composed admission, carrier PRIORITY is the
//! adapters' declared probe rank, and a same-stem carrier collision
//! resolves through the resolver by that declared priority — never by the
//! classifier's longest-suffix matching order nor by a derived sort.

use verter_resolution::path_utils::resolve_known_dependency_id;
use verter_session::{HostConfig, VerterHost};
use verter_session_query::resolution::build_known_file_index;

#[test]
fn the_default_host_probes_scripts_then_carriers_in_catalog_order() {
    let host = VerterHost::new_standalone(HostConfig::default());
    assert_eq!(
        host.known_dependency_extensions(),
        ["", ".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs", ".cts", ".cjs", ".vue", ".svelte",]
            .map(String::from)
            .to_vec(),
        "the bare specifier and scripts probe before carriers, which follow the \
         adapters' declared probe rank"
    );
}

#[test]
fn a_same_stem_carrier_collision_resolves_by_catalog_priority() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let known_index = build_known_file_index(&[
        "/src/Widget.vue".to_string(),
        "/src/Widget.svelte".to_string(),
    ]);
    let resolved = resolve_known_dependency_id(
        "/src/Consumer.ts",
        "./Widget",
        &known_index,
        &host.known_dependency_extensions(),
    );
    assert_eq!(
        resolved.as_deref(),
        Some("/src/Widget.vue"),
        "the resolver is first-match-wins, so the same-stem collision must resolve by \
         the adapters' declared probe rank — not by the classifier's suffix-length \
         order, which would select the longer `.svelte` suffix"
    );
}

/// The probe order is a RESOLUTION priority, so it must not be derived by
/// sorting extensions — a derived order is what let suffix length flip the
/// same-stem winner. Both orders contain the same admitted carriers.
#[test]
fn the_probe_priority_is_independent_of_classifier_matching_order() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let classifier_order: Vec<String> = host
        .language_classifier()
        .carrier_extensions()
        .into_iter()
        .map(|extension| format!(".{extension}"))
        .collect();
    assert_eq!(
        classifier_order,
        [".svelte", ".vue"].map(String::from).to_vec(),
        "the classifier's carrier order is longest-suffix-first"
    );
    let probe_carriers: Vec<String> = host
        .known_dependency_extensions()
        .into_iter()
        .filter(|extension| classifier_order.contains(extension))
        .collect();
    assert_eq!(
        probe_carriers,
        [".vue", ".svelte"].map(String::from).to_vec()
    );
}
