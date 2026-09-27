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
