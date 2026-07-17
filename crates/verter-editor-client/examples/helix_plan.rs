//! Export the parsed shipped Helix plan for the real stdio smoke.

use std::path::Path;

use serde_json::json;
use toml::Value;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("usage: helix_plan <root> <verter-lsp>");
    let binary = args.next().expect("usage: helix_plan <root> <verter-lsp>");
    assert!(args.next().is_none(), "unexpected extra argument");

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("editors/helix/languages.toml");
    let parsed: Value = toml::from_str(
        &std::fs::read_to_string(&manifest).expect("read shipped Helix languages.toml"),
    )
    .expect("parse shipped Helix languages.toml");
    let server = parsed["language-server"]["verter"]
        .as_table()
        .expect("[language-server.verter]");
    let mut launch_args = server["args"]
        .as_array()
        .expect("verter args")
        .iter()
        .map(|value| value.as_str().expect("string arg").to_owned())
        .collect::<Vec<_>>();
    launch_args.push(root);
    let options = serde_json::to_value(&server["config"]).expect("serialize Helix config");

    println!(
        "{}",
        json!({
            "editor": "helix",
            "command": binary,
            "args": launch_args,
            "initializationOptions": options,
            "languages": ["vue", "svelte"],
        })
    );
}
