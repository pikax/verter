//! What printing an answer costs, measured apart from evaluating it.
//!
//! `[S] extends [T] ? 1 : 2` over two object unions of `arms` arms whose
//! target is reversed: a request out of operations answers the undecided
//! conditional, partial, with the unions it never related behind it. Its
//! print names `S` and `T`, so projecting and rendering it costs the same
//! however wide the unions are. A complete answer prints all it holds:
//! printing `S` costs in proportion to the text it prints. Neither print
//! may pay for structure it does not print.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use super::{alloc_count, peak_live_bytes, reset_alloc_counter};

const SCENARIO: &str = "/print/scenario.ts";

fn reversed_unions(arms: usize) -> String {
    let source: Vec<String> = (0..arms).map(|i| format!("{{ p{i}: {i} }}")).collect();
    let target: Vec<String> = (0..arms)
        .rev()
        .map(|i| format!("{{ p{i}: number }}"))
        .collect();
    format!(
        "type S = {};\ntype T = {};\ntype __Probe = [S] extends [T] ? 1 : 2;\nexport {{}};\n",
        source.join(" | "),
        target.join(" | ")
    )
}

/// What observing one answer cost on this thread, and what it printed.
#[derive(Debug)]
struct Print {
    text: String,
    allocations: u64,
    peak_live_bytes: u64,
}

/// Evaluate `symbol` over unions of `arms` arms under a projection fuse of
/// `projection_op_budget`, then project and render its answer alone.
fn print(arms: usize, symbol: &str, projection_op_budget: usize) -> Print {
    let source = reversed_unions(arms);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file(SCENARIO.to_string(), Arc::from(source.as_str()));
    let workspace: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(
        HostConfig {
            projection_op_budget,
            ..HostConfig::default()
        },
        workspace,
    ));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(SCENARIO.to_string()),
        input_id: SCENARIO.to_string(),
        source: Arc::from(source.as_str()),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    let (outcome, _) = host
        .resolve_named_symbol_with_audit(SCENARIO, symbol, None)
        .into_parts();
    let node = outcome
        .expect("the named demand completes")
        .expect("the symbol exists");
    reset_alloc_counter();
    let wire = host
        .project_node_to_type_expr_json_bytes(&node)
        .expect("the answer projects");
    let value: serde_json::Value = serde_json::from_slice(&wire).expect("the wire answer is JSON");
    let answer = verter_type_expr::type_expr_from_json(&value).expect("a wire TypeExpr");
    let text = verter_type_expr::render_type_expr_display(&answer)
        .expect("the answer renders")
        .text;
    Print {
        allocations: alloc_count(),
        peak_live_bytes: u64::try_from(peak_live_bytes()).unwrap_or(0),
        text,
    }
}

/// A partial answer prints what it names: the cost of printing the
/// undecided conditional stays flat from 50 to 200 arms behind it.
#[test]
fn printing_a_partial_answer_costs_what_it_prints_not_what_it_hides() {
    let runs = [50usize, 100, 200].map(|arms| print(arms, "__Probe", 1));
    eprintln!("PARTIAL_PRINT {runs:?}");
    for run in &runs {
        assert_eq!(
            run.text, "[S] extends [T] ? 1 : 2",
            "the out-of-operations answer is the undecided conditional, its operands named"
        );
    }
    let [narrow, _, wide] = &runs;
    assert!(
        wide.allocations <= narrow.allocations + narrow.allocations / 4 + 16,
        "four times the hidden arms leaves the print's allocations flat: {runs:?}"
    );
    assert!(
        wide.peak_live_bytes <= narrow.peak_live_bytes + narrow.peak_live_bytes / 4 + 4096,
        "four times the hidden arms leaves the print's peak live bytes flat: {runs:?}"
    );
}

/// A complete answer prints everything it holds, and its print costs in
/// proportion to that text: twice the arms, about twice the cost.
#[test]
fn printing_a_complete_answer_costs_in_proportion_to_its_text() {
    let runs = [100usize, 200].map(|arms| print(arms, "S", 0));
    let [narrow, wide] = &runs;
    eprintln!(
        "COMPLETE_PRINT 100 arms: {} chars, {} allocations, {} peak bytes; \
         200 arms: {} chars, {} allocations, {} peak bytes",
        narrow.text.len(),
        narrow.allocations,
        narrow.peak_live_bytes,
        wide.text.len(),
        wide.allocations,
        wide.peak_live_bytes
    );
    assert!(narrow.text.starts_with("{ p0: 0 } | "));
    let text_growth = wide.text.len() as f64 / narrow.text.len() as f64;
    for (what, narrow, wide) in [
        ("allocations", narrow.allocations, wide.allocations),
        (
            "peak live bytes",
            narrow.peak_live_bytes,
            wide.peak_live_bytes,
        ),
    ] {
        let growth = wide as f64 / narrow as f64;
        assert!(
            growth <= text_growth * 1.25,
            "{what} grow with the printed text ({growth:.2}x for {text_growth:.2}x the text)"
        );
    }
}
