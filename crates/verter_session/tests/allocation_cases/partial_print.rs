//! What printing an answer costs, measured apart from evaluating it.
//!
//! `[S] extends [T] ? 1 : 2` over two object unions of `arms` arms whose
//! target is reversed: a request out of operations answers the undecided
//! conditional, partial, with the unions it never related behind it. Its
//! print names `S` and `T`, so projecting and rendering it costs the same
//! however wide the unions are. A relation stopped partway, out of
//! structured comparisons, answers the checker's TS2859 recovery with the
//! comparisons it ran behind it, and its print is as flat. A complete
//! answer prints all it holds: printing `S` costs in proportion to the
//! text it prints. No print may pay for structure it does not print.

use std::sync::Arc;

use verter_session::for_tests::with_relation_comparisons_for_tests;
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

/// The pairs relating `arms` reversed arms compares in full: each arm of
/// `S` scans `T` up to its partner.
fn reversed_pairs(arms: usize) -> usize {
    arms * (arms + 1) / 2
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
        peak_live_bytes: u64::try_from(peak_live_bytes())
            .expect("the peak live bytes since the reset are never negative"),
        text,
    }
}

/// A partial answer prints what it names: the cost of printing the
/// undecided conditional stays flat from 50 to 200 arms behind it.
#[test]
fn printing_a_partial_answer_costs_what_it_prints_not_what_it_hides() {
    // The fuse alone makes the answer partial: without it the request
    // relates the unions and answers the checker's `1`.
    assert_eq!(
        print(50, "__Probe", 0).text,
        "1",
        "the unfused request completes"
    );
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

/// A relation stopped partway prints what it answers. Each request runs
/// about half the comparisons its reversed unions need before its allowance
/// runs out — about four times as many at each doubling of the arms — and
/// answers the checker's TS2859 recovery, which names neither operand. The
/// cost of printing it stays flat however far the relation got.
#[test]
fn printing_a_mid_relation_partial_costs_what_it_prints_not_what_it_ran() {
    let allowance = |pairs: usize| u32::try_from(pairs).expect("a small allowance");
    // The allowance alone stops the relation: one with room for every
    // pair relates the unions and answers the checker's `1`.
    assert_eq!(
        with_relation_comparisons_for_tests(allowance(2 * reversed_pairs(50)), || {
            print(50, "__Probe", 0)
        })
        .text,
        "1",
        "an allowance with room for every pair relates the unions"
    );
    let sizes = [50usize, 100, 200];
    let runs = sizes.map(|arms| {
        // A cold check refuses a comparison only once it has recorded its
        // whole allowance, so a request refused under half the pairs ran
        // that half first.
        with_relation_comparisons_for_tests(allowance(reversed_pairs(arms) / 2), || {
            print(arms, "__Probe", 0)
        })
    });
    eprintln!("MID_RELATION_PRINT {runs:?}");
    for (arms, run) in sizes.into_iter().zip(&runs) {
        assert_eq!(
            run.text, "2",
            "{arms} arms: half the pairs cannot relate the unions, and the refused check \
             recovers as false"
        );
    }
    let [narrow, _, wide] = &runs;
    assert!(
        wide.allocations <= narrow.allocations + narrow.allocations / 4 + 16,
        "about sixteen times the comparisons run leaves the print's allocations flat: {runs:?}"
    );
    assert!(
        wide.peak_live_bytes <= narrow.peak_live_bytes + narrow.peak_live_bytes / 4 + 4096,
        "about sixteen times the comparisons run leaves the print's peak live bytes flat: \
         {runs:?}"
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
