//! Export the parsed shipped Helix plan for the real stdio smoke.

use std::path::Path;

use serde_json::json;
use toml::Value;

fn shipping_plan(binary: &str) -> serde_json::Value {
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
    let launch_args = server["args"]
        .as_array()
        .expect("verter args")
        .iter()
        .map(|value| value.as_str().expect("string arg").to_owned())
        .collect::<Vec<_>>();
    let options = serde_json::to_value(&server["config"]).expect("serialize Helix config");

    json!({
        "editor": "helix",
        "command": binary,
        "args": launch_args,
        "workspaceRootTransport": "initialize",
        "initializationOptions": options,
        "languages": ["vue", "svelte"],
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let binary = args.next().expect("usage: helix_plan <verter-lsp>");
    assert!(args.next().is_none(), "unexpected extra argument");
    println!("{}", shipping_plan(&binary));
}

#[cfg(test)]
mod tests {
    use super::shipping_plan;

    #[test]
    fn exported_plan_preserves_the_shipped_rootless_argv() {
        let plan = shipping_plan("/bin/verter-lsp");
        assert_eq!(plan["args"], serde_json::json!(["--type-provider=tsgo"]));
        assert_eq!(plan["workspaceRootTransport"], "initialize");
    }
}
