//! Verter Zed extension — a thin LSP launcher that tells Zed to spawn the native
//! `verter-lsp` binary over stdio for `.vue` / `.svelte` files.
//!
//! The extension owns no semantics. In `language_server_command` it asks the
//! shared launch-contract crate ([`verter_editor_client`]) for the `verter-lsp`
//! argv, the binary source to launch, and (in
//! `language_server_initialization_options`) the `initializationOptions` payload,
//! then hands Zed a [`zed::Command`]. Every launch-contract decision (the
//! `--type-provider=tsgo` clamp, the server-read option parity set, the discovery
//! precedence) lives in that one shared crate so the Zed and Lapce clients cannot
//! diverge. PATH resolution itself is delegated to Zed's `worktree.which`, which
//! returns the absolute path (handling `.exe`/`PATHEXT` on Windows).
//!
//! # Thin launcher — out of the per-message hot path
//!
//! This extension is a launcher and nothing more. After `language_server_command`
//! returns a [`zed::Command`], Zed's native LSP client spawns the process and
//! speaks LSP directly to it over stdio; the WASM extension is NOT on the
//! per-LSP-message path. The `Extension` trait exposes no per-request hook — there
//! is no marshaling, transform, or proxy here for diagnostics/hover/completion. The
//! extension therefore adds zero latency to every LSP request.
//!
//! # Dual-target structure
//!
//! The decision surface ([`plan_launch`], the [`LaunchPlan`]/[`LaunchError`]
//! types) is pure — std + `serde_json` + [`verter_editor_client`] only — so it
//! compiles and unit-tests on the host toolchain. The real `zed_extension_api`
//! glue (`register_extension!`, the `impl zed::Extension` translating
//! `Worktree` (`root_path` / `which`) and `LspSettings` into the pure functions)
//! lives behind `#[cfg(target_os = "wasi")]` and is built only for the
//! `wasm32-wasip2` extension artifact.

#![forbid(unsafe_code)]

use std::fmt;

use serde_json::Value;
use verter_editor_client::{
    DiscoveryError, DiscoveryInputs, ServerSource, build_server_args, resolve_server,
};

/// The exact, fully-resolved instruction the extension hands Zed: which binary to
/// launch and with which arguments. The `initializationOptions` are produced
/// separately (Zed pulls them via a distinct trait method), so they are not part
/// of this plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// The resolved `verter-lsp` binary to spawn. For an override or managed
    /// source this is an absolute path; for a PATH source it is the absolute path
    /// the host already resolved on `PATH` (Zed's `worktree.which`, which handles
    /// the `.exe`/`PATHEXT` lookup itself) — never a re-derived bare basename.
    pub command_path: String,
    /// The `verter-lsp` CLI argument vector (from
    /// [`verter_editor_client::build_server_args`]): the clamped
    /// `--type-provider=tsgo` flag, any surviving user extras, and the trailing
    /// positional workspace root.
    pub args: Vec<String>,
}

/// Why the extension could not produce a launch plan.
///
/// Wraps the shared crate's [`DiscoveryError`] so its two distinct reasons
/// (a PATH binary found but not opted into, versus nothing usable anywhere) are
/// preserved, while [`fmt::Display`] augments the message with actionable guidance
/// that names the `lsp.verter.binary.path` override and the
/// `serverSource = "path"` opt-in — guidance the pure crate's `Display` cannot
/// carry on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchError {
    /// The underlying discovery decision that failed.
    source: DiscoveryError,
}

impl LaunchError {
    /// The wrapped [`DiscoveryError`], so callers can still branch on its distinct
    /// variants ([`DiscoveryError::PathFoundButNotOptedIn`] vs
    /// [`DiscoveryError::NothingResolved`]).
    pub fn discovery_error(&self) -> &DiscoveryError {
        &self.source
    }
}

impl From<DiscoveryError> for LaunchError {
    fn from(source: DiscoveryError) -> Self {
        LaunchError { source }
    }
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Keep the shared crate's distinct reason, then append actionable,
        // variant-tailored guidance naming the Zed settings keys the user can set,
        // and a pointer to the editor docs page for the full setup steps.
        write!(f, "{}", self.source)?;
        match self.source {
            DiscoveryError::PathFoundButNotOptedIn { .. } => f.write_str(
                ". Set `lsp.verter.binary.path` to the absolute path of a verter-lsp \
                 binary, or opt into PATH discovery with \
                 `lsp.verter.settings.serverSource = \"path\"` \
                 (see the Zed README or https://verterjs.dev/editor/other-editors).",
            ),
            DiscoveryError::NothingResolved { .. } => f.write_str(
                ". Set `lsp.verter.binary.path` to the absolute path of a verter-lsp \
                 binary, or install verter-lsp on your PATH and opt in with \
                 `lsp.verter.settings.serverSource = \"path\"` \
                 (see the Zed README or https://verterjs.dev/editor/other-editors).",
            ),
        }
    }
}

impl std::error::Error for LaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Read the `serverSource = "path"` PATH-discovery opt-in from the server
/// settings blob (`lsp.verter.settings` in Zed).
///
/// Part of the host-testable surface: the WASI glue reads this to decide whether
/// to consult `worktree.which("verter-lsp")` at all, and passes the result as
/// `plan_launch`'s `path_opt_in`.
pub fn path_source_opt_in(settings: &Value) -> bool {
    settings.get("serverSource").and_then(Value::as_str) == Some("path")
}

/// Map a resolved [`ServerSource`] to the command path Zed spawns.
///
/// Override/managed sources carry an absolute path; a PATH source carries the
/// absolute path the host already resolved on `PATH` (Zed's `worktree.which`) —
/// never a re-derived bare basename.
fn server_source_command(source: &ServerSource) -> String {
    source.path().to_string()
}

/// Build the [`LaunchPlan`] from the resolved workspace root, the user's verter
/// server settings, an optional explicit binary override, and an optional real
/// PATH hit the host already resolved.
///
/// All launch-contract logic is delegated to [`verter_editor_client`]:
/// * discovery precedence → [`resolve_server`] over [`DiscoveryInputs`]. The
///   `override_path` comes from the host's `lsp.verter.binary.path`. The
///   `path_found` is an INJECTED real host signal — the absolute path Zed's
///   `worktree.which("verter-lsp")` resolved on `PATH` (which performs the
///   `.exe`/`PATHEXT` lookup itself), or `None` when the binary is not on `PATH`.
///   This function NEVER synthesizes a PATH hit from the platform matrix: a
///   missing PATH binary under an active opt-in resolves to
///   [`DiscoveryError::NothingResolved`] and fails loud. A managed binary is
///   never present in this interim (no managed download yet).
/// * argv → [`build_server_args`], with `root` forwarded as the trailing
///   positional (a WASI extension's cwd is not the workspace). User
///   `binary.arguments` reach the builder through `settings.lsp.serverArgs` (see
///   [`merge_binary_arguments_into_settings`]), so they are filtered + ordered by
///   the shared contract rather than appended raw.
///
/// `path_opt_in` mirrors `settings.serverSource == "path"` and is passed by the
/// caller so the injected `path_found` is honored only under an explicit opt-in;
/// a PATH hit found WITHOUT the opt-in surfaces the distinct
/// [`DiscoveryError::PathFoundButNotOptedIn`] reason.
///
/// Returns a loud, actionable [`LaunchError`] when no binary source resolves; the
/// caller must surface it and NOT launch a server.
pub fn plan_launch(
    workspace_root: Option<&str>,
    settings: &Value,
    binary_override: Option<&str>,
    path_found: Option<&str>,
    path_opt_in: bool,
) -> Result<LaunchPlan, LaunchError> {
    let override_path = binary_override
        .map(str::trim)
        .filter(|path| !path.is_empty());

    let inputs = DiscoveryInputs {
        override_path,
        // No managed download exists yet; a managed binary is never present here.
        managed_present: None,
        path_opt_in,
        // The injected real host PATH hit — never a fabricated basename.
        path_found,
    };

    let source = resolve_server(&inputs)?;

    Ok(LaunchPlan {
        command_path: server_source_command(&source),
        args: build_server_args(workspace_root, settings),
    })
}

/// Merge user-supplied `binary.arguments` into `settings.lsp.serverArgs` so they
/// flow through the shared [`build_server_args`] filtering/ordering instead of
/// being appended raw after the contract's argv.
///
/// Routing extras through the contract is what keeps the workspace root LAST and
/// drops crate-owned args a user could otherwise reinject (a later
/// `--type-provider=` would defeat the `tsgo` clamp; a bare positional would
/// override the root). The merge is purely structural (assembling the settings
/// `Value`); the filtering lives in the shared crate.
///
/// Order: any `binary_args` are appended AFTER any pre-existing
/// `settings.lsp.serverArgs` entries, preserving deterministic precedence (the
/// user's `settings`-blob `serverArgs` come first, then the `binary.arguments`).
/// Non-array / non-object existing shapes are replaced with a fresh array so the
/// merge is total. Returns `settings` unchanged when `binary_args` is empty.
pub fn merge_binary_arguments_into_settings(
    mut settings: Value,
    binary_args: Vec<String>,
) -> Value {
    if binary_args.is_empty() {
        return settings;
    }

    // Ensure `settings` is an object so `settings["lsp"]["serverArgs"]` can be set.
    if !settings.is_object() {
        settings = Value::Object(serde_json::Map::new());
    }
    let root = settings
        .as_object_mut()
        .expect("settings was just coerced to an object");

    let lsp = root
        .entry("lsp")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !lsp.is_object() {
        *lsp = Value::Object(serde_json::Map::new());
    }
    let lsp = lsp
        .as_object_mut()
        .expect("lsp was just coerced to an object");

    // Collect any pre-existing string `serverArgs`, then append the binary args.
    let mut merged: Vec<Value> = lsp
        .get("serverArgs")
        .and_then(Value::as_array)
        .map(|existing| {
            existing
                .iter()
                .filter(|entry| entry.is_string())
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    merged.extend(binary_args.into_iter().map(Value::String));

    lsp.insert("serverArgs".to_string(), Value::Array(merged));
    settings
}

// ---------------------------------------------------------------------------
// WASI extension glue — only compiled for the wasm32-wasip2 artifact.
// ---------------------------------------------------------------------------
#[cfg(target_os = "wasi")]
mod wasi_extension {
    use verter_editor_client::build_initialization_options;
    use zed_extension_api::{self as zed, LanguageServerId, Result, settings::LspSettings};

    /// The Zed extension. Holds no caches: discovery is cheap and Zed calls
    /// `language_server_command` rarely (once per server launch), so a per-launch
    /// resolve is fine and avoids stale-path bugs across worktrees.
    struct VerterExtension;

    impl VerterExtension {
        /// Read the user's verter server settings (`lsp.verter.settings`) for the
        /// given worktree, defaulting to `null` when unset. This blob is the source
        /// the shared crate reads for argv extras and `initializationOptions`.
        fn server_settings(worktree: &zed::Worktree) -> zed::serde_json::Value {
            LspSettings::for_worktree("verter", worktree)
                .ok()
                .and_then(|lsp| lsp.settings)
                .unwrap_or(zed::serde_json::Value::Null)
        }

        /// Read the user's explicit binary override
        /// (`lsp.verter.binary.path`), if any.
        fn binary_override(worktree: &zed::Worktree) -> Option<String> {
            LspSettings::for_worktree("verter", worktree)
                .ok()
                .and_then(|lsp| lsp.binary)
                .and_then(|binary| binary.path)
        }

        /// Read any user-supplied extra binary arguments
        /// (`lsp.verter.binary.arguments`). These are merged INTO
        /// `settings.lsp.serverArgs` (via
        /// [`super::merge_binary_arguments_into_settings`]) BEFORE
        /// [`super::plan_launch`], so the shared launch contract filters
        /// crate-owned args and keeps the workspace root last — they are NOT
        /// appended raw after the contract's argv.
        fn binary_arguments(worktree: &zed::Worktree) -> Vec<String> {
            LspSettings::for_worktree("verter", worktree)
                .ok()
                .and_then(|lsp| lsp.binary)
                .and_then(|binary| binary.arguments)
                .unwrap_or_default()
        }

        /// Read any user-supplied environment overrides
        /// (`lsp.verter.binary.env`).
        fn binary_env(worktree: &zed::Worktree) -> Vec<(String, String)> {
            LspSettings::for_worktree("verter", worktree)
                .ok()
                .and_then(|lsp| lsp.binary)
                .and_then(|binary| binary.env)
                .map(|env| env.into_iter().collect())
                .unwrap_or_default()
        }
    }

    impl zed::Extension for VerterExtension {
        fn new() -> Self {
            VerterExtension
        }

        fn language_server_command(
            &mut self,
            _language_server_id: &LanguageServerId,
            worktree: &zed::Worktree,
        ) -> Result<zed::Command> {
            // A WASI extension's cwd is not the workspace, so the workspace root is
            // taken from the worktree and forwarded positionally; `build_server_args`
            // depends on it.
            let root = worktree.root_path();
            let base_settings = Self::server_settings(worktree);
            let binary_override = Self::binary_override(worktree);

            // PATH discovery is opt-in only. ONLY when the user opted in do we ask
            // the host for a real PATH hit: `worktree.which("verter-lsp")` resolves
            // the bare name itself (handling `.exe`/`PATHEXT` on Windows) and returns
            // the ABSOLUTE path, or `None` when the binary is absent from `PATH`.
            // The real `Option` is injected into `plan_launch`; an opt-in with no hit
            // resolves to `NothingResolved` and fails loud — never a fabricated name.
            let path_opt_in = super::path_source_opt_in(&base_settings);
            let path_found: Option<String> = if path_opt_in {
                worktree.which("verter-lsp")
            } else {
                None
            };

            // User `binary.arguments` flow through the shared contract: merge them
            // into `settings.lsp.serverArgs` so `build_server_args` filters
            // crate-owned args and keeps the workspace root last (never a raw
            // post-append that could displace the root or reinject `--type-provider`).
            let settings = super::merge_binary_arguments_into_settings(
                base_settings,
                Self::binary_arguments(worktree),
            );

            let plan = super::plan_launch(
                Some(root.as_str()),
                &settings,
                binary_override.as_deref(),
                path_found.as_deref(),
                path_opt_in,
            )
            // FAIL LOUD: never launch a rootless/pathless server. The actionable
            // message names the override key + the PATH opt-in.
            .map_err(|err| err.to_string())?;

            // The launch contract already owns the leading provider flag, the
            // filtered user extras, and the trailing root; `binary.env` overrides
            // flow through verbatim (env is not argv).
            Ok(zed::Command {
                command: plan.command_path,
                args: plan.args,
                env: Self::binary_env(worktree),
            })
        }

        fn language_server_initialization_options(
            &mut self,
            _language_server_id: &LanguageServerId,
            worktree: &zed::Worktree,
        ) -> Result<Option<zed::serde_json::Value>> {
            // The init-options parity set is owned by the shared crate (it drops
            // `frameworks` and emits `statistics.enabled:false`); the extension only
            // forwards the user's `lsp.verter.settings` blob into it.
            let settings = Self::server_settings(worktree);
            Ok(Some(build_initialization_options(&settings)))
        }
    }

    zed::register_extension!(VerterExtension);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_launch_builds_exact_contract_with_explicit_override() {
        // The highest-value test: a root + explicit binary override yields the EXACT
        // launch plan, routed entirely through the shared crate's contract.
        let root = "/home/dev/proj";
        let settings = json!({ "lint": { "enabled": true, "preset": "strict" } });
        let plan = plan_launch(
            Some(root),
            &settings,
            Some("/opt/verter/verter-lsp"),
            None,
            false,
        )
        .expect("explicit override resolves a plan");

        // Exact launch tuple.
        assert_eq!(
            plan.command_path, "/opt/verter/verter-lsp",
            "command path is the override"
        );
        assert_eq!(
            plan.args,
            vec!["--type-provider=tsgo".to_string(), root.to_string()],
            "args are the default shared contract (provider clamp + trailing root)"
        );
    }

    #[test]
    fn init_options_come_from_shared_builder_with_parity_negatives() {
        // The init-options are produced by the shared builder. Prove the wiring:
        // `frameworks` is ABSENT, `statistics.enabled == false` is present, and the
        // editor/UI-only keys never leak.
        use verter_editor_client::build_initialization_options;
        let settings = json!({
            "lint": { "enabled": true, "preset": "strict" },
            // Throw in keys the server never reads to prove none leak.
            "mcp": { "port": 9229 },
            "configuration": { "anything": 1 },
            "decorations": { "enabled": true },
            "frameworks": ["react"]
        });
        let options = build_initialization_options(&settings);

        assert_eq!(options["lint"]["enabled"], json!(true));
        assert_eq!(options["lint"]["preset"], json!("strict"));

        // NEGATIVE: the shared crate drops `frameworks` (dead protocol surface).
        assert!(
            options.get("frameworks").is_none(),
            "frameworks must NOT be emitted: {options:?}"
        );
        // POSITIVE: `statistics` IS emitted, defaulting OFF.
        assert_eq!(
            options["statistics"]["enabled"],
            json!(false),
            "statistics must be present and default off"
        );
        // NEGATIVE: never-forwarded editor/UI-only keys are absent.
        for forbidden in ["mcp", "configuration", "decorations"] {
            assert!(
                options.get(forbidden).is_none(),
                "{forbidden:?} must not leak into options: {options:?}"
            );
        }
    }

    #[test]
    fn type_provider_clamps_sdk_dependent_and_typos_to_tsgo() {
        // The shared crate clamps every non-{tsgo,off} value to tsgo and NEVER emits
        // the configured token — in particular never the `tgo` typo. This is the
        // §1a revert-test target.
        for configured in ["tgo", "tsserver", "auto", "bogus", ""] {
            let settings = json!({ "typeProvider": configured });
            let plan = plan_launch(Some("/r"), &settings, Some("/p"), None, false)
                .expect("explicit override resolves a plan");
            assert!(
                plan.args.contains(&"--type-provider=tsgo".to_string()),
                "configured {configured:?} must clamp to tsgo: {:?}",
                plan.args
            );
            assert!(
                !plan.args.contains(&format!("--type-provider={configured}")),
                "configured token {configured:?} must NEVER be emitted: {:?}",
                plan.args
            );
        }
        // The sharpest negative: a `tgo` typo never reaches the CLI.
        let settings = json!({ "typeProvider": "tgo" });
        let plan = plan_launch(Some("/r"), &settings, Some("/p"), None, false).unwrap();
        assert!(
            !plan.args.contains(&"--type-provider=tgo".to_string()),
            "the `tgo` typo must never reach argv: {:?}",
            plan.args
        );
    }

    #[test]
    fn no_source_configured_fails_loud_and_does_not_plan() {
        // Neither an override nor a PATH opt-in: nothing resolves and a loud,
        // actionable error (naming the override key + the PATH opt-in) is returned.
        let err = plan_launch(Some("/x"), &json!({}), None, None, false).unwrap_err();
        assert!(
            matches!(
                err.discovery_error(),
                DiscoveryError::NothingResolved { .. }
            ),
            "empty settings + no override must map to NothingResolved, got {err:?}"
        );
        let message = err.to_string();
        assert!(
            message.contains("lsp.verter.binary.path"),
            "error must name the override key; got: {message}"
        );
        assert!(
            message.contains("serverSource"),
            "error must mention the PATH opt-in; got: {message}"
        );
    }

    #[test]
    fn path_found_but_not_opted_in_reason_is_distinct_at_shared_layer() {
        // A real PATH hit injected WITHOUT the opt-in must map to the DISTINCT
        // PathFoundButNotOptedIn reason. plan_launch forwards the injected
        // `path_found` to the shared resolver, which produces that variant; the
        // wrapper preserves it and still names the override key.
        let err = plan_launch(
            Some("/x"),
            &json!({}),
            None,
            Some("/on/path/verter-lsp"),
            false,
        )
        .unwrap_err();
        assert!(
            matches!(
                err.discovery_error(),
                DiscoveryError::PathFoundButNotOptedIn { .. }
            ),
            "a PATH hit without opt-in must map to PathFoundButNotOptedIn, got {err:?}"
        );
        // The two variants give different guidance, but both name the override key.
        let message = err.to_string();
        assert!(message.contains("lsp.verter.binary.path"));
        assert!(message.contains("serverSource"));
    }

    #[test]
    fn launch_error_preserves_both_distinct_discovery_reasons() {
        // The wrapper keeps the shared crate's two DISTINCT reasons so the host can
        // give targeted guidance.

        // Nothing on disk, nothing on PATH → NothingResolved.
        let nothing = plan_launch(Some("/x"), &json!({}), None, None, false).unwrap_err();
        assert!(matches!(
            nothing.discovery_error(),
            DiscoveryError::NothingResolved { .. }
        ));

        // Opted into PATH but the host found nothing (`path_found = None`) also fails
        // loud as NothingResolved — never a fabricated binary name.
        let settings = json!({ "serverSource": "path" });
        let absent = plan_launch(Some("/x"), &settings, None, None, true).unwrap_err();
        assert!(
            matches!(
                absent.discovery_error(),
                DiscoveryError::NothingResolved { .. }
            ),
            "an opt-in with no PATH hit must fail loud (no guessed binary), got {absent:?}"
        );
    }

    #[test]
    fn launch_error_points_at_setup_docs_while_keeping_both_remedies() {
        // Beyond naming the two settings-key remedies, the actionable guidance must
        // also point the user at where to read the full setup steps — the editor
        // docs page — so a fresh user who hit the loud fail has a discoverable next
        // step. This holds for BOTH distinct discovery reasons, and the error stays
        // a loud failure (never silently resolves to an Ok launch).

        // NothingResolved: empty settings, no override.
        let nothing = plan_launch(Some("/x"), &json!({}), None, None, false).unwrap_err();
        let nothing_msg = nothing.to_string();
        assert!(
            nothing_msg.contains("verterjs.dev/editor"),
            "NothingResolved guidance must point at the editor docs page; got: {nothing_msg}"
        );
        assert!(
            nothing_msg.contains("lsp.verter.binary.path"),
            "NothingResolved guidance must still name the override key; got: {nothing_msg}"
        );
        assert!(
            nothing_msg.contains("serverSource"),
            "NothingResolved guidance must still name the serverSource opt-in; got: {nothing_msg}"
        );

        // PathFoundButNotOptedIn: a real PATH hit injected WITHOUT opt-in maps to the
        // distinct reason; the wrapper's Display must carry the same docs reference.
        let path_not_opted = plan_launch(
            Some("/x"),
            &json!({}),
            None,
            Some("/on/path/verter-lsp"),
            false,
        )
        .unwrap_err();
        assert!(
            matches!(
                path_not_opted.discovery_error(),
                DiscoveryError::PathFoundButNotOptedIn { .. }
            ),
            "a PATH hit without opt-in must map to PathFoundButNotOptedIn, got {path_not_opted:?}"
        );
        let path_msg = path_not_opted.to_string();
        assert!(
            path_msg.contains("verterjs.dev/editor"),
            "PathFoundButNotOptedIn guidance must point at the editor docs page; got: {path_msg}"
        );
        assert!(
            path_msg.contains("lsp.verter.binary.path"),
            "PathFoundButNotOptedIn guidance must still name the override key; got: {path_msg}"
        );
        assert!(
            path_msg.contains("serverSource"),
            "PathFoundButNotOptedIn guidance must still name the serverSource opt-in; got: {path_msg}"
        );
    }

    #[test]
    fn override_wins_even_with_path_opt_in() {
        // An explicit binary override beats a simultaneous PATH opt-in (even with a
        // real PATH hit injected).
        let settings = json!({ "serverSource": "path" });
        let plan = plan_launch(
            Some("/x"),
            &settings,
            Some("/abs/verter-lsp"),
            Some("/usr/bin/verter-lsp"),
            true,
        )
        .expect("override resolves a plan");
        assert_eq!(
            plan.command_path, "/abs/verter-lsp",
            "the override path must win over PATH discovery"
        );
        // Negative: the injected PATH hit did NOT become the command.
        assert_ne!(plan.command_path, "/usr/bin/verter-lsp");
    }

    #[test]
    fn path_opt_in_uses_the_injected_host_hit_verbatim() {
        // PATH discovery uses the INJECTED real host hit verbatim (Zed's
        // `worktree.which`, which already resolved the absolute path incl.
        // `.exe`/`PATHEXT`). plan_launch must NOT re-derive a bare basename — the
        // absolute path the host found is used as-is. Inject the platform-appropriate
        // real path so the windows/unix `.exe` distinction is preserved as data, not
        // re-synthesized by plan_launch.
        let settings = json!({ "serverSource": "path" });

        let win = plan_launch(
            Some("/x"),
            &settings,
            None,
            Some("C:/tools/verter-lsp.exe"),
            true,
        )
        .expect("windows PATH opt-in resolves");
        assert_eq!(
            win.command_path, "C:/tools/verter-lsp.exe",
            "the windows PATH hit (already carrying .exe) is used verbatim"
        );

        let mac = plan_launch(
            Some("/x"),
            &settings,
            None,
            Some("/usr/local/bin/verter-lsp"),
            true,
        )
        .expect("macos PATH opt-in resolves");
        assert_eq!(
            mac.command_path, "/usr/local/bin/verter-lsp",
            "the macos PATH hit is used verbatim"
        );
        assert!(
            !mac.command_path.ends_with(".exe"),
            "a non-windows host hit has no .exe: {}",
            mac.command_path
        );

        let linux = plan_launch(
            Some("/x"),
            &settings,
            None,
            Some("/usr/local/bin/verter-lsp"),
            true,
        )
        .expect("linux PATH opt-in resolves");
        assert_eq!(
            linux.command_path, "/usr/local/bin/verter-lsp",
            "the linux PATH hit is used verbatim"
        );
    }

    #[test]
    fn path_opt_in_uses_the_real_hit_not_a_re_derived_basename() {
        // The bug-catching test for Defect 1: an OPT-IN with NO host PATH hit
        // (`path_found = None`) must FAIL LOUD as NothingResolved and produce NO
        // plan — plan_launch must never fabricate a `verter-lsp`/`verter-lsp.exe`
        // basename from the platform matrix. This FAILS against the old fabricating
        // code (which returned Ok with a bare basename command).
        let settings = json!({ "serverSource": "path" });
        let err = plan_launch(Some("/x"), &settings, None, None, true).unwrap_err();
        assert!(
            matches!(
                err.discovery_error(),
                DiscoveryError::NothingResolved { .. }
            ),
            "opt-in + no host PATH hit must fail loud as NothingResolved, got {err:?}"
        );
        // And the real injected hit is honored verbatim (used as-is, not re-derived).
        let plan = plan_launch(
            Some("/x"),
            &settings,
            None,
            Some("/abs/path/verter-lsp"),
            true,
        )
        .expect("a real PATH hit resolves a plan");
        assert_eq!(
            plan.command_path, "/abs/path/verter-lsp",
            "the resolved command is the real host hit, used verbatim"
        );
    }

    #[test]
    fn empty_or_blank_override_is_treated_as_unset() {
        // A whitespace-only override must not be honored; with a PATH opt-in + a real
        // hit it falls through to the injected PATH hit rather than being used as a
        // (blank) path.
        let settings = json!({ "serverSource": "path" });
        for blank in ["", "   ", "\t"] {
            let plan = plan_launch(
                Some("/x"),
                &settings,
                Some(blank),
                Some("/usr/bin/verter-lsp"),
                true,
            )
            .expect("blank override falls through to the PATH opt-in");
            assert_eq!(
                plan.command_path, "/usr/bin/verter-lsp",
                "blank override {blank:?} must fall through to the injected PATH hit"
            );
        }
        // With no PATH opt-in, a blank override fails loud (nothing resolves).
        let err = plan_launch(Some("/x"), &json!({}), Some("   "), None, false).unwrap_err();
        assert!(matches!(
            err.discovery_error(),
            DiscoveryError::NothingResolved { .. }
        ));
    }

    #[test]
    fn root_is_the_trailing_positional_argument() {
        // A WASI extension's cwd is not the workspace, so the root MUST be forwarded
        // explicitly as the trailing positional (not a --flag), and it must be LAST.
        let settings = json!({});
        let plan = plan_launch(
            Some("/home/dev/my project"),
            &settings,
            Some("/p"),
            None,
            false,
        )
        .unwrap();
        let last = plan.args.last().expect("argv is non-empty");
        assert_eq!(
            last, "/home/dev/my project",
            "the workspace root must be the trailing positional arg"
        );
        assert!(
            !last.starts_with("--"),
            "the trailing root must be positional, not a flag: {last:?}"
        );
        // The provider flag is FIRST, the root is LAST.
        assert_eq!(plan.args.first().unwrap(), "--type-provider=tsgo");
    }

    #[test]
    fn user_server_args_pass_through_with_provider_first_and_root_last() {
        // The shared crate inserts benign `lsp.serverArgs` extras after the provider
        // and before the trailing root, and filters crate-owned args.
        let settings = json!({
            "lsp": { "serverArgs": ["--foo", "--type-provider=tsserver", "bare", "--bar=1"] }
        });
        let plan = plan_launch(Some("/ws"), &settings, Some("/p"), None, false).unwrap();
        assert_eq!(plan.args[0], "--type-provider=tsgo", "provider is first");
        assert_eq!(plan.args.last().unwrap(), "/ws", "root is last");
        assert!(
            plan.args.contains(&"--foo".to_string()),
            "benign flag survives: {:?}",
            plan.args
        );
        assert!(
            plan.args.contains(&"--bar=1".to_string()),
            "benign flag survives: {:?}",
            plan.args
        );
        // Negative: the crate-owned provider override and the bare positional are dropped.
        assert!(
            !plan.args.iter().any(|arg| arg.contains("tsserver")),
            "user --type-provider override must be dropped: {:?}",
            plan.args
        );
        assert!(
            !plan.args.iter().any(|arg| arg == "bare"),
            "bare positional must be dropped: {:?}",
            plan.args
        );
    }

    #[test]
    fn merged_binary_arguments_are_filtered_and_root_stays_last() {
        // Defect 2: user `binary.arguments` flow THROUGH the shared contract — they
        // must NOT bypass its filtering/ordering. This drives the REAL seam:
        // `binary.arguments` are fed into `merge_binary_arguments_into_settings`
        // (NOT injected directly under `lsp.serverArgs`), and the merged result is
        // handed to `plan_launch`. A `--type-provider=tgo` reinjection is DROPPED
        // (the clamp wins, provider stays tsgo FIRST), a bare positional is DROPPED
        // (the root stays LAST), and benign flags survive. This FAILS against the old
        // post-append code (where `tgo`/`bare` would leak and the root would be
        // displaced) AND against a merge routed to the wrong settings key (which
        // would silently drop the args, leaving `--foo`/`--bar=1` absent).
        let base_settings = json!({});
        let settings = merge_binary_arguments_into_settings(
            base_settings,
            vec![
                "--foo".to_string(),
                "--type-provider=tgo".to_string(),
                "bare".to_string(),
                "--bar=1".to_string(),
            ],
        );
        // Guard the seam itself: the merge MUST land the args under
        // `lsp.serverArgs` (a wrong-key merge fails here before plan_launch).
        assert_eq!(
            settings["lsp"]["serverArgs"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0),
            4,
            "merge must place all four binary args under lsp.serverArgs: {settings:?}"
        );
        let plan = plan_launch(Some("/ws"), &settings, Some("/p"), None, false).unwrap();

        // Provider is the clamped tsgo and it is FIRST; the user `tgo` never appears.
        assert_eq!(
            plan.args.first().unwrap(),
            "--type-provider=tsgo",
            "provider clamp must be first: {:?}",
            plan.args
        );
        assert!(
            !plan
                .args
                .iter()
                .any(|arg| arg.contains("tgo") && arg != "--type-provider=tsgo"),
            "the user --type-provider=tgo must be dropped, never emitted: {:?}",
            plan.args
        );
        // Benign flags survive.
        assert!(
            plan.args.contains(&"--foo".to_string()),
            "benign --foo must survive: {:?}",
            plan.args
        );
        assert!(
            plan.args.contains(&"--bar=1".to_string()),
            "benign --bar=1 must survive: {:?}",
            plan.args
        );
        // The bare positional is DROPPED and the workspace root is LAST.
        assert!(
            !plan.args.iter().any(|arg| arg == "bare"),
            "the bare positional must be dropped: {:?}",
            plan.args
        );
        assert_eq!(
            plan.args.last().unwrap(),
            "/ws",
            "the workspace root must be the trailing positional: {:?}",
            plan.args
        );
    }

    #[test]
    fn merge_binary_arguments_appends_under_lsp_server_args() {
        // The merge helper places `binary.arguments` under `settings.lsp.serverArgs`
        // so the shared contract (not a raw post-append) consumes them.
        let merged = merge_binary_arguments_into_settings(json!({}), vec!["--x".to_string()]);
        let server_args = merged["lsp"]["serverArgs"]
            .as_array()
            .expect("serverArgs is an array after the merge");
        assert_eq!(
            server_args.last().and_then(Value::as_str),
            Some("--x"),
            "the merged binary arg must end up under lsp.serverArgs: {merged:?}"
        );
    }

    #[test]
    fn merge_binary_arguments_appends_after_existing_server_args_in_order() {
        // Pre-existing `serverArgs` come FIRST, then the `binary.arguments`, in order.
        let base = json!({ "lsp": { "serverArgs": ["--a"] } });
        let merged = merge_binary_arguments_into_settings(base, vec!["--b".to_string()]);
        let server_args: Vec<&str> = merged["lsp"]["serverArgs"]
            .as_array()
            .expect("serverArgs is an array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(
            server_args,
            vec!["--a", "--b"],
            "existing serverArgs precede the merged binary args, in order: {merged:?}"
        );
    }

    #[test]
    fn merge_binary_arguments_empty_is_a_noop() {
        // An empty `binary.arguments` leaves the settings untouched (no spurious
        // `lsp.serverArgs` key materializes).
        let base = json!({ "lint": { "enabled": true } });
        let merged = merge_binary_arguments_into_settings(base.clone(), vec![]);
        assert_eq!(merged, base, "empty binary args must be a no-op");
        assert!(
            merged.get("lsp").is_none(),
            "no spurious lsp key may be created: {merged:?}"
        );
    }

    #[test]
    fn path_source_opt_in_reads_only_the_exact_path_token() {
        // The opt-in is `serverSource == "path"` exactly; everything else is NOT an
        // opt-in (so the glue never consults `which` and a stray PATH binary can't
        // silently launch).
        assert!(path_source_opt_in(&json!({ "serverSource": "path" })));
        assert!(!path_source_opt_in(&json!({ "serverSource": "managed" })));
        assert!(!path_source_opt_in(&json!({ "serverSource": "PATH" })));
        assert!(!path_source_opt_in(&json!({})));
        assert!(!path_source_opt_in(&Value::Null));
    }
}

/// The `extension.toml` manifest is the load-bearing contract Zed reads to decide
/// which language server to contribute, which languages it binds to, and how to
/// map each Zed language name to the LSP `languageId`. These tests parse the
/// committed manifest and pin the fields the launch contract depends on, so an
/// accidental edit (a renamed server id, a dropped language, an added grammar)
/// fails loudly.
#[cfg(test)]
mod manifest_tests {
    /// The committed manifest, embedded at compile time so the test sees exactly
    /// the file that ships next to the crate.
    const EXTENSION_TOML: &str = include_str!("../extension.toml");

    fn manifest() -> toml::Value {
        toml::from_str(EXTENSION_TOML).expect("extension.toml must be valid TOML")
    }

    #[test]
    fn top_level_id_and_schema_version_present() {
        let manifest = manifest();
        assert_eq!(
            manifest.get("id").and_then(toml::Value::as_str),
            Some("verter"),
            "extension id must be `verter`"
        );
        assert_eq!(
            manifest
                .get("schema_version")
                .and_then(toml::Value::as_integer),
            Some(1),
            "schema_version must be 1"
        );
        assert_eq!(
            manifest.get("version").and_then(toml::Value::as_str),
            Some("0.1.0"),
            "version must be 0.1.0"
        );
    }

    #[test]
    fn single_verter_server_id_with_display_name() {
        // ONE server id `verter` (the tsgo extension's single-id + plural
        // `languages` shape), never a per-carrier server id.
        let manifest = manifest();
        let servers = manifest
            .get("language_servers")
            .and_then(toml::Value::as_table)
            .expect("extension.toml must declare a [language_servers] table");
        assert_eq!(
            servers.len(),
            1,
            "exactly one server id must be declared: {:?}",
            servers.keys().collect::<Vec<_>>()
        );
        let verter = servers
            .get("verter")
            .and_then(toml::Value::as_table)
            .expect("the single server id must be `verter`");
        assert_eq!(
            verter.get("name").and_then(toml::Value::as_str),
            Some("Verter"),
            "the server display name must be `Verter`"
        );
        // NEGATIVE: no per-carrier server id — one `verter` serves both carriers.
        assert!(
            !servers.contains_key("verter-vue") && !servers.contains_key("verter-svelte"),
            "no per-carrier server id may be declared: {:?}",
            servers.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn languages_are_exactly_vue_and_svelte_via_plural_array() {
        // Plural `languages = ["Vue.js", "Svelte"]` on the single id.
        let manifest = manifest();
        let languages = manifest
            .get("language_servers")
            .and_then(|s| s.get("verter"))
            .and_then(|v| v.get("languages"))
            .and_then(toml::Value::as_array)
            .expect("[language_servers.verter] must declare a `languages` array");
        let names: Vec<&str> = languages.iter().filter_map(toml::Value::as_str).collect();
        assert_eq!(
            names.len(),
            2,
            "exactly two languages must bind to the verter server: {names:?}"
        );
        assert!(
            names.contains(&"Vue.js"),
            "languages must include `Vue.js`: {names:?}"
        );
        assert!(
            names.contains(&"Svelte"),
            "languages must include `Svelte`: {names:?}"
        );
    }

    #[test]
    fn language_ids_map_zed_names_to_lsp_ids() {
        // `language_ids` maps each Zed language NAME to the LSP `languageId` the
        // server expects (`vue` / `svelte`).
        let manifest = manifest();
        let language_ids = manifest
            .get("language_servers")
            .and_then(|s| s.get("verter"))
            .and_then(|v| v.get("language_ids"))
            .and_then(toml::Value::as_table)
            .expect("[language_servers.verter] must declare a `language_ids` table");
        assert_eq!(
            language_ids.get("Vue.js").and_then(toml::Value::as_str),
            Some("vue"),
            "Vue.js must map to the `vue` languageId"
        );
        assert_eq!(
            language_ids.get("Svelte").and_then(toml::Value::as_str),
            Some("svelte"),
            "Svelte must map to the `svelte` languageId"
        );
    }

    #[test]
    fn no_grammars_table_language_server_only() {
        // Verter is language-server-ONLY (§4.2). A `[grammars]` table would mean
        // Verter ships/owns a grammar — it must NOT. This guards the
        // language-server-only decision permanently.
        let manifest = manifest();
        assert!(
            manifest.get("grammars").is_none(),
            "extension.toml must NOT declare a [grammars] table (language-server-only): {manifest:?}"
        );
    }
}

/// The committed `Cargo.lock` pins `zed_extension_api` to the 0.7.x line the glue
/// is written against. The `Extension` trait method signatures and the
/// `Worktree` (`root_path` / `which`) / `LspSettings` shapes changed across 0.x
/// minor versions, so a drift to a different minor would break the wasi build.
/// This test guards the pin permanently: if the lock moves off 0.7.x, the wasm
/// build breaks and this fails first with a precise reason.
#[cfg(test)]
mod lockfile_tests {
    /// The committed lockfile, embedded at compile time.
    const CARGO_LOCK: &str = include_str!("../Cargo.lock");

    fn zed_api_version() -> String {
        let lock: toml::Value = toml::from_str(CARGO_LOCK).expect("Cargo.lock must be valid TOML");
        let packages = lock
            .get("package")
            .and_then(toml::Value::as_array)
            .expect("Cargo.lock must list [[package]] entries");
        let pkg = packages
            .iter()
            .find(|p| p.get("name").and_then(toml::Value::as_str) == Some("zed_extension_api"))
            .expect("Cargo.lock must contain a pinned zed_extension_api package");
        pkg.get("version")
            .and_then(toml::Value::as_str)
            .expect("zed_extension_api package must have a version")
            .to_string()
    }

    /// Parse a dotted semver into `(major, minor)`, ignoring patch/pre-release.
    fn major_minor(version: &str) -> (u64, u64) {
        let mut parts = version.split('.');
        let major = parts
            .next()
            .and_then(|p| p.parse().ok())
            .expect("version must have a numeric major");
        let minor = parts
            .next()
            .and_then(|p| p.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok())
            .unwrap_or(0);
        (major, minor)
    }

    #[test]
    fn zed_extension_api_is_pinned_to_the_0_7_line() {
        let version = zed_api_version();
        let (major, minor) = major_minor(&version);
        assert!(
            major == 0 && minor == 7,
            "zed_extension_api must stay on the 0.7.x line the glue compiles against; \
             found {version}. Re-pin with: cargo update -p zed_extension_api --precise 0.7.0"
        );
    }
}
