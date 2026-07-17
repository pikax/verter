//! Export the exact production Zed launch plan for the real stdio smoke.

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

    let settings = json!({});
    let plan = verter_zed::plan_launch(Some(&root), &settings, Some(&binary), None, false)
        .expect("Zed production plan must resolve the explicit server path");
    assert_eq!(plan.command_path, binary);

    println!(
        "{}",
        json!({
            "editor": "zed",
            "command": plan.command_path,
            "args": plan.args,
            "initializationOptions": verter_editor_client::build_initialization_options(&settings),
            "languages": ["vue", "svelte"],
        })
    );
}
