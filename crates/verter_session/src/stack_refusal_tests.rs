//! A source whose parse, or the walk-stack lease of its walks, is refused
//! is typed operational incompleteness at the source stage: the stage
//! publishes nothing for it, the upsert reports the typed refusal, and the
//! host goes on serving. A retry once the stack can be had publishes the
//! file. The refusals are injected at the reservation (`oxc_parse::faults`)
//! for exactly the source's size, so nothing exhausts the machine.

use std::sync::Arc;

use oxc_span::SourceType;
use verter_parser::oxc_parse::faults::{fail_reservations_needing, Reservation};
use verter_scheduler::job::SchedulerError;

use crate::types::HostConfig;
use crate::{FileLanguage, HostError, UpsertRequest, VerterHost};

/// A module whose first constant nests `depth` parentheses deep: its parse
/// and its walks need a stack region on a scheduler worker.
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

/// Upsert a deep module whose `purpose` reservation is refused once: the
/// upsert reports the typed refusal and the source stage publishes nothing;
/// the same upsert again publishes the module's bindings.
fn refused_once_then_retried(id: &str, depth: usize, purpose: Reservation) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = deep_module(depth);
    let needed = verter_parser::oxc_parse::parse_stack_bytes(&source, SourceType::ts());
    fail_reservations_needing(purpose, needed, 1);
    let refused = upsert(&host, id, &source);
    fail_reservations_needing(purpose, needed, 0);
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
