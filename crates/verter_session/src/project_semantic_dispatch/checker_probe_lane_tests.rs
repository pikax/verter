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

/// The project a probe module is checked in: its sibling files, when set
/// the `compilerOptions` of the tsconfig that owns them (TypeScript's
/// defaults with `strict` otherwise), and when set the ambient library the
/// project's global declarations come from (none otherwise).
#[derive(Clone, Copy, Default)]
pub(super) struct ProbeProject<'a> {
    /// `(file name, source)` modules beside the probe module, in program
    /// order.
    pub(super) files: &'a [(&'a str, &'a str)],
    /// The owning tsconfig's `compilerOptions` object.
    pub(super) compiler_options: Option<&'a str>,
    /// The ambient library registered against the probe's project — the
    /// global `String`, `Array`, … declarations a checker reads from its
    /// `lib` files.
    pub(super) ambient_lib: Option<&'a str>,
}

const PROBE_ROOT: &str = "/wb";
const PROBE_FILE: &str = "/wb/checker_probe.ts";

/// Answer `probe` in TYPE position over a module of `source` and hand the
/// reduced node to `read`.
pub(super) fn with_probe<R>(
    source: &str,
    probe: &str,
    read: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R,
) -> R {
    with_probe_in(ProbeProject::default(), source, probe, read)
}

/// [`with_probe`] in `project`.
pub(super) fn with_probe_in<R>(
    project: ProbeProject<'_>,
    source: &str,
    probe: &str,
    read: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R,
) -> R {
    let host = probe_host(project);
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
    let result = flow_return_of(&host, "__checker_probe")
        .unwrap_or_else(|| panic!("the probe `{probe}` produced no flow-return result"));
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    // A read of a global the ambient library declares is scoped by the
    // probe module's project, as the member access in its body would be.
    let _demand_scope = project.ambient_lib.map(|_| {
        super::LexicalDemandScopeGuard::push(&dispatch.lexical_demand_scope, Arc::from(PROBE_FILE))
    });
    let node = dispatch
        .normalize_node_keeping_declaration_refs_for_tests(
            result.return_type(),
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .into_complete_node()
        .unwrap_or_else(|| panic!("the probe `{probe}` reduced to a partial demand"));
    read(&dispatch, node)
}

/// The degradation the body-derived return of `function` in a module of
/// `source`, checked in `project`, carries — `Err(())` when it produced
/// no value.
pub(super) fn degradation_in(
    project: ProbeProject<'_>,
    source: &str,
    function: &str,
) -> Result<Option<crate::semantic_query::FlowReturnDegradation>, ()> {
    let host = probe_host(project);
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        PROBE_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(source),
        crate::FileLanguage::script_ts(),
    );
    flow_return_of(&host, function)
        .map(|result| result.degradation())
        .ok_or(())
}

/// The audited body-derived return of the probe module's `function`.
fn flow_return_of(
    host: &Arc<crate::VerterHost>,
    function: &str,
) -> Option<Arc<crate::semantic_query::FlowReturnResult>> {
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(PROBE_FILE),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(function),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    host.get_flow_return_type_with_audit(
        &identity,
        crate::semantic_query::ReturnProjectionDemand::whole_return(),
    )
    .as_result()
    .ok()
    .cloned()
}

/// A host for `project` with its sibling files and ambient library
/// registered; the probe module itself is not yet upserted.
fn probe_host(project: ProbeProject<'_>) -> Arc<crate::VerterHost> {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host;
    // A registered ambient library attaches to a configured project, so a
    // probe that reads one is checked in the tsconfig project it belongs to.
    let compiler_options = project
        .compiler_options
        .or(project.ambient_lib.map(|_| r#"{ "strict": true }"#));
    let host = match compiler_options {
        None => make_audit_host(),
        Some(options) => Arc::new(crate::VerterHost::new_standalone_with_tsconfig_projects(
            crate::HostConfig {
                analysis_level: crate::types::AnalysisLevel::Full,
                audit_enabled: true,
                footprint_capture: false,
                ..crate::HostConfig::default()
            },
            &[(
                PROBE_ROOT,
                &format!(r#"{{ "compilerOptions": {options} }}"#),
            )],
        )),
    };
    if let Some(lib) = project.ambient_lib {
        host.workspace()
            .register_ambient_lib(verter_workspace::AmbientLibSpec {
                project_id: None,
                canonical_id: Arc::from("lib.probe.d.ts"),
                source: Arc::from(lib),
            })
            .expect("the probe's ambient library registers against its project");
    }
    for (name, file_source) in project.files {
        let path = format!("{PROBE_ROOT}/{name}");
        // A sibling is classified by its name, as the host classifies it: a
        // `.d.ts` sibling is a declaration file.
        let language = crate::LanguageRegistry::global()
            .classify_static(&path)
            .static_resolution();
        crate::u6_flow_shape_corpus_tests::upsert(&host, &path, file_source, language);
    }
    host
}

/// Every `(probe, checker print)` pair whose live answer does not match the
/// print structurally, each with what the lane measured.
pub(super) fn mismatches(source: &str, rows: &[(&str, &str)]) -> Vec<String> {
    mismatches_in(ProbeProject::default(), source, rows)
}

/// [`mismatches`] in `project`.
pub(super) fn mismatches_in(
    project: ProbeProject<'_>,
    source: &str,
    rows: &[(&str, &str)],
) -> Vec<String> {
    rows.iter()
        .filter_map(|(probe, checker)| {
            let expected = checker_syntax::parse(checker)
                .unwrap_or_else(|err| panic!("the checker print `{checker}` must parse: {err}"));
            with_probe_in(project, source, probe, |dispatch, node| {
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

/// Every `(probe, checker type)` pair whose EVALUATED value does not match
/// the checker type structurally. The lane keeps a declaration application
/// by name, as the checker prints it (`Partial<Face | Obj>`); this reads
/// what the application evaluates to under a publication demand, compared
/// with a type the checker holds mutually assignable to it.
pub(super) fn evaluated_mismatches(source: &str, rows: &[(&str, &str)]) -> Vec<String> {
    rows.iter()
        .filter_map(|(probe, checker)| {
            let expected = checker_syntax::parse(checker)
                .unwrap_or_else(|err| panic!("the checker type `{checker}` must parse: {err}"));
            with_probe(source, probe, |dispatch, node| {
                let value = dispatch
                    .normalize_node_for_structural_fact_demand(
                        node,
                        ProjectionReductionContext::published(ProjectionMode::Expanded),
                    )
                    .into_complete_node()
                    .unwrap_or_else(|| panic!("the probe `{probe}` evaluated to a partial demand"));
                (!checker_syntax::matches_node(dispatch, value, &expected, 0)).then(|| {
                    format!(
                        "`{probe}`: the checker holds `{checker}`, the lane evaluated `{}`",
                        render_node(dispatch, value, 0)
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
