//! Construction of the `verter-lsp` launch argument vector.
//!
//! `verter-lsp` hand-parses its CLI. The `--type-provider=<v>` flag is matched
//! against a closed set (`off | tsgo | tsserver`, recognized by the server; this
//! client emits only `tsgo`/`off` — see below); every other value silently
//! falls through to the `auto` default. That makes the *literal* provider token
//! load-bearing: a typo such as `tgo` would not error — it would quietly select
//! `auto` and defeat native-provider selection.
//!
//! A thin, SDK-less editor client (Lapce/Zed) can only *correctly* request the
//! two providers that need no TypeScript SDK: `tsgo` and `off`. `tsserver` and
//! the `auto` mode (which frequently resolves to tsserver) both require a TS SDK
//! the client never supplies (it never passes `--tsdk`), so emitting either
//! produces a config that validates but fails at runtime. This module is
//! therefore the single place that clamps the emitted provider token to the
//! client-emittable set `{tsgo, off}`, mapping everything else — including the
//! SDK-dependent `tsserver`/`auto`/`extension` and any typo — to the safe
//! default ([`DEFAULT_TYPE_PROVIDER`]).
//!
//! On the default (`tsgo`) path the argv carries *only* the provider flag and
//! the trailing workspace root: never `--tsdk`, `--plugin-path`, `--mcp-port`,
//! or `--mcp-lint-preset` (those are tsserver-only or ignored by the server).

use serde_json::Value;

/// The provider token emitted when no valid provider is configured.
///
/// `tsgo` is the literal the `verter-lsp` CLI matches; it is intentionally NOT
/// `tgo`, which would fall through to `auto` on the server side.
pub const DEFAULT_TYPE_PROVIDER: &str = "tsgo";

/// The closed set of provider tokens this SDK-less client may *emit*.
///
/// Only `tsgo` and `off` are client-emittable: both run without a TypeScript SDK.
/// The CLI *also* recognises `tsserver` and an `auto` mode, but both depend on a
/// TS SDK the client cannot supply, so they are deliberately NOT in this set and
/// clamp to [`DEFAULT_TYPE_PROVIDER`] — see [`clamp_type_provider`].
const VALID_TYPE_PROVIDERS: [&str; 2] = ["tsgo", "off"];

/// Resolve a configured `typeProvider` value against the client-emittable set.
///
/// Returns the value unchanged only for the two SDK-free providers this client
/// can correctly request (`tsgo`, `off`). Everything else clamps to
/// [`DEFAULT_TYPE_PROVIDER`] so a stray/typo'd value never reaches the CLI.
///
/// Note: the recognised-but-SDK-dependent values `tsserver`, `auto`, and
/// `extension` are intentionally clamped to `tsgo` here. A SDK-less editor
/// client never passes `--tsdk`, so requesting those would yield a config that
/// validates but fails at runtime depending on workspace shape; clamping keeps
/// the launch contract correct. Surfacing a user-visible warning when an
/// explicitly-configured value is clamped is the host's concern (this pure crate
/// has no logging channel); the clamp itself is documented here so the behavior
/// is discoverable.
pub fn clamp_type_provider(configured: Option<&str>) -> &'static str {
    match configured {
        Some(value) => VALID_TYPE_PROVIDERS
            .into_iter()
            .find(|valid| *valid == value)
            .unwrap_or(DEFAULT_TYPE_PROVIDER),
        None => DEFAULT_TYPE_PROVIDER,
    }
}

/// Build the `verter-lsp` launch argv.
///
/// Layout: `["--type-provider=<P>", <filtered user lsp.serverArgs extras...>, <root>]`.
///
/// * Exactly one `--type-provider=` argument is emitted; the provider always
///   precedes any user extras.
/// * `root` (when `Some`) is appended LAST and is a positional (non-`--`) token,
///   because a wasm editor client's cwd is not the workspace.
/// * User extras are read from `settings.lsp.serverArgs` (a JSON string array);
///   non-string entries are ignored.
///
/// The crate owns three parts of the launch contract, so user extras that would
/// collide with them are DROPPED before being appended (only `--`-prefixed flags
/// OTHER than the crate-owned ones survive):
/// * the `--type-provider` namespace (any entry with that prefix) — the crate
///   emits the single, clamped provider flag; a user duplicate/override would
///   defeat the clamp.
/// * the `--tsdk` namespace (any entry with that prefix) — these SDK-less clients
///   never supply a TS SDK, so a user `--tsdk` is meaningless/misleading here.
/// * any bare positional token (one that does not start with `--`) — the crate
///   owns the single positional (the trailing workspace root); a user positional
///   would be mis-parsed by the server's `CliArgs` as a second/overriding root.
///
/// Surviving extras are appended after the provider flag and before the trailing
/// root, so the root stays last.
pub fn build_server_args(root: Option<&str>, settings: &Value) -> Vec<String> {
    let provider = clamp_type_provider(
        settings
            .get("typeProvider")
            .and_then(|value| value.as_str()),
    );

    let mut args = Vec::new();
    args.push(format!("--type-provider={provider}"));

    // User extras: `settings.lsp.serverArgs`, a JSON string array. Non-string
    // entries are ignored; surviving string entries pass through verbatim, in
    // order, after the provider flag — but crate-owned args are filtered first
    // to protect the launch contract (see the fn docstring).
    if let Some(extras) = settings
        .get("lsp")
        .and_then(|lsp| lsp.get("serverArgs"))
        .and_then(|value| value.as_array())
    {
        for extra in extras {
            if let Some(s) = extra.as_str() {
                if is_crate_owned_arg(s) {
                    continue;
                }
                args.push(s.to_string());
            }
        }
    }

    // The workspace root is a trailing positional argument: a wasm client's cwd
    // is not the workspace, so the server must receive the root explicitly.
    if let Some(root) = root {
        args.push(root.to_string());
    }

    args
}

/// Whether a user-supplied `lsp.serverArgs` entry collides with an arg the crate
/// owns and must therefore be dropped from the passthrough.
///
/// Crate-owned: the `--type-provider` and `--tsdk` flag NAMESPACES, plus the
/// single trailing positional (any non-`--` token). The two flags are dropped by
/// namespace PREFIX (`--type-provider*`, `--tsdk*`) — not just the exact flag and
/// the `--flag=value` spelling — because the `verter-lsp` CLI recognises no other
/// flag in those prefixes, so any entry in the namespace is crate-owned (the crate
/// emits the single clamped `--type-provider=` flag and never `--tsdk`). An
/// unrelated flag outside the namespace (e.g. `--type-checker`, which does not
/// start with `--type-provider`) is NOT owned and passes through. See
/// [`build_server_args`].
fn is_crate_owned_arg(arg: &str) -> bool {
    // The crate owns the single trailing positional (the workspace root); any
    // bare (non-flag) user token would become a second/overriding root.
    if !arg.starts_with("--") {
        return true;
    }
    // The crate owns the `--type-provider` and `--tsdk` namespaces wholesale: drop
    // anything in those prefixes (subsumes both `--flag` and `--flag=value`).
    ["--type-provider", "--tsdk"]
        .into_iter()
        .any(|owned| arg.starts_with(owned))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider_args(args: &[String]) -> Vec<&String> {
        args.iter()
            .filter(|a| a.starts_with("--type-provider="))
            .collect()
    }

    #[test]
    fn default_config_produces_exact_tsgo_argv() {
        // THE anchor test: an empty settings object selects the default provider
        // and yields provider-flag + trailing root, nothing else.
        let settings = json!({});
        let args = build_server_args(Some("/work/space"), &settings);
        assert_eq!(
            args,
            vec![
                "--type-provider=tsgo".to_string(),
                "/work/space".to_string()
            ]
        );
    }

    #[test]
    fn invalid_provider_clamps_to_tsgo_and_never_emits_invalid_token() {
        // Guard test (§1a revert-test target): a typo such as `tgo` must clamp to
        // `tsgo` and the literal `--type-provider=tgo` must NEVER be emitted.
        for bad in ["tgo", "bogus", "TSGO", ""] {
            let settings = json!({ "typeProvider": bad });
            let args = build_server_args(Some("/ws"), &settings);
            assert!(
                args.contains(&"--type-provider=tsgo".to_string()),
                "expected clamp to tsgo for input {bad:?}, got {args:?}"
            );
            assert!(
                !args.contains(&format!("--type-provider={bad}")),
                "invalid provider token {bad:?} leaked into argv {args:?}"
            );
        }
        // Spell out the exact `tgo`-absent assertion the guard relies on.
        let args = build_server_args(Some("/ws"), &json!({ "typeProvider": "tgo" }));
        assert!(!args.contains(&"--type-provider=tgo".to_string()));
    }

    #[test]
    fn tsgo_path_omits_tsserver_and_mcp_flags() {
        // Even when the input settings carry tsdk / plugin / mcp values, the
        // tsgo path must not forward them.
        let settings = json!({
            "typeProvider": "tsgo",
            "tsdk": "/some/tsdk",
            "pluginPath": "/some/plugin",
            "mcpPort": 9229,
            "mcpLintPreset": "strict",
            "lsp": { "tsdk": "/x", "pluginPath": "/y" }
        });
        let args = build_server_args(Some("/ws"), &settings);
        for forbidden_prefix in [
            "--tsdk=",
            "--plugin-path=",
            "--mcp-port=",
            "--mcp-lint-preset=",
        ] {
            assert!(
                !args.iter().any(|a| a.starts_with(forbidden_prefix)),
                "tsgo argv must not contain {forbidden_prefix}: {args:?}"
            );
        }
    }

    #[test]
    fn literal_latest_is_never_synthesised_and_bare_token_is_filtered() {
        // `latest` is a release-asset concern, never a launch arg: the builder must
        // never synthesise it on its own.
        let clean = build_server_args(Some("/ws"), &json!({}));
        assert!(
            !clean.iter().any(|a| a.contains("latest")),
            "builder synthesised `latest`: {clean:?}"
        );
        // A user-supplied bare `latest` token is a positional and is filtered by the
        // F6 launch-contract guard (the crate owns the single positional root); the
        // `--`-flag sibling survives.
        let settings = json!({
            "typeProvider": "tsgo",
            "lsp": { "serverArgs": ["--something", "latest"] }
        });
        let with_extra = build_server_args(Some("/ws"), &settings);
        assert!(
            with_extra.contains(&"--something".to_string()),
            "benign --something flag must survive: {with_extra:?}"
        );
        assert!(
            !with_extra.iter().any(|a| a == "latest"),
            "bare positional `latest` must be filtered: {with_extra:?}"
        );
        assert_eq!(
            with_extra.last().unwrap(),
            "/ws",
            "root must stay last: {with_extra:?}"
        );
    }

    #[test]
    fn tsserver_clamps_to_tsgo_and_is_never_emitted() {
        // A SDK-less editor client cannot supply a TS SDK, so `tsserver` (and the
        // SDK-dependent `auto`) are NOT client-emittable: they clamp to `tsgo`.
        for sdk_dependent in ["tsserver", "auto", "extension"] {
            let settings = json!({ "typeProvider": sdk_dependent });
            let args = build_server_args(Some("/ws"), &settings);
            assert!(
                args.contains(&"--type-provider=tsgo".to_string()),
                "{sdk_dependent:?} must clamp to tsgo: {args:?}"
            );
            assert!(
                !args.contains(&format!("--type-provider={sdk_dependent}")),
                "SDK-dependent provider {sdk_dependent:?} leaked into argv: {args:?}"
            );
        }
    }

    #[test]
    fn exactly_one_type_provider_arg_for_emittable_set() {
        // Only the emittable set ({tsgo, off}) round-trips as itself; everything
        // else clamps to tsgo. Either way exactly one provider flag is emitted.
        let cases = [
            ("tsgo", "tsgo"),
            ("off", "off"),
            ("tsserver", "tsgo"),
            ("auto", "tsgo"),
        ];
        for (configured, expected) in cases {
            let settings = json!({ "typeProvider": configured });
            let args = build_server_args(Some("/ws"), &settings);
            assert_eq!(
                provider_args(&args).len(),
                1,
                "expected exactly one --type-provider= for {configured:?}: {args:?}"
            );
            assert!(
                args.contains(&format!("--type-provider={expected}")),
                "configured {configured:?} should emit {expected:?}: {args:?}"
            );
        }
    }

    #[test]
    fn root_is_trailing_and_non_dashdash() {
        let args = build_server_args(Some("/ws/root"), &json!({}));
        let last = args.last().expect("argv not empty");
        assert_eq!(last, "/ws/root");
        assert!(
            !last.starts_with("--"),
            "trailing root must be positional: {last:?}"
        );
    }

    #[test]
    fn none_root_yields_provider_only() {
        let args = build_server_args(None, &json!({}));
        assert_eq!(args, vec!["--type-provider=tsgo".to_string()]);
    }

    #[test]
    fn user_server_args_passthrough_with_provider_first_and_root_last() {
        let settings = json!({
            "typeProvider": "tsgo",
            "lsp": { "serverArgs": ["--foo", "--bar=baz", 42, true, "--qux"] }
        });
        let args = build_server_args(Some("/ws"), &settings);
        // provider precedes extras
        assert_eq!(args[0], "--type-provider=tsgo");
        // non-string extras (42, true) are ignored; strings pass through in order
        let body: Vec<String> = args[1..args.len() - 1].to_vec();
        assert_eq!(
            body,
            vec![
                "--foo".to_string(),
                "--bar=baz".to_string(),
                "--qux".to_string()
            ]
        );
        // root stays last
        assert_eq!(args.last().unwrap(), "/ws");
    }

    #[test]
    fn server_args_passthrough_protects_crate_owned_args() {
        // F6: the crate owns the provider flag, the (non-existent) `--tsdk`, and
        // the single trailing positional root. A user `lsp.serverArgs` that tries
        // to inject any of those must be filtered; only OTHER `--`-flags survive.
        let settings = json!({
            "typeProvider": "tsgo",
            "lsp": { "serverArgs": [
                "--type-provider=tsserver", // crate owns the provider flag → dropped
                "--tsdk=/x",                // SDK-less client → dropped
                "extrapositional",          // bare positional → dropped (root is owned)
                "--foo=1"                   // benign flag → survives
            ] }
        });
        let args = build_server_args(Some("/ws/root"), &settings);

        // Exactly one provider flag, and it is the crate's tsgo (not the user tsserver).
        assert_eq!(
            provider_args(&args).len(),
            1,
            "exactly one --type-provider= must survive: {args:?}"
        );
        assert!(
            args.contains(&"--type-provider=tsgo".to_string()),
            "crate-owned tsgo provider must win: {args:?}"
        );
        assert!(
            !args.iter().any(|a| a.contains("tsserver")),
            "user --type-provider=tsserver must be dropped: {args:?}"
        );
        // No --tsdk leaks through.
        assert!(
            !args.iter().any(|a| a.starts_with("--tsdk")),
            "user --tsdk must be dropped: {args:?}"
        );
        // The bare positional is gone; the crate-owned root is the LAST arg.
        assert!(
            !args.iter().any(|a| a == "extrapositional"),
            "bare positional must be dropped: {args:?}"
        );
        assert_eq!(
            args.last().unwrap(),
            "/ws/root",
            "the crate-owned root must be the trailing positional, not a user one: {args:?}"
        );
        // The benign flag survives, after the provider and before the root.
        assert!(
            args.contains(&"--foo=1".to_string()),
            "benign --foo=1 must survive: {args:?}"
        );
        let foo_idx = args.iter().position(|a| a == "--foo=1").unwrap();
        assert!(
            foo_idx > 0,
            "extra must come after the provider flag: {args:?}"
        );
        assert!(
            foo_idx < args.len() - 1,
            "extra must come before the trailing root: {args:?}"
        );
    }

    #[test]
    fn server_args_drops_crate_owned_namespace_prefixes_not_just_exact_or_eq() {
        // `--type-provider` and `--tsdk` are crate-owned NAMESPACES: the verter-lsp
        // CLI recognises no other flag in those prefixes, so the whole prefix is
        // dropped — not just the exact flag (`--tsdk`) and the `=value` spelling
        // (`--tsdk=/x`). The three no-`=` namespace entries below
        // (`--type-providerfoo`, `--tsdk-path`, `--tsdkx`) must NOT leak.
        let settings = json!({
            "typeProvider": "tsgo",
            "lsp": { "serverArgs": [
                "--type-providerfoo", // no-`=` namespace entry → must drop
                "--tsdk-path",        // no-`=` namespace entry → must drop
                "--tsdkx",            // no-`=` namespace entry → must drop
                "--type-provider=tsserver", // `=` spelling → must drop (passthrough)
                "--tsdk=/x",          // `=` spelling → must drop
                "--type-checker",     // NEGATIVE CONTROL: NOT in --type-provider ns → survives
                "--foo=1"             // benign flag → survives
            ] }
        });
        let args = build_server_args(Some("/ws/root"), &settings);

        // None of the namespace entries (no-`=` or `=`) leak as a passthrough.
        for leaked in ["--type-providerfoo", "--tsdk-path", "--tsdkx", "--tsdk=/x"] {
            assert!(
                !args.iter().any(|a| a == leaked),
                "crate-owned namespace entry {leaked:?} leaked: {args:?}"
            );
        }
        // The user's `--type-provider=tsserver` must not survive as a passthrough,
        // and no `tsserver` token may appear anywhere.
        assert!(
            !args.iter().any(|a| a.contains("tsserver")),
            "user --type-provider=tsserver must be dropped: {args:?}"
        );

        // The crate still emits its single clamped provider flag.
        assert_eq!(
            provider_args(&args).len(),
            1,
            "exactly one --type-provider= must survive: {args:?}"
        );
        assert!(
            args.contains(&"--type-provider=tsgo".to_string()),
            "crate-owned --type-provider=tsgo must be present: {args:?}"
        );

        // NEGATIVE CONTROL: `--type-checker` does NOT start with `--type-provider`
        // (the namespace is `--type-provider`, not `--type`), so it must survive —
        // the prefix fix must not over-drop unrelated flags.
        assert!(
            args.contains(&"--type-checker".to_string()),
            "unrelated --type-checker must NOT be over-dropped: {args:?}"
        );

        // The benign flag survives.
        assert!(
            args.contains(&"--foo=1".to_string()),
            "benign --foo=1 must survive: {args:?}"
        );

        // Root stays LAST.
        assert_eq!(
            args.last().unwrap(),
            "/ws/root",
            "root must stay last: {args:?}"
        );
    }

    #[test]
    fn benign_server_args_pass_through_with_root_last() {
        // A single benign flag survives and the root remains the trailing token.
        let settings = json!({ "lsp": { "serverArgs": ["--foo"] } });
        let args = build_server_args(Some("/ws"), &settings);
        assert!(
            args.contains(&"--foo".to_string()),
            "benign flag dropped: {args:?}"
        );
        assert_eq!(args.last().unwrap(), "/ws", "root must stay last: {args:?}");
    }

    #[test]
    fn clamp_type_provider_unit() {
        // Emittable set: only tsgo + off round-trip as themselves.
        assert_eq!(clamp_type_provider(None), "tsgo");
        assert_eq!(clamp_type_provider(Some("tsgo")), "tsgo");
        assert_eq!(clamp_type_provider(Some("off")), "off");
        // Recognised-but-SDK-dependent values clamp to tsgo (NOT pass-through):
        // an SDK-less client cannot supply the `--tsdk` they require.
        assert_eq!(clamp_type_provider(Some("tsserver")), "tsgo");
        assert_eq!(clamp_type_provider(Some("auto")), "tsgo");
        assert_eq!(clamp_type_provider(Some("extension")), "tsgo");
        // unknown / typo / wrong-case clamp to the default
        assert_eq!(clamp_type_provider(Some("tgo")), "tsgo");
        assert_eq!(clamp_type_provider(Some("TSGO")), "tsgo");
        assert_eq!(clamp_type_provider(Some("")), "tsgo");
        assert_eq!(clamp_type_provider(Some("bogus")), "tsgo");
    }
}
