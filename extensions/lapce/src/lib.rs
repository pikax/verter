//! Verter Lapce volt — a thin LSP client that tells Lapce to spawn the native
//! `verter-lsp` binary over stdio for `.vue` / `.svelte` files.
//!
//! The volt owns no semantics. On the LSP `initialize` request it asks the shared
//! launch-contract crate ([`verter_editor_client`]) for the `verter-lsp` argv, the
//! `initializationOptions` payload, and the binary source to launch, then hands
//! that plan to Lapce. Every launch-contract decision (the `--type-provider=tsgo`
//! clamp, the server-read option parity set, the discovery precedence, the
//! platform binary-name matrix) lives in that one shared crate so the Lapce and
//! Zed clients cannot diverge.
//!
//! # Thin launcher — out of the per-message hot path
//!
//! This volt is a launcher and nothing more. After it issues the one-time
//! [`LspLauncher::start_lsp`] on `initialize`, Lapce's native LSP client speaks
//! directly to the native `verter-lsp` process over stdio; the WASM plugin is NOT
//! on the per-LSP-message path. There is no per-message proxy, transform, or
//! middleware here — [`handle_initialize`] handles only `initialize` to perform
//! the single `start_lsp`, and forwards nothing per request. The plugin therefore
//! adds zero latency to hover, completion, diagnostics, and every other request.
//!
//! # Dual-target structure
//!
//! The decision surface ([`plan_launch`], [`handle_initialize`], the
//! [`LspLauncher`] seam) is pure — std + `serde_json` + [`verter_editor_client`]
//! only — so it compiles and unit-tests on the host toolchain. The real
//! `lapce-plugin` glue (`register_plugin!`, `PLUGIN_RPC`, `VoltEnvironment`) lives
//! behind `#[cfg(target_os = "wasi")]` and is built only for the `wasm32-wasip1`
//! volt artifact.

#![forbid(unsafe_code)]

use std::fmt;

use serde_json::Value;
use verter_editor_client::{
    binary_file_name, build_initialization_options, build_server_args, from_host, resolve_server,
    DiscoveryError, DiscoveryInputs, ServerSource,
};

/// A single document-filter entry in the LSP document selector.
///
/// Host-testable mirror of `lapce_plugin::psp_types::lsp_types::DocumentFilter`,
/// converted to the real type only in the wasi glue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorEntry {
    /// The carrier language id (`vue` / `svelte`).
    pub language: String,
    /// The URI scheme the filter applies to (`file`).
    pub scheme: String,
}

/// The exact, fully-resolved instruction the volt hands Lapce on `initialize`:
/// which binary to launch, with which arguments, for which documents, and with
/// which `initializationOptions`. This tuple is the volt's launch contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspLaunchPlan {
    /// The `urn:` launch token: `urn:<absolute-path>` for an override/managed
    /// binary, or `urn:<basename>` for a PATH lookup Lapce resolves itself.
    pub uri: String,
    /// The `verter-lsp` CLI argument vector (from
    /// [`verter_editor_client::build_server_args`]).
    pub args: Vec<String>,
    /// The document selector (`.vue` + `.svelte`, scheme `file`).
    pub selector: Vec<SelectorEntry>,
    /// The `initializationOptions` forwarded to the server (from
    /// [`verter_editor_client::build_initialization_options`]).
    pub options: Value,
}

/// Why the volt could not produce a launch plan.
///
/// Wraps the shared crate's [`DiscoveryError`] so its two distinct reasons
/// (a PATH binary found but not opted into, versus nothing usable anywhere) are
/// preserved, while [`fmt::Display`] augments the message with actionable
/// guidance that names `lsp.serverPath` and the `lsp.serverSource = "path"`
/// opt-in — guidance the pure crate's `Display` cannot carry on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchError {
    /// The underlying discovery decision that failed.
    source: DiscoveryError,
}

impl LaunchError {
    /// The wrapped [`DiscoveryError`], so callers can still branch on its
    /// distinct variants ([`DiscoveryError::PathFoundButNotOptedIn`] vs
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
        // variant-tailored guidance naming the config keys the user can set.
        write!(f, "{}", self.source)?;
        match self.source {
            DiscoveryError::PathFoundButNotOptedIn { .. } => f.write_str(
                ". Set `lsp.serverPath` to the absolute path of a verter-lsp binary, \
                 or opt into PATH discovery with `lsp.serverSource = \"path\"`.",
            ),
            DiscoveryError::NothingResolved { .. } => f.write_str(
                ". Set `lsp.serverPath` to the absolute path of a verter-lsp binary, \
                 or install verter-lsp on your PATH and opt in with \
                 `lsp.serverSource = \"path\"`.",
            ),
        }
    }
}

impl std::error::Error for LaunchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// The document selector the volt advertises: `.vue` and `.svelte`, scheme
/// `file`. Vue/Svelte are the carriers; the server projects them to TS itself.
pub fn document_selector() -> Vec<SelectorEntry> {
    vec![
        SelectorEntry {
            language: "vue".to_string(),
            scheme: "file".to_string(),
        },
        SelectorEntry {
            language: "svelte".to_string(),
            scheme: "file".to_string(),
        },
    ]
}

/// Read `lsp.serverPath`, treating an empty / whitespace-only value as unset.
fn configured_server_path(cfg: &Value) -> Option<&str> {
    cfg.get("lsp")
        .and_then(|lsp| lsp.get("serverPath"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Whether the user opted into PATH discovery via `lsp.serverSource = "path"`.
fn path_source_opt_in(cfg: &Value) -> bool {
    cfg.get("lsp")
        .and_then(|lsp| lsp.get("serverSource"))
        .and_then(Value::as_str)
        == Some("path")
}

/// Map a resolved [`ServerSource`] to the `urn:` launch token Lapce consumes.
///
/// Override/managed sources carry an absolute path; a PATH source carries the
/// bare platform basename Lapce resolves on `PATH`.
fn server_source_urn(source: &ServerSource) -> String {
    format!("urn:{}", source.path())
}

/// Build the complete [`LspLaunchPlan`] from the resolved workspace root, the
/// volt config, and the host platform strings.
///
/// All launch-contract logic is delegated to [`verter_editor_client`]:
/// * discovery precedence → [`resolve_server`] over [`DiscoveryInputs`] gathered
///   from `cfg`. When the user opted into PATH discovery, the PATH candidate
///   basename is the platform binary name from [`from_host`] + [`binary_file_name`]
///   (so PATH resolves `verter-lsp.exe` on Windows); a managed binary is never
///   present in this interim (no managed download yet).
/// * argv → [`build_server_args`].
/// * options → [`build_initialization_options`].
/// * selector → [`document_selector`].
///
/// Returns a loud, actionable [`LaunchError`] when no binary source resolves; the
/// caller must surface it and NOT launch a server.
pub fn plan_launch(
    workspace_root: Option<&str>,
    cfg: &Value,
    os: &str,
    arch: &str,
) -> Result<LspLaunchPlan, LaunchError> {
    let override_path = configured_server_path(cfg);
    let path_opt_in = path_source_opt_in(cfg);

    // The PATH candidate is the platform-aware basename, computed only when the
    // user opted into PATH discovery. An unsupported platform yields no candidate
    // (rather than a guess), so discovery fails loud below instead of launching a
    // wrong/bare name.
    let path_basename: Option<&'static str> = if path_opt_in {
        from_host(os, arch)
            .ok()
            .map(|(os, _arch)| binary_file_name(os))
    } else {
        None
    };

    let inputs = DiscoveryInputs {
        override_path,
        // No managed download exists yet; a managed binary is never present here.
        managed_present: None,
        path_opt_in,
        path_found: path_basename,
    };

    let source = resolve_server(&inputs)?;

    Ok(LspLaunchPlan {
        uri: server_source_urn(&source),
        args: build_server_args(workspace_root, cfg),
        selector: document_selector(),
        options: build_initialization_options(cfg),
    })
}

/// Seam over the host launch call so the contract is testable without the WASI
/// runtime. The real wasi implementation forwards to `PLUGIN_RPC.start_lsp`; a
/// test injects a recorder and asserts the exact plan.
pub trait LspLauncher {
    /// Hand Lapce the launch plan (real impl calls `start_lsp`; the converted
    /// `DocumentSelector` / `Url` are built at the wasi boundary).
    fn start_lsp(&mut self, plan: &LspLaunchPlan);
    /// Surface a loud, user-facing error (real impl calls `window_show_message`).
    fn show_error(&mut self, message: &str);
}

/// The loud, actionable error shown when an `initialize` request carries no
/// resolvable workspace root. Named so the wasi glue and the host tests assert
/// the same message; it names both the symptom (no workspace root) and the cause
/// (a missing / non-`file:` `root_uri`).
const NO_WORKSPACE_ROOT_ERROR: &str =
    "Verter: no workspace root in the LSP initialize request (root_uri missing or not a \
     file:// path); cannot launch verter-lsp without a workspace root.";

/// Handle an `initialize` request: build the launch plan and either launch the
/// server or surface a loud error. Pure orchestration over the injected
/// [`LspLauncher`] — host-testable. Returns `true` iff a server was launched.
///
/// A WASI launch REQUIRES a resolved workspace root: the volt's cwd is the volt
/// directory (not the workspace), so [`build_server_args`] forwards the workspace
/// root as the trailing positional and the server cannot infer it. A `None`
/// `workspace_root` therefore FAILS LOUD here — it surfaces
/// [`NO_WORKSPACE_ROOT_ERROR`] and returns `false` WITHOUT launching, rather than
/// spawning a rootless server that would root at the volt dir.
pub fn handle_initialize(
    launcher: &mut dyn LspLauncher,
    workspace_root: Option<&str>,
    cfg: &Value,
    os: &str,
    arch: &str,
) -> bool {
    // Enforce the launch-entry invariant: a launch is only valid with a resolved
    // workspace root. Fail loud on `None` before discovery so a missing root can
    // never produce a rootless launch.
    if workspace_root.is_none() {
        launcher.show_error(NO_WORKSPACE_ROOT_ERROR);
        return false;
    }

    match plan_launch(workspace_root, cfg, os, arch) {
        Ok(plan) => {
            launcher.start_lsp(&plan);
            true
        }
        Err(err) => {
            launcher.show_error(&err.to_string());
            false
        }
    }
}

// ---------------------------------------------------------------------------
// WASI volt glue — only compiled for the wasm32-wasip1 artifact.
// ---------------------------------------------------------------------------
#[cfg(target_os = "wasi")]
mod wasi_volt {
    use super::{handle_initialize, LspLaunchPlan, LspLauncher};
    use lapce_plugin::{
        psp_types::{
            lsp_types::{
                request::Initialize, DocumentFilter, DocumentSelector, InitializeParams,
                MessageType, Url,
            },
            Request,
        },
        register_plugin, LapcePlugin, VoltEnvironment, PLUGIN_RPC,
    };
    use serde_json::Value;

    #[derive(Default)]
    struct State {}
    register_plugin!(State);

    /// Real launcher backed by Lapce's `PLUGIN_RPC`.
    struct PluginRpcLauncher;

    impl PluginRpcLauncher {
        /// Convert the host-testable selector into Lapce's `DocumentSelector`.
        fn document_selector(plan: &LspLaunchPlan) -> DocumentSelector {
            plan.selector
                .iter()
                .map(|entry| DocumentFilter {
                    language: Some(entry.language.clone()),
                    scheme: Some(entry.scheme.clone()),
                    pattern: None,
                })
                .collect()
        }
    }

    impl LspLauncher for PluginRpcLauncher {
        fn start_lsp(&mut self, plan: &LspLaunchPlan) {
            // The plan's `uri` is a `urn:` token Lapce resolves (explicit path or
            // PATH basename). Parsing is infallible for the `urn:` shape; if it is
            // somehow malformed, surface a loud error rather than panicking.
            let uri = match Url::parse(&plan.uri) {
                Ok(uri) => uri,
                Err(parse_err) => {
                    self.show_error(&format!(
                        "Verter LSP launch URI {:?} is invalid: {parse_err}",
                        plan.uri
                    ));
                    return;
                }
            };
            let selector = Self::document_selector(plan);
            let _ =
                PLUGIN_RPC.start_lsp(uri, plan.args.clone(), selector, Some(plan.options.clone()));
        }

        fn show_error(&mut self, message: &str) {
            let _ = PLUGIN_RPC.window_show_message(MessageType::ERROR, message.to_string());
        }
    }

    impl LapcePlugin for State {
        fn handle_request(&mut self, _id: u64, method: String, params: Value) {
            // The volt handles ONLY `initialize`, to issue the one-time
            // `start_lsp`. Every other LSP message flows directly between Lapce's
            // native client and the native verter-lsp process — this plugin is not
            // on the per-message path.
            if method.as_str() != Initialize::METHOD {
                return;
            }

            let params: InitializeParams = match serde_json::from_value(params) {
                Ok(parsed) => parsed,
                Err(parse_err) => {
                    let mut launcher = PluginRpcLauncher;
                    launcher
                        .show_error(&format!("Verter: malformed initialize params: {parse_err}"));
                    return;
                }
            };

            // A WASI plugin's cwd is the volt directory, not the workspace, so the
            // workspace root is derived from the `initialize` request. Prefer the
            // modern `workspaceFolders[0]` (the deprecated `root_uri` is the
            // fallback), accepting only a `file:` URI that converts to a path. When
            // neither resolves, `root` stays `None` and `handle_initialize` fails
            // loud (it never launches a rootless server).
            let uri_to_root = |uri: &Url| -> Option<String> {
                uri.to_file_path()
                    .ok()
                    .map(|path| path.to_string_lossy().into_owned())
            };
            let root: Option<String> = params
                .workspace_folders
                .as_ref()
                .and_then(|folders| folders.first())
                .and_then(|folder| uri_to_root(&folder.uri))
                .or_else(|| params.root_uri.as_ref().and_then(uri_to_root));

            // The volt config arrives as `initializationOptions`.
            let cfg: Value = params.initialization_options.clone().unwrap_or(Value::Null);

            let os = VoltEnvironment::operating_system().unwrap_or_default();
            let arch = VoltEnvironment::architecture().unwrap_or_default();

            let mut launcher = PluginRpcLauncher;
            handle_initialize(&mut launcher, root.as_deref(), &cfg, &os, &arch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Records exactly what the volt handed the launcher, so the launch CONTRACT
    /// is a discriminating host-target unit test without a WASI runtime.
    #[derive(Default)]
    struct RecordingLauncher {
        launched: Option<LspLaunchPlan>,
        error: Option<String>,
    }

    impl LspLauncher for RecordingLauncher {
        fn start_lsp(&mut self, plan: &LspLaunchPlan) {
            self.launched = Some(plan.clone());
        }
        fn show_error(&mut self, message: &str) {
            self.error = Some(message.to_string());
        }
    }

    #[test]
    fn document_selector_has_exactly_two_vue_and_svelte_file_filters() {
        let sel = document_selector();
        assert_eq!(sel.len(), 2, "exactly two document filters");
        assert!(sel.contains(&SelectorEntry {
            language: "vue".to_string(),
            scheme: "file".to_string(),
        }));
        assert!(sel.contains(&SelectorEntry {
            language: "svelte".to_string(),
            scheme: "file".to_string(),
        }));
        // Negative: every filter is scheme "file" (no stray scheme leaked in).
        assert!(sel.iter().all(|entry| entry.scheme == "file"));
    }

    #[test]
    fn handle_initialize_launches_exact_contract_with_explicit_path() {
        // The highest-value test: a root + explicit serverPath yields the EXACT
        // launch tuple, routed entirely through the shared crate's contract.
        let root = "/home/dev/proj";
        let cfg = json!({
            "lsp": { "serverPath": "/opt/verter/verter-lsp" },
            "lint": { "enabled": true, "preset": "strict" }
        });
        let mut launcher = RecordingLauncher::default();
        let launched = handle_initialize(&mut launcher, Some(root), &cfg, "linux", "x86_64");

        assert!(launched, "a server must be launched when serverPath is set");
        let plan = launcher.launched.expect("plan recorded");
        assert!(launcher.error.is_none(), "no error on the happy path");

        // Exact launch tuple.
        assert_eq!(
            plan.uri, "urn:/opt/verter/verter-lsp",
            "uri is the override path urn"
        );
        assert_eq!(
            plan.args,
            vec!["--type-provider=tsgo".to_string(), root.to_string()],
            "args are the default shared contract (provider clamp + trailing root)"
        );
        assert_eq!(
            plan.selector,
            document_selector(),
            "selector is vue+svelte/file"
        );
        // Options reflect the config via the shared parity mapping.
        assert_eq!(plan.options["lint"]["enabled"], json!(true));
        assert_eq!(plan.options["lint"]["preset"], json!("strict"));

        // NEGATIVE: the shared crate drops `frameworks` (dead protocol surface).
        assert!(
            plan.options.get("frameworks").is_none(),
            "frameworks must NOT be emitted: {:?}",
            plan.options
        );
        // NEGATIVE/positive: `statistics` IS emitted, defaulting OFF.
        assert_eq!(
            plan.options["statistics"]["enabled"],
            json!(false),
            "statistics must be present and default off"
        );
        // NEGATIVE: no editor/UI-only `configuration` key leaked into options.
        assert!(
            plan.options.get("configuration").is_none(),
            "configuration must not leak into options"
        );
        // NEGATIVE: no `latest` token anywhere in argv.
        assert!(
            !plan.args.iter().any(|arg| arg.contains("latest")),
            "argv must not contain `latest`: {:?}",
            plan.args
        );
        // NEGATIVE: tsserver-only flags are absent on the tsgo path.
        assert!(
            !plan.args.iter().any(|arg| arg.starts_with("--tsdk")),
            "--tsdk must be absent: {:?}",
            plan.args
        );
        assert!(
            !plan.args.iter().any(|arg| arg.starts_with("--plugin-path")),
            "--plugin-path must be absent: {:?}",
            plan.args
        );
    }

    #[test]
    fn type_provider_clamps_sdk_dependent_and_typos_to_tsgo() {
        // The shared crate clamps every non-{tsgo,off} value to tsgo and NEVER
        // emits the configured token — in particular never the `tgo` typo.
        for configured in ["tgo", "tsserver", "auto", "bogus", ""] {
            let cfg = json!({
                "lsp": { "serverPath": "/p" },
                "typeProvider": configured
            });
            let plan = plan_launch(Some("/r"), &cfg, "linux", "x86_64")
                .expect("explicit path resolves a plan");
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
        let cfg = json!({ "lsp": { "serverPath": "/p" }, "typeProvider": "tgo" });
        let plan = plan_launch(Some("/r"), &cfg, "linux", "x86_64").unwrap();
        assert!(!plan.args.contains(&"--type-provider=tgo".to_string()));
    }

    #[test]
    fn type_provider_off_round_trips() {
        // `off` is one of the two SDK-free emittable values, so it survives.
        let cfg = json!({ "lsp": { "serverPath": "/p" }, "typeProvider": "off" });
        let plan = plan_launch(Some("/r"), &cfg, "linux", "x86_64").unwrap();
        assert!(
            plan.args.contains(&"--type-provider=off".to_string()),
            "off must round-trip: {:?}",
            plan.args
        );
        assert!(
            !plan.args.contains(&"--type-provider=tsgo".to_string()),
            "off must not be clamped to tsgo: {:?}",
            plan.args
        );
    }

    #[test]
    fn discovery_override_wins_even_with_path_opt_in() {
        // serverPath override beats a simultaneous serverSource = "path".
        let cfg = json!({
            "lsp": { "serverPath": "/abs/verter-lsp", "serverSource": "path" }
        });
        let mut launcher = RecordingLauncher::default();
        let launched = handle_initialize(&mut launcher, Some("/x"), &cfg, "windows", "x86_64");
        assert!(launched, "override must launch a server");
        let plan = launcher.launched.expect("plan recorded");
        assert_eq!(
            plan.uri, "urn:/abs/verter-lsp",
            "the override path must win over PATH discovery"
        );
        // Negative: a windows PATH opt-in did NOT turn this into a basename urn.
        assert_ne!(plan.uri, "urn:verter-lsp.exe");
    }

    #[test]
    fn discovery_path_opt_in_uses_platform_exe_name() {
        // The .exe cross-platform discrimination: PATH opt-in resolves the
        // platform binary basename via the shared platform matrix.
        let cfg = json!({ "lsp": { "serverSource": "path" } });

        let mut win = RecordingLauncher::default();
        let win_launched = handle_initialize(&mut win, Some("/x"), &cfg, "windows", "x86_64");
        assert!(win_launched, "windows PATH opt-in must launch");
        assert_eq!(
            win.launched.unwrap().uri,
            "urn:verter-lsp.exe",
            "windows PATH basename must carry the .exe suffix"
        );

        let mut mac = RecordingLauncher::default();
        let mac_launched = handle_initialize(&mut mac, Some("/x"), &cfg, "macos", "aarch64");
        assert!(mac_launched, "macos PATH opt-in must launch");
        let mac_uri = mac.launched.unwrap().uri;
        assert_eq!(mac_uri, "urn:verter-lsp", "macos basename has no .exe");
        assert!(
            !mac_uri.ends_with(".exe"),
            "non-windows launch urn must not end with .exe: {mac_uri}"
        );
    }

    #[test]
    fn no_source_configured_fails_loud_and_does_not_launch() {
        // Neither an override nor a PATH opt-in: nothing launches and a loud,
        // actionable error (naming lsp.serverPath) is shown instead. The cases
        // cover the empty config, the reserved-but-unavailable "managed" source,
        // and the shipped "none" default — all three are honest non-launch
        // postures that surface setup guidance instead of guessing.
        for cfg in [
            json!({}),
            json!({ "lsp": { "serverSource": "managed" } }),
            json!({ "lsp": { "serverSource": "none" } }),
        ] {
            let mut launcher = RecordingLauncher::default();
            let launched = handle_initialize(&mut launcher, Some("/x"), &cfg, "linux", "x86_64");

            assert!(
                !launched,
                "must NOT launch when no source is configured: {cfg:?}"
            );
            assert!(
                launcher.launched.is_none(),
                "start_lsp must not be called: {cfg:?}"
            );
            let err = launcher.error.expect("a loud error must be shown");
            assert!(
                err.contains("lsp.serverPath"),
                "error must be actionable (name lsp.serverPath); got: {err}"
            );
            assert!(
                err.contains("serverSource"),
                "error must mention the PATH opt-in; got: {err}"
            );
        }
    }

    /// The `lsp.serverSource` value baked into the shipped `volt.toml`, read from
    /// the committed manifest so this test binds to the value users actually get
    /// (and cannot silently drift if the default is changed). The manifest is
    /// embedded at compile time, so the test sees exactly the file that ships.
    fn shipped_server_source_default() -> String {
        const VOLT_TOML: &str = include_str!("../volt.toml");
        let manifest: toml::Value =
            toml::from_str(VOLT_TOML).expect("volt.toml must be valid TOML");
        manifest
            .get("config")
            .and_then(|c| c.get("lsp.serverSource"))
            .and_then(|s| s.get("default"))
            .and_then(toml::Value::as_str)
            .expect("volt.toml must declare config.\"lsp.serverSource\".default")
            .to_string()
    }

    #[test]
    fn fresh_default_serversource_fails_loud_actionably_not_phantom_managed() {
        // A FRESH user configures nothing, so the volt's DEFAULT `lsp.serverSource`
        // governs discovery. Read that default straight from the shipped manifest
        // so this test is a genuine default-binding guard — it tracks whatever
        // value actually ships, it does not restate a hardcoded constant.
        let default_source = shipped_server_source_default();

        // RED-on-revert hook: the v0 default must NOT name the "managed" capability,
        // which does not exist (plan_launch hardcodes `managed_present: None`). With
        // the old phantom `"managed"` default this assertion fails RED, pinning the
        // honest-default decision permanently.
        assert_ne!(
            default_source, "managed",
            "the shipped lsp.serverSource default must not be the phantom \"managed\" \
             capability (no managed provisioning exists in v0): got {default_source:?}"
        );

        // Build the exact cfg a fresh user gets: only the volt's default
        // serverSource, no serverPath override, no explicit opt-in.
        let cfg = json!({ "lsp": { "serverSource": default_source } });

        let mut launcher = RecordingLauncher::default();
        let launched = handle_initialize(&mut launcher, Some("/x"), &cfg, "linux", "x86_64");

        // The honest v0 posture: no auto-discovery source is selected, so nothing
        // launches and a loud, actionable error is shown instead of a silent guess.
        assert!(
            !launched,
            "a fresh-default launch must NOT start a server (no discovery source selected)"
        );
        assert!(
            launcher.launched.is_none(),
            "start_lsp must not be called on the fresh default"
        );

        let err = launcher
            .error
            .expect("a loud, actionable error must be shown");
        // The error names the override key the user can set...
        assert!(
            err.contains("lsp.serverPath"),
            "fresh-default error must name lsp.serverPath; got: {err}"
        );
        // ...and the PATH opt-in (`serverSource = \"path\"`), so the user has two
        // concrete next steps.
        assert!(
            err.contains("serverSource"),
            "fresh-default error must mention the serverSource PATH opt-in; got: {err}"
        );
    }

    #[test]
    fn launch_error_preserves_distinct_discovery_reasons() {
        // The wrapper must keep the shared crate's two DISTINCT reasons so the
        // host can give targeted guidance, while still naming lsp.serverPath.

        // Nothing on disk, nothing on PATH → NothingResolved.
        let nothing = plan_launch(Some("/x"), &json!({}), "linux", "x86_64").unwrap_err();
        assert!(
            matches!(
                nothing.discovery_error(),
                DiscoveryError::NothingResolved { .. }
            ),
            "empty config must map to NothingResolved, got {nothing:?}"
        );
        assert!(nothing.to_string().contains("lsp.serverPath"));

        // PATH would resolve, but the user opted in, so this is NothingResolved
        // ONLY when nothing is found. To exercise PathFoundButNotOptedIn we need a
        // PATH hit without opt-in; the volt only computes a PATH candidate when
        // opted in, so this distinct variant is asserted at the shared-crate layer.
        // Here we assert the opted-in-but-unsupported-platform path also fails loud
        // (an unsupported platform yields no candidate rather than a guess).
        let cfg = json!({ "lsp": { "serverSource": "path" } });
        let unsupported = plan_launch(Some("/x"), &cfg, "plan9", "sparc").unwrap_err();
        assert!(
            matches!(
                unsupported.discovery_error(),
                DiscoveryError::NothingResolved { .. }
            ),
            "an unsupported platform must fail loud (no guessed binary), got {unsupported:?}"
        );
    }

    #[test]
    fn empty_server_path_is_treated_as_unset() {
        // A whitespace-only serverPath must not be honored; with a PATH opt-in it
        // falls through to the PATH basename, not used as a (blank) path.
        let cfg = json!({ "lsp": { "serverPath": "   ", "serverSource": "path" } });
        let plan =
            plan_launch(Some("/x"), &cfg, "linux", "x86_64").expect("falls through to PATH opt-in");
        assert_eq!(
            plan.uri, "urn:verter-lsp",
            "blank serverPath must fall through to the PATH basename"
        );
    }

    #[test]
    fn user_server_args_pass_through_with_provider_first_and_root_last() {
        // The shared crate inserts benign extras after the provider and before the
        // trailing root, and filters crate-owned args.
        let cfg = json!({
            "lsp": {
                "serverPath": "/p",
                "serverArgs": ["--foo", "--type-provider=tsserver", "bare", "--bar=1"]
            }
        });
        let plan = plan_launch(Some("/ws"), &cfg, "linux", "x86_64").unwrap();
        assert_eq!(plan.args[0], "--type-provider=tsgo", "provider is first");
        assert_eq!(plan.args.last().unwrap(), "/ws", "root is last");
        assert!(
            plan.args.contains(&"--foo".to_string()),
            "benign flag survives"
        );
        assert!(
            plan.args.contains(&"--bar=1".to_string()),
            "benign flag survives"
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
    fn options_honor_user_overrides_and_keep_parity_defaults() {
        // A spot-check that init options flow through the shared builder: a user
        // override wins and a never-forwarded key (mcp) does not leak.
        let cfg = json!({
            "lsp": { "serverPath": "/p" },
            "inlayHints": { "enabled": false },
            "mcp": { "port": 9229 }
        });
        let plan = plan_launch(Some("/ws"), &cfg, "linux", "x86_64").unwrap();
        assert_eq!(plan.options["inlayHints"]["enabled"], json!(false));
        assert_eq!(plan.options["viteConfig"]["enabled"], json!(true));
        assert!(
            plan.options.get("mcp").is_none(),
            "mcp must not be forwarded: {:?}",
            plan.options
        );
    }

    #[test]
    fn launch_plan_passes_workspace_root_as_trailing_positional() {
        // A WASI plugin's cwd is the volt dir, not the workspace, so the root MUST
        // be forwarded explicitly as the trailing positional (not a --flag).
        let cfg = json!({ "lsp": { "serverPath": "/p" } });
        let plan = plan_launch(Some("/home/dev/my project"), &cfg, "linux", "x86_64").unwrap();
        let last = plan.args.last().expect("argv is non-empty");
        assert_eq!(
            last, "/home/dev/my project",
            "the workspace root must be the trailing positional arg"
        );
        assert!(
            !last.starts_with("--"),
            "the trailing root must be positional, not a flag: {last:?}"
        );
    }

    #[test]
    fn handle_initialize_with_no_workspace_root_fails_loud_and_does_not_launch() {
        // A WASI launch REQUIRES a resolved workspace root: `build_server_args`
        // depends on the trailing positional root (the server cannot infer the
        // workspace from the volt-dir cwd). A `None` root — root_uri missing, a
        // non-`file:` scheme, or an unconvertible URI — must FAIL LOUD, never
        // launch rootless. A valid serverPath is supplied so discovery itself
        // would otherwise succeed; this isolates the root requirement from the
        // discovery precedence.
        let cfg = json!({ "lsp": { "serverPath": "/opt/verter/verter-lsp" } });
        let mut launcher = RecordingLauncher::default();
        let launched = handle_initialize(&mut launcher, None, &cfg, "linux", "x86_64");

        assert!(
            !launched,
            "a None workspace root must NOT launch, even with a valid serverPath"
        );
        assert!(
            launcher.launched.is_none(),
            "start_lsp must NOT be called when the workspace root is absent"
        );
        let err = launcher
            .error
            .expect("a loud, actionable error must be shown when no root resolves");
        assert!(
            err.contains("workspace root"),
            "error must name the missing workspace root; got: {err}"
        );
        assert!(
            err.contains("root_uri"),
            "error must name root_uri as the cause; got: {err}"
        );
    }
}

/// The `volt.toml` manifest is the load-bearing contract Lapce reads to decide
/// when to load the volt, which `.wasm` to run, and which config keys to surface
/// as `initializationOptions`. These tests parse the committed manifest and pin
/// the fields the launch contract depends on, so an accidental edit (a renamed
/// wasm path, a dropped language activation, a changed config key) fails loudly.
#[cfg(test)]
mod manifest_tests {
    /// The committed manifest, embedded at compile time so the test sees exactly
    /// the file that ships next to the crate.
    const VOLT_TOML: &str = include_str!("../volt.toml");

    fn manifest() -> toml::Value {
        toml::from_str(VOLT_TOML).expect("volt.toml must be valid TOML")
    }

    #[test]
    fn wasm_points_at_the_built_artifact_path() {
        let manifest = manifest();
        let wasm = manifest
            .get("wasm")
            .and_then(toml::Value::as_str)
            .expect("volt.toml must declare a `wasm` path");
        assert_eq!(
            wasm, "bin/verter-lapce.wasm",
            "wasm path must match the build:lapce copy destination"
        );
    }

    #[test]
    fn activation_languages_are_exactly_vue_and_svelte() {
        let manifest = manifest();
        let languages = manifest
            .get("activation")
            .and_then(|a| a.get("language"))
            .and_then(toml::Value::as_array)
            .expect("[activation] language must be an array");
        let names: Vec<&str> = languages.iter().filter_map(toml::Value::as_str).collect();
        assert!(
            names.contains(&"vue"),
            "activation languages must include vue: {names:?}"
        );
        assert!(
            names.contains(&"svelte"),
            "activation languages must include svelte: {names:?}"
        );
        // NEGATIVE: no stray third language activation leaked in.
        assert_eq!(
            names.len(),
            2,
            "exactly vue + svelte activate the volt: {names:?}"
        );
    }

    #[test]
    fn workspace_contains_globs_vue_and_svelte_files() {
        let manifest = manifest();
        let globs = manifest
            .get("activation")
            .and_then(|a| a.get("workspace-contains"))
            .and_then(toml::Value::as_array)
            .expect("[activation] workspace-contains must be an array");
        let patterns: Vec<&str> = globs.iter().filter_map(toml::Value::as_str).collect();
        assert!(
            patterns.iter().any(|p| p.ends_with("*.vue")),
            "workspace-contains must glob *.vue: {patterns:?}"
        );
        assert!(
            patterns.iter().any(|p| p.ends_with("*.svelte")),
            "workspace-contains must glob *.svelte: {patterns:?}"
        );
    }

    #[test]
    fn config_section_declares_every_launch_contract_key() {
        // The dotted `[config."a.b"]` keys are delivered to the plugin as NESTED
        // initializationOptions; every key the shared launch contract reads must be
        // declared so Lapce surfaces it. A dropped key here means a silently
        // un-settable option.
        let manifest = manifest();
        let config = manifest
            .get("config")
            .and_then(toml::Value::as_table)
            .expect("volt.toml must declare a [config] table");
        for key in [
            "lsp.serverPath",
            "lsp.serverArgs",
            "lsp.serverSource",
            "typeProvider",
            "lint.enabled",
            "lint.preset",
            "inlayHints.enabled",
            "viteConfig.enabled",
            "viteConfig.trustedFiles",
            "experimental.conditionalRootNarrowing",
            "experimental.strictSlots",
            "hover.provenance",
            "statistics.enabled",
        ] {
            assert!(
                config.contains_key(key),
                "[config] must declare {key:?}; present keys: {:?}",
                config.keys().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn type_provider_default_is_the_clamp_target_never_the_tgo_typo() {
        // The emitted provider arg clamps to tsgo; the manifest's user-facing
        // default must be a value that round-trips to tsgo (tsgo itself) and must
        // NOT be the `tgo` typo, which would read as a non-tsgo provider.
        let manifest = manifest();
        let default = manifest
            .get("config")
            .and_then(|c| c.get("typeProvider"))
            .and_then(|tp| tp.get("default"))
            .and_then(toml::Value::as_str)
            .expect("typeProvider must declare a default");
        assert_eq!(default, "tsgo", "typeProvider default must be tsgo");
        assert_ne!(default, "tgo", "the `tgo` typo must never be the default");
    }
}

/// The committed `Cargo.lock` is the canonical mechanism that pins `lsp-types` to
/// a pre-0.95 version. From 0.95 the `Url` type was renamed to `Uri`, and the
/// upstream `psp-types` dependency (which declares an unpinned `lsp-types = "0"`)
/// then fails to compile against it. This test guards the pin permanently: if the
/// lock drifts to a `Url`->`Uri` version, the wasm build breaks and this fails
/// first with a precise reason.
#[cfg(test)]
mod lockfile_tests {
    /// The committed lockfile, embedded at compile time.
    const CARGO_LOCK: &str = include_str!("../Cargo.lock");

    fn lsp_types_version() -> String {
        let lock: toml::Value = toml::from_str(CARGO_LOCK).expect("Cargo.lock must be valid TOML");
        let packages = lock
            .get("package")
            .and_then(toml::Value::as_array)
            .expect("Cargo.lock must list [[package]] entries");
        let pkg = packages
            .iter()
            .find(|p| p.get("name").and_then(toml::Value::as_str) == Some("lsp-types"))
            .expect("Cargo.lock must contain a pinned lsp-types package");
        pkg.get("version")
            .and_then(toml::Value::as_str)
            .expect("lsp-types package must have a version")
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
    fn lsp_types_is_pinned_below_the_url_to_uri_rename() {
        let version = lsp_types_version();
        let (major, minor) = major_minor(&version);
        assert!(
            major == 0 && minor < 95,
            "lsp-types must stay < 0.95 (the Url->Uri rename that breaks psp-types); \
             found {version}. Re-pin with: cargo update -p lsp-types --precise 0.94.1"
        );
    }
}
