//! The checker-probe lane the signature and relation tests compare against:
//! a probe in TYPE position over a module, read back through the public
//! audited flow-return boundary and reduced to the altitude the checker
//! prints — the signature corpus's own observation lane — then matched
//! structurally against a recorded TypeScript print.

use std::sync::Arc;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ProjectionMode, ProjectionReductionContext, SemanticNodeData, SemanticNodeId,
};
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};

const PROBE_FILE: &str = "/wb/checker_probe.ts";

/// Answer `probe` in TYPE position over a module of `source` and hand the
/// reduced node to `read`.
pub(super) fn with_probe<R>(
    source: &str,
    probe: &str,
    read: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R,
) -> R {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host;
    let host = make_audit_host();
    let module = format!(
        "{source}\nexport function __checker_probe() {{ \
            const __probe: {probe} = null as any; \
            return __probe; \
        }}\n"
    );
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        PROBE_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(&module),
        crate::FileLanguage::script_ts(),
    );
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(PROBE_FILE),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("__checker_probe"),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let carrier = host.get_flow_return_type_with_audit(
        &identity,
        crate::semantic_query::ReturnProjectionDemand::whole_return(),
    );
    let result = carrier
        .as_result()
        .unwrap_or_else(|_| panic!("the probe `{probe}` produced no flow-return result"));
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let node = dispatch
        .normalize_node_keeping_declaration_refs_for_tests(
            result.return_type(),
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .into_complete_node()
        .unwrap_or_else(|| panic!("the probe `{probe}` reduced to a partial demand"));
    read(&dispatch, node)
}

/// Every `(probe, checker print)` pair whose live answer does not match the
/// print structurally, each with what the lane measured.
pub(super) fn mismatches(source: &str, rows: &[(&str, &str)]) -> Vec<String> {
    rows.iter()
        .filter_map(|(probe, checker)| {
            let expected = checker_syntax::parse(checker)
                .unwrap_or_else(|err| panic!("the checker print `{checker}` must parse: {err}"));
            with_probe(source, probe, |dispatch, node| {
                (!checker_syntax::matches_node(dispatch, node, &expected, 0)).then(|| {
                    format!(
                        "`{probe}`: the checker answers `{checker}`, the lane measured `{}`",
                        render_node(dispatch, node, 0)
                    )
                })
            })
        })
        .collect()
}

/// The label of each element of the tuple `probe` settles on — the
/// parameter NAMES a `Parameters<…>` answer carries.
pub(super) fn tuple_labels(source: &str, probe: &str) -> Vec<Option<String>> {
    with_probe(source, probe, |dispatch, node| {
        match dispatch.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Tuple { elements, .. }) => elements
                .iter()
                .map(|element| element.label.as_deref().map(str::to_owned))
                .collect(),
            _ => panic!(
                "`{probe}` must settle on a tuple, measured `{}`",
                render_node(dispatch, node, 0)
            ),
        }
    })
}

/// Whether the lane still holds `probe` as an undecided conditional — the
/// relation behind its branch selection gave no verdict.
pub(super) fn holds_deferred_conditional(source: &str, probe: &str) -> bool {
    with_probe(source, probe, |dispatch, node| {
        matches!(
            dispatch.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Conditional { .. })
        )
    })
}
