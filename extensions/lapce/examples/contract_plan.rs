//! Export the exact production Lapce launch plan for the real stdio smoke.

use serde_json::json;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .expect("usage: contract_plan <root> <verter-lsp>");
    let binary = args
        .next()
        .expect("usage: contract_plan <root> <verter-lsp>");
    assert!(args.next().is_none(), "unexpected extra argument");

    let config = json!({ "lsp": { "serverPath": binary } });
    let plan = verter_lapce::plan_launch(
        Some(&root),
        &config,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
    .expect("Lapce production plan must resolve the explicit server path");
    assert_eq!(plan.uri, format!("urn:{binary}"));

    println!(
        "{}",
        json!({
            "editor": "lapce",
            "command": binary,
            "args": plan.args,
            "initializationOptions": plan.options,
            "languages": plan.selector.into_iter().map(|entry| entry.language).collect::<Vec<_>>(),
        })
    );
}
