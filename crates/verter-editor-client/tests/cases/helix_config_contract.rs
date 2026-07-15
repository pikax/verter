//! Contract test for the shipped Helix `languages.toml` snippet.
//!
//! Helix has a built-in native LSP client, so Verter's Helix support is pure
//! `languages.toml` configuration (no compiled extension, no server-side
//! change). The shipped snippet lives at `editors/helix/languages.toml`. This
//! test parses it and asserts every load-bearing field of the launch contract,
//! tying the snippet's `args` back to the SHARED launch contract
//! ([`verter_editor_client::build_server_args`]) so the two cannot drift.
//!
//! It is hermetic: no Helix binary and no `verter-lsp` process. Semantic meaning
//! is read from parsed TOML values, never from string-scraping the file.

use std::path::{Path, PathBuf};

use serde_json::json;
use toml::Value;

/// Locate the shipped `editors/helix/languages.toml` relative to this crate's
/// manifest dir (`crates/verter-editor-client`). Built with `Path::join` so the
/// path is correct on Windows, macOS, and Linux (no hardcoded separators).
fn languages_toml_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..") // crates/
        .join("..") // repo root
        .join("editors")
        .join("helix")
        .join("languages.toml")
}

fn load_languages_toml() -> Value {
    let path = languages_toml_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    // Parsing here also proves the shipped snippet is valid TOML.
    toml::from_str::<Value>(&text)
        .unwrap_or_else(|e| panic!("editors/helix/languages.toml is not valid TOML: {e}"))
}

/// `[language-server.verter]` as a table.
fn verter_server(root: &Value) -> &toml::value::Table {
    root.get("language-server")
        .and_then(Value::as_table)
        .and_then(|t| t.get("verter"))
        .and_then(Value::as_table)
        .expect("[language-server.verter] table must exist")
}

/// `[language-server.verter].args` as a `Vec<String>`.
fn verter_args(root: &Value) -> Vec<String> {
    verter_server(root)
        .get("args")
        .and_then(Value::as_array)
        .expect("[language-server.verter].args must be an array")
        .iter()
        .map(|v| v.as_str().expect("every arg must be a string").to_string())
        .collect()
}

/// Find the `[[language]]` entry whose `name` equals `name`.
fn language_entry<'a>(root: &'a Value, name: &str) -> &'a toml::value::Table {
    root.get("language")
        .and_then(Value::as_array)
        .expect("at least one [[language]] entry must exist")
        .iter()
        .filter_map(Value::as_table)
        .find(|t| t.get("name").and_then(Value::as_str) == Some(name))
        .unwrap_or_else(|| panic!("[[language]] name = {name:?} must exist"))
}

/// `language-servers` of a `[[language]]` entry as a list of server names.
/// Helix allows entries to be a bare string or a `{ name, ... }` table; this
/// extracts the server name from either shape.
fn language_servers(entry: &toml::value::Table) -> Vec<String> {
    entry
        .get("language-servers")
        .and_then(Value::as_array)
        .expect("language-servers must be an array")
        .iter()
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Table(t) => t
                .get("name")
                .and_then(Value::as_str)
                .expect("a table language-server entry must carry a name")
                .to_string(),
            other => panic!("unexpected language-server entry shape: {other:?}"),
        })
        .collect()
}

/// THE ANCHOR ASSERTION: the snippet's `args` MUST equal the shared launch
/// contract for a no-root, default-settings launch. Helix injects the workspace
/// root via `workspaceFolders`, not argv, so the contract call uses `None`. If
/// the contract's provider flag ever changes (e.g. `DEFAULT_TYPE_PROVIDER`),
/// this fails until `languages.toml` is updated — the load-bearing SSoT tie.
#[test]
fn args_equal_shared_launch_contract() {
    let root = load_languages_toml();
    let args = verter_args(&root);

    let expected = verter_editor_client::build_server_args(None, &json!({}));
    assert_eq!(
        args, expected,
        "editors/helix/languages.toml args must equal build_server_args(None, {{}}) \
         (Helix sends the root via workspaceFolders, not argv); got {args:?}, expected {expected:?}"
    );

    // The contract for a default no-root launch is exactly the tsgo provider.
    assert_eq!(expected, vec!["--type-provider=tsgo".to_string()]);

    // And the command launches the native binary.
    let command = verter_server(&root)
        .get("command")
        .and_then(Value::as_str)
        .expect("[language-server.verter].command must be a string");
    assert_eq!(command, "verter-lsp", "command must launch verter-lsp");
}

/// Discriminating arg-shape assertions, each catching a real drift.
#[test]
fn args_carry_tsgo_and_no_forbidden_tokens() {
    let root = load_languages_toml();
    let args = verter_args(&root);

    // The literal provider token is load-bearing.
    assert!(
        args.contains(&"--type-provider=tsgo".to_string()),
        "args must contain --type-provider=tsgo: {args:?}"
    );
    // The `tgo` typo silently falls through to `auto` server-side — it must
    // never appear.
    assert!(
        !args.contains(&"--type-provider=tgo".to_string()),
        "args must NOT contain the --type-provider=tgo typo: {args:?}"
    );

    // tsserver-only flags must not appear: this client never supplies a TS SDK.
    assert!(
        !args.iter().any(|a| a.starts_with("--tsdk")),
        "args must NOT carry any --tsdk* flag: {args:?}"
    );
    assert!(
        !args.iter().any(|a| a.starts_with("--plugin-path")),
        "args must NOT carry any --plugin-path* flag: {args:?}"
    );

    // Helix injects no root, so there must be NO positional (non-`--`) arg.
    assert!(
        args.iter().all(|a| a.starts_with("--")),
        "args must contain no positional (non --) token — Helix sends the root \
         via workspaceFolders: {args:?}"
    );
}

/// The `config` table is EXACTLY the server-read init parity set — no more, no
/// less — AND it equals the shared SSoT `build_initialization_options(&{})`. An
/// ignored key would "lie to users"; a missing parity key would silently desync
/// the Helix config from the launch contract.
#[test]
fn config_keys_are_exactly_the_server_read_subset() {
    let root = load_languages_toml();
    let config = verter_server(&root)
        .get("config")
        .and_then(Value::as_table)
        .expect("[language-server.verter].config must be a table");

    // Exactly these six server-read keys, asserted by set equality. `statistics`
    // is server-read (lifecycle.rs reads `initializationOptions.statistics`), so
    // it is part of the parity set, shipped OFF by default.
    let mut keys: Vec<&str> = config.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut expected = vec![
        "experimental",
        "hover",
        "inlayHints",
        "lint",
        "statistics",
        "viteConfig",
    ];
    expected.sort_unstable();
    assert_eq!(
        keys, expected,
        "config keys must be EXACTLY the server-read parity set {expected:?}, got {keys:?}"
    );

    // Each server-read key is present (redundant with set-equality, but states
    // the positive contract explicitly).
    for present in [
        "lint",
        "inlayHints",
        "viteConfig",
        "experimental",
        "hover",
        "statistics",
    ] {
        assert!(
            config.contains_key(present),
            "config must contain the server-read key {present:?}: {config:?}"
        );
    }

    // Genuinely-omitted / not-read keys must be ABSENT. (`statistics` is NOT in
    // this list — it IS server-read and shipped explicitly OFF.)
    for absent in ["configuration", "mcp", "decorations", "frameworks"] {
        assert!(
            !config.contains_key(absent),
            "config must NOT contain the non-server-read key {absent:?}: {config:?}"
        );
    }

    // THE SSoT CONFIG ANCHOR: the shipped `config` table MUST equal the shared
    // launch contract's default init-options object, mirroring the `args` anchor.
    // Convert the parsed TOML `config` table to `serde_json::Value` (toml::Value
    // is Serialize) and compare typed values — no string-scraping. If the SSoT
    // adds/removes a parity key, or a default flips in the toml, this fails RED
    // until the Helix config is re-aligned. (`build_initialization_options`
    // clamps `lint.preset` and fills defaults; `&json!({})` yields exactly the
    // six-key object the shipped table must match value-for-value.)
    let config_json: serde_json::Value = serde_json::to_value(Value::Table(config.clone()))
        .expect("parsed TOML config table must convert to serde_json::Value");
    let expected_options = verter_editor_client::build_initialization_options(&json!({}));
    assert_eq!(
        config_json, expected_options,
        "editors/helix/languages.toml config must equal \
         build_initialization_options(&{{}}) (the SSoT parity set); \
         got {config_json:#}, expected {expected_options:#}"
    );
}

/// Both carrier languages attach verter, and BOTH drop the built-in server
/// (the replace decision). The list must be EXACTLY `["verter"]` — an extra
/// attached server would reintroduce Helix's merged diagnostics/completion/
/// code-action double-publish, which exact equality catches (set-membership
/// alone would not).
#[test]
fn vue_and_svelte_attach_verter_replacing_builtins() {
    let root = load_languages_toml();

    for (lang, builtin) in [("vue", "vuels"), ("svelte", "svelteserver")] {
        let entry = language_entry(&root, lang);
        let servers = language_servers(entry);
        // Exact equality subsumes "contains verter" AND "no built-in" AND "no
        // accidental extra server" in one assertion.
        assert_eq!(
            servers,
            vec!["verter".to_string()],
            "[[language]] {lang:?} language-servers must be EXACTLY [\"verter\"] \
             (replace the built-in {builtin:?}, no merged second server): {servers:?}"
        );
    }
}

/// Negative shape: the vue/svelte entries are MINIMAL overrides — they do not
/// restate Helix-owned metadata and do not set `language-id`; and the default
/// snippet does not set `required-root-patterns` (opt-in only).
#[test]
fn entries_are_minimal_overrides_and_no_opt_in_gating() {
    let root = load_languages_toml();

    for lang in ["vue", "svelte"] {
        let entry = language_entry(&root, lang);
        for frozen in [
            "grammar",
            "scope",
            "file-types",
            "roots",
            "injection-regex",
            "language-id",
        ] {
            assert!(
                !entry.contains_key(frozen),
                "[[language]] {lang:?} must NOT restate Helix-owned {frozen:?} \
                 (minimal override preserves built-ins by per-field merge): {entry:?}"
            );
        }
    }

    // The default snippet must not ship `required-root-patterns` — gating is an
    // opt-in README knob, not a default.
    let server = verter_server(&root);
    assert!(
        !server.contains_key("required-root-patterns"),
        "default snippet must NOT set required-root-patterns (opt-in only): {server:?}"
    );
}
