//! Construction of the LSP `initializationOptions` payload sent to `verter-lsp`.
//!
//! The server reads a fixed set of nested option groups (`lint`, `inlayHints`,
//! `viteConfig`, `experimental`, `hover`, `statistics`). This module projects an
//! editor's raw settings `Value` onto exactly that parity set, filling defaults
//! for absent fields and letting user-provided values win. Editor/UI-only
//! settings the clients must not forward (`configuration`, `decorations`, `mcp`,
//! `analysis`, `trace`) are never emitted: the builder constructs the output
//! POSITIVELY — it only ever inserts the known parity keys — so a non-parity key
//! is structurally unable to leak. The exact emitted key set is pinned by the
//! `top_level_key_set_is_exactly_the_emitted_parity_set` test.

use serde_json::{json, Map, Value};

/// The closed set of `lint.preset` tokens the `verter-lsp` server recognises.
///
/// Anything outside this set is unknown; the server keeps its default on an
/// unknown value, so the client clamps to [`DEFAULT_LINT_PRESET`] rather than
/// forwarding a token that would be silently ignored.
const VALID_LINT_PRESETS: [&str; 6] = [
    "essential",
    "recommended",
    "all",
    "performance",
    "a11y",
    "strict",
];

/// The `lint.preset` value emitted when none/invalid is configured.
const DEFAULT_LINT_PRESET: &str = "recommended";

/// Resolve a configured `lint.preset` value against the valid preset set.
///
/// A recognised value (exact, case-sensitive match — the server compares
/// case-sensitively) passes through; anything else clamps to
/// [`DEFAULT_LINT_PRESET`] so a stray/typo'd preset is never forwarded (the same
/// closed-token-fallthrough class as the `--type-provider` clamp). Exact match
/// over the const array only — no prefix/heuristic matching.
fn clamp_lint_preset(configured: Option<&str>) -> &'static str {
    match configured {
        Some(value) => VALID_LINT_PRESETS
            .into_iter()
            .find(|valid| *valid == value)
            .unwrap_or(DEFAULT_LINT_PRESET),
        None => DEFAULT_LINT_PRESET,
    }
}

/// Build the `initializationOptions` object from an editor's settings `Value`.
///
/// Output shape (defaults shown):
/// * `lint`: `{ enabled: false, preset: "recommended" }`
/// * `inlayHints`: `{ enabled: true }`
/// * `viteConfig`: `{ enabled: true, trustedFiles: [] }`
/// * `experimental`: `{ conditionalRootNarrowing: false, strictSlots: false }`
/// * `hover`: `{ provenance: false }`
/// * `statistics`: `{ enabled: false }`
///
/// User-provided nested values override the corresponding default; absent fields
/// fall back to the default. `lint.preset` is clamped to the server's valid set
/// (see [`clamp_lint_preset`]). Only the parity keys above are inserted, so
/// editor/UI-only settings can never leak into the output.
pub fn build_initialization_options(settings: &Value) -> Value {
    let mut out = Map::new();

    out.insert(
        "lint".to_string(),
        json!({
            "enabled": nested_bool(settings, "lint", "enabled", false),
            "preset": clamp_lint_preset(
                settings.get("lint").and_then(|g| g.get("preset")).and_then(|v| v.as_str()),
            ),
        }),
    );

    out.insert(
        "inlayHints".to_string(),
        json!({
            "enabled": nested_bool(settings, "inlayHints", "enabled", true),
        }),
    );

    out.insert(
        "viteConfig".to_string(),
        json!({
            "enabled": nested_bool(settings, "viteConfig", "enabled", true),
            "trustedFiles": nested_string_array(settings, "viteConfig", "trustedFiles"),
        }),
    );

    out.insert(
        "experimental".to_string(),
        json!({
            "conditionalRootNarrowing":
                nested_bool(settings, "experimental", "conditionalRootNarrowing", false),
            "strictSlots": nested_bool(settings, "experimental", "strictSlots", false),
        }),
    );

    out.insert(
        "hover".to_string(),
        json!({
            "provenance": nested_bool(settings, "hover", "provenance", false),
        }),
    );

    // The server defaults statistics OFF when absent; emit the field explicitly
    // so a user opt-in is honored.
    out.insert(
        "statistics".to_string(),
        json!({
            "enabled": nested_bool(settings, "statistics", "enabled", false),
        }),
    );

    Value::Object(out)
}

/// Read `settings[group][field]` as a bool, falling back to `default` when the
/// group, the field, or its bool coercion is absent.
fn nested_bool(settings: &Value, group: &str, field: &str, default: bool) -> bool {
    settings
        .get(group)
        .and_then(|g| g.get(field))
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

/// Read `settings[group][field]` as an array of strings. Absent ⇒ empty array;
/// non-string entries are dropped.
fn nested_string_array(settings: &Value, group: &str, field: &str) -> Vec<String> {
    settings
        .get(group)
        .and_then(|g| g.get(field))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn maps_lint_enabled_and_preset_from_input() {
        let settings = json!({ "lint": { "enabled": true, "preset": "strict" } });
        let out = build_initialization_options(&settings);
        assert_eq!(out["lint"]["enabled"], json!(true));
        assert_eq!(out["lint"]["preset"], json!("strict"));
    }

    #[test]
    fn all_parity_keys_present_with_correct_defaults() {
        let out = build_initialization_options(&json!({}));

        assert_eq!(out["lint"]["enabled"], json!(false));
        assert_eq!(out["lint"]["preset"], json!("recommended"));

        assert_eq!(out["inlayHints"]["enabled"], json!(true));

        assert_eq!(out["viteConfig"]["enabled"], json!(true));
        assert_eq!(out["viteConfig"]["trustedFiles"], json!([]));

        assert_eq!(
            out["experimental"]["conditionalRootNarrowing"],
            json!(false)
        );
        assert_eq!(out["experimental"]["strictSlots"], json!(false));

        assert_eq!(out["hover"]["provenance"], json!(false));

        // F1: statistics is emitted, defaulting OFF (the server defaults it OFF
        // when absent).
        assert_eq!(out["statistics"]["enabled"], json!(false));
    }

    #[test]
    fn top_level_key_set_is_exactly_the_emitted_parity_set() {
        // F7: the output top-level keys must be EXACTLY the emitted parity set —
        // a missing key AND an extra key both fail (sorted-set comparison). This
        // is the real contract guarantee: `build_initialization_options` only
        // ever inserts these keys.
        let out = build_initialization_options(&json!({
            // Throw in editor/UI-only and never-emitted keys to prove none leak.
            "configuration": { "anything": 1 },
            "decorations": { "enabled": true },
            "mcp": { "port": 9229 },
            "analysis": { "deep": true },
            "trace": { "server": "verbose" },
            "statistics": { "enabled": true },
            "frameworks": ["react"],
            "lint": { "enabled": true }
        }));
        let actual: BTreeSet<String> = out
            .as_object()
            .expect("init options is an object")
            .keys()
            .cloned()
            .collect();
        let expected: BTreeSet<String> = [
            "lint",
            "inlayHints",
            "viteConfig",
            "experimental",
            "hover",
            "statistics",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        assert_eq!(
            actual, expected,
            "init-options top-level key set drifted from the emitted parity set"
        );
        // Spot negatives: the never-emitted editor/UI-only keys and the dropped
        // `frameworks` field must be absent (covered by the exact-set check, but
        // stated explicitly as a regression tripwire).
        for forbidden in [
            "configuration",
            "decorations",
            "mcp",
            "analysis",
            "trace",
            "frameworks",
        ] {
            assert!(
                !actual.contains(forbidden),
                "{forbidden:?} must not appear in init options"
            );
        }
    }

    #[test]
    fn frameworks_field_is_not_emitted() {
        // F2: the hardcoded `frameworks` field is dropped entirely — the server
        // reads no `frameworks` field, so emitting it was dead protocol surface.
        let out = build_initialization_options(&json!({}));
        let map = out.as_object().expect("init options is an object");
        assert!(
            !map.contains_key("frameworks"),
            "frameworks must not be emitted: {out:?}"
        );
        // Even if the user supplies one, it is still not forwarded.
        let with_user = build_initialization_options(&json!({ "frameworks": ["react"] }));
        assert!(
            !with_user
                .as_object()
                .expect("object")
                .contains_key("frameworks"),
            "user-supplied frameworks must not be forwarded: {with_user:?}"
        );
    }

    #[test]
    fn statistics_enabled_defaults_off_and_honors_user_value() {
        // F1: default OFF.
        let out = build_initialization_options(&json!({}));
        assert_eq!(out["statistics"]["enabled"], json!(false));
        // User opt-in → true.
        let on = build_initialization_options(&json!({ "statistics": { "enabled": true } }));
        assert_eq!(on["statistics"]["enabled"], json!(true));
        // Non-bool coerces back to the default OFF.
        let bad = build_initialization_options(&json!({ "statistics": { "enabled": "yes" } }));
        assert_eq!(bad["statistics"]["enabled"], json!(false));
    }

    #[test]
    fn lint_preset_clamps_unknown_to_recommended() {
        // F5: an invalid/wrong-case preset clamps to the default and the invalid
        // token is never emitted; valid presets pass through.
        for bad in ["Strict", "bogus", ""] {
            let out = build_initialization_options(&json!({ "lint": { "preset": bad } }));
            assert_eq!(
                out["lint"]["preset"],
                json!("recommended"),
                "invalid preset {bad:?} must clamp to recommended: {out:?}"
            );
            assert_ne!(
                out["lint"]["preset"],
                json!(bad),
                "invalid preset {bad:?} leaked into output"
            );
        }
        // Every valid preset passes through unchanged.
        for good in [
            "essential",
            "recommended",
            "all",
            "performance",
            "a11y",
            "strict",
        ] {
            let out = build_initialization_options(&json!({ "lint": { "preset": good } }));
            assert_eq!(
                out["lint"]["preset"],
                json!(good),
                "valid preset {good:?} must pass through: {out:?}"
            );
        }
    }

    #[test]
    fn clamp_lint_preset_unit() {
        assert_eq!(clamp_lint_preset(None), "recommended");
        assert_eq!(clamp_lint_preset(Some("strict")), "strict");
        assert_eq!(clamp_lint_preset(Some("a11y")), "a11y");
        assert_eq!(clamp_lint_preset(Some("Strict")), "recommended");
        assert_eq!(clamp_lint_preset(Some("bogus")), "recommended");
        assert_eq!(clamp_lint_preset(Some("")), "recommended");
    }

    #[test]
    fn user_override_wins_over_default() {
        let settings = json!({
            "inlayHints": { "enabled": false },
            "viteConfig": { "enabled": false, "trustedFiles": ["vite.config.ts"] },
            "experimental": { "strictSlots": true },
            "hover": { "provenance": true }
        });
        let out = build_initialization_options(&settings);
        assert_eq!(out["inlayHints"]["enabled"], json!(false));
        assert_eq!(out["viteConfig"]["enabled"], json!(false));
        assert_eq!(out["viteConfig"]["trustedFiles"], json!(["vite.config.ts"]));
        assert_eq!(out["experimental"]["strictSlots"], json!(true));
        // a sibling default within a partially-overridden group is preserved
        assert_eq!(
            out["experimental"]["conditionalRootNarrowing"],
            json!(false)
        );
        assert_eq!(out["hover"]["provenance"], json!(true));
    }
}
