//! A source whose parse, or the walk-stack lease of its walks, is refused
//! is typed operational incompleteness at the source stage: the stage
//! publishes nothing for it, the upsert reports the typed refusal, and the
//! host goes on serving. A retry once the stack can be had publishes the
//! file. The refusals are injected at the reservation (`oxc_parse::faults`)
//! for exactly the source's size, so nothing exhausts the machine, and the
//! sources are forced onto the region path
//! (`oxc_parse::faults::force_reservations`) so a shallow source reserves
//! a region like a deep one: a refusal below is the injected fault on a
//! thread of any stack, never a source too deep for it. A forced purpose
//! sizes its reservation from the syntax scan, so the size a test computes
//! is the size the operation reserves whatever stack the thread has.

use std::sync::Arc;

use oxc_span::SourceType;
use verter_parser::oxc_parse::faults::{
    fail_reservations_here, fail_reservations_needing, force_reservations, ForcedRegions,
    Reservation,
};
use verter_scheduler::job::SchedulerError;

use crate::types::HostConfig;
use crate::{FileLanguage, HostError, UpsertRequest, VerterHost};

/// Force `purposes` onto the region path for as long as the returned guard
/// lives, so a reservation is made — and can be refused — wherever it is
/// made, a scheduler worker's included.
fn forcing(purposes: &[Reservation]) -> ForcedRegions {
    force_reservations(purposes)
}

/// A module whose first constant nests `depth` parentheses deep. The depth
/// no longer decides whether the source reserves: the forcing does. It is
/// what makes each test's reservation a size no other test parses, so a
/// fault armed for it finds this one alone.
fn deep_module(depth: usize) -> String {
    format!(
        "const v = {}1{};\nconst w = 2;\n",
        "(".repeat(depth),
        ")".repeat(depth)
    )
}

fn upsert(host: &VerterHost, id: &str, source: &str) -> Result<(), HostError> {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(source),
        file_language: FileLanguage::script_ts(),
        aliases: Vec::new(),
    })
    .map(drop)
}

/// The names the host's analysis of `id` binds.
fn bindings(host: &VerterHost, id: &str) -> Vec<String> {
    host.get_analysis(id)
        .map(|analysis| {
            analysis
                .bindings
                .iter()
                .map(|binding| binding.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Upsert a module whose `purpose` reservation is refused once: the upsert
/// reports the typed refusal and the source stage publishes nothing; the
/// same upsert again publishes the module's bindings. The forcing is what
/// makes the source's parse or the walks' lease reserve, so a shallow
/// module proves the refusal on any thread's stack.
fn refused_once_then_retried(id: &str, depth: usize, purpose: Reservation) {
    let _forcing = forcing(&[purpose]);
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = deep_module(depth);
    let needed = verter_parser::oxc_parse::parse_stack_bytes(&source, SourceType::ts());
    let before = verter_parser::oxc_parse::faults::reservations_needing(purpose, needed);
    fail_reservations_needing(purpose, needed, 1);
    let refused = upsert(&host, id, &source);
    fail_reservations_needing(purpose, needed, 0);
    let attempted =
        verter_parser::oxc_parse::faults::reservations_needing(purpose, needed) - before;
    match refused {
        Err(HostError::Scheduler(SchedulerError::StackUnavailable {
            file_id,
            needed: refused,
        })) => {
            assert_eq!(file_id, id);
            assert_eq!(refused, needed);
        }
        other => panic!("expected the typed stack refusal, got {other:?}"),
    }
    assert!(
        attempted >= 1,
        "the {purpose:?} reservation of {needed} bytes was attempted"
    );
    assert!(
        host.scheduler.try_get_source(id).is_none(),
        "a refused source stage publishes no snapshot"
    );
    upsert(&host, id, &source).expect("the retry parses and publishes");
    assert!(host.scheduler.try_get_source(id).is_some());
    assert_eq!(bindings(&host, id), ["v", "w"]);
}

/// A parse whose region is refused publishes nothing, and a retry parses.
#[test]
fn a_refused_parse_publishes_nothing_and_a_retry_publishes_the_module() {
    refused_once_then_retried("/src/refused_parse.ts", 3_331, Reservation::Parse);
}

/// A module that parsed but whose walk-stack lease is refused runs none of
/// its walks and publishes nothing; a retry publishes it.
#[test]
fn a_refused_walk_stack_lease_publishes_nothing_and_a_retry_publishes_the_module() {
    refused_once_then_retried("/src/refused_lease.ts", 3_337, Reservation::Lease);
}

/// The analysis read's rebuild lane parses a session overlay's source
/// afresh: when that parse is refused, the read serves no analysis, never
/// the empty file's; once the parse can have its stack, the same read
/// serves the overlay's module.
#[test]
fn a_refused_overlay_rebuild_serves_no_analysis() {
    let _forcing = forcing(&[Reservation::Parse]);
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let id = "/src/refused_overlay.ts";
    upsert(&host, id, "const base = 1;\n").expect("the base module parses");
    let overlay = deep_module(3_343);
    let needed = verter_parser::oxc_parse::parse_stack_bytes(&overlay, SourceType::ts());
    let mut overlays = rustc_hash::FxHashMap::default();
    overlays.insert(id.to_string(), Arc::<str>::from(overlay));
    let view = crate::session_view::OverlaidView::new(Arc::clone(&host), overlays);
    fail_reservations_needing(Reservation::Parse, needed, 1);
    let refused = host.get_analysis_via_view(id, &view);
    fail_reservations_needing(Reservation::Parse, needed, 0);
    assert!(
        refused.is_none(),
        "a refused parse serves no analysis, got bindings {:?}",
        refused.map(|analysis| analysis
            .bindings
            .iter()
            .map(|binding| binding.name.clone())
            .collect::<Vec<_>>())
    );
    let served = host
        .get_analysis_via_view(id, &view)
        .expect("the overlay parses once its stack can be had");
    let names: Vec<_> = served
        .bindings
        .iter()
        .map(|binding| binding.name.clone())
        .collect();
    assert_eq!(names, ["v", "w"]);
}

/// The whole-function return of `pf` in `/src/<name>.ts`.
fn flow_return_of_pf(
    host: &VerterHost,
    id: &str,
) -> Result<
    Option<crate::semantic_query::FlowReturnDegradation>,
    crate::host_flow_return_audit::FlowReturnError,
> {
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(id),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("pf"),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    host.get_flow_return_type_with_audit(
        &identity,
        crate::semantic_query::ReturnProjectionDemand::whole_return(),
    )
    .into_result()
    .map(|result| result.degradation())
}

/// A declaration flight whose evaluation program is refused for want of
/// stack publishes no artifact read off the refused (empty) program: the
/// request that met the refusal parses the program again and answers `pf`'s
/// return, where an artifact of the empty program answers `pf` missing.
#[test]
fn a_refused_evaluation_program_publishes_no_artifact() {
    use verter_parser::oxc_parse::faults::reservations_needing;
    let _forcing = forcing(&[Reservation::Parse]);
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let id = "/src/refused_program.ts";
    let source = format!(
        "{}export function pf() {{ return 1; }}\n",
        deep_module(3_347)
    );
    let needed = verter_parser::oxc_parse::parse_stack_bytes(&source, SourceType::ts());
    upsert(&host, id, &source).expect("the module parses");
    let upserted = reservations_needing(Reservation::Parse, needed);
    fail_reservations_needing(Reservation::Parse, needed, 1);
    let answered = flow_return_of_pf(&host, id);
    fail_reservations_needing(Reservation::Parse, needed, 0);
    let parses = reservations_needing(Reservation::Parse, needed) - upserted;
    assert_eq!(answered, Ok(None));
    assert!(parses >= 2, "the refused parse and its retry: {parses}");
}

/// An overlay's cold materialisation whose evaluation program, or whose
/// snapshot's walk-stack lease, is refused publishes no indexed artifact;
/// once the stack can be had the same materialisation publishes it.
#[test]
fn a_refused_overlay_materialisation_publishes_no_artifact() {
    for (purpose, depth) in [(Reservation::Parse, 3_349), (Reservation::Lease, 3_353)] {
        let _forcing = forcing(&[purpose]);
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        let id = "/src/refused_materialisation.ts";
        upsert(&host, id, "const base = 1;\n").expect("the base module parses");
        let overlay = deep_module(depth);
        let needed = verter_parser::oxc_parse::parse_stack_bytes(&overlay, SourceType::ts());
        let mut overlays = rustc_hash::FxHashMap::default();
        overlays.insert(id.to_string(), Arc::<str>::from(overlay));
        let view = crate::session_view::OverlaidView::new(Arc::clone(&host), overlays);
        fail_reservations_needing(purpose, needed, 1);
        let refused = host.materialize_overlay_indexed_ready_with_view(id, &view);
        fail_reservations_needing(purpose, needed, 0);
        assert!(
            refused.is_none(),
            "{purpose:?}: a refused flight publishes nothing"
        );
        assert!(
            host.materialize_overlay_indexed_ready_with_view(id, &view)
                .is_some(),
            "{purpose:?}: the flight publishes once its stack can be had"
        );
    }
}

/// A Vue component whose `<script setup>` first constant nests `depth`
/// parentheses deep, and that script's text.
fn deep_component(depth: usize) -> (String, String) {
    let script = format!(
        "const v = {}1{}\nconst w = 2",
        "(".repeat(depth),
        ")".repeat(depth)
    );
    let component = format!(
        "<script setup lang=\"ts\">{script}</script>\n<template><div>{{{{ w }}}}</div></template>\n"
    );
    (component, script)
}

fn upsert_component(host: &VerterHost, id: &str, source: &str) {
    host.upsert(UpsertRequest {
        canonical_id: Some(id.to_string()),
        input_id: id.to_string(),
        source: Arc::from(source),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    })
    .map(drop)
    .expect("the component parses");
}

/// Whether `diagnostics` carry the compile's stack refusal.
fn refused_its_stack(diagnostics: &crate::types::DiagnosticsSnapshot) -> bool {
    diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == crate::types::HOST_STACK_UNAVAILABLE)
}

/// A compile request whose script parse is refused its stack fails with the
/// typed stack refusal and publishes no product built from the empty
/// program; the same request once the stack can be had compiles.
#[test]
fn a_compile_request_whose_parse_is_refused_publishes_no_product() {
    use verter_compiler::compile_request::{
        CompileProduct, CompileRequest, FrameworkCompileRequest, RuntimeProductRequest,
        VueBackendRequest, VueCompileRequest,
    };
    let _forcing = forcing(&[Reservation::Parse]);
    let host = VerterHost::new_standalone(HostConfig::default());
    let id = "/src/RefusedRequest.vue";
    let (component, script) = deep_component(877);
    upsert_component(&host, id, &component);
    let needed = verter_parser::oxc_parse::parse_stack_bytes(&script, SourceType::ts());
    let request = || {
        CompileRequest::new(
            vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )],
            FrameworkCompileRequest::Vue(VueCompileRequest {
                backend: VueBackendRequest::Inferred,
                script_custom_element: Some(false),
                ..VueCompileRequest::default()
            }),
            None,
            None,
            None,
            false,
            false,
        )
        .expect("the demand constructs")
    };
    // The request compiles on this thread; a background worker parsing the
    // same script for its own flight is left alone.
    fail_reservations_here(Reservation::Parse, needed, 1);
    let refused = host.compile_request(id, request());
    fail_reservations_here(Reservation::Parse, needed, 0);
    match refused {
        Err(crate::types::CompileRequestFailure::Refused { diagnostics, .. }) => {
            assert!(refused_its_stack(&diagnostics), "{diagnostics:?}");
        }
        other => panic!("expected the typed stack refusal, got {other:?}"),
    }
    let retried = host.compile_request(id, request());
    assert!(retried.is_ok(), "the retry compiles: {retried:?}");
}

/// A virtual-file compile whose script parse is refused its stack fails
/// with the typed stack refusal, a failure blocked on an input outside the
/// bytes, and serves no module built from the empty program; the same read
/// once the stack can be had serves the module.
#[test]
fn a_virtual_file_whose_parse_is_refused_serves_no_module() {
    let _forcing = forcing(&[Reservation::Parse]);
    let host = VerterHost::new_standalone(HostConfig::default());
    let id = "/src/RefusedVirtual.vue";
    let (component, script) = deep_component(881);
    upsert_component(&host, id, &component);
    let needed = verter_parser::oxc_parse::parse_stack_bytes(&script, SourceType::ts());
    let read = || {
        host.get_virtual_file(crate::types::VirtualQuery {
            raw_id: None,
            canonical_id: Some(id.to_string()),
            node_kind: Some(crate::types::VirtualNodeKind::Main),
            compile_profile: crate::types::CompileProfile::default(),
        })
    };
    fail_reservations_here(Reservation::Parse, needed, 1);
    let refused = read();
    fail_reservations_here(Reservation::Parse, needed, 0);
    match refused {
        Err(HostError::CompileError(failure)) => {
            assert!(refused_its_stack(&failure.diagnostics), "{failure:?}");
            assert!(failure.blocked_on_unavailable_input());
        }
        other => panic!("expected the typed stack refusal, got {other:?}"),
    }
    let served = read().expect("the retry compiles");
    assert!(served.code.contains("_sfc_main"), "{}", served.code);
}

/// Upsert `component` as `language` with each parse of `expression` its
/// source stage makes refused its stack in turn: each fails the stage with
/// the typed refusal and publishes nothing, and the same upsert once the
/// stack can be had publishes the component.
fn every_parse_of_the_source_stage_refused(
    language: FileLanguage,
    extension: &str,
    component: &str,
    expression: &str,
) {
    use verter_parser::oxc_parse::faults::{fail_reservations_needing_after, reservations_needing};
    let _forcing = forcing(&[Reservation::Parse]);
    let needed = verter_parser::oxc_parse::parse_stack_bytes(expression, SourceType::ts());
    let upsert_component = |host: &VerterHost, id: &str| {
        host.upsert(UpsertRequest {
            canonical_id: Some(id.to_string()),
            input_id: id.to_string(),
            source: Arc::from(component),
            file_language: language.clone(),
            aliases: Vec::new(),
        })
        .map(drop)
    };
    let before = reservations_needing(Reservation::Parse, needed);
    upsert_component(
        &VerterHost::new_standalone(HostConfig::default()),
        &format!("/src/Counted.{extension}"),
    )
    .expect("the component parses");
    let parses = reservations_needing(Reservation::Parse, needed) - before;
    assert!(
        parses >= 1,
        "the source stage parses the expression on a region"
    );
    for skip in 0..parses {
        let host = VerterHost::new_standalone(HostConfig::default());
        let id = format!("/src/Refused{skip}.{extension}");
        fail_reservations_needing_after(Reservation::Parse, needed, skip, 1);
        let refused = upsert_component(&host, &id);
        fail_reservations_needing(Reservation::Parse, needed, 0);
        match refused {
            Err(HostError::Scheduler(SchedulerError::StackUnavailable {
                file_id,
                needed: refused,
            })) => {
                assert_eq!(file_id, id);
                assert_eq!(refused, needed);
            }
            other => {
                panic!("parse {skip} of {parses}: expected the typed stack refusal, got {other:?}")
            }
        }
        assert!(host.scheduler.try_get_source(&id).is_none());
        upsert_component(&host, &id).expect("the retry parses and publishes");
        assert!(host.scheduler.try_get_source(&id).is_some());
    }
}

/// The parentheses of a markup expression nesting `depth` deep.
fn deep_expression(depth: usize) -> String {
    format!("{}n{}", "(".repeat(depth), ")".repeat(depth))
}

/// A Svelte markup expression is parsed by the carrier projection: its
/// refusal refuses the projection, and the stage with it.
#[test]
fn a_refused_svelte_projection_publishes_nothing() {
    let expression = deep_expression(907);
    every_parse_of_the_source_stage_refused(
        FileLanguage::svelte(),
        "svelte",
        &format!("<script>let n = 1;</script>\n<p>{{{expression}}}</p>\n"),
        &expression,
    );
}

/// A Vue interpolation is parsed by the template analysis a read of the
/// file's analysis builds: its parse refused its stack, the read serves no
/// template analysis, never one read off the empty expression in its
/// place, and the next read builds it.
#[test]
fn a_refused_vue_template_analysis_serves_no_template() {
    let _forcing = forcing(&[Reservation::Parse]);
    let expression = deep_expression(911);
    let host = VerterHost::new_standalone(HostConfig::default());
    let id = "/src/RefusedTemplate.vue";
    upsert_component(
        &host,
        id,
        &format!(
            "<script setup>const n = 1</script>\n<template><p>{{{{ {expression} }}}}</p></template>\n"
        ),
    );
    let needed = verter_parser::oxc_parse::parse_stack_bytes(&expression, SourceType::ts());
    fail_reservations_needing(Reservation::Parse, needed, 1);
    let refused = host.get_analysis(id);
    fail_reservations_needing(Reservation::Parse, needed, 0);
    let refused = refused.expect("the script analysis is served");
    assert!(
        refused.template.is_none(),
        "a refused template analysis is absent, not empty"
    );
    let served = host
        .get_analysis(id)
        .expect("the script analysis is served");
    assert!(served.template.is_some(), "the next read builds it");
}

/// A public-API projection whose parses of the script are each refused
/// their stack in turn fails with the typed stack refusal, its subject the
/// whole source, or serves the projection the script has (a refused parse
/// of a prefetch leaves the projection to parse again): never declarations
/// read off the empty program in a refused parse's place. The read whose
/// own extract was refused fails, and caches no extract of it.
#[test]
fn a_refused_public_api_projection_serves_no_wrong_projection() {
    use verter_parser::oxc_parse::faults::{fail_reservations_here_after, reservations_here};
    let _forcing = forcing(&[Reservation::Parse]);
    let depth = 887;
    let script = format!(
        "const v = {}1{}\nconst w = 2\ndefineProps<{{ label: string }}>()",
        "(".repeat(depth),
        ")".repeat(depth)
    );
    let component = format!("<script setup lang=\"ts\">{script}</script>\n<template><div>{{{{ w }}}}</div></template>\n");
    let needed = verter_parser::oxc_parse::parse_stack_bytes(&script, SourceType::ts());
    let projection = |host: &VerterHost, id: &str| {
        host.get_public_api(id)
            .map(|response| response.map(|response| response.ts_labeled_code().to_string()))
    };
    let clean_host = VerterHost::new_standalone(HostConfig::default());
    upsert_component(&clean_host, "/src/Clean.vue", &component);
    let before = reservations_here(Reservation::Parse, needed);
    let clean = projection(&clean_host, "/src/Clean.vue")
        .expect("the projection succeeds")
        .expect("the component has a public API");
    let parses = reservations_here(Reservation::Parse, needed) - before;
    assert!(clean.contains("label"), "{clean}");
    assert!(
        parses >= 1,
        "the projection parses the script on this thread"
    );
    let mut refusals = 0;
    for skip in 0..parses {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert_component(&host, "/src/Clean.vue", &component);
        // The projection parses on this thread; a background flight parsing
        // the same script is left alone.
        fail_reservations_here_after(Reservation::Parse, needed, skip, 1);
        let refused = projection(&host, "/src/Clean.vue");
        fail_reservations_here(Reservation::Parse, needed, 0);
        match refused {
            Err(crate::PublicApiProjectionError::TscGeneration(
                verter_compiler::tsc::TscGenerationError::StackUnavailable { needed: refused },
            )) => {
                assert_eq!(refused, needed);
                refusals += 1;
            }
            Ok(None) => panic!("parse {skip} of {parses}: an absent projection"),
            Ok(Some(code)) => assert_eq!(code, clean, "parse {skip} of {parses}"),
            Err(error) => panic!("parse {skip} of {parses}: {error:?}"),
        }
        let served = projection(&host, "/src/Clean.vue").expect("the retry succeeds");
        assert_eq!(
            served.as_deref(),
            Some(clean.as_str()),
            "parse {skip}: the retry"
        );
    }
    assert!(
        refusals >= 1,
        "the read whose own extract was refused fails with the typed refusal"
    );
}

/// Set in the child process [`every_walk_of_the_host_operations_runs_under_a_lease`]
/// runs its operations in.
const UNLEASED_WALK_CHILD: &str = "VERTER_UNLEASED_WALK_GUARD_CHILD";

/// Every walk of oxc's that the host's operations, and the compiler's
/// entries outside a host, make over syntax nesting past what the
/// thread's stack holds by its length runs under a walk-stack lease its
/// operation holds, the operation's one fallible step: none reserves a
/// region of its own, a failure no operation could report. The host
/// operations (upsert, analysis, a flow return, the runtime compile, a
/// compile request, the public-API projection) and the standalone entries
/// (the direct, prepared and batched compile, the `tsc` generation, the
/// specifier inventory) run over TypeScript, Vue
/// (script setup and options API) and Svelte (runes and legacy) sources
/// whose constructs nest 201 levels deep, in a child process of their own
/// so that no other test's walks are counted.
#[test]
fn every_walk_of_the_host_operations_runs_under_a_lease() {
    if std::env::var(UNLEASED_WALK_CHILD).is_ok() {
        host_operations_over_deep_sources();
        return;
    }
    let exe = std::env::current_exe().expect("the unit-test executable");
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, module)| module);
    let child = std::process::Command::new(exe)
        .arg("--exact")
        .arg(format!(
            "{module}::every_walk_of_the_host_operations_runs_under_a_lease"
        ))
        .arg("--nocapture")
        .env(UNLEASED_WALK_CHILD, "1")
        .output()
        .expect("spawn the child process");
    let stdout = String::from_utf8_lossy(&child.stdout);
    let stderr = String::from_utf8_lossy(&child.stderr);
    assert!(
        child.status.success() && stdout.contains("test result: ok. 1 passed"),
        "the child process runs this test and passes it: {}
{stdout}
{stderr}",
        child.status
    );
}

fn host_operations_over_deep_sources() {
    use verter_compiler::compile_request::{
        CompileProduct, CompileRequest, FrameworkCompileRequest, VueBackendRequest,
        VueCompileRequest,
    };
    use verter_parser::oxc_parse::faults::take_unleased_walks;
    let deep = deep_expression(201);
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let compile = |id: &str| {
        for node_kind in [
            crate::types::VirtualNodeKind::Main,
            crate::types::VirtualNodeKind::Script,
        ] {
            let _ = host.get_virtual_file(crate::types::VirtualQuery {
                raw_id: None,
                canonical_id: Some(id.to_string()),
                node_kind: Some(node_kind),
                compile_profile: crate::types::CompileProfile::default(),
            });
        }
    };
    let module = format!(
        "enum E {{ A = {deep} }}\nnamespace N {{ export const x = {deep}; }}\n\
         class C {{ f = {deep}; m(a = {deep}) {{ return {deep}; }} }}\n\
         const {{ a = {deep} }} = {{ a: {deep} }};\nconst t = `${{{deep}}}`;\n\
         const arrow = (b = {deep}) => {deep};\n\
         export function pf(p = {deep}) {{ if ({deep}) {{ return {deep}; }} return new C().m(); }}\n\
         export const q = pf();\n"
    );
    upsert(&host, "/src/deep.ts", &module).expect("the module parses");
    let _ = host.get_analysis("/src/deep.ts");
    let _ = flow_return_of_pf(&host, "/src/deep.ts");
    let setup = format!(
        "<script setup lang=\"ts\">\nimport {{ ref, computed }} from 'vue'\n\
         const props = withDefaults(defineProps<{{ label?: string }}>(), {{ label: () => String({deep}) }})\n\
         const emit = defineEmits<{{ (e: 'change', v: number): void }}>()\n\
         const n = ref({deep})\nconst c = computed(() => {deep})\n\
         function go() {{ emit('change', {deep}) }}\n</script>\n\
         <template><div :title=\"{deep}\" @click=\"{deep}\" v-if=\"{deep}\">\
         <span v-for=\"i in {deep}\" :key=\"i\">{{{{ {deep} }}}}</span><slot :v=\"{deep}\" /></div></template>\n"
    );
    upsert_component(&host, "/src/Setup.vue", &setup);
    let _ = host.get_analysis("/src/Setup.vue");
    compile("/src/Setup.vue");
    let _ = host.get_public_api("/src/Setup.vue");
    let request = CompileRequest::new(
        vec![
            CompileProduct::RuntimeClient(Default::default()),
            CompileProduct::IdeCompanion(Default::default()),
        ],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend: VueBackendRequest::Inferred,
            script_custom_element: Some(false),
            ..VueCompileRequest::default()
        }),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("the demand constructs");
    let _ = host.compile_request("/src/Setup.vue", request);
    let options = format!(
        "<script lang=\"ts\">\nexport default {{\n  props: {{ label: {{ type: String, default: () => String({deep}) }} }},\n\
         data() {{ return {{ n: {deep} }} }},\n  computed: {{ c() {{ return {deep} }} }},\n\
         methods: {{ go() {{ return {deep} }} }},\n  watch: {{ n() {{ return {deep} }} }},\n}}\n</script>\n\
         <template><div>{{{{ {deep} }}}}</div></template>\n"
    );
    upsert_component(&host, "/src/Options.vue", &options);
    let _ = host.get_analysis("/src/Options.vue");
    compile("/src/Options.vue");
    let _ = host.get_public_api("/src/Options.vue");
    for (id, source) in [
        (
            "/src/Runes.svelte",
            format!(
                "<script lang=\"ts\">\nlet {{ a = {deep} }} = $props();\nlet n = $state({deep});\n\
                 let d = $derived({deep});\n$effect(() => {{ n = {deep}; }});\n</script>\n\
                 <p title={{{deep}}} onclick={{() => {deep}}}>{{{deep}}}</p>\n\
                 {{#if {deep}}}<b>{{n}}</b>{{/if}}\n{{#each [{deep}] as item}}<i>{{item}}</i>{{/each}}\n\
                 <input bind:value={{n}} />\n"
            ),
        ),
        (
            "/src/Legacy.svelte",
            format!(
                "<script>\nimport {{ writable }} from 'svelte/store';\nexport let a = {deep};\n\
                 const s = writable({deep});\nlet n = {deep};\n$: d = {deep};\n</script>\n\
                 <p title={{{deep}}} on:click={{() => n = {deep}}}>{{$s}} {{{deep}}}</p>\n"
            ),
        ),
    ] {
        host.upsert(UpsertRequest {
            canonical_id: Some(id.to_string()),
            input_id: id.to_string(),
            source: Arc::from(source.as_str()),
            file_language: FileLanguage::svelte(),
            aliases: Vec::new(),
        })
        .map(drop)
        .expect("the component parses");
        let _ = host.get_analysis(id);
        compile(id);
    }
    standalone_entries_over_deep_sources(&setup, &options);
    let unleased = take_unleased_walks();
    assert!(
        unleased.is_empty(),
        "walks outside any walk-stack lease (route each through a lease its \
         operation holds):\n{}",
        unleased.join("\n")
    );
}

/// A Svelte component whose script and markup expressions nest `depth`
/// parentheses deep.
fn deep_runes_component(depth: usize) -> String {
    let deep = deep_expression(depth);
    format!(
        "<script lang=\"ts\">\nlet {{ a = {deep} }} = $props();\nlet n = $state({deep});\n</script>\n\
         <p title={{{deep}}} onclick={{() => n = {deep}}}>{{{deep}}}</p>\n"
    )
}

/// Every walk-stack lease a Svelte compile takes on the calling thread (the
/// compile's own walks, and the materialisation of the inputs it reads),
/// refused in turn, either fails the compile with the typed stack refusal
/// or leaves the module the compile serves with every lease granted (a
/// refused prefetch publishes nothing, and the compile materialises its
/// input again): never a module read off an empty program. The same
/// compile once the stack can be had serves the module.
#[test]
fn every_refused_lease_of_a_compile_serves_no_module() {
    every_refused_lease_of_a_compile_serves_no_module_here();
}

/// Every refusal here is the injected fault: the forcing is what makes each
/// of the compile's leases reserve, so the enumeration is the same on a
/// thread of any stack.
fn every_refused_lease_of_a_compile_serves_no_module_here() {
    use verter_parser::oxc_parse::faults::{
        fail_reservations_here_after, reservations_here_of, ANY_SIZE,
    };
    let _forcing = forcing(&[Reservation::Lease]);
    let component = deep_runes_component(151);
    let id = "/src/Leased.svelte";
    let host_with_component = || {
        let host = VerterHost::new_standalone(HostConfig::default());
        host.upsert(UpsertRequest {
            canonical_id: Some(id.to_string()),
            input_id: id.to_string(),
            source: Arc::from(component.as_str()),
            file_language: FileLanguage::svelte(),
            aliases: Vec::new(),
        })
        .map(drop)
        .expect("the component parses");
        host
    };
    let read = |host: &VerterHost| {
        host.get_virtual_file(crate::types::VirtualQuery {
            raw_id: None,
            canonical_id: Some(id.to_string()),
            node_kind: Some(crate::types::VirtualNodeKind::Main),
            compile_profile: crate::types::CompileProfile::default(),
        })
    };
    let host = host_with_component();
    let before = reservations_here_of(Reservation::Lease);
    let clean = read(&host).expect("the component compiles").code;
    let leases = reservations_here_of(Reservation::Lease) - before;
    assert!(leases >= 1, "the compile leases a region on this thread");
    let mut refusals = 0;
    for skip in 0..leases {
        let host = host_with_component();
        fail_reservations_here_after(Reservation::Lease, ANY_SIZE, skip, 1);
        let refused = read(&host);
        fail_reservations_here(Reservation::Lease, ANY_SIZE, 0);
        match refused {
            Err(HostError::CompileError(failure)) => {
                assert!(
                    refused_its_stack(&failure.diagnostics),
                    "lease {skip} of {leases}: {failure:?}"
                );
                refusals += 1;
            }
            Ok(served) => assert_eq!(served.code, clean, "lease {skip} of {leases}"),
            other => panic!(
                "lease {skip} of {leases}: expected the typed stack refusal, got {:?}",
                other.map(|served| served.code)
            ),
        }
        let served = read(&host).expect("the retry compiles");
        assert_eq!(served.code, clean, "lease {skip}: the retry");
    }
    assert!(refusals >= 1, "a compile's own lease refused fails it");
}

/// Every walk-stack lease the indexed materialisation of a Svelte
/// component takes on the calling thread, refused in turn, publishes no
/// indexed artifact (never one whose framework candidates were read off an
/// empty scan); the materialisation once the stack can be had publishes it.
#[test]
fn every_refused_lease_of_an_indexed_materialisation_publishes_nothing() {
    {
        use verter_parser::oxc_parse::faults::{
            fail_reservations_here_after, reservations_here_of, ANY_SIZE,
        };
        let _forcing = forcing(&[Reservation::Lease]);
        let component = deep_runes_component(151);
        let id = "/src/Indexed.svelte";
        let host_with_component = || {
            let host = VerterHost::new_standalone(HostConfig::default());
            host.upsert(UpsertRequest {
                canonical_id: Some(id.to_string()),
                input_id: id.to_string(),
                source: Arc::from(component.as_str()),
                file_language: FileLanguage::svelte(),
                aliases: Vec::new(),
            })
            .map(drop)
            .expect("the component parses");
            host
        };
        let before = reservations_here_of(Reservation::Lease);
        assert!(host_with_component()
            .ensure_indexed_ready_serve(id)
            .is_some());
        let leases = reservations_here_of(Reservation::Lease) - before;
        assert!(
            leases >= 1,
            "the materialisation leases a region on this thread"
        );
        for skip in 0..leases {
            let host = host_with_component();
            fail_reservations_here_after(Reservation::Lease, ANY_SIZE, skip, 1);
            let refused = host.ensure_indexed_ready_serve(id);
            fail_reservations_here(Reservation::Lease, ANY_SIZE, 0);
            assert!(refused.is_none(), "lease {skip} of {leases}: published");
            assert!(
                host.ensure_indexed_ready_serve(id).is_some(),
                "lease {skip}: the retry publishes"
            );
        }
    }
}

/// The compiler's entries outside a host, over the guard's deep sources:
/// the standalone compile (direct, prepared and batched; Vue runtime and
/// IDE products, Svelte runtime), the standalone checker's `tsc`
/// generation (one-shot, and extracted then generated), and the specifier
/// inventory. Their outcomes are the entries' own tests' concern; the guard
/// reads only which walks they make.
fn standalone_entries_over_deep_sources(setup: &str, options: &str) {
    use verter_compiler::compile::types::VueExecutionInputs;
    use verter_compiler::compile::VueMacroSemanticInput;
    use verter_compiler::compile_request::{
        CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest,
        RuntimeProductRequest, SvelteCompileRequest, VueCompileRequest,
    };
    use verter_compiler::standalone::{
        DirectExecutionInputs, StandaloneCompiler, SvelteExecutionInputs,
    };
    let request = |products: Vec<CompileProduct>, framework: FrameworkCompileRequest| {
        CompileRequest::new(products, framework, None, None, None, false, false)
            .expect("the demand constructs")
    };
    let vue = |products| {
        request(
            products,
            FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        )
    };
    let vue_inputs = VueExecutionInputs::default();
    let macros = VueMacroSemanticInput::Unavailable;
    for source in [setup, options] {
        for products in [
            vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )],
            vec![CompileProduct::IdeCompanion(IdeProductRequest {
                want_source_map: true,
                ..IdeProductRequest::default()
            })],
        ] {
            let request = vue(products);
            let inputs = || DirectExecutionInputs::Vue {
                execution: &vue_inputs,
                macros: &macros,
            };
            let _ = StandaloneCompiler.compile(source, &request, inputs());
            let prepared = StandaloneCompiler.prepare(source, &request);
            let _ = StandaloneCompiler.compile_prepared(source, &prepared, &request, inputs());
        }
        let _ = verter_compiler::tsc::generate_tsc_output(source, "Deep");
        if let Ok(Some(state)) = verter_compiler::tsc::extract_tsc_state(
            source,
            "Deep",
            &verter_compiler::tsc::TscExtractOptions::default(),
        ) {
            let _ = verter_compiler::tsc::generate_tsc_from_state(
                &state,
                "Deep",
                verter_compiler::tsc::TscMode::Public,
                verter_compiler::tsc::MacroTscInput::NotRequired,
                &verter_compiler::tsc::FallthroughPropsProjection::none(),
            );
        }
        if let Ok(Ok(output)) = verter_compiler::tsc::generate_tsc_output(source, "Deep")
            .map(|output| Ok::<_, ()>(output.code))
        {
            let _ = verter_compiler::tsc::collect_module_specifier_spans(&output);
        }
    }
    let svelte = deep_runes_component(201);
    let request = request(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
    );
    let svelte_inputs = SvelteExecutionInputs::default();
    let inputs = || DirectExecutionInputs::Svelte {
        execution: &svelte_inputs,
    };
    let _ = StandaloneCompiler.compile(&svelte, &request, inputs());
    let prepared = StandaloneCompiler.prepare(&svelte, &request);
    let _ = StandaloneCompiler.compile_prepared(&svelte, &prepared, &request, inputs());
}
