//! Differential tests between the two template render routes.
//!
//! Compiles a corpus through the in-place renderers and through the target
//! executors and reports the differences, so progress on each target is a
//! count of matching cases.

mod compare;
mod corpus;
mod correction_probes;
mod expected;
mod text_entity_tests;
mod vapor_loop_sources;
mod vapor_render_contracts;

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

use oxc_allocator::Allocator;
use verter_compiler::compile::render_route::{with_route, RenderRoute, NOT_IMPLEMENTED_PREFIX};
use verter_compiler::compile::types::VueExecutionInputs;
use verter_compiler::compile::{compile_with_parsed, VueMacroSemanticInput};
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, CompileRequestError, FrameworkCompileRequest, ProductKind,
    RuntimeProductRequest, VueBackendRequest, VueCompileRequest,
};

use compare::{compare_views, render_diff, view_of, CompiledView, Difference};
use corpus::Case;
use expected::ExpectedKind;

/// The render targets the routes are compared on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    ClientTree,
    Server,
    FineGrained,
}

impl Target {
    const ALL: [Target; 3] = [Target::ClientTree, Target::Server, Target::FineGrained];

    fn label(self) -> &'static str {
        match self {
            Target::ClientTree => "client tree",
            Target::Server => "server",
            Target::FineGrained => "fine-grained client",
        }
    }

    /// The modes this target is compared in. Server output has no inline
    /// mode, and the request layer refuses inline fine-grained output before
    /// either route runs, so only the client tree is compared inline.
    fn modes(self) -> &'static [Mode] {
        match self {
            Target::Server | Target::FineGrained => &[Mode::Separate],
            Target::ClientTree => &[Mode::Separate, Mode::Inline],
        }
    }
}

/// Whether the render function stands alone or sits inside `setup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Separate,
    Inline,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Separate => "separate",
            Mode::Inline => "inline",
        }
    }
}

/// What one route did with one case.
#[derive(Debug)]
enum RouteResult {
    Compiled(Box<CompiledView>),
    /// The route has no implementation for this case.
    NotImplemented,
    /// The request layer refused the request.
    Refused(CompileRequestError),
    Panicked(String),
}

fn request_for(target: Target, mode: Mode) -> CompileRequest {
    let runtime = RuntimeProductRequest {
        inline: Some(mode == Mode::Inline),
        runtime_source_map: true,
        ..Default::default()
    };
    let product = match target {
        Target::Server => CompileProduct::RuntimeServer(runtime),
        Target::ClientTree | Target::FineGrained => CompileProduct::RuntimeClient(runtime),
    };
    let backend = match target {
        Target::ClientTree | Target::Server => VueBackendRequest::Vdom,
        Target::FineGrained => VueBackendRequest::Vapor,
    };
    CompileRequest::new(
        vec![product],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend,
            ..Default::default()
        }),
        None,
        Some("App.vue".to_string()),
        None,
        false,
        true,
    )
    .expect("a lone runtime product must construct")
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_string())
}

/// Compile one case on one route, catching panics.
fn run_route(route: RenderRoute, case: &Case, target: Target, mode: Mode) -> RouteResult {
    let request = request_for(target, mode);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        with_route(route, || {
            let allocator = Allocator::new();
            compile_with_parsed(
                &case.source,
                &request,
                &VueExecutionInputs::default(),
                &VueMacroSemanticInput::Unavailable,
                &allocator,
            )
            .map(|(parsed, result)| {
                let mut view = view_of(&result);
                let product = match target {
                    Target::ClientTree | Target::FineGrained => Some(ProductKind::RuntimeClient),
                    Target::Server => Some(ProductKind::RuntimeServer),
                };
                if let Some(product) = product.filter(|_| result.errors.is_empty()) {
                    view.module = module_of(
                        route,
                        product,
                        &case.source,
                        &parsed,
                        result,
                        &request,
                        &allocator,
                    );
                }
                Box::new(view)
            })
        })
    }));
    match outcome {
        Ok(Ok(view)) => RouteResult::Compiled(view),
        Ok(Err(error)) => RouteResult::Refused(error),
        Err(payload) => classify_panic(panic_message(payload.as_ref())),
    }
}

/// The legacy composer and the runtime pipeline write complete modules from
/// the same admitted source. Each side's map is encoded at its own terminal
/// boundary and compared by decoded tokens. A legacy module whose bytes do
/// not parse is not published, so it differs from a published target module
/// and needs a row; a target module always publishes.
fn module_of(
    route: RenderRoute,
    product: ProductKind,
    source: &str,
    parsed: &verter_compiler::parser::types::ParsedSfc,
    result: verter_compiler::compile::VerterCompileResult,
    request: &CompileRequest,
    allocator: &Allocator,
) -> Option<(String, String)> {
    use verter_compiler::assembly::fragment::FragmentDialect;
    use verter_compiler::assembly::vue_module::{compose_main_module, VueMainModuleRequest};
    use verter_compiler::assembly::VueMainDecoration;
    use verter_compiler::framework::vue::assembly::{compile_vue_runtime, VueCompileInputs};
    use verter_compiler::work_ledger::CompilerWorkLedger;
    let decoration = VueMainDecoration {
        emit_ssr_module_registration: false,
        ..Default::default()
    };
    match route {
        RenderRoute::EditInPlace => {
            let compiled =
                verter_compiler::framework_common::vue_bundle::vue_result_to_runtime_bundle(
                    source, parsed, result,
                );
            let script_map = compiled
                .script
                .as_ref()
                .filter(|s| !s.source_map.is_empty())
                .map(|s| {
                    verter_compiler::oxc_sourcemap::SourceMap::from_json_string(&s.source_map)
                        .unwrap()
                });
            let template_map_json = compiled
                .template
                .as_ref()
                .filter(|t| !t.source_map.is_empty())
                .map(|t| t.source_map.clone());
            let set = compose_main_module(VueMainModuleRequest {
                canonical_id: "App.vue",
                compiled: &compiled,
                dialect: FragmentDialect::JavaScript,
                planned_kind: product,
                runtime: "vue",
                want_maps: true,
                source_root: None,
                script_map: script_map.as_ref(),
                template_map_json,
                decoration,
            })
            .ok()?;
            let artifact = set.artifact(product).expect("the requested module");
            Some((
                artifact.code().to_owned(),
                artifact.runtime_source_map().unwrap_or_default().to_owned(),
            ))
        }
        RenderRoute::Targets => {
            let set = compile_vue_runtime(
                &VueCompileInputs {
                    source,
                    parsed,
                    request,
                    execution: &Default::default(),
                    macros: &Default::default(),
                    block_content: &Default::default(),
                    decoration: &decoration,
                    canonical_id: "App.vue",
                },
                allocator,
                &mut CompilerWorkLedger::<verter_audit::observe::Uncaptured>::new(),
            )
            .expect("the target module compiles");
            let staged = set.stage(product).expect("the target module stages");
            Some((
                staged.code().to_string(),
                staged.source_map().unwrap_or_default().to_owned(),
            ))
        }
    }
}

/// Sort a caught panic into what it says about the route.
fn classify_panic(message: String) -> RouteResult {
    if message.starts_with(NOT_IMPLEMENTED_PREFIX) {
        RouteResult::NotImplemented
    } else {
        RouteResult::Panicked(message)
    }
}

/// The verdict on one case in one mode.
#[derive(Debug)]
enum Verdict {
    Matched,
    /// The target route is not implemented for this case and no row says so.
    NotImplemented,
    /// The target route is not implemented for this case, as a row says.
    Pending,
    /// A row says the target route cannot render this case, but it did.
    PendingNowRenders(ExpectedKind),
    /// The routes differ and the difference is listed as expected.
    ExpectedDifference(ExpectedKind),
    Differs(Vec<Difference>),
    /// The target route panicked or refused.
    NewRouteFailed(String),
    /// The in-place route panicked or refused and no row says so.
    UnexpectedLegacyFailure(String),
    /// A row lists a difference that is gone.
    ExpectedDifferenceNowMatches(ExpectedKind),
    /// A row names a failure the in-place route did not have.
    ExpectedKindMismatch {
        expected: ExpectedKind,
        found: &'static str,
    },
}

impl Verdict {
    /// What the verdict says, for reports.
    fn describe(&self) -> String {
        match self {
            Verdict::Matched => "matched".to_string(),
            Verdict::NotImplemented => "the target route is not implemented".to_string(),
            Verdict::Pending => "pending on the target route".to_string(),
            Verdict::PendingNowRenders(kind) => {
                format!("a row expects {kind:?}, but the target route renders the case")
            }
            Verdict::ExpectedDifference(kind) => format!("an expected difference: {kind:?}"),
            Verdict::Differs(differences) => format!("{} differences", differences.len()),
            Verdict::NewRouteFailed(why) => format!("the target route failed: {why}"),
            Verdict::UnexpectedLegacyFailure(why) => {
                format!("the in-place route failed and no row expects it: {why}")
            }
            Verdict::ExpectedDifferenceNowMatches(kind) => {
                format!("a row expects {kind:?}, but the routes now match")
            }
            Verdict::ExpectedKindMismatch { expected, found } => {
                format!("a row expects {expected:?}, but the in-place route {found}")
            }
        }
    }

    fn is_failure(&self) -> bool {
        !matches!(
            self,
            Verdict::Matched
                | Verdict::NotImplemented
                | Verdict::Pending
                | Verdict::ExpectedDifference(_)
        )
    }
}

fn judge(legacy: &RouteResult, new: &RouteResult, expected: Option<ExpectedKind>) -> Verdict {
    match new {
        RouteResult::NotImplemented => {
            return match expected {
                Some(ExpectedKind::Pending { .. }) => Verdict::Pending,
                _ => Verdict::NotImplemented,
            };
        }
        RouteResult::Panicked(message) => {
            return Verdict::NewRouteFailed(format!("panicked: {message}"));
        }
        RouteResult::Refused(error) => {
            return Verdict::NewRouteFailed(format!("refused: {error:?}"));
        }
        RouteResult::Compiled(_) => {}
    }
    if let Some(kind @ ExpectedKind::Pending { .. }) = expected {
        return Verdict::PendingNowRenders(kind);
    }
    let (legacy_view, new_view) = match (legacy, new) {
        (RouteResult::Compiled(legacy_view), RouteResult::Compiled(new_view)) => {
            (legacy_view, new_view)
        }
        (RouteResult::Panicked(message), _) => {
            return judge_legacy_failure(expected, ExpectedKind::LegacyPanicked, message);
        }
        (RouteResult::Refused(error), _) => {
            return judge_legacy_failure(
                expected,
                ExpectedKind::LegacyRefused,
                &format!("{error:?}"),
            );
        }
        (RouteResult::NotImplemented, _) => {
            return Verdict::UnexpectedLegacyFailure(
                "the in-place route reported itself unimplemented".to_string(),
            );
        }
        (RouteResult::Compiled(_), _) => unreachable!("the new route compiled"),
    };
    let differences = compare_views(legacy_view, new_view);
    match (expected, differences.is_empty()) {
        (None, true) => Verdict::Matched,
        (None, false) => Verdict::Differs(differences),
        (Some(kind @ ExpectedKind::Corrected { .. }), false) => Verdict::ExpectedDifference(kind),
        (Some(kind @ ExpectedKind::Corrected { .. }), true) => {
            Verdict::ExpectedDifferenceNowMatches(kind)
        }
        (Some(kind), _) => Verdict::ExpectedKindMismatch {
            expected: kind,
            found: "compiled",
        },
    }
}

fn judge_legacy_failure(
    expected: Option<ExpectedKind>,
    happened: ExpectedKind,
    message: &str,
) -> Verdict {
    match expected {
        Some(kind) if kind == happened => Verdict::ExpectedDifference(kind),
        Some(kind) => Verdict::ExpectedKindMismatch {
            expected: kind,
            found: match happened {
                ExpectedKind::LegacyPanicked => "panicked",
                ExpectedKind::LegacyRefused => "refused",
                ExpectedKind::Corrected { .. } => "corrected",
                ExpectedKind::Pending { .. } => "is pending",
            },
        },
        None => Verdict::UnexpectedLegacyFailure(message.to_string()),
    }
}

/// The result of one target over a set of cases.
#[derive(Debug, Default)]
struct Report {
    total: usize,
    matched: usize,
    /// Cases the target route cannot render and no row lists.
    not_implemented: Vec<String>,
    expected_differences: Vec<String>,
    pending: Vec<String>,
    failures: Vec<String>,
}

impl Report {
    fn summary(&self, target: Target) -> String {
        let mut out = format!(
            "{}: {}/{} matched, {} corrected, {} pending, {} not implemented",
            target.label(),
            self.matched,
            self.total,
            self.expected_differences.len(),
            self.pending.len(),
            self.not_implemented.len()
        );
        for line in &self.expected_differences {
            out.push_str(&format!("\n  expected: {line}"));
        }
        for line in &self.pending {
            out.push_str(&format!("\n  pending: {line}"));
        }
        for line in &self.not_implemented {
            out.push_str(&format!("\n  not implemented: {line}"));
        }
        for failure in &self.failures {
            out.push_str(&format!("\n{failure}"));
        }
        out
    }
}

fn failure_text(
    case: &Case,
    mode: Mode,
    verdict: &Verdict,
    legacy: &RouteResult,
    new: &RouteResult,
) -> String {
    let head = format!("FAIL {} ({})", case.name, mode.label());
    match verdict {
        Verdict::Differs(differences) => {
            let mut out = head;
            for difference in differences {
                out.push_str(&format!("\n  {difference}"));
            }
            if let (RouteResult::Compiled(legacy), RouteResult::Compiled(new)) = (legacy, new) {
                let diff = render_diff(&legacy.render, &new.render);
                if !diff.is_empty() {
                    out.push_str(&format!("\n{diff}"));
                }
            }
            out
        }
        other => format!("{head}\n  {}", other.describe()),
    }
}

/// Compare the two routes for one target over `cases`, in `modes`.
fn run_target(target: Target, modes: &[Mode], cases: &[Case]) -> Report {
    let mut report = Report::default();
    for case in cases {
        for &mode in modes {
            let expected = expected::rows(target, mode)
                .iter()
                .find(|row| row.case == case.name)
                .map(|row| row.kind);
            report.total += 1;
            let new = run_route(RenderRoute::Targets, case, target, mode);
            // The in-place route runs only when there is something to compare.
            let legacy = if matches!(new, RouteResult::NotImplemented) {
                RouteResult::NotImplemented
            } else {
                run_route(RenderRoute::EditInPlace, case, target, mode)
            };
            let verdict = judge(&legacy, &new, expected);
            match &verdict {
                Verdict::Matched => report.matched += 1,
                Verdict::NotImplemented => {
                    report
                        .not_implemented
                        .push(format!("{} ({})", case.name, mode.label()))
                }
                Verdict::Pending => {
                    let Some(ExpectedKind::Pending { reason }) = expected else {
                        unreachable!("a pending verdict comes from a pending row")
                    };
                    report
                        .pending
                        .push(format!("{} ({}): {reason}", case.name, mode.label()));
                }
                Verdict::ExpectedDifference(kind) => {
                    let why = match kind {
                        ExpectedKind::Corrected { reason } => (*reason).to_string(),
                        ExpectedKind::LegacyPanicked => "the in-place route panics".to_string(),
                        ExpectedKind::LegacyRefused => "the in-place route refuses".to_string(),
                        ExpectedKind::Pending { .. } => {
                            unreachable!("a pending row is never an expected difference")
                        }
                    };
                    report.expected_differences.push(format!(
                        "{} ({}): {why}",
                        case.name,
                        mode.label()
                    ));
                }
                failing => report
                    .failures
                    .push(failure_text(case, mode, failing, &legacy, &new)),
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(render: &str) -> RouteResult {
        RouteResult::Compiled(Box::new(CompiledView {
            render: render.to_string(),
            ..Default::default()
        }))
    }

    const CORRECTED: ExpectedKind = ExpectedKind::Corrected {
        reason: "the in-place output is wrong",
    };

    /// Runs one target in `modes` over the conformance corpus and the
    /// regression components. Every case the target route cannot render must
    /// have a pending row, so the test never passes without comparing.
    fn assert_target_matches_compared_cases(target: Target, modes: &[Mode]) -> Report {
        let report = run_target(target, modes, &corpus::compared_cases());
        println!("{}", report.summary(target));
        assert!(report.total > 0, "{}", report.summary(target));
        assert!(
            report.not_implemented.is_empty(),
            "{}",
            report.summary(target)
        );
        assert!(report.failures.is_empty(), "{}", report.summary(target));
        report
    }

    #[test]
    fn module_bytes_match_the_legacy_route() {
        assert_target_matches_compared_cases(Target::ClientTree, &[Mode::Separate]);
    }

    #[test]
    fn client_tree_inline_route_matches() {
        assert_target_matches_compared_cases(Target::ClientTree, &[Mode::Inline]);
    }

    #[test]
    fn server_route_matches() {
        assert_target_matches_compared_cases(Target::Server, Target::Server.modes());
    }

    #[test]
    fn fine_grained_route_matches() {
        let target = Target::FineGrained;
        let report = assert_target_matches_compared_cases(target, target.modes());
        assert!(report.pending.is_empty(), "{}", report.summary(target));
    }

    const PENDING: ExpectedKind = ExpectedKind::Pending {
        reason: "the target route lacks this construct",
    };

    #[test]
    fn a_pending_row_accounts_for_an_unimplemented_case() {
        let verdict = judge(&view("old"), &RouteResult::NotImplemented, Some(PENDING));
        assert!(matches!(verdict, Verdict::Pending));
        assert!(!verdict.is_failure());
    }

    #[test]
    fn a_pending_row_whose_case_renders_fails() {
        let verdict = judge(&view("same"), &view("same"), Some(PENDING));
        assert!(matches!(verdict, Verdict::PendingNowRenders(_)));
        assert!(verdict.is_failure());
    }

    #[test]
    fn an_expected_difference_that_matches_fails() {
        let verdict = judge(&view("same"), &view("same"), Some(CORRECTED));
        assert!(matches!(verdict, Verdict::ExpectedDifferenceNowMatches(_)));
        assert!(verdict.is_failure());
    }

    #[test]
    fn an_expected_difference_that_differs_passes() {
        let verdict = judge(&view("old"), &view("new"), Some(CORRECTED));
        assert!(matches!(verdict, Verdict::ExpectedDifference(_)));
        assert!(!verdict.is_failure());
    }

    #[test]
    fn an_unexpected_difference_fails() {
        let verdict = judge(&view("old"), &view("new"), None);
        assert!(matches!(verdict, Verdict::Differs(_)));
        assert!(verdict.is_failure());
    }

    #[test]
    fn a_legacy_panic_needs_a_row() {
        let legacy = RouteResult::Panicked("boom".to_string());
        assert!(judge(&legacy, &view("new"), None).is_failure());
        let verdict = judge(&legacy, &view("new"), Some(ExpectedKind::LegacyPanicked));
        assert!(matches!(verdict, Verdict::ExpectedDifference(_)));
    }

    #[test]
    fn a_new_route_panic_always_fails() {
        let new = RouteResult::Panicked("boom".to_string());
        let verdict = judge(&view("old"), &new, Some(ExpectedKind::LegacyPanicked));
        assert!(verdict.is_failure());
    }

    #[test]
    fn an_unimplemented_target_is_not_a_failure() {
        let verdict = judge(&view("old"), &RouteResult::NotImplemented, None);
        assert!(!verdict.is_failure());
    }

    #[test]
    fn a_caught_panic_is_classified_by_its_message() {
        assert!(matches!(
            classify_panic("not yet implemented: render".to_string()),
            RouteResult::NotImplemented
        ));
        assert!(matches!(
            classify_panic("index out of bounds".to_string()),
            RouteResult::Panicked(_)
        ));
    }

    #[test]
    fn the_in_place_route_compiles_a_template() {
        let case = Case {
            name: "inline.vue".to_string(),
            source: "<template><div>{{ msg }}</div></template>".to_string(),
        };
        for target in Target::ALL {
            let result = run_route(RenderRoute::EditInPlace, &case, target, Mode::Separate);
            let RouteResult::Compiled(view) = result else {
                panic!("{} did not compile: {result:?}", target.label());
            };
            assert!(!view.render.is_empty(), "{}", target.label());
        }
    }

    #[test]
    fn force_js_loop_sources_remove_type_assertions() {
        let all_targets = &Target::ALL[..];
        for (template, targets) in [
            (
                r#"<i v-for="item in ([1] as const)">{{ item }}</i>"#,
                all_targets,
            ),
            (
                r#"<i v-for="item in (items as number[])">{{ item }}</i>"#,
                all_targets,
            ),
            (
                r#"<i v-for="item in ([1] as const) ">{{ item }}</i>"#,
                all_targets,
            ),
        ] {
            let case = Case {
                name: "loop.vue".to_string(),
                source: format!(
                    "<script setup lang=\"ts\">const items = [1]</script><template>{template}</template>"
                ),
            };
            for &target in targets {
                for &mode in target.modes() {
                    let result = run_route(RenderRoute::Targets, &case, target, mode);
                    let RouteResult::Compiled(view) = result else {
                        panic!("{template} ({target:?}, {mode:?}): {result:?}");
                    };
                    assert!(view.diagnostics.is_empty(), "{:?}", view.diagnostics);
                    assert!(!view.render.contains(" as "), "{}", view.render);
                    let (module, _) = view.module.expect("a complete JS module is staged");
                    let allocator = Allocator::new();
                    let parsed =
                        oxc_parser::Parser::new(&allocator, &module, oxc_span::SourceType::mjs())
                            .parse();
                    assert!(
                        parsed.diagnostics.is_empty(),
                        "{:?}\n{module}",
                        parsed.diagnostics
                    );
                }
            }
        }
    }

    /// A typed catch binding is erased from force-JS script output, as Vue's
    /// script output followed by TypeScript erasure writes it.
    #[test]
    fn force_js_catch_parameters_follow_pinned_erasure() {
        use verter_vue_conformance::canon::{canonicalize_module, Canon};
        use verter_vue_conformance::{authored_identifiers, read_text_normalized, sha256_hex};

        fn declaration(
            code: &str,
            name: &str,
            exact: &std::collections::BTreeSet<String>,
        ) -> Canon {
            fn find(node: &Canon, name: &str) -> Option<Canon> {
                let Canon::Node(kind, children) = node else {
                    return None;
                };
                if *kind == "FunctionDeclaration"
                    && matches!(children.first(), Some(Canon::Leaf("ident", n)) if n == name)
                {
                    return Some(node.clone());
                }
                children.iter().find_map(|child| find(child, name))
            }
            let module = canonicalize_module(code, exact).expect("a valid JavaScript module");
            find(&module.tree, name).unwrap_or_else(|| panic!("`{name}` is declared:\n{code}"))
        }

        let source = "<script setup lang=\"ts\">function load() { try { JSON.parse('') } catch (err: unknown) { report(err) } }\nfunction report(err: unknown) {}</script>\n<template><button @click=\"load()\">x</button></template>\n";
        let root = verter_vue_conformance::corpus_root()
            .with_file_name("corrections")
            .join("goldens/3.6.0-rc.15/script");
        let metadata: serde_json::Value = serde_json::from_str(
            &read_text_normalized(&root.join("force-js-catch-parameter.meta.json")).unwrap(),
        )
        .unwrap();
        let vue = read_text_normalized(&root.join("force-js-catch-parameter.js")).unwrap();
        assert_eq!(metadata["vueVersion"], "3.6.0-rc.15");
        assert_eq!(metadata["typescriptVersion"], "5.9.3");
        assert_eq!(metadata["sourceSha256"], sha256_hex(source));
        assert_eq!(metadata["codeSha256"], sha256_hex(&vue));
        let case = Case {
            name: "catch.vue".to_string(),
            source: source.to_string(),
        };
        for target in [Target::ClientTree, Target::Server] {
            let RouteResult::Compiled(view) =
                run_route(RenderRoute::Targets, &case, target, Mode::Separate)
            else {
                panic!("{target:?} compiles the typed catch binding");
            };
            assert!(view.diagnostics.is_empty(), "{:?}", view.diagnostics);
            let (module, _) = view.module.expect("a complete JS module is staged");
            let exact = authored_identifiers(&format!("{source}\n{module}\n{vue}"));
            for name in ["load", "report"] {
                assert_eq!(
                    declaration(&module, name, &exact),
                    declaration(&vue, name, &exact),
                    "{target:?} `{name}`:\n{module}"
                );
            }
        }
    }

    #[test]
    fn the_route_override_is_restored_after_a_panic() {
        let before = verter_compiler::compile::render_route::current();
        let outcome = catch_unwind(|| {
            with_route(RenderRoute::Targets, || panic!("boom"));
        });
        assert!(outcome.is_err());
        assert_eq!(verter_compiler::compile::render_route::current(), before);
    }

    #[test]
    fn the_in_place_route_compiles_every_conformance_case_on_every_target() {
        let cases = corpus::conformance_cases();
        assert!(!cases.is_empty());
        let mut failures = Vec::new();
        for case in &cases {
            for target in Target::ALL {
                for &mode in target.modes() {
                    let result = run_route(RenderRoute::EditInPlace, case, target, mode);
                    if !matches!(result, RouteResult::Compiled(_)) {
                        failures.push(format!(
                            "{} ({}, {}): {result:?}",
                            case.name,
                            target.label(),
                            mode.label()
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Inline fine-grained output is left out of the comparison because the
    /// request layer refuses it on both routes. Once it compiles, this fails
    /// and the fine-grained target gains its inline mode.
    #[test]
    fn inline_fine_grained_output_is_refused_on_both_routes() {
        let case = Case {
            name: "inline.vue".to_string(),
            source: "<template><div>{{ msg }}</div></template>".to_string(),
        };
        for route in [RenderRoute::EditInPlace, RenderRoute::Targets] {
            let result = run_route(route, &case, Target::FineGrained, Mode::Inline);
            assert!(
                matches!(
                    result,
                    RouteResult::Refused(CompileRequestError::VaporInlineNotYetImplemented)
                ),
                "{route:?}: {result:?}"
            );
        }
    }

    #[test]
    #[ignore = "compares the external corpus VERTER_DIFFERENTIAL_CORPUS names; the target executors do not render templates yet"]
    fn external_corpus_matches_on_every_target() {
        let cases = corpus::external_cases()
            .unwrap_or_else(|| panic!("{} names no corpus", corpus::EXTERNAL_CORPUS_VARIABLE));
        assert!(!cases.is_empty());
        for target in Target::ALL {
            let report = run_target(target, target.modes(), &cases);
            println!("{}", report.summary(target));
            assert!(
                report.not_implemented.is_empty(),
                "{}",
                report.summary(target)
            );
            assert!(report.failures.is_empty(), "{}", report.summary(target));
        }
    }
}
