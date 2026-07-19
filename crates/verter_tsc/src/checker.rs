//! Batch Vue SFC codegen and type checking.
//!
//! Three-stage pipeline:
//!
//! **Stub stage — Public API stubs:**
//!   For each .vue file → `get_public_api()` → public-API stub with real component types.
//!   Enables cross-component prop/emit/slot type checking (imports resolve to actual types
//!   instead of the generic `DefineComponent<{}, {}, any>` wildcard shim).
//!
//! **Validation stage (TSX):**
//!   For each .vue file → `compile()` with `CompileTarget::IDE` → full TSX with source map.
//!   The carrier public-API imports (the reserved `.verter.ts` virtual suffix) are
//!   rewritten to point to stubs.
//!   Type-checks script body + template. Reports ALL type errors.
//!
//! **Declaration-generation stage (TSC):**
//!   For each .vue file → `generate_tsc_output()` → write `.tsc.tsx` to tempdir.
//!   Only when `--declaration` is requested. Reuses the shared
//!   `VerterHost` produced by the stub stage.
//!
//! All stages invoke `tsgo` (or `tsc`) as a subprocess and remap diagnostics
//! via source maps back to `.vue` positions.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use oxc_allocator::Allocator;
use rayon::prelude::*;
use tempfile::TempDir;
use verter_compiler::compile::{CodegenOptions, CompileTarget, VerterCompileOptions};
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

use crate::api_check;
use crate::error_map::map_tsc_position;
use crate::reporter::{self, Diagnostic, TscDiagnostic};
use crate::tsconfig::{strip_unc_prefix, TsConfig};

/// Options controlling what the checker emits.
pub struct EmitOptions {
    /// Type-check only (don't write .d.ts files).
    pub no_emit: bool,
    /// Emit .d.ts declaration files.
    pub declaration: bool,
    /// Directory to write declarations into.
    pub declaration_dir: Option<PathBuf>,
}

/// Result of a type-checking run.
pub struct CheckResult {
    pub diagnostics: Vec<Diagnostic>,
    pub emitted_files: Vec<PathBuf>,
}

struct CheckerInvocation {
    output: String,
    success: bool,
}

/// Generate public-API stub carriers for cross-component type resolution.
///
/// For each `.vue` file, generates a stub containing the component's public API
/// (props, emits, slots, exposed bindings) so that cross-component imports resolve to
/// real types instead of the generic `DefineComponent<{}, {}, any>` wildcard shim.
///
/// IN-MEMORY: the stub is NOT written to disk — `base_dir` only roots the stub's
/// deterministic virtual path (`<base>/Name_<hash>.vue.ts`), which the in-memory
/// `--api` overlay serves and the synthetic tsconfig lists in `files`.
///
/// Returns:
/// - `stub_files`: `(virtual stub path, stub source)` for the overlay + `files`
/// - `vue_ts_map`: canonical `.vue` path → virtual stub path (for import rewriting)
fn generate_public_api_stubs(
    host: &VerterHost,
    vue_files: &[PathBuf],
    base_dir: &Path,
) -> (Vec<(PathBuf, String)>, HashMap<String, PathBuf>) {
    let mut stub_files = Vec::new();
    let mut vue_ts_map = HashMap::new();

    // ONE batched public-API call for every .vue file: the batch captures a
    // single per-batch fixed store view and threads its shared cold seed into
    // every item, collapsing the per-call O(N²) store-view cliff that the
    // per-file `get_public_api` loop incurred (each macro-bearing call took its
    // own store-view read, which missed the warm cache because the call's first
    // deep semantic demand advanced the store-view token). Slots come back in
    // input order, one per id.
    let canonical_ids: Vec<String> = vue_files
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let responses = {
        let id_refs: Vec<&str> = canonical_ids.iter().map(String::as_str).collect();
        host.get_public_api_batch(&id_refs)
    };

    for ((vue_path, canonical_id), tsc_response) in
        vue_files.iter().zip(canonical_ids).zip(responses)
    {
        let tsc_response = match tsc_response {
            Some(r) => r,
            None => continue,
        };

        // Rewrite relative imports in the public API code to absolute paths
        // (the stub will live in temp_dir, not the vue file's directory).
        let vue_dir = vue_path.parent().unwrap_or(Path::new("."));
        let code = rewrite_relative_imports(&tsc_response.code, vue_dir);

        let raw_name = vue_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Component");
        let component_name = sanitize_component_name(raw_name);
        let hash = simple_hash(canonical_id.as_bytes());
        // The stub's on-disk name is internal: `lower_tsc_validation_carrier_specifiers` connects
        // the codegen's carrier-API specifier to this file via `vue_ts_map`, so
        // the `.vue.ts` extension here only needs `allowImportingTsExtensions`.
        let stub_name = format!("{component_name}_{hash:016x}.vue.ts");
        let stub_path = base_dir.join(&stub_name);

        vue_ts_map.insert(canonical_id, stub_path.clone());
        stub_files.push((stub_path, code));
    }

    (stub_files, vue_ts_map)
}

/// Validation stage: generate full TSX (script body + template) for every `.vue` file in parallel.
///
/// Uses `compile()` with `CompileTarget::TSX` for full type checking.
/// IN-MEMORY: nothing is written to disk — `base_dir` only roots each carrier's
/// deterministic virtual path (`<base>/Name_<hash>.tsx`). Returns
/// `(vue_path, tsx_code, virtual tsx_path)` tuples; the in-memory `--api` overlay
/// serves the code and the synthetic tsconfig lists the path in `files`.
fn generate_all_tsx(vue_files: &[PathBuf], base_dir: &Path) -> Vec<(PathBuf, String, PathBuf)> {
    vue_files
        .par_iter()
        .map(|vue_path| {
            let source = match fs::read_to_string(vue_path) {
                Ok(s) => s,
                Err(_) => return None,
            };

            let raw_name = vue_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Component");
            let component_name = sanitize_component_name(raw_name);

            let filename = vue_path.to_string_lossy().replace('\\', "/");
            let alloc = Allocator::default();
            let options = CodegenOptions {
                filename: Some(filename),
                target: CompileTarget::TSX,
                skip_source_map: false,
                embed_ambient_types: false,
                ..Default::default()
            };
            let verter_options = VerterCompileOptions {
                source_map: true,
                ..Default::default()
            };
            let result =
                verter_compiler::compile::compile(&source, &options, &verter_options, &alloc);

            let tsx_block = result.tsx?;

            // Rewrite relative imports (both `import('...')` and `from '...'` patterns)
            let vue_dir = vue_path.parent().unwrap_or(Path::new("."));
            let mut code = rewrite_relative_imports(&tsx_block.code, vue_dir);

            // Append inline source map so `map_tsc_position()` can remap errors.
            if !tsx_block.source_map.is_empty() {
                let encoded =
                    base64::prelude::BASE64_STANDARD.encode(tsx_block.source_map.as_bytes());
                code.push_str(&format!(
                    "\n//# sourceMappingURL=data:application/json;base64,{encoded}\n"
                ));
            }

            let hash = simple_hash(vue_path.to_string_lossy().as_bytes());
            let tsx_name = format!("{component_name}_{hash:016x}.tsx");
            let tsx_path = base_dir.join(&tsx_name);

            Some((vue_path.clone(), code, tsx_path))
        })
        .flatten()
        .collect()
}

/// Declaration-generation stage: generate minimal TSC declaration output for every `.vue` file in parallel.
///
/// Uses the host-backed public API path so imported macro types resolve the same
/// way they do in the IDE. Accepts a shared `VerterHost` whose files have
/// already been upserted by the caller, to avoid duplicate work.
/// Returns `(vue_path, tsc_code, tsc_tsx_path)` tuples written to `temp_dir`.
fn generate_all_tsc(
    host: &VerterHost,
    vue_files: &[PathBuf],
    temp_dir: &Path,
) -> Vec<(PathBuf, String, PathBuf)> {
    // ONE batched public-API call for every .vue file (mirrors
    // `generate_public_api_stubs`): the batch captures a single per-batch fixed
    // store view and threads its shared cold seed into every item, collapsing
    // the per-file `get_public_api` O(N²) store-view cliff that a per-file loop
    // re-incurred (each macro-bearing call took its own store-view read, which
    // missed the warm cache because the call's first deep semantic demand
    // advanced the store-view token). Slots come back in input order, one per id.
    let canonical_ids: Vec<String> = vue_files
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let responses = {
        let id_refs: Vec<&str> = canonical_ids.iter().map(String::as_str).collect();
        host.get_public_api_batch(&id_refs)
    };

    vue_files
        .iter()
        .zip(canonical_ids)
        .zip(responses)
        .filter_map(|((vue_path, _canonical_id), tsc_response)| {
            let tsc_out = tsc_response?;

            // Rewrite relative import() paths in the generated code to absolute paths.
            // The .tsc.tsx files live in a temp dir, so relative imports like
            // `import('./types')` need to resolve from the .vue file's directory.
            let vue_dir = vue_path.parent().unwrap_or(Path::new("."));
            let code = rewrite_relative_imports(&tsc_out.code, vue_dir);

            let raw_name = vue_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Component");
            let component_name = sanitize_component_name(raw_name);
            let hash = simple_hash(vue_path.to_string_lossy().as_bytes());
            let tsc_tsx_name = format!("{component_name}_{hash:016x}.tsc.tsx");
            let tsc_tsx_path = temp_dir.join(&tsc_tsx_name);

            if fs::write(&tsc_tsx_path, &code).is_err() {
                return None;
            }

            Some((vue_path.clone(), code, tsc_tsx_path))
        })
        .collect()
}

/// Single source of truth for the [`HostConfig`] the production `verter-tsc`
/// checker constructs its shared [`VerterHost`] from.
///
/// `verter-tsc` is a one-shot batch type-check: it builds the host, upserts
/// every `.vue` file, generates public-API stubs + validation TSX, runs the
/// external checker, and exits. It never serves interactive LSP queries, so it
/// routes through the [`HostConfig::batch_typecheck`] preset — BUILD analysis
/// scope, the `Build` query profile, and lazily-spawned host-owned pools (zero
/// eager pool threads at construction) — rather than the Full / LSP-interactive
/// [`HostConfig::default`]. Reverting this one body to `HostConfig::default()`
/// flips both the production host and the discriminating unit test together.
fn build_host_config() -> HostConfig {
    HostConfig::batch_typecheck()
}

/// Run the full type-checking pipeline.
///
/// The `--noEmit` TYPECHECK diagnostic set is produced IN-MEMORY through the tsgo
/// `--api` backend ([`run_inmemory_typecheck`]) — no temp files, no subprocess.
/// The `--declaration` EMIT stage stays on the temp-file `tsgo --project` path
/// ([`run_declaration_stage`]) because tsgo `--api` exposes no emit surface.
///
/// Returns [`Err`] when the in-memory `--api` typecheck stage cannot run (engine
/// absent, or a connect/init/updateSnapshot/protocol/project-not-found failure),
/// or when the declaration/emit stage fails (engine unresolvable, invocation
/// failure, or an error exit with no parseable diagnostics) — the caller surfaces
/// it as a non-zero process exit. This fail-closed contract is why a
/// broken/missing engine can never masquerade as a clean typecheck or a clean emit.
pub fn run(
    config: &TsConfig,
    tsconfig_path: &Path,
    opts: &EmitOptions,
) -> Result<CheckResult, api_check::TypecheckError> {
    if config.vue_files.is_empty() {
        return Ok(CheckResult {
            diagnostics: Vec::new(),
            emitted_files: Vec::new(),
        });
    }

    // ONE shared VerterHost (Batch preset): upsert every `.vue` once. Both stages
    // build from it — the typecheck overlay carriers and the declaration `.tsc.tsx`.
    let host = VerterHost::new_standalone(build_host_config());
    for vue_path in &config.vue_files {
        let source = match fs::read_to_string(vue_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let canonical_id = vue_path.to_string_lossy().replace('\\', "/");
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical_id.clone()),
            input_id: canonical_id,
            source: std::sync::Arc::<str>::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        });
    }

    // ── Typecheck stage: in-memory tsgo `--api` (the `--noEmit` diagnostic set). ──
    // A hard failure here (engine absent / connect / protocol) aborts the whole run
    // with `Err` — we never proceed to emit against a compromised typecheck.
    let mut diagnostics = run_inmemory_typecheck(&host, config, tsconfig_path)?;

    // ── Declaration/emit stage: temp-file `tsgo --project` (tsgo `--api` has no
    //    emit surface). Only when `--declaration` is requested. FAIL-CLOSED: an
    //    engine that cannot run the emit is a hard error, never silent success. ──
    let emitted_files = if opts.declaration {
        let (decl_diagnostics, emitted) =
            run_declaration_stage(&host, config, tsconfig_path, opts)?;
        diagnostics.extend(decl_diagnostics);
        emitted
    } else {
        Vec::new()
    };

    Ok(CheckResult {
        diagnostics,
        emitted_files,
    })
}

/// The wildcard `*.vue` ambient module so importing TS resolves
/// `import X from '*.vue'` (without it, TS2307). Served as an in-memory overlay
/// carrier for the typecheck stage and written to disk for the declaration stage.
const VUE_SHIMS_DTS: &str = "declare module '*.vue' {\n  \
     import type { DefineComponent } from 'vue'\n  \
     const component: DefineComponent<{}, {}, any>\n  \
     export default component\n}\n";

/// Augments Vue's `HTMLAttributes`/`SVGAttributes` with `children` so JSX children
/// on intrinsic elements don't trip TS2322/TS2559. Separate from [`VUE_SHIMS_DTS`]
/// because a top-level `import` would turn that file into a module and break its
/// ambient `declare module '*.vue'`.
const HTML_ATTRS_AUGMENT_DTS: &str = "import '@vue/runtime-dom'\n\
     declare module '@vue/runtime-dom' {\n  \
       interface HTMLAttributes {\n    \
         children?: any\n  \
       }\n  \
       interface SVGAttributes {\n    \
         children?: any\n  \
       }\n\
     }\n\
     export {}\n";

/// Forward-slash an absolute carrier/config path for the in-memory overlay + the
/// synthetic tsconfig `files`. The carriers are VIRTUAL (never on disk), so this
/// never canonicalizes.
fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The four ambient `.d.ts` shim carriers `(virtual path, content)`, rooted at
/// virtual in-project paths under `base`.
fn ambient_shim_carriers(base: &Path) -> Vec<(String, String)> {
    let mut vue_jsx_runtime_augment = String::from("import \"vue/jsx-runtime\";\n");
    vue_jsx_runtime_augment.push_str(verter_compiler::VUE_JSX_RUNTIME_AUGMENTATION);
    vue_jsx_runtime_augment.push_str("\nexport {};\n");
    vec![
        (
            slash(&base.join("vue-shims.d.ts")),
            VUE_SHIMS_DTS.to_string(),
        ),
        (
            slash(&base.join("html-attrs-augment.d.ts")),
            HTML_ATTRS_AUGMENT_DTS.to_string(),
        ),
        (
            slash(&base.join("__verter_types.d.ts")),
            verter_compiler::VERTER_TYPES_AMBIENT_MODULE.to_string(),
        ),
        (
            slash(&base.join("vue-jsx-runtime-augment.d.ts")),
            vue_jsx_runtime_augment,
        ),
    ]
}

/// Resolve the gated tsgo engine for verter-tsc via the 4-tier toolchain
/// resolver ([`verter_tsgo_api::toolchain::discovery`]): shared
/// (`VERTER_TSGO_BIN`, then PATH) → project-local ancestor `node_modules` →
/// temp update cache → bundled sidecar; the first WORKING candidate wins
/// (bounded version probe + support policy + capability smoke per candidate).
/// A resolution failure carries the actionable tier report.
///
/// This is the ONLY resolution path verter-tsc uses — BOTH the in-memory
/// typecheck stage and the declaration/emit stage resolve through it (a
/// `--version`-only selection can mask a working candidate behind a broken one
/// and is banned). Sync wrapper: builds a private runtime; call the async
/// [`verter_tsgo_api::toolchain::discovery::resolve`] from async contexts.
fn resolve_tsgo_engine(
    root: &Path,
    requirement: verter_tsgo_api::toolchain::validation::Capability,
) -> Result<PathBuf, verter_tsgo_api::toolchain::discovery::ResolveError> {
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        requirement,
        Some(root.to_path_buf()),
    );
    resolve_tsgo_engine_for(&request).map(|resolution| resolution.path)
}

/// The injectable seam of [`resolve_tsgo_engine`]: the full capability-validated
/// resolution over an EXPLICIT request (tests drive it without touching the
/// process environment).
fn resolve_tsgo_engine_for(
    request: &verter_tsgo_api::toolchain::discovery::ResolutionRequest,
) -> Result<
    verter_tsgo_api::toolchain::discovery::Resolution,
    verter_tsgo_api::toolchain::discovery::ResolveError,
> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(
            |e| verter_tsgo_api::toolchain::discovery::ResolveError::NoUsableCandidate {
                rejections: Vec::new(),
                notes: vec![format!(
                    "verter-tsc: failed to start the tsgo resolution runtime: {e}"
                )],
                requirement: request.requirement,
            },
        )?;
    runtime.block_on(verter_tsgo_api::toolchain::discovery::resolve(request))
}

/// The in-memory tsgo `--api` typecheck stage: generate the validation carriers
/// (full TSX + public-API stubs + ambient shims) and the synthetic tsconfig as an
/// in-memory overlay, then drive the gated [`api_check::typecheck`] over EVERY
/// configured-project root file. No temp files, no subprocess, no tsc fallback.
fn run_inmemory_typecheck(
    host: &VerterHost,
    config: &TsConfig,
    tsconfig_path: &Path,
) -> Result<Vec<Diagnostic>, api_check::TypecheckError> {
    let root = strip_unc_prefix(&config.root_dir);

    // Resolve the GATED `--api` engine (a supported tsgo native binary —
    // validated end-to-end by the resolver). No tsc fallback for the typecheck
    // path by design — a missing or wire-diverged engine is a HARD failure
    // (surfaced as a non-zero exit), NOT a silent empty diagnostic set that
    // would masquerade as a clean run.
    let engine = match resolve_tsgo_engine(
        &root,
        verter_tsgo_api::toolchain::validation::Capability::Api,
    ) {
        Ok(p) => strip_unc_prefix(&p),
        Err(e) => {
            return Err(api_check::TypecheckError::new(format!(
                "verter-tsc: {e}. (There is no tsc fallback for the typecheck path.)"
            )));
        }
    };

    // Generate the validation carriers IN-MEMORY, rooted at deterministic
    // in-project virtual paths (so node_modules resolution walks from the root).
    let (stub_files, vue_ts_map) = generate_public_api_stubs(host, &config.vue_files, &root);
    let mut tsx_files = generate_all_tsx(&config.vue_files, &root);
    // Lower the generated TSX's OWN carrier specifiers to the public-API stubs (or
    // strip back to the bare carrier for the `*.vue` wildcard shim).
    for (_, code, _) in &mut tsx_files {
        let rewritten = lower_tsc_validation_carrier_specifiers(code, &vue_ts_map);
        if rewritten != *code {
            *code = rewritten;
        }
    }

    // Assemble the overlay carriers + the synthetic tsconfig `files` membership.
    let mut overlay_files: Vec<api_check::OverlayFile> = Vec::new();
    let mut config_files: Vec<String> = Vec::new();
    for (path, content) in ambient_shim_carriers(&root) {
        config_files.push(path.clone());
        overlay_files.push(api_check::OverlayFile {
            path,
            content,
            remap: api_check::RemapKind::Passthrough,
        });
    }
    for (stub_path, code) in &stub_files {
        let path = slash(stub_path);
        config_files.push(path.clone());
        overlay_files.push(api_check::OverlayFile {
            path,
            content: code.clone(),
            remap: api_check::RemapKind::Passthrough,
        });
    }
    for (vue_path, code, tsx_path) in &tsx_files {
        let path = slash(tsx_path);
        config_files.push(path.clone());
        overlay_files.push(api_check::OverlayFile {
            path,
            content: code.clone(),
            remap: api_check::RemapKind::SourceMapped {
                vue_path: slash(vue_path),
            },
        });
    }

    // Synthetic tsconfig: byte-identical to the temp-file validation config (same
    // `synthetic_tsconfig_value` builder), served in-memory at a virtual in-project
    // path so node_modules + the real user tsconfig (via `extends`) resolve from disk.
    let original_abs = match tsconfig_path.canonicalize() {
        Ok(p) => slash(&strip_unc_prefix(&p)),
        Err(e) => {
            return Err(api_check::TypecheckError::new(format!(
                "verter-tsc: cannot resolve tsconfig {}: {e}",
                tsconfig_path.display()
            )));
        }
    };
    let validation_opts = EmitOptions {
        no_emit: true,
        declaration: false,
        declaration_dir: None,
    };
    let tsconfig_value = synthetic_tsconfig_value(
        &original_abs,
        &config_files,
        &validation_opts,
        &config.root_dir,
    );
    let tsconfig_bytes = match serde_json::to_string_pretty(&tsconfig_value) {
        Ok(s) => s,
        Err(e) => {
            return Err(api_check::TypecheckError::new(format!(
                "verter-tsc: failed to serialize synthetic tsconfig: {e}"
            )));
        }
    };
    let virtual_tsconfig_path = slash(&root.join("verter-tsc-check.tsconfig.json"));

    api_check::typecheck(api_check::TypecheckInputs {
        engine: engine.as_path(),
        cwd: root.as_path(),
        tsconfig_path: virtual_tsconfig_path,
        tsconfig_bytes,
        files: overlay_files,
    })
}

/// The temp-file `--declaration` emit stage (retained permanently — tsgo `--api`
/// exposes no emit surface). Generates the minimal `.tsc.tsx` carriers + the
/// vue-shims ambient on disk, runs `tsgo --project --declaration` (resolved
/// through the SAME capability-validated first-working resolver as the
/// typecheck stage — no `--version`-only selection), remaps diagnostics, and
/// post-processes `.tsc.tsx.d.ts` → `.vue.d.ts`.
///
/// FAIL-CLOSED: a resolution failure, a staging failure, an invocation failure
/// (spawn/timeout), or an engine that exits in error producing NO parseable
/// diagnostics is a hard [`api_check::TypecheckError`] (→ non-zero process
/// exit) — never a silent `Ok` with zero declarations, which would masquerade
/// as a clean emit. A non-zero engine exit WITH parseable diagnostics is an
/// ordinary type-error run (the diagnostics surface; the process exits 1).
fn run_declaration_stage(
    host: &VerterHost,
    config: &TsConfig,
    tsconfig_path: &Path,
    opts: &EmitOptions,
) -> Result<(Vec<Diagnostic>, Vec<PathBuf>), api_check::TypecheckError> {
    // The temp dir MUST be inside the project root so tsc resolves node_modules
    // (e.g. `import("vue")`) from the generated `.tsc.tsx` files.
    let temp_dir = TempDir::new_in(&config.root_dir).map_err(|e| {
        api_check::TypecheckError::new(format!(
            "verter-tsc: failed to create temp directory for declaration emit in {}: {e}",
            config.root_dir.display()
        ))
    })?;

    // Minimal macro-only `.tsc.tsx` carriers (declaration codegen), on disk.
    let decl_dir = temp_dir.path().join("_tsc");
    let _ = fs::create_dir_all(&decl_dir);
    let declaration_generated = generate_all_tsc(host, &config.vue_files, &decl_dir);

    // vue-shims so the checker resolves `import X from '*.vue'`.
    let shims_path = temp_dir.path().join("vue-shims.d.ts");
    let _ = fs::write(&shims_path, VUE_SHIMS_DTS);

    // Declaration file set + the `.tsc.tsx` → `.vue` source-map remap table.
    let mut tsx_to_vue: HashMap<String, (PathBuf, String)> = HashMap::new();
    let mut tsc_tsx_paths: Vec<PathBuf> = vec![shims_path];
    for (vue_path, tsc_code, tsc_tsx_path) in &declaration_generated {
        let canon = strip_unc_prefix(
            &tsc_tsx_path
                .canonicalize()
                .unwrap_or_else(|_| tsc_tsx_path.clone()),
        );
        tsx_to_vue.insert(
            canon.to_string_lossy().replace('\\', "/"),
            (vue_path.clone(), tsc_code.clone()),
        );
        tsc_tsx_paths.push(tsc_tsx_path.clone());
    }
    // When emitting declarations, the checker needs the original `.ts` files too.
    for ts_path in &config.ts_files {
        tsc_tsx_paths.push(ts_path.clone());
    }

    // Resolve the checker for the temp-file path through the SAME first-working,
    // capability-VALIDATED resolver as the typecheck stage. `Capability::Lsp`
    // is the closest validated surface to the CLI-compiler invocation: it
    // proves the binary spawns and completes a real handshake, so a candidate
    // that merely answers `--version` can no longer mask a working one. A
    // resolution failure is a HARD failure (fail-closed).
    let root = strip_unc_prefix(&config.root_dir);
    let checker_bin = match resolve_tsgo_engine(
        &root,
        verter_tsgo_api::toolchain::validation::Capability::Lsp,
    ) {
        Ok(path) => {
            eprintln!(
                "verter-tsc: declaration emit using tsgo at {}",
                path.display()
            );
            strip_unc_prefix(&path)
        }
        Err(e) => {
            return Err(api_check::TypecheckError::new(format!(
                "verter-tsc: declaration emit cannot run: {e}"
            )));
        }
    };

    let decl_opts = EmitOptions {
        no_emit: false,
        declaration: true,
        declaration_dir: opts.declaration_dir.clone(),
    };
    let decl_tsconfig = write_temp_tsconfig(
        temp_dir.path(),
        tsconfig_path,
        &tsc_tsx_paths,
        &decl_opts,
        &config.root_dir,
    )
    .map_err(|e| {
        api_check::TypecheckError::new(format!(
            "verter-tsc: failed to write declaration tsconfig: {e}"
        ))
    })?;

    let invocation = invoke_checker(&checker_bin, &decl_tsconfig, &decl_opts).map_err(|e| {
        api_check::TypecheckError::new(format!("verter-tsc: declaration stage failed: {e}"))
    })?;

    let raw_diags = reporter::parse_tsc_output(&invocation.output);
    let diagnostics = remap_diagnostics(raw_diags, &tsx_to_vue);

    if !invocation.success && diagnostics.is_empty() {
        // FAIL-CLOSED: the engine exited in error and produced NO parseable
        // diagnostics — it did not typecheck/emit anything. This is an engine
        // failure, not a clean run: surface it (non-zero exit) rather than
        // returning an empty diagnostic set + zero declarations (a broken
        // engine masquerading as a successful emit).
        return Err(api_check::TypecheckError::new(format!(
            "verter-tsc: declaration stage failed: the engine at {} exited in error \
             producing no diagnostics and no declarations — the declaration emit did \
             not run (fail-closed: a failed emit is never a silent success)",
            checker_bin.display()
        )));
    }

    if !invocation.success {
        eprintln!(
            "verter-tsc: declaration stage had errors; post-processing emitted declarations anyway"
        );
    }

    // Post-process: rename `.tsc.tsx.d.ts` → `.vue.d.ts`. Always run, even when the
    // checker exits with errors: it emits `.d.ts` for non-erroring files
    // (noEmitOnError: false), so skipping would leave 0 `.vue.d.ts` files.
    if let Some(decl_dir_out) = &opts.declaration_dir {
        postprocess_vue_declarations(decl_dir_out, &declaration_generated, &config.root_dir);
    }
    let emitted = opts
        .declaration_dir
        .as_ref()
        .map(|d| collect_dts_files(d))
        .unwrap_or_default();

    Ok((diagnostics, emitted))
}

/// Invoke the type-checker binary and return its combined stdout+stderr output
/// plus whether the subprocess exited successfully.
fn invoke_checker(
    checker_bin: &Path,
    tsconfig_path: &Path,
    opts: &EmitOptions,
) -> Result<CheckerInvocation, String> {
    let mut cmd = if cfg!(target_os = "windows")
        && !reporter::is_native_binary(checker_bin)
        && checker_bin
            .extension()
            .map(|e| e.eq_ignore_ascii_case("cmd"))
            .unwrap_or(false)
    {
        let mut c = std::process::Command::new("cmd.exe");
        c.arg("/C").arg(checker_bin);
        c
    } else {
        std::process::Command::new(checker_bin)
    };

    let tsconfig_clean = strip_unc_prefix(tsconfig_path);
    cmd.arg("--project").arg(&tsconfig_clean);
    if opts.no_emit {
        cmd.arg("--noEmit");
    }
    if opts.declaration {
        cmd.arg("--declaration");
        if let Some(dir) = &opts.declaration_dir {
            cmd.arg("--declarationDir").arg(dir);
        }
    }

    // Spawn with piped I/O and drain stdout/stderr in background threads to
    // avoid deadlock (child blocks on full pipe buffer if we don't read).
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run {}: {e}", checker_bin.display()))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut std::io::BufReader::new(stdout), &mut buf).ok();
        buf
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut std::io::BufReader::new(stderr), &mut buf).ok();
        buf
    });

    // Poll with timeout
    let timeout = std::time::Duration::from_secs(300); // 5 minutes
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    // Wait for reader threads to finish after kill
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Err(format!(
                        "type checker timed out after {}s",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(format!("error waiting for type checker: {e}")),
        }
    };

    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();

    Ok(CheckerInvocation {
        output: String::from_utf8_lossy(&stdout_bytes).into_owned()
            + &String::from_utf8_lossy(&stderr_bytes),
        success: status.success(),
    })
}

/// Write a synthetic tsconfig.json in `temp_dir` that:
/// - Extends the original tsconfig
/// - Includes all .tsc.tsx files
/// - Sets `rootDir` to `root_dir` so tsc mirrors the source tree in output
fn write_temp_tsconfig(
    temp_dir: &Path,
    original_tsconfig: &Path,
    tsc_tsx_files: &[PathBuf],
    opts: &EmitOptions,
    root_dir: &Path,
) -> Result<PathBuf, String> {
    let original_abs = strip_unc_prefix(
        &original_tsconfig
            .canonicalize()
            .map_err(|e| format!("cannot resolve original tsconfig: {e}"))?,
    );

    // Build file list with absolute paths (strip \\?\ prefix for Windows compatibility).
    let files: Vec<String> = tsc_tsx_files
        .iter()
        .map(|p| {
            let canon = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
            strip_unc_prefix(&canon)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    let tsconfig_json = synthetic_tsconfig_value(
        &original_abs.to_string_lossy().replace('\\', "/"),
        &files,
        opts,
        root_dir,
    );

    let suffix = if opts.declaration { "decl" } else { "check" };
    let temp_tsconfig = temp_dir.join(format!("verter-tsc-{suffix}.tsconfig.json"));
    std::fs::write(
        &temp_tsconfig,
        serde_json::to_string_pretty(&tsconfig_json)
            .map_err(|e| format!("serialization error: {e}"))?,
    )
    .map_err(|e| format!("write error: {e}"))?;

    Ok(temp_tsconfig)
}

/// Build the synthetic tsconfig JSON value shared by BOTH backends: the in-memory
/// `--api` typecheck (served as an overlay file) and the temp-file `--declaration`
/// stage ([`write_temp_tsconfig`] serializes + writes it). Keeping ONE builder
/// guarantees the in-memory typecheck sees byte-identical compiler options +
/// membership to the path the PERF-0 Rail B parity oracle pinned.
///
/// `original_abs` is the forward-slashed absolute path of the user tsconfig the
/// synthetic config `extends`; `files` are the forward-slashed absolute carrier
/// paths (virtual for typecheck, on-disk `.tsc.tsx` for declaration).
fn synthetic_tsconfig_value(
    original_abs: &str,
    files: &[String],
    opts: &EmitOptions,
    root_dir: &Path,
) -> serde_json::Value {
    let mut compiler_options = serde_json::json!({
        "skipLibCheck": true,
        "noEmit": opts.no_emit,
        // Disable composite mode: the parent tsconfig may have `composite: true`
        // which requires all referenced files to be in the project file list.
        // Our generated carriers import from project .ts files that aren't listed.
        "composite": false,
        // Fix rootDir so tsc mirrors the source tree structure in declarationDir.
        // Without this, tsc computes rootDir from the common ancestor of all input
        // files, which is unpredictable when mixing generated carriers and source .ts.
        "rootDir": root_dir.to_string_lossy().replace('\\', "/"),
        // Allow importing .vue.ts public API stubs (cross-component type resolution).
        // Requires noEmit or emitDeclarationOnly (both true in our generated configs).
        "allowImportingTsExtensions": true,
    });
    // Validation (typecheck) uses TSX files that contain JSX syntax.
    // Standard Vue TSX config: `jsx: "react-jsx"` + `jsxImportSource: "vue"`.
    if !opts.declaration {
        compiler_options["jsx"] = serde_json::json!("react-jsx");
        compiler_options["jsxImportSource"] = serde_json::json!("vue");
        // Clear jsxFactory/jsxFragmentFactory — they conflict with react-jsx mode.
        // The parent tsconfig may set these (e.g. `jsxFactory: "vue"`), and tsc
        // errors if both jsxFactory and react-jsx are present.
        compiler_options["jsxFactory"] = serde_json::json!(null);
        compiler_options["jsxFragmentFactory"] = serde_json::json!(null);
    }
    if opts.declaration {
        compiler_options["declaration"] = serde_json::json!(true);
        compiler_options["emitDeclarationOnly"] = serde_json::json!(true);
        // Override the parent tsconfig's potential `noEmitOnError: true`.
        // Without this, tsc refuses to emit ANY .d.ts files when the project
        // has type errors, even for non-erroring Vue components.
        compiler_options["noEmitOnError"] = serde_json::json!(false);
        if let Some(dir) = &opts.declaration_dir {
            compiler_options["declarationDir"] =
                serde_json::json!(dir.to_string_lossy().replace('\\', "/"));
        }
    }

    serde_json::json!({
        "extends": original_abs,
        "files": files,
        // Override parent's `include` to prevent scanning the source tree.
        // All carriers are listed explicitly in `files` (generated carriers + shims,
        // plus original .ts files when emitting declarations).
        "include": [],
        "compilerOptions": compiler_options,
    })
}

/// Remap raw tsc diagnostics from `.tsc.tsx` positions to `.vue` positions.
fn remap_diagnostics(
    raw: Vec<TscDiagnostic>,
    tsx_to_vue: &HashMap<String, (PathBuf, String)>,
) -> Vec<Diagnostic> {
    raw.into_iter()
        .filter(|d| !is_vue_jsx_type_gap_error(d))
        .filter(|d| !is_temp_tsconfig_error(d))
        .map(|d| {
            // Try to find a matching vue entry using a suffix match on the file path.
            let file_canon = strip_unc_prefix(
                &PathBuf::from(&d.file)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&d.file)),
            );
            let file_key = file_canon.to_string_lossy().replace('\\', "/");

            // Direct map lookup.
            let maybe_vue = tsx_to_vue.get(&file_key).or_else(|| {
                // Fallback: suffix match (tsc may shorten paths).
                tsx_to_vue.iter().find_map(|(k, v)| {
                    if k.ends_with(&file_key) || file_key.ends_with(k.as_str()) {
                        Some(v)
                    } else {
                        None
                    }
                })
            });

            let (remapped_file, remapped_line, remapped_col) =
                if let Some((vue_path, tsc_code)) = maybe_vue {
                    // Try source map lookup.
                    if let Some((src_name, pos)) = map_tsc_position(tsc_code, d.line, d.col) {
                        // If the mapped source name is a URL/absolute path, try to canonicalize it.
                        let display_path = if src_name.starts_with("file://") {
                            src_name
                                .trim_start_matches("file:///")
                                .trim_start_matches("file://")
                                .replace("%20", " ")
                        } else if src_name.starts_with('/') || src_name.contains(':') {
                            src_name
                        } else {
                            // Relative — resolve against vue file's parent.
                            vue_path
                                .parent()
                                .map(|p| p.join(&src_name).to_string_lossy().into_owned())
                                .unwrap_or(src_name)
                        };
                        (
                            Some(display_path.replace('\\', "/")),
                            pos.line + 1,
                            pos.col + 1,
                        )
                    } else {
                        // Source map lookup failed: report at line 1 of the .vue file.
                        (Some(vue_path.to_string_lossy().replace('\\', "/")), 1, 1)
                    }
                } else {
                    (None, d.line, d.col)
                };

            d.into_diagnostic(remapped_file, remapped_line, remapped_col)
        })
        .collect()
}

/// Check if a diagnostic is a TS2322 or TS2559 false positive caused by missing
/// properties in Vue's JSX type definitions. Known gaps:
/// - `children` — Vue's HTMLAttributes/SVGAttributes lack `children?: any`, but JSX
///   always passes children as props. A module augmentation in `html-attrs-augment.d.ts`
///   fixes this for tsc, but tsgo (preview) doesn't support cross-file augmentation.
/// - `textContent` — Generated from `v-text="expr"` directive. Valid DOM property but
///   not in Vue's JSX types.
/// - `innerHTML` — Generated from `v-html="expr"` directive. Same issue.
fn is_vue_jsx_type_gap_error(d: &TscDiagnostic) -> bool {
    is_vue_jsx_type_gap(d.ts_code, &d.message)
}

/// The carrier-agnostic Vue-JSX type-gap predicate, shared by the temp-file
/// declaration remap ([`is_vue_jsx_type_gap_error`]) and the in-memory `--api`
/// typecheck remap ([`crate::api_check`]). Both paths must suppress the same
/// `children` / `textContent` / `innerHTML` false positives on Vue intrinsic
/// attribute types (tsgo preview does not honor the cross-file `HTMLAttributes`
/// augmentation, so this filter — not the augmentation shim — is what removes
/// them).
pub(crate) fn is_vue_jsx_type_gap(ts_code: u32, message: &str) -> bool {
    if !matches!(ts_code, 2322 | 2559) {
        return false;
    }
    let has_gap_prop = message.contains("children")
        || message.contains("textContent")
        || message.contains("innerHTML");
    // Match any Vue intrinsic element attribute type (HTMLAttributes, SVGAttributes,
    // InputHTMLAttributes, LabelHTMLAttributes, etc.) or ReservedProps.
    has_gap_prop
        && (message.contains("HTMLAttributes")
            || message.contains("SVGAttributes")
            || message.contains("ReservedProps"))
}

/// Filter out diagnostics from the generated temporary tsconfig file itself.
/// These are config-level warnings (e.g. TS5102 "baseUrl removed", TS5090 "non-relative paths")
/// that come from settings inherited from the user's tsconfig. They're not actionable for
/// the user because they originate from verter-tsc's internal temp config.
fn is_temp_tsconfig_error(d: &TscDiagnostic) -> bool {
    d.file.contains("verter-tsc-") && d.file.ends_with(".tsconfig.json")
}

/// Collect all `.d.ts` files under a directory.
fn collect_dts_files(dir: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().map(|ext| ext == "ts").unwrap_or(false)
                && e.file_name().to_string_lossy().ends_with(".d.ts")
        })
        .map(|e| e.into_path())
        .collect()
}

/// Rewrite relative import paths in generated code to absolute paths.
///
/// The generated files are placed in a temp directory, so relative imports need
/// to be resolved relative to the original .vue file's directory, not the temp dir.
///
/// Handles two patterns:
/// - `import('./types')` — dynamic import syntax
/// - `from './types'` — ES module import/export syntax
fn rewrite_relative_imports(code: &str, vue_dir: &Path) -> String {
    let mut result = String::with_capacity(code.len());
    let mut rest = code;

    loop {
        // Find the earliest occurrence of either pattern.
        let import_paren = rest.find("import(");
        let from_kw = rest.find("from ");

        let (pos, kind) = match (import_paren, from_kw) {
            (Some(a), Some(b)) if a <= b => (a, ImportKind::DynamicImport),
            (Some(_), Some(b)) => (b, ImportKind::FromKeyword),
            (Some(a), None) => (a, ImportKind::DynamicImport),
            (None, Some(b)) => (b, ImportKind::FromKeyword),
            (None, None) => break,
        };

        result.push_str(&rest[..pos]);

        match kind {
            ImportKind::DynamicImport => {
                let after = &rest[pos + 7..]; // skip "import("
                match rewrite_quoted_path(after, vue_dir) {
                    Some((rewritten, consumed)) => {
                        result.push_str("import(");
                        result.push_str(&rewritten);
                        rest = &after[consumed..];
                    }
                    None => {
                        result.push_str("import(");
                        rest = after;
                    }
                }
            }
            ImportKind::FromKeyword => {
                let after = &rest[pos + 5..]; // skip "from "
                match rewrite_quoted_path(after, vue_dir) {
                    Some((rewritten, consumed)) => {
                        result.push_str("from ");
                        result.push_str(&rewritten);
                        rest = &after[consumed..];
                    }
                    None => {
                        result.push_str("from ");
                        rest = after;
                    }
                }
            }
        }
    }

    result.push_str(rest);
    result
}

enum ImportKind {
    DynamicImport,
    FromKeyword,
}

/// Try to extract a quoted path, resolve it if relative, and return the rewritten
/// quoted string plus the number of bytes consumed from `after` (including closing quote).
fn rewrite_quoted_path(after: &str, vue_dir: &Path) -> Option<(String, usize)> {
    let quote = match after.chars().next() {
        Some(q @ '\'') | Some(q @ '"') => q,
        _ => return None,
    };
    let path_start = 1; // skip opening quote
    let path_end = after[path_start..].find(quote)? + path_start;
    let import_path = &after[path_start..path_end];

    // Relative classification is the full TS `pathIsRelative` class (bare
    // `.`/`..` plus the `./`/`../`/`.\`/`..\` prefixes) — the SAME shared
    // predicate the workspace resolver uses. A narrower `./`/`../` prefix
    // check leaves the bare and backslash spellings un-absolutized in the
    // generated temp TSX, and TypeScript then resolves them against the
    // TEMP directory: spurious missing-module diagnostics on this lane.
    let result = if verter_workspace::resolver::is_relative_specifier(import_path) {
        // Check if the path after "./" is already an absolute path (e.g., "./D:/...")
        // This happens when the IDE codegen embeds a full filename in import('./filename.vue.verter.ts').
        let after_dot = import_path.strip_prefix("./").unwrap_or(import_path);
        if after_dot.contains(':') || after_dot.starts_with('/') {
            // Already absolute — just strip the "./" prefix
            format!("{quote}{after_dot}{quote}")
        } else if import_path == "." {
            // Bare `.` — the importer directory's own index module.
            // Joining "." would leave a trailing `/.` segment
            // (Path::join does not normalize "." segments on all
            // platforms), so emit the directory itself.
            let abs_path = vue_dir.to_string_lossy().replace('\\', "/");
            format!("{quote}{abs_path}{quote}")
        } else {
            // `\` is a module-specifier separator in the same
            // `pathIsRelative` class (TS `normalizeSlashes`) — normalize
            // before joining so `..\x` joins identically to `../x`.
            // Then strip a leading "./" before joining to avoid
            // "dir/./rest" in the result (Path::join does not normalize
            // "." segments on all platforms). Bare `..` joins as-is —
            // TypeScript normalizes the `..` segment during resolution,
            // exactly as it does for the `../x` forms.
            let normalized: std::borrow::Cow<'_, str> = if import_path.contains('\\') {
                std::borrow::Cow::Owned(import_path.replace('\\', "/"))
            } else {
                std::borrow::Cow::Borrowed(import_path)
            };
            let clean_rel = normalized.strip_prefix("./").unwrap_or(&normalized);
            let resolved = vue_dir.join(clean_rel);
            let abs_path = resolved.to_string_lossy().replace('\\', "/");
            format!("{quote}{abs_path}{quote}")
        }
    } else {
        format!("{quote}{import_path}{quote}")
    };

    // consumed = opening quote + path + closing quote
    Some((result, path_end + 1))
}

/// The virtual-file suffixes the IDE codegen appends onto a carrier path, in
/// longest-first match order (so `.verter.ts` is tried before any suffix that
/// could be its tail). Each is stripped to recover the bare carrier path.
///
/// - [`CARRIER_API_VIRTUAL_SUFFIX`] (`.verter.ts`) — the API carrier: the
///   public-default re-export (`export { default } from './Foo.vue.verter.ts'`)
///   and the `___VERTER___instance` self-import.
/// - `.tsx` / `.jsx` — the IDE carrier: an in-project bare `.vue`/`.svelte`
///   import rewritten to its bare-import-probe identity (`./Comp.vue` →
///   `./Comp.vue.tsx`). These are the TypeScript/JSX companion extensions, not a
///   reserved Verter suffix; the `path_is_carrier` gate below is what restricts
///   the strip to genuine carrier companions (a plain `./Widget.tsx` is left
///   untouched because `Widget` is not a carrier).
const CARRIER_VIRTUAL_IMPORT_SUFFIXES: &[&str] = &[
    verter_workspace::CARRIER_API_VIRTUAL_SUFFIX, // ".verter.ts"
    ".tsx",
    ".jsx",
];

/// TSC-validation carrier-specifier lowering: rewrite the GENERATED validation
/// TSX's OWN carrier-API import specifiers so the plugin-less `verter_tsc`
/// validation Program resolves them.
///
/// This is NOT the forbidden "post-hoc import rewriting" of USER/project source
/// (rewriting a user's imports to paper over project binding). It operates ONLY on
/// the carrier-virtual specifiers Verter's OWN IDE codegen EMITTED into the
/// generated validation TSX — the `Foo.vue.verter.ts` API carrier (public-default
/// re-export + `$instance` self-import) and the `Foo.vue.tsx` IDE carrier
/// (in-project component imports), see [`CARRIER_VIRTUAL_IMPORT_SUFFIXES`]. By the
/// time this runs the specifier has already been resolved to an absolute
/// carrier-virtual path by [`rewrite_relative_imports`].
///
/// For known carriers (present in `vue_ts_map`, keyed by canonical carrier path),
/// it lowers the import to the temp-dir public-API stub — the stub re-exports the
/// component's public default, so it satisfies both surfaces. For unknown carrier
/// paths (e.g. from node_modules), it strips the virtual suffix back to the bare
/// carrier path (`Bar.vue` / `Bar.svelte`) so the `*.vue` / `*.svelte` wildcard
/// shim matches.
///
/// ONLY real module-specifier positions are lowered: the quoted path after a
/// `from` clause (static `import …`/`export … from`), the dynamic `import("…")`
/// argument, and the side-effect `import "…"` specifier. The validation TSX lowers
/// the user's `<script setup>` body verbatim, so an ordinary string literal, a
/// comment, or a template literal that merely SPELLS a carrier path is NOT a
/// specifier and is left BYTE-FOR-BYTE untouched (see
/// [`lower_carrier_specifiers_in_module_positions`]) — user source is never
/// touched, only the generated carrier specifiers Verter itself emitted.
fn lower_tsc_validation_carrier_specifiers(
    code: &str,
    vue_ts_map: &HashMap<String, PathBuf>,
) -> String {
    // Fast path: skip files that mention neither a carrier-virtual suffix nor a
    // bare carrier source extension anywhere. A bare in-project carrier import
    // (`./Comp.vue`, no virtual suffix) must still be examined — the precise
    // carrier classification happens per-specifier in the scan. This is a cheap,
    // deliberately conservative pre-filter against the registry's carrier
    // extensions (`.vue`/`.svelte`), not a hardcoded `.vue` literal.
    let carrier_source_exts = verter_workspace::carrier_source_extensions();
    let mentions_carrier = CARRIER_VIRTUAL_IMPORT_SUFFIXES
        .iter()
        .any(|s| code.contains(s))
        || carrier_source_exts
            .iter()
            .any(|ext| code.contains(&format!(".{ext}")));
    if !mentions_carrier {
        return code.to_string();
    }

    lower_carrier_specifiers_in_module_positions(code, vue_ts_map)
}

/// What a freshly-read `import`/`export` keyword is waiting to consume next.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SpecifierExpect {
    /// Not inside an import/export construct seeking a specifier.
    None,
    /// Inside a static `import …`/`export …` statement; the specifier is the
    /// quoted string immediately following the next code-level `from` keyword.
    AfterFrom,
    /// The next code-level quoted string is the specifier (side-effect
    /// `import "x"`, or the first string inside a dynamic `import( … )`).
    NextString,
}

/// Lexical carrier-specifier lowering over module-specifier positions: copies
/// `code` to the output verbatim, lowering ONLY the carrier specifier that
/// occupies a genuine module-specifier position. It recognizes the
/// specifier-introducing token shapes and skips all non-specifier context
/// (comments, ordinary string literals, template literals) so user code that
/// merely mentions a carrier path is never corrupted.
///
/// This is `verter_tsc`'s own minimal syntactic position scanner over the
/// generated validation TSX — a focused specifier-position lexer, NOT a type-text
/// heuristic and NOT a full TS parser. It maintains just enough lexical state to
/// (a) never treat a token inside a comment/string/template as code, and (b)
/// arm the specifier position only for real `import`/`export` constructs.
///
/// Specifier shapes covered (each at a real code-level position):
/// - `import X from "x"`, `import type { X } from "x"`, `import { type X } from "x"`
/// - `import * as X from "x"`
/// - `export * from "x"`, `export type * from "x"`
/// - `export { X } from "x"`, `export type { X } from "x"`
/// - dynamic `import("x")` and `import ( "x" )` (whitespace-tolerant)
/// - side-effect `import "x"`
///
/// The `from` keyword arms a specifier ONLY inside an `import`/`export` construct
/// (tracked via [`SpecifierExpect::AfterFrom`]); a bare `const from = "./x"` is
/// never treated as a specifier introducer. Byte-level scanning is safe: every
/// syntactic token is ASCII, and non-ASCII bytes appear only inside
/// strings/comments/identifiers, which are copied through unaltered.
fn lower_carrier_specifiers_in_module_positions(
    code: &str,
    vue_ts_map: &HashMap<String, PathBuf>,
) -> String {
    let bytes = code.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut i = 0usize;
    let mut expect = SpecifierExpect::None;

    while i < len {
        let b = bytes[i];

        // Line comment: copy through to end of line, no state change.
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            let start = i;
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            result.push_str(&code[start..i]);
            continue;
        }

        // Block comment: copy through to the closing `*/`, no state change.
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(len); // consume closing `*/` (or run to EOF)
            result.push_str(&code[start..i]);
            continue;
        }

        // Template literal: copy through verbatim (including `${ … }` spans). A
        // carrier path inside a template is never a specifier. Interpolations are
        // skipped wholesale via brace-depth so an inner string/template does not
        // re-arm a specifier.
        if b == b'`' {
            let start = i;
            i += 1;
            while i < len {
                let c = bytes[i];
                if c == b'\\' {
                    i += 2;
                    continue;
                }
                if c == b'`' {
                    i += 1;
                    break;
                }
                if c == b'$' && i + 1 < len && bytes[i + 1] == b'{' {
                    // Skip the interpolation to its matching close brace.
                    i += 2;
                    let mut depth = 1usize;
                    while i < len && depth > 0 {
                        match bytes[i] {
                            b'{' => depth += 1,
                            b'}' => depth -= 1,
                            _ => {}
                        }
                        i += 1;
                    }
                    continue;
                }
                i += 1;
            }
            result.push_str(&code[start..i]);
            continue;
        }

        // String literal (single or double quote).
        if b == b'\'' || b == b'"' {
            let quote = b;
            let content_start = i + 1;
            let mut j = content_start;
            while j < len {
                let c = bytes[j];
                if c == b'\\' {
                    j += 2;
                    continue;
                }
                if c == quote {
                    break;
                }
                j += 1;
            }
            // `j` indexes the closing quote (or `len` if unterminated).
            let closed = j < len;
            let specifier = &code[content_start..j];

            // Emit the opening quote.
            result.push(quote as char);

            if expect == SpecifierExpect::AfterFrom || expect == SpecifierExpect::NextString {
                // This string is a genuine module specifier — classify it.
                match carrier_virtual_import_target(specifier, vue_ts_map) {
                    Some(Rewrite::Stub(stub)) => result.push_str(&stub),
                    Some(Rewrite::CarrierPath(keep)) => result.push_str(&specifier[..keep]),
                    None => result.push_str(specifier),
                }
            } else {
                // Ordinary (non-specifier) string literal — leave verbatim.
                result.push_str(specifier);
            }
            // The specifier (or any string) closes this import/export construct's
            // pending specifier expectation.
            expect = SpecifierExpect::None;

            if closed {
                result.push(quote as char);
                i = j + 1;
            } else {
                i = j; // unterminated — already at EOF
            }
            continue;
        }

        // Identifier / keyword token at code level.
        if is_ident_start(b) {
            let start = i;
            i += 1;
            while i < len && is_ident_continue(bytes[i]) {
                i += 1;
            }
            let word = &code[start..i];
            result.push_str(word);
            // Leading-context guard: `import`/`export`/`from` introduce a specifier
            // construct ONLY at a statement/expression position, never as a member
            // name. When the previous significant code byte is `.` the word is a
            // property access — `loader.import("x")`, `loader?.import("x")` (the
            // significant prior byte is still `.`), `obj.from(…)`, `obj.export` —
            // so any string that follows is an ordinary value, not a module
            // specifier. Copy the word verbatim and leave `expect` unchanged.
            // (Matching INSIDE a longer identifier like `importer`/`fromage` is
            // already prevented by the `is_ident_start`/`is_ident_continue` token
            // boundary; this guard covers the `.`-prefixed member-access case.)
            let member_access = prev_significant_byte(bytes, start) == Some(b'.');
            if !member_access {
                match word {
                    "import" => {
                        // Decide the specifier form from the next significant byte:
                        // `(` → dynamic, a quote → side-effect, else a static import
                        // whose specifier follows a `from` clause.
                        match next_significant_byte(bytes, i) {
                            Some(b'(') => expect = SpecifierExpect::NextString,
                            Some(b'\'') | Some(b'"') => expect = SpecifierExpect::NextString,
                            _ => expect = SpecifierExpect::AfterFrom,
                        }
                    }
                    "export" => {
                        // `export … from "x"` carries a specifier; a specifier-less
                        // `export` (e.g. `export const`) simply never reaches a
                        // `from` before the construct's string/`;` clears it.
                        expect = SpecifierExpect::AfterFrom;
                    }
                    "from" if expect == SpecifierExpect::AfterFrom => {
                        // The next code-level string is the specifier.
                        expect = SpecifierExpect::NextString;
                    }
                    _ => {}
                }
            }
            continue;
        }

        // Any other byte: a `;` ends a statement, clearing a dangling
        // specifier-less `export`/`import` expectation so a later unrelated
        // string is not captured. All other bytes are copied verbatim.
        if b == b';' {
            expect = SpecifierExpect::None;
        }
        result.push(b as char);
        i += 1;
    }

    result
}

/// Whether `b` can start a JS/TS identifier-or-keyword token. Word boundaries
/// keep the scanner from matching `import`/`export`/`from` inside a longer
/// identifier (e.g. `importer`, `fromage`).
fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$' || b >= 0x80
}

/// Whether `b` continues a JS/TS identifier-or-keyword token.
fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

/// The next byte after `from` that is not ASCII whitespace, skipping `//` and
/// `/* */` comments. Used to disambiguate `import(` / `import "x"` / `import X`.
fn next_significant_byte(bytes: &[u8], mut i: usize) -> Option<u8> {
    let len = bytes.len();
    while i < len {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(len);
            continue;
        }
        return Some(b);
    }
    None
}

/// The last significant code byte strictly BEFORE index `start`, or `None` when
/// no code-level byte precedes it. Used by the specifier scanner's leading-context
/// guard to tell a keyword introducer (`import`/`export`/`from` at a
/// statement/expression position) from a member name (`obj.import(…)`,
/// `obj?.import(…)`, `obj.from(…)`), whose prior significant byte is `.`.
///
/// The lexical state at `start` is established by scanning FORWARD from the
/// beginning of `bytes` with the SAME string / template / block-comment / line-
/// comment state machine the forward specifier scan ([`lower_carrier_specifiers_in_module_positions`])
/// uses — not a backward line-local heuristic. This is the only correct way to
/// classify a `//`, a `.`, or a quote that lies on a continuation line of a
/// MULTILINE construct: a `//` inside an unterminated template literal (or a block
/// comment) carried over from a prior physical line is template / comment text, not
/// a code-level line comment, and the `.` beside it is not a member-access
/// qualifier. A line-local backward scan cannot know the line began mid-template
/// and would wrongly suppress a genuine specifier that follows the construct's
/// close. By replaying the forward lexical state, the byte reported is the last one
/// at true code level (skipping whitespace, comments, and string/template bodies),
/// so the member-access guard fires only on a real `.`-qualified keyword.
///
/// Template interpolations (`${ … }`) are skipped wholesale (brace-balanced),
/// exactly as the forward scanner treats them: the forward scan never tokenizes an
/// `import`/`from`/`export` keyword INSIDE an interpolation, so `start` is always a
/// code-level position outside any interpolation and the opaque skip keeps the two
/// scanners consistent. Escaped backticks (`` \` ``) inside a template do not close
/// it (and an escape pair inside a single/double string is consumed), so a string
/// or template that spans physical lines is tracked across them.
fn prev_significant_byte(bytes: &[u8], start: usize) -> Option<u8> {
    let len = bytes.len();
    let end = start.min(len);
    let mut i = 0usize;
    let mut last_significant: Option<u8> = None;
    while i < end {
        let b = bytes[i];

        // Line comment: skip to end of line (the byte is comment text, never
        // significant). A `//` is a comment opener ONLY here, at true code level.
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Block comment: skip to its closing `*/`, across physical lines.
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(len); // consume the `*/` (or run to EOF)
            continue;
        }

        // Template literal: skip its body across physical lines, consuming escape
        // pairs and skipping `${ … }` interpolations wholesale by brace depth — so a
        // `//`, a `.`, or a quote inside the template is never seen as code, and an
        // inner string/template cannot re-enter code level.
        if b == b'`' {
            i += 1;
            while i < len {
                let c = bytes[i];
                if c == b'\\' {
                    i += 2; // an escaped backtick does not close the template
                    continue;
                }
                if c == b'`' {
                    i += 1;
                    break;
                }
                if c == b'$' && i + 1 < len && bytes[i + 1] == b'{' {
                    i += 2;
                    let mut depth = 1usize;
                    while i < len && depth > 0 {
                        match bytes[i] {
                            b'{' => depth += 1,
                            b'}' => depth -= 1,
                            _ => {}
                        }
                        i += 1;
                    }
                    continue;
                }
                i += 1;
            }
            continue;
        }

        // String literal (single or double quote): skip its body across the line,
        // consuming escape pairs, so a `//` or `.` inside it is never seen as code.
        if b == b'\'' || b == b'"' {
            let quote = b;
            i += 1;
            while i < len {
                let c = bytes[i];
                if c == b'\\' {
                    i += 2;
                    continue;
                }
                if c == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // A code-level byte: whitespace is not significant; everything else is the
        // running candidate for "last significant code byte before `start`".
        if !b.is_ascii_whitespace() {
            last_significant = Some(b);
        }
        i += 1;
    }
    last_significant
}

/// The rewrite decision for a single quoted import specifier.
enum Rewrite {
    /// Replace the specifier with this temp-dir stub path (known carrier).
    Stub(String),
    /// Keep only the leading `len` bytes (the bare carrier path), dropping the
    /// virtual suffix (unknown carrier → wildcard-shim fallback).
    CarrierPath(usize),
}

/// Classify a quoted import `specifier` as a carrier-virtual import.
///
/// Returns `None` if the specifier is not a carrier companion (left untouched).
/// Otherwise resolves the stub via an EXACT canonical `vue_ts_map` lookup. The
/// map is keyed by the canonical carrier path, and `rewrite_relative_imports`
/// has already absolutized every real specifier to that canonical form, so a
/// true import always exact-hits. There is no basename fallback: matching by
/// filename alone is ambiguous when two carriers in different directories share
/// a name, and would route a same-basename specifier to the wrong stub.
///
/// Two carrier shapes route here:
/// - A suffixed virtual specifier (`…/Foo.vue.tsx` / `…/Foo.vue.verter.ts`):
///   strip the suffix, then a known carrier → `Stub`, an unknown carrier →
///   `CarrierPath` (drop the suffix back to the bare carrier path for the
///   `*.vue` wildcard shim).
/// - An already-bare in-project carrier (`…/Foo.vue` / `…/Foo.svelte`): a known
///   carrier → the generic-bearing public-API `Stub`; an unknown carrier →
///   `None` (left bare for the `*.vue` wildcard shim — a bare path has no suffix
///   to drop, so `CarrierPath` would corrupt it).
fn carrier_virtual_import_target(
    specifier: &str,
    vue_ts_map: &HashMap<String, PathBuf>,
) -> Option<Rewrite> {
    // The bare carrier path the specifier targets, and whether a virtual suffix
    // was stripped to recover it. Suffixed forms strip the first matching
    // suffix (longest-first so `.verter.ts` wins); an already-bare carrier is
    // taken as-is. A specifier that is neither is not a carrier companion.
    let (carrier_path, suffix_stripped) = CARRIER_VIRTUAL_IMPORT_SUFFIXES
        .iter()
        .find_map(|suffix| specifier.strip_suffix(suffix))
        .filter(|carrier| verter_workspace::path_is_carrier(carrier))
        .map(|carrier| (carrier, true))
        .or_else(|| verter_workspace::path_is_carrier(specifier).then_some((specifier, false)))?;

    // Known carrier → temp-dir stub, by EXACT canonical lookup only.
    match vue_ts_map.get(carrier_path) {
        Some(stub_path) => Some(Rewrite::Stub(
            stub_path.to_string_lossy().replace('\\', "/"),
        )),
        // Unknown carrier. A suffixed specifier drops its virtual suffix back to
        // the bare carrier path (for the `*.vue` wildcard shim); an already-bare
        // carrier is left exactly as-is (`None`) — there is no suffix to drop.
        None if suffix_stripped => Some(Rewrite::CarrierPath(carrier_path.len())),
        None => None,
    }
}

/// Sanitize a component name to be a valid JavaScript identifier.
///
/// - Prepends `_` if it starts with a digit (e.g. `404` → `_404`)
/// - Replaces non-alphanumeric chars with `_`
/// - Prefixes JS reserved words (e.g. `default` → `_default`, `export` → `_export`)
fn sanitize_component_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect();

    let result = if sanitized.is_empty() {
        "Component".to_string()
    } else if sanitized.chars().next().unwrap().is_ascii_digit() {
        format!("_{sanitized}")
    } else {
        sanitized
    };

    // Prefix reserved words
    match result.as_str() {
        "default" | "export" | "import" | "class" | "function" | "return" | "var" | "let"
        | "const" | "if" | "else" | "for" | "while" | "do" | "switch" | "case" | "break"
        | "continue" | "new" | "delete" | "typeof" | "void" | "this" | "with" | "throw" | "try"
        | "catch" | "finally" | "in" | "of" | "yield" | "await" | "async" | "extends" | "super"
        | "static" | "enum" | "implements" | "interface" | "package" | "private" | "protected"
        | "public" => format!("_{result}"),
        _ => result,
    }
}

/// Simple non-cryptographic hash for generating unique temp file names.
fn simple_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Build a map from emitted `.d.ts` filenames to target `.vue.d.ts` relative paths.
///
/// key = "Button_a1b2c3d4.tsc.tsx.d.ts"  (the filename tsc emits)
/// val = "src/components/Button.vue.d.ts" (target relative path under declarationDir)
fn build_dts_rename_map(
    generated: &[(PathBuf, String, PathBuf)],
    root_dir: &Path,
) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();
    for (vue_path, _, tsc_tsx_path) in generated {
        // The filename tsc emits: e.g. "Button_a1b2c3d4.tsc.tsx.d.ts"
        let emitted_name = format!(
            "{}.d.ts",
            tsc_tsx_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );

        // Target: vue_path relative to root_dir, with .d.ts appended.
        // e.g. /project/src/Button.vue → src/Button.vue.d.ts
        let rel = vue_path
            .strip_prefix(root_dir)
            .unwrap_or(vue_path.as_path());
        let target = PathBuf::from(format!("{}.d.ts", rel.to_string_lossy().replace('\\', "/")));

        map.insert(emitted_name, target);
    }
    map
}

/// Post-process tsc-emitted `.vue` declaration files.
///
/// Renames `.tsc.tsx.d.ts` files to `.vue.d.ts` with correct directory structure,
/// rewrites absolute `import()` paths to relative, and cleans up artifacts.
fn postprocess_vue_declarations(
    decl_dir: &Path,
    generated: &[(PathBuf, String, PathBuf)],
    root_dir: &Path,
) {
    let rename_map = build_dts_rename_map(generated, root_dir);
    if rename_map.is_empty() {
        return;
    }

    // Also build a map from tsc.tsx stem → vue relative path (without .d.ts) for import rewriting.
    // key = "Button_a1b2c3d4.tsc.tsx" → val = "src/components/Button.vue"
    let mut import_rewrite_map: HashMap<String, String> = HashMap::new();
    for (vue_path, _, tsc_tsx_path) in generated {
        let tsx_stem = tsc_tsx_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let rel = vue_path
            .strip_prefix(root_dir)
            .unwrap_or(vue_path.as_path());
        import_rewrite_map.insert(tsx_stem, rel.to_string_lossy().replace('\\', "/"));
    }

    // Scan declarationDir recursively for .tsc.tsx.d.ts files.
    let entries: Vec<PathBuf> = walkdir::WalkDir::new(decl_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tsc.tsx.d.ts"))
        .map(|e| e.into_path())
        .collect();

    for entry in &entries {
        let filename = entry
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        if let Some(target_rel) = rename_map.get(&filename) {
            let target_path = decl_dir.join(target_rel);

            // Read and rewrite imports.
            let content = match fs::read_to_string(entry) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let target_dir = target_path.parent().unwrap_or(decl_dir);
            let rewritten =
                rewrite_dts_imports(&content, target_dir, root_dir, &import_rewrite_map);

            // Create parent directories and write.
            if let Some(parent) = target_path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!(
                        "verter-tsc: failed to create directory {}: {e}",
                        parent.display()
                    );
                    continue;
                }
            }
            if let Err(e) = fs::write(&target_path, rewritten) {
                eprintln!("verter-tsc: failed to write {}: {e}", target_path.display());
                continue;
            }

            // Delete original temp-named file.
            if let Err(e) = fs::remove_file(entry) {
                eprintln!(
                    "verter-tsc: failed to remove temp file {}: {e}",
                    entry.display()
                );
            }
        }
    }

    // Delete vue-shims.d.ts artifact if emitted.
    let shims = decl_dir.join("vue-shims.d.ts");
    if shims.exists() {
        let _ = fs::remove_file(&shims);
    }

    // Clean up empty directories left behind.
    cleanup_empty_dirs(decl_dir);
}

/// Rewrite absolute `import("...")` paths in `.d.ts` content.
///
/// `rewrite_relative_imports` runs first and converts relative paths to absolute.
/// tsc propagates these into `.d.ts` output. This function converts them back to
/// relative paths from the target `.vue.d.ts` file's location.
///
/// Also rewrites references to `.tsc.tsx` temp files to their `.vue` counterparts.
fn rewrite_dts_imports(
    content: &str,
    target_dir: &Path,
    root_dir: &Path,
    import_rewrite_map: &HashMap<String, String>,
) -> String {
    let mut result = String::with_capacity(content.len());
    let mut rest = content;

    let root_str = root_dir.to_string_lossy().replace('\\', "/");

    while let Some(pos) = rest.find("import(") {
        result.push_str(&rest[..pos]);
        let after_import = &rest[pos + 7..]; // skip "import("

        // Determine quote character.
        let quote = match after_import.chars().next() {
            Some(q @ '\'') | Some(q @ '"') => q,
            _ => {
                result.push_str("import(");
                rest = after_import;
                continue;
            }
        };

        // Find closing quote.
        let path_start = 1; // skip opening quote
        let path_end = match after_import[path_start..].find(quote) {
            Some(i) => path_start + i,
            None => {
                result.push_str("import(");
                rest = after_import;
                continue;
            }
        };

        let import_path = &after_import[path_start..path_end];
        let rewritten_path = rewrite_single_import_path(
            import_path,
            target_dir,
            &root_str,
            root_dir,
            import_rewrite_map,
        );

        result.push_str(&format!("import({quote}{rewritten_path}{quote}"));
        rest = &after_import[path_end + 1..]; // skip closing quote
    }

    result.push_str(rest);
    result
}

/// Rewrite a single import path from a `.d.ts` file.
///
/// Handles three cases:
/// 1. Absolute path under root_dir → convert to relative from target_dir
/// 2. Path containing a `.tsc.tsx` temp file reference → replace with `.vue` path
/// 3. Bare module import → preserve unchanged
fn rewrite_single_import_path(
    import_path: &str,
    target_dir: &Path,
    root_str: &str,
    root_dir: &Path,
    import_rewrite_map: &HashMap<String, String>,
) -> String {
    let normalized = import_path.replace('\\', "/");

    // Case 1: Absolute path starting with root_dir.
    if let Some(stripped) = normalized.strip_prefix(root_str) {
        let rel_from_root = stripped.trim_start_matches('/');
        let target_path = root_dir.join(rel_from_root);
        let target_dir_normalized = target_dir.to_string_lossy().replace('\\', "/");
        return compute_relative_path(
            &target_dir_normalized,
            &target_path.to_string_lossy().replace('\\', "/"),
        );
    }

    // Case 2: Check if this path references a .tsc.tsx temp file.
    // tsc may emit relative paths like "./Button_a1b2c3d4.tsc.tsx"
    for (tsx_name, vue_rel) in import_rewrite_map {
        let tsx_stem = tsx_name.trim_end_matches(".tsc.tsx");
        if normalized.contains(tsx_stem) && normalized.contains(".tsc.tsx") {
            // Replace with relative path to the .vue file.
            let vue_abs = root_dir.join(vue_rel);
            let target_dir_normalized = target_dir.to_string_lossy().replace('\\', "/");
            return compute_relative_path(
                &target_dir_normalized,
                &vue_abs.to_string_lossy().replace('\\', "/"),
            );
        }
    }

    // Case 3: Bare module import — preserve as-is.
    normalized
}

/// Compute a relative path from `from_dir` to `to_path`.
///
/// Both inputs should use forward slashes. Returns a path starting with `./` or `../`.
fn compute_relative_path(from_dir: &str, to_path: &str) -> String {
    let from_parts: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to_path.split('/').filter(|s| !s.is_empty()).collect();

    // Find common prefix length.
    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Number of `..` needed = remaining segments in from_dir.
    let ups = from_parts.len() - common;
    let remaining = &to_parts[common..];

    let mut parts: Vec<&str> = vec![".."; ups];
    parts.extend_from_slice(remaining);

    if parts.is_empty() {
        ".".to_string()
    } else if ups == 0 {
        format!("./{}", parts.join("/"))
    } else {
        parts.join("/")
    }
}

/// Remove empty directories under `dir` (bottom-up).
fn cleanup_empty_dirs(dir: &Path) {
    // Collect directories bottom-up (deepest first).
    let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir() && e.path() != dir)
        .map(|e| e.into_path())
        .collect();
    // Sort by depth descending (longest path first).
    dirs.sort_by_key(|b| std::cmp::Reverse(b.components().count()));
    for d in dirs {
        // remove_dir only succeeds if empty.
        let _ = fs::remove_dir(&d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsconfig::load_tsconfig;

    /// The minimal `--lsp` handshake arm (POSIX sh) spliced into every mock
    /// checker script: the capability-validated resolver spawns each candidate
    /// with `--lsp --stdio` and requires an `initialize` response whose
    /// `serverInfo.version` agrees with the `--version` probe. The arm answers
    /// every framed request carrying an `id` with that serverInfo, then serves
    /// until EOF (the smoke kills the process after the handshake).
    const MOCK_LSP_HANDSHAKE_SH: &str = r#"
for arg in "$@"; do
  if [ "$arg" = "--lsp" ]; then
    while IFS= read -r line; do
      line=${line%$'\r'}
      len=0
      while [ -n "$line" ]; do
        case "$line" in
          [Cc]ontent-[Ll]ength:*) len=$(printf '%s' "${line#*:}" | tr -d ' ') ;;
        esac
        IFS= read -r line || exit 0
        line=${line%$'\r'}
      done
      [ "$len" -gt 0 ] || continue
      body=$(dd bs=1 count="$len" 2>/dev/null)
      id=$(printf '%s' "$body" | sed -n 's/.*"id"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)
      [ -n "$id" ] || continue
      resp="{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"capabilities\":{},\"serverInfo\":{\"name\":\"mock-tsc\",\"version\":\"7.0.2\"}}}"
      printf 'Content-Length: %s\r\n\r\n%s' "${#resp}" "$resp"
    done
    exit 0
  fi
done
"#;

    /// The same handshake arm (PowerShell) for the Windows mock scripts.
    const MOCK_LSP_HANDSHAKE_PS1: &str = r#"
if ($Args -contains '--lsp') {
    $reader = [System.Console]::In
    $writer = [System.Console]::Out
    while ($true) {
        $len = 0
        $line = $reader.ReadLine()
        if ($null -eq $line) { exit 0 }
        while ($line -ne '') {
            if ($line -match 'Content-Length:\s*(\d+)') { $len = [int]$Matches[1] }
            $line = $reader.ReadLine()
            if ($null -eq $line) { exit 0 }
        }
        if ($len -le 0) { continue }
        $buf = New-Object char[] $len
        $off = 0
        while ($off -lt $len) {
            $n = $reader.Read($buf, $off, $len - $off)
            if ($n -le 0) { exit 0 }
            $off += $n
        }
        $body = -join $buf
        if ($body -match '"id"\s*:\s*(\d+)') {
            $resp = '{"jsonrpc":"2.0","id":' + $Matches[1] + ',"result":{"capabilities":{},"serverInfo":{"name":"mock-tsc","version":"7.0.2"}}}'
            $writer.Write("Content-Length: " + $resp.Length + "`r`n`r`n" + $resp)
            $writer.Flush()
        }
    }
}
"#;

    /// Discriminating gate for the `build_host_config()` seam: the production
    /// `verter-tsc` host MUST construct through the Batch typecheck preset
    /// (BUILD analysis scope + `Build` query profile + lazily-spawned
    /// host-owned CPU pool), NOT the Full / LSP-interactive default.
    ///
    /// RED when `build_host_config()` returns `HostConfig::default()`; GREEN
    /// when it returns `HostConfig::batch_typecheck()`. Asserts both the
    /// positive identity (== the Batch preset, i.e. BUILD scope / `Build`
    /// profile) and the negative (!= the default Full preset, i.e. != LSP scope
    /// / != `LspInteractive` profile) on scope, query profile, and host-pool
    /// spawn timing, then re-checks through a constructed host's public
    /// accessors. The `verter_tsc` crate depends only on `verter_session` (not
    /// `verter_semantic`) and `verter_session` re-exports neither `AnalysisScope`
    /// nor `QueryProfile`, so the BUILD/`Build` and LSP/`LspInteractive` targets
    /// are sourced from the canonical preset constructors rather than named
    /// variants — semantically identical, and it ties the seam directly to the
    /// presets it must select between.
    #[test]
    fn build_host_config_routes_production_host_through_batch_preset() {
        let cfg = build_host_config();
        let batch = HostConfig::batch_typecheck();
        let full = HostConfig::default();

        // Effective analysis scope == BUILD (Batch preset), != LSP (default).
        assert_eq!(
            cfg.effective_scope(),
            batch.effective_scope(),
            "build_host_config() must use the Batch BUILD analysis scope"
        );
        assert_ne!(
            cfg.effective_scope(),
            full.effective_scope(),
            "build_host_config() must NOT use the default LSP analysis scope"
        );

        // Query profile == Build (Batch preset), != LspInteractive (default).
        assert_eq!(
            cfg.query_profile, batch.query_profile,
            "build_host_config() must use the Build query profile"
        );
        assert_ne!(
            cfg.query_profile, full.query_profile,
            "build_host_config() must NOT use the LspInteractive query profile"
        );

        // Host-owned CPU pool spawns lazily under Batch, eagerly under default.
        assert_eq!(
            cfg.resource_policy.host_cpu_pool.spawn, batch.resource_policy.host_cpu_pool.spawn,
            "build_host_config() must use lazy host-owned CPU pool spawn"
        );
        assert_ne!(
            cfg.resource_policy.host_cpu_pool.spawn, full.resource_policy.host_cpu_pool.spawn,
            "build_host_config() must NOT use eager host-owned CPU pool spawn"
        );

        // The same identity must hold through a constructed host's public API.
        let host = VerterHost::new_standalone(build_host_config());
        assert_eq!(host.config().effective_scope(), batch.effective_scope());
        assert_ne!(host.config().effective_scope(), full.effective_scope());
        assert_eq!(host.query_profile(), batch.query_profile);
        assert_ne!(host.query_profile(), full.query_profile);
    }

    /// Install a mock checker the DECLARATION stage resolves through the
    /// toolchain resolver's project-local `.bin` shim tier. Its `--version` +
    /// `--lsp` handshake arms make it pass the capability smoke (version
    /// `7.0.2`, matching `serverInfo`); its `--project`/`--declaration`
    /// behavior is the fixture under test. It does NOT speak the `--api`
    /// wire, so the in-memory typecheck stage cannot run against it — these
    /// tests drive [`run_declaration_stage`] directly (see
    /// [`run_declaration_only`]), leaving the declaration-stage diagnostics as
    /// the only ones they observe.
    fn write_mock_tsc(project_root: &Path, mode: &str) {
        let bin_dir = project_root.join("node_modules").join(".bin");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(bin_dir.join("mock-mode.txt"), mode).unwrap();

        #[cfg(target_os = "windows")]
        {
            let ps1 = bin_dir.join("mock-tsc.ps1");
            fs::write(
                &ps1,
                r#"
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Args)

if ($Args -contains '--version') {
    Write-Output 'Version 7.0.2'
    exit 0
}

__MOCK_LSP_HANDSHAKE_PS1__

$project = ''
$declaration = $false
$declarationDir = ''

for ($i = 0; $i -lt $Args.Length; $i++) {
    switch ($Args[$i]) {
        '--project' {
            $project = $Args[$i + 1]
            $i++
            continue
        }
        '--declaration' {
            $declaration = $true
            continue
        }
        '--declarationDir' {
            $declarationDir = $Args[$i + 1]
            $i++
            continue
        }
    }
}

if (-not $declaration) {
    exit 0
}

$mode = (Get-Content (Join-Path $PSScriptRoot 'mock-mode.txt') -Raw).Trim()
$tsconfigDir = Split-Path -Parent $project
$tscTsx = Get-ChildItem -Path $tsconfigDir -Recurse -Filter *.tsc.tsx | Select-Object -First 1

if ($mode -eq 'phase-b-fail') {
    $file = $tscTsx.FullName.Replace('\', '/')
    Write-Output "$file(1,1): error TS2304: Cannot find name 'MissingType'."
    exit 2
}

New-Item -ItemType Directory -Force -Path $declarationDir | Out-Null
$emitted = Join-Path $declarationDir ($tscTsx.Name + '.d.ts')
Set-Content -Path $emitted -Value 'export declare const ok: number;'
exit 0
"#
                .replace("__MOCK_LSP_HANDSHAKE_PS1__", MOCK_LSP_HANDSHAKE_PS1),
            )
            .unwrap();
            fs::write(
                bin_dir.join("tsgo.cmd"),
                "@echo off\r\npowershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0mock-tsc.ps1\" %*\r\nexit /b %ERRORLEVEL%\r\n",
            )
            .unwrap();
        }

        #[cfg(not(target_os = "windows"))]
        {
            let script = bin_dir.join("tsgo");
            fs::write(
                &script,
                r#"#!/bin/sh
project=""
declaration=0
declaration_dir=""

for arg in "$@"; do
  if [ "$arg" = "--version" ]; then
    echo "Version 7.0.2"
    exit 0
  fi
done

__MOCK_LSP_HANDSHAKE_SH__

while [ "$#" -gt 0 ]; do
  case "$1" in
    --project)
      project="$2"
      shift 2
      ;;
    --declaration)
      declaration=1
      shift
      ;;
    --declarationDir)
      declaration_dir="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [ "$declaration" -ne 1 ]; then
  exit 0
fi

mode=$(tr -d '\r\n' < "$(dirname "$0")/mock-mode.txt")
tsconfig_dir=$(dirname "$project")
tsc_tsx=$(find "$tsconfig_dir" -name '*.tsc.tsx' | head -n 1)

if [ "$mode" = "phase-b-fail" ]; then
  printf "%s(1,1): error TS2304: Cannot find name 'MissingType'.\n" "$tsc_tsx"
  exit 2
fi

mkdir -p "$declaration_dir"
printf "export declare const ok: number;\n" > "$declaration_dir/$(basename "$tsc_tsx").d.ts"
"#
                .replace("__MOCK_LSP_HANDSHAKE_SH__", MOCK_LSP_HANDSHAKE_SH),
            )
            .unwrap();

            use std::os::unix::fs::PermissionsExt;

            let mut perms = fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).unwrap();
        }
    }

    /// Write a mock tsc that reports an error (exit 1) but STILL emits a .d.ts file.
    /// This simulates real tsc behavior where errors in some files don't prevent
    /// emission of declarations for other (non-erroring) files.
    fn write_mock_tsc_error_with_emit(project_root: &Path, _decl_dir: &Path) {
        let bin_dir = project_root.join("node_modules").join(".bin");
        fs::create_dir_all(&bin_dir).unwrap();

        #[cfg(target_os = "windows")]
        {
            let ps1 = bin_dir.join("mock-tsc.ps1");
            fs::write(
                &ps1,
                r#"
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Args)

if ($Args -contains '--version') {
    Write-Output 'Version 7.0.2'
    exit 0
}

__MOCK_LSP_HANDSHAKE_PS1__

$project = ''
$declaration = $false
$declarationDir = ''

for ($i = 0; $i -lt $Args.Length; $i++) {
    switch ($Args[$i]) {
        '--project' {
            $project = $Args[$i + 1]
            $i++
            continue
        }
        '--declaration' {
            $declaration = $true
            continue
        }
        '--declarationDir' {
            $declarationDir = $Args[$i + 1]
            $i++
            continue
        }
    }
}

if (-not $declaration) {
    exit 0
}

$tsconfigDir = Split-Path -Parent $project
$tscTsx = Get-ChildItem -Path $tsconfigDir -Recurse -Filter *.tsc.tsx | Select-Object -First 1

# Emit the .d.ts file despite errors
New-Item -ItemType Directory -Force -Path $declarationDir | Out-Null
$emitted = Join-Path $declarationDir ($tscTsx.Name + '.d.ts')
Set-Content -Path $emitted -Value 'export declare const ok: number;'

# Also report an error from a different file
Write-Output "src/other.ts(5,10): error TS2304: Cannot find name 'SomeMissingType'."
exit 1
"#,
            )
            .unwrap();
            fs::write(
                bin_dir.join("tsgo.cmd"),
                "@echo off\r\npowershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0mock-tsc.ps1\" %*\r\nexit /b %ERRORLEVEL%\r\n",
            )
            .unwrap();
        }

        #[cfg(not(target_os = "windows"))]
        {
            let script = bin_dir.join("tsgo");
            fs::write(
                &script,
                r#"#!/bin/sh
project=""
declaration=0
declaration_dir=""

for arg in "$@"; do
  if [ "$arg" = "--version" ]; then
    echo "Version 7.0.2"
    exit 0
  fi
done

__MOCK_LSP_HANDSHAKE_SH__

while [ "$#" -gt 0 ]; do
  case "$1" in
    --project)
      project="$2"
      shift 2
      ;;
    --declaration)
      declaration=1
      shift
      ;;
    --declarationDir)
      declaration_dir="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [ "$declaration" -ne 1 ]; then
  exit 0
fi

tsconfig_dir=$(dirname "$project")
tsc_tsx=$(find "$tsconfig_dir" -name '*.tsc.tsx' | head -n 1)

# Emit the .d.ts file despite errors
mkdir -p "$declaration_dir"
printf "export declare const ok: number;\n" > "$declaration_dir/$(basename "$tsc_tsx").d.ts"

# Report an error from a different file
printf "src/other.ts(5,10): error TS2304: Cannot find name 'SomeMissingType'.\n"
exit 1
"#
                .replace("__MOCK_LSP_HANDSHAKE_SH__", MOCK_LSP_HANDSHAKE_SH),
            )
            .unwrap();

            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).unwrap();
        }
    }

    /// Drive ONLY the declaration/emit stage (the temp-file `tsgo --project`
    /// path), bypassing the in-memory `--api` typecheck stage. [`run`] now
    /// hard-fails (returns `Err`) when the `--api` engine is absent — tsgo-only,
    /// no tsc fallback — so declaration-stage tests, which install a
    /// `--project`-only mock and never provide an `--api` engine, exercise
    /// [`run_declaration_stage`] directly here. Mirrors [`run`]'s host
    /// construction + per-`.vue` upsert.
    fn run_declaration_only(
        config: &TsConfig,
        tsconfig_path: &Path,
        opts: &EmitOptions,
    ) -> (Vec<Diagnostic>, Vec<PathBuf>) {
        let host = VerterHost::new_standalone(build_host_config());
        for vue_path in &config.vue_files {
            let source = match fs::read_to_string(vue_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let canonical_id = vue_path.to_string_lossy().replace('\\', "/");
            let _ = host.upsert(UpsertRequest {
                canonical_id: Some(canonical_id.clone()),
                input_id: canonical_id,
                source: std::sync::Arc::<str>::from(source),
                file_language: FileLanguage::vue(),
                aliases: Vec::new(),
            });
        }
        run_declaration_stage(&host, config, tsconfig_path, opts)
            .expect("the declaration stage must run against the validating mock checker")
    }

    fn create_run_fixture(
        mode: &str,
    ) -> (
        tempfile::TempDir,
        crate::tsconfig::TsConfig,
        PathBuf,
        PathBuf,
    ) {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("project");
        let src_dir = root.join("src");
        let vue_path = src_dir.join("Test.vue");
        let tsconfig_path = root.join("tsconfig.json");
        let decl_dir = root.join("dist").join("types");

        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            &vue_path,
            r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ props.msg }}</div></template>
"#,
        )
        .unwrap();
        fs::write(
            &tsconfig_path,
            r#"{
  "compilerOptions": {
    "strict": true
  },
  "files": ["src/Test.vue"]
}"#,
        )
        .unwrap();
        write_mock_tsc(&root, mode);

        let config = load_tsconfig(&tsconfig_path).expect("test tsconfig should load");
        (temp, config, tsconfig_path, decl_dir)
    }

    /// Helper: write a minimal tsconfig and read back the `files` array.
    fn written_files(tsc_tsx_files: &[PathBuf], declaration: bool) -> Vec<String> {
        let temp = tempfile::TempDir::new().unwrap();
        // Write a base tsconfig for `extends` to reference.
        let base_tsconfig = temp.path().join("base-tsconfig.json");
        fs::write(&base_tsconfig, r#"{ "compilerOptions": {} }"#).unwrap();

        let opts = EmitOptions {
            no_emit: !declaration,
            declaration,
            declaration_dir: if declaration {
                Some(temp.path().join("dist"))
            } else {
                None
            },
        };

        let result = write_temp_tsconfig(
            temp.path(),
            &base_tsconfig,
            tsc_tsx_files,
            &opts,
            temp.path(),
        );
        let tsconfig_path = result.expect("write_temp_tsconfig should succeed");
        let content = fs::read_to_string(&tsconfig_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        json["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn write_temp_tsconfig_includes_all_provided_files() {
        let temp = tempfile::TempDir::new().unwrap();
        let shim = temp.path().join("vue-shims.d.ts");
        let tsx = temp.path().join("App_abc123.tsc.tsx");
        let ts_file = temp.path().join("index.ts");
        // Create the files so canonicalize works.
        fs::write(&shim, "").unwrap();
        fs::write(&tsx, "").unwrap();
        fs::write(&ts_file, "").unwrap();

        let files = written_files(&[shim, tsx, ts_file], true);
        assert_eq!(files.len(), 3, "should include shim + tsx + ts file");
    }

    #[test]
    fn write_temp_tsconfig_no_emit_only_tsx_files() {
        let temp = tempfile::TempDir::new().unwrap();
        let shim = temp.path().join("vue-shims.d.ts");
        let tsx = temp.path().join("App_abc123.tsc.tsx");
        fs::write(&shim, "").unwrap();
        fs::write(&tsx, "").unwrap();

        let files = written_files(&[shim, tsx], false);
        assert_eq!(files.len(), 2, "should include only shim + tsx");
    }

    #[test]
    fn write_temp_tsconfig_declaration_sets_emit_flags() {
        let temp = tempfile::TempDir::new().unwrap();
        let base_tsconfig = temp.path().join("base-tsconfig.json");
        fs::write(&base_tsconfig, r#"{ "compilerOptions": {} }"#).unwrap();

        let opts = EmitOptions {
            no_emit: false,
            declaration: true,
            declaration_dir: Some(temp.path().join("dist")),
        };

        let result =
            write_temp_tsconfig(temp.path(), &base_tsconfig, &[], &opts, temp.path()).unwrap();
        let content = fs::read_to_string(&result).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        let co = &json["compilerOptions"];
        assert_eq!(co["declaration"], true, "should set declaration: true");
        assert_eq!(
            co["emitDeclarationOnly"], true,
            "should set emitDeclarationOnly: true"
        );
        assert_eq!(co["noEmit"], false, "should set noEmit: false");
        // rootDir must be set
        assert!(co["rootDir"].is_string(), "should set rootDir");
        // include must be empty (all files listed explicitly)
        assert_eq!(
            json["include"],
            serde_json::json!([]),
            "include must be empty"
        );
    }

    #[test]
    fn rewrite_relative_imports_rewrites_dotslash() {
        let code = r#"import('./types').Props"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("/project/src/types"),
            "should resolve relative path: {result}"
        );
        assert!(
            !result.contains("/./"),
            "resolved path must not contain '/./' segment: {result}"
        );
        assert!(
            !result.contains("'./types'"),
            "original relative path should be replaced"
        );
    }

    #[test]
    fn rewrite_relative_imports_preserves_absolute() {
        let code = r#"import('vue').DefineComponent"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("'vue'"),
            "absolute import should be preserved: {result}"
        );
    }

    #[test]
    fn rewrite_relative_imports_from_keyword() {
        let code = r#"import type { Props } from './types'"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("/project/src/types"),
            "from keyword relative path should be rewritten: {result}"
        );
        assert!(
            !result.contains("/./"),
            "resolved path must not contain '/./' segment: {result}"
        );
        assert!(
            !result.contains("from './types'"),
            "original relative path should be replaced: {result}"
        );
    }

    #[test]
    fn rewrite_relative_imports_from_keyword_preserves_bare_module() {
        let code = r#"import { defineComponent } from "vue""#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("\"vue\""),
            "bare module import should be preserved: {result}"
        );
    }

    #[test]
    fn rewrite_relative_imports_preserves_pseudo_relative_absolute_path() {
        // The instance declaration generates import('./D:/full/path/file.vue.ts')
        // which starts with "./" but is actually already absolute after the prefix.
        let code = r#"import('./D:/project/src/file.vue.ts')['default']"#;
        let result = rewrite_relative_imports(code, Path::new("D:/project/src"));
        // Positive: should contain the absolute path, not doubled
        assert!(
            result.contains("'D:/project/src/file.vue.ts'"),
            "pseudo-relative absolute path should be resolved correctly: {result}"
        );
        // Negative: should NOT have doubled path
        assert!(
            !result.contains("D:/project/src/./D:/"),
            "should not double the path: {result}"
        );
    }

    #[test]
    fn rewrite_relative_imports_normalizes_dot_slash() {
        // `import('./components/Foo.vue.ts')` with vue_dir = "D:/project/src"
        // should produce "D:/project/src/components/Foo.vue.ts" — NOT "D:/project/src/./components/..."
        let code = r#"import('./components/Foo.vue.ts')['default']"#;
        let result = rewrite_relative_imports(code, Path::new("D:/project/src"));
        // Positive: the resolved absolute path should be clean
        assert!(
            result.contains("'D:/project/src/components/Foo.vue.ts'"),
            "dot-slash import should resolve to clean absolute path: {result}"
        );
        // Negative: must NOT contain "./" in the middle of the resolved path
        assert!(
            !result.contains("/./"),
            "resolved path must not contain '/./' segment: {result}"
        );
    }

    #[test]
    fn rewrite_relative_imports_normalizes_dot_slash_from_syntax() {
        // `from './types'` with vue_dir = "D:/project/src"
        let code = r#"import { Foo } from './types'"#;
        let result = rewrite_relative_imports(code, Path::new("D:/project/src"));
        assert!(
            result.contains("'D:/project/src/types'"),
            "from-syntax dot-slash import should resolve cleanly: {result}"
        );
        assert!(
            !result.contains("/./"),
            "resolved path must not contain '/./' segment: {result}"
        );
    }

    #[test]
    fn rewrite_relative_imports_absolutizes_bare_dot_dot() {
        // TS `pathIsRelative` classifies bare `..` as relative (the parent
        // directory's index module). Left un-absolutized in the generated
        // temp TSX, TypeScript resolves it against the TEMP directory —
        // spurious missing module on the validation lane.
        let code = r#"import type { Foo } from '..'"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("from '/project/src/..'"),
            "bare '..' must absolutize against the vue dir (TS normalizes \
             the '..' segment during resolution): {result}"
        );
        assert!(
            !result.contains("from '..'"),
            "the bare '..' specifier must not survive un-absolutized: {result}"
        );
    }

    #[test]
    fn rewrite_relative_imports_absolutizes_bare_dot() {
        // Bare `.` — the importer directory's own index module.
        let code = r#"import type { Foo } from '.'"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("from '/project/src'"),
            "bare '.' must absolutize to the vue dir itself: {result}"
        );
        assert!(
            !result.contains("from '.'"),
            "the bare '.' specifier must not survive un-absolutized: {result}"
        );
        assert!(
            !result.contains("/src/.'"),
            "bare '.' must not leave a trailing '/.' segment: {result}"
        );
    }

    #[test]
    fn rewrite_relative_imports_absolutizes_backslash_relative() {
        // `..\x` / `.\x` are the same TS `pathIsRelative` class ([\\/]);
        // the rewritten output is separator-normalized.
        let code = r#"import type { Foo } from '..\x'"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("'/project/src/../x'"),
            "'..\\x' must absolutize with normalized separators: {result}"
        );
        let code = r#"import type { Foo } from '.\x'"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("'/project/src/x'"),
            "'.\\x' must absolutize with normalized separators: {result}"
        );
    }

    #[test]
    fn rewrite_relative_imports_preserves_non_relative_dot_prefixed_and_bare() {
        // `.foo` is NOT in the pathIsRelative class (no separator after
        // the leading `.`) — package-ish, preserved byte-for-byte, like a
        // bare package name.
        let code = r#"import { x } from '.foo'"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("from '.foo'"),
            "non-relative dot-prefixed specifier must be preserved: {result}"
        );
        let code = r#"import { x } from 'pkg'"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("from 'pkg'"),
            "bare package specifier must be preserved: {result}"
        );
    }

    // ── carrier public-API import rewriting tests ──────────────

    #[test]
    fn lower_tsc_validation_carrier_specifiers_rewrites_known_and_strips_unknown() {
        let mut map = HashMap::new();
        map.insert(
            "D:/project/src/components/Foo.vue".to_string(),
            PathBuf::from("C:/tmp/Foo_abc.vue.ts"),
        );
        // Bar.vue is NOT in the map — should fall back to stripping `.verter.ts`.

        let code = r#"import('D:/project/src/components/Foo.vue.verter.ts')['default']
import type { Props } from 'D:/project/src/components/Bar.vue.verter.ts'"#;

        let result = lower_tsc_validation_carrier_specifiers(code, &map);

        // Positive: known Foo carrier-API should be rewritten to its temp stub.
        assert!(
            result.contains("'C:/tmp/Foo_abc.vue.ts'"),
            "known Foo carrier-API should become temp stub path: {result}"
        );

        // Positive: unknown Bar carrier-API should strip back to the carrier path.
        assert!(
            result.contains("'D:/project/src/components/Bar.vue'"),
            "unknown Bar carrier-API should become Bar.vue: {result}"
        );

        // Negative: original Foo carrier-API path should not remain.
        assert!(
            !result.contains("D:/project/src/components/Foo.vue.verter.ts"),
            "original Foo carrier-API path should be replaced: {result}"
        );
    }

    /// Discriminating regression for the carrier-API suffix migration: the
    /// rewrite matches the reserved `.verter.ts` suffix (not the legacy
    /// `.vue.ts` stub extension). Carrier resolution is EXACT-canonical only —
    /// a carrier-API specifier whose stripped path does not exact-hit
    /// `vue_ts_map` strips back to the bare carrier path (for the `*.vue`
    /// wildcard shim), regardless of any basename coincidence (the ambiguous
    /// basename fallback was removed).
    #[test]
    fn lower_tsc_validation_carrier_specifiers_matches_verter_suffix_exact_canonical() {
        let mut map = HashMap::new();
        map.insert(
            "D:/project/src/Foo.vue".to_string(),
            PathBuf::from("C:/tmp/Foo_abc.vue.ts"),
        );

        // Known carrier-API, EXACT canonical path → stub (the real-pipeline path:
        // post-`rewrite_relative_imports` specifiers are absolute and exact-hit).
        let known = r#"import('D:/project/src/Foo.vue.verter.ts')['default']"#;
        let known_out = lower_tsc_validation_carrier_specifiers(known, &map);
        assert!(
            known_out.contains("'C:/tmp/Foo_abc.vue.ts'"),
            "exact-canonical known carrier-API should map to its stub: {known_out}"
        );
        assert!(
            !known_out.contains("Foo.vue.verter.ts"),
            "the original carrier-API specifier should be gone: {known_out}"
        );

        // A bare-basename carrier-API specifier (`Foo.vue.verter.ts`, no directory)
        // does NOT exact-hit the canonical map key → strips to the bare carrier
        // path (NOT the stub: the ambiguous basename fallback is gone).
        let bare_basename = r#"import('Foo.vue.verter.ts')['default']"#;
        let bare_out = lower_tsc_validation_carrier_specifiers(bare_basename, &map);
        assert!(
            bare_out.contains("'Foo.vue'"),
            "a bare-basename carrier-API that does not exact-hit must strip to the \
             bare carrier path, not the stub: {bare_out}"
        );
        assert!(
            !bare_out.contains("C:/tmp/Foo_abc.vue.ts"),
            "a bare-basename carrier-API must NOT route to the stub via basename: {bare_out}"
        );

        // Unknown carrier-API, bare basename → stripped to the carrier path.
        let unknown = r#"import('Bar.vue.verter.ts')['default']"#;
        let unknown_out = lower_tsc_validation_carrier_specifiers(unknown, &map);
        assert!(
            unknown_out.contains("'Bar.vue'"),
            "unknown carrier-API should strip back to Bar.vue: {unknown_out}"
        );
        assert!(
            !unknown_out.contains(".verter.ts"),
            "no `.verter.ts` should survive for the unknown carrier: {unknown_out}"
        );

        // A legacy `.vue.ts` specifier is NOT a carrier-API import and is left
        // untouched (proves the matcher keys on `.verter.ts`, not `.vue.ts`).
        let legacy = r#"import('D:/project/src/Foo.vue.ts')['default']"#;
        let legacy_out = lower_tsc_validation_carrier_specifiers(legacy, &map);
        assert_eq!(
            legacy_out, legacy,
            "a legacy `.vue.ts` specifier must not be treated as carrier-API: {legacy_out}"
        );
    }

    /// The IDE carrier surface: an in-project component import is rewritten to
    /// the component's IDE carrier (`./Comp.vue` → `./Comp.vue.tsx`). `verter_tsc`
    /// must map that `.vue.tsx` specifier to the same stub as the API carrier,
    /// and a plain `.tsx` import that is NOT a carrier companion must be left
    /// alone (the `path_is_carrier` gate).
    #[test]
    fn lower_tsc_validation_carrier_specifiers_maps_ide_carrier_and_ignores_plain_tsx() {
        let mut map = HashMap::new();
        map.insert(
            "D:/project/src/Child.vue".to_string(),
            PathBuf::from("C:/tmp/Child_abc.vue.ts"),
        );

        // Known IDE carrier import → stub.
        let known = r#"import Child from 'D:/project/src/Child.vue.tsx'"#;
        let known_out = lower_tsc_validation_carrier_specifiers(known, &map);
        assert!(
            known_out.contains("'C:/tmp/Child_abc.vue.ts'"),
            "known `.vue.tsx` IDE carrier should map to its stub: {known_out}"
        );
        assert!(
            !known_out.contains(".vue.tsx"),
            "the IDE-carrier specifier should be gone: {known_out}"
        );

        // Unknown IDE carrier → strip `.tsx` back to the carrier path for the shim.
        let unknown = r#"import Other from 'D:/vendor/Other.vue.tsx'"#;
        let unknown_out = lower_tsc_validation_carrier_specifiers(unknown, &map);
        assert!(
            unknown_out.contains("'D:/vendor/Other.vue'"),
            "unknown `.vue.tsx` should strip back to the carrier path: {unknown_out}"
        );

        // A plain `.tsx` import (not a carrier companion) must be untouched.
        let plain = r#"import W from './Widget.tsx'"#;
        let plain_out = lower_tsc_validation_carrier_specifiers(plain, &map);
        assert_eq!(
            plain_out, plain,
            "a non-carrier `.tsx` import must be left untouched: {plain_out}"
        );
    }

    /// A BARE in-project carrier specifier (no virtual suffix) routes to the
    /// generic-bearing public-API stub via the exact `vue_ts_map` lookup — the
    /// same target the `.tsx`/`.verter.ts`-suffixed surfaces resolve to. This is
    /// the direct classifier contract: a known bare `…/Comp.vue` is `Stub`, not
    /// `None`. (An unknown bare carrier stays `None` so the `*.vue` wildcard shim
    /// handles it — covered by the negative test below.)
    #[test]
    fn carrier_virtual_import_target_routes_bare_in_project_carrier_to_stub() {
        let mut map = HashMap::new();
        map.insert(
            "/ws/src/GenericComp.vue".to_string(),
            PathBuf::from("/tmp/GenericComp_abc.vue.ts"),
        );

        // Bare in-project carrier → the public-API stub (exact canonical hit).
        match carrier_virtual_import_target("/ws/src/GenericComp.vue", &map) {
            Some(Rewrite::Stub(stub)) => assert_eq!(
                stub, "/tmp/GenericComp_abc.vue.ts",
                "bare in-project carrier must route to its public-API stub"
            ),
            other => panic!(
                "bare in-project carrier must be Rewrite::Stub, got {}",
                rewrite_label(&other)
            ),
        }

        // Unknown bare carrier (not in the map) → None: left bare for the
        // `*.vue` wildcard shim. NOT a `CarrierPath` strip (a bare carrier has
        // no suffix to drop — stripping would corrupt the path).
        assert!(
            carrier_virtual_import_target("/ws/src/Unknown.vue", &map).is_none(),
            "unknown bare carrier must be None (left bare for the wildcard shim), \
             never a CarrierPath strip"
        );

        // Non-carrier bare specifiers → None (untouched), unchanged behavior.
        assert!(
            carrier_virtual_import_target("./types", &map).is_none(),
            "a relative non-carrier specifier must be None"
        );
        assert!(
            carrier_virtual_import_target("lodash", &map).is_none(),
            "a bare package specifier must be None"
        );
        // A `.d.ts` whose stem (`./foo`) is not a carrier → None.
        assert!(
            carrier_virtual_import_target("./foo.d.ts", &map).is_none(),
            "a non-carrier `.d.ts` must be None"
        );

        // The existing suffixed-form behavior still holds: a known `.vue.tsx`
        // resolves to the stub, an unknown `.vue.tsx` strips back to the carrier.
        match carrier_virtual_import_target("/ws/src/GenericComp.vue.tsx", &map) {
            Some(Rewrite::Stub(stub)) => assert_eq!(
                stub, "/tmp/GenericComp_abc.vue.ts",
                "known `.vue.tsx` must still resolve to the stub"
            ),
            other => panic!(
                "known `.vue.tsx` must be Rewrite::Stub, got {}",
                rewrite_label(&other)
            ),
        }
        match carrier_virtual_import_target("/ws/src/Unknown.vue.tsx", &map) {
            Some(Rewrite::CarrierPath(len)) => assert_eq!(
                &"/ws/src/Unknown.vue.tsx"[..len],
                "/ws/src/Unknown.vue",
                "unknown `.vue.tsx` must strip the suffix back to the carrier path"
            ),
            other => panic!(
                "unknown `.vue.tsx` must be Rewrite::CarrierPath, got {}",
                rewrite_label(&other)
            ),
        }
    }

    /// The CLASS contract: once the bare-carrier classifier and the fast-path
    /// gate are fixed, `lower_tsc_validation_carrier_specifiers` rewrites EVERY quoted specifier
    /// shape that targets a bare in-project carrier — a default import, a dynamic
    /// `import("…")`, and a re-export `export … from "…"` — to the public-API
    /// stub. A code string whose ONLY carrier reference is bare `.vue` (no
    /// `.tsx`/`.jsx`/`.verter.ts` anywhere) must NOT be early-returned unchanged.
    #[test]
    fn lower_tsc_validation_carrier_specifiers_rewrites_bare_in_project_carrier_class() {
        let mut map = HashMap::new();
        map.insert(
            "/ws/src/GenericComp.vue".to_string(),
            PathBuf::from("/tmp/GenericComp_abc.vue.ts"),
        );

        // Default import of a bare in-project carrier (the regression case).
        let default_import = r#"import GenericComp from "/ws/src/GenericComp.vue";"#;
        let default_out = lower_tsc_validation_carrier_specifiers(default_import, &map);
        assert!(
            default_out.contains(r#""/tmp/GenericComp_abc.vue.ts""#),
            "bare default import must be rewritten to the stub: {default_out}"
        );
        assert!(
            !default_out.contains("/ws/src/GenericComp.vue\""),
            "the bare carrier specifier must be gone after rewrite: {default_out}"
        );

        // Dynamic import of the same bare carrier.
        let dynamic = r#"const C = import("/ws/src/GenericComp.vue");"#;
        let dynamic_out = lower_tsc_validation_carrier_specifiers(dynamic, &map);
        assert!(
            dynamic_out.contains(r#""/tmp/GenericComp_abc.vue.ts""#),
            "bare dynamic import must be rewritten to the stub: {dynamic_out}"
        );

        // Re-export `export { default } from "…"` of the same bare carrier.
        let reexport = r#"export { default } from "/ws/src/GenericComp.vue";"#;
        let reexport_out = lower_tsc_validation_carrier_specifiers(reexport, &map);
        assert!(
            reexport_out.contains(r#""/tmp/GenericComp_abc.vue.ts""#),
            "bare re-export must be rewritten to the stub: {reexport_out}"
        );

        // The fast-path gate must NOT early-return a file whose only carrier
        // reference is bare `.vue` — the rewrite above already proves it ran, but
        // assert the gate independently against a single bare import.
        let bare_only = r#"import C from "/ws/src/GenericComp.vue";"#;
        assert_ne!(
            lower_tsc_validation_carrier_specifiers(bare_only, &map),
            bare_only,
            "a file whose only carrier import is bare `.vue` must not be \
             early-returned unchanged by the fast-path gate"
        );
    }

    /// Negative class coverage for the bare-carrier path: unknown bare carriers
    /// and non-carrier specifiers must survive `lower_tsc_validation_carrier_specifiers`
    /// unchanged (so the `*.vue` wildcard shim resolves the unknown carrier and
    /// real modules are untouched).
    #[test]
    fn lower_tsc_validation_carrier_specifiers_leaves_unknown_bare_carrier_and_non_carriers() {
        let mut map = HashMap::new();
        map.insert(
            "/ws/src/GenericComp.vue".to_string(),
            PathBuf::from("/tmp/GenericComp_abc.vue.ts"),
        );

        // Unknown bare carrier (not in the map) → left bare for the wildcard shim.
        let unknown = r#"import U from "/ws/src/Unknown.vue";"#;
        assert_eq!(
            lower_tsc_validation_carrier_specifiers(unknown, &map),
            unknown,
            "an unknown bare carrier must be left bare for the `*.vue` shim"
        );

        // Non-carrier specifiers (a relative module and a package) → untouched.
        let non_carrier = r#"import { a } from "./types";
import _ from "lodash";"#;
        assert_eq!(
            lower_tsc_validation_carrier_specifiers(non_carrier, &map),
            non_carrier,
            "non-carrier specifiers must be unchanged"
        );

        // A `.d.ts` whose stem is not a carrier → untouched.
        let dts = r#"import type { F } from "./foo.d.ts";"#;
        assert_eq!(
            lower_tsc_validation_carrier_specifiers(dts, &map),
            dts,
            "a non-carrier `.d.ts` import must be unchanged"
        );
    }

    /// Test helper: a short label for a `carrier_virtual_import_target` outcome,
    /// used only in `panic!` messages so failures name the wrong variant.
    fn rewrite_label(rewrite: &Option<Rewrite>) -> String {
        match rewrite {
            Some(Rewrite::Stub(s)) => format!("Stub({s})"),
            Some(Rewrite::CarrierPath(len)) => format!("CarrierPath({len})"),
            None => "None".to_string(),
        }
    }

    /// CONTEXT-SCOPING (Part 1): `lower_tsc_validation_carrier_specifiers` must rewrite ONLY the
    /// quoted string that occupies a real module-specifier position — the path
    /// after an import/export-from token, the dynamic `import("…")` argument, and
    /// the side-effect `import "…"` specifier. A user string literal whose VALUE
    /// equals a carrier path (exact-canonical OR relative), the same text inside a
    /// `//`/`/* */` comment, and the same text inside a template literal must be
    /// left BYTE-FOR-BYTE verbatim — the validation TSX lowers the user's
    /// `<script setup>` body, so those positions are real user code that the
    /// type-checker must see unaltered.
    #[test]
    fn lower_tsc_validation_carrier_specifiers_rewrites_only_module_specifiers() {
        let mut map = HashMap::new();
        map.insert(
            "/ws/src/GenericComp.vue".to_string(),
            PathBuf::from("/tmp/GenericComp_abc.vue.ts"),
        );
        let stub = "/tmp/GenericComp_abc.vue.ts";

        // A real default import of the bare in-project carrier, sharing the file
        // with user string literals / a comment / a template that all spell the
        // SAME carrier path but are NOT specifier positions.
        let code = r#"import GenericComp from "/ws/src/GenericComp.vue";
const route = { component: "/ws/src/GenericComp.vue" };
const other = "/ws/src/GenericComp.vue";
const rel = "./GenericComp.vue";
// see /ws/src/GenericComp.vue for details
/* block: /ws/src/GenericComp.vue */
const tmpl = `/ws/src/GenericComp.vue`;
const nested = `path is ${other} for /ws/src/GenericComp.vue`;
"#;
        let out = lower_tsc_validation_carrier_specifiers(code, &map);

        // The real import specifier IS rewritten to the stub.
        assert!(
            out.contains(&format!("import GenericComp from \"{stub}\";")),
            "the real default-import specifier must be rewritten to the stub: {out}"
        );

        // Every NON-specifier occurrence is left verbatim. There must be exactly
        // ONE rewrite (the import); count the surviving literal occurrences.
        assert!(
            out.contains(r#"const route = { component: "/ws/src/GenericComp.vue" };"#),
            "an object-literal property string must be left verbatim: {out}"
        );
        assert!(
            out.contains(r#"const other = "/ws/src/GenericComp.vue";"#),
            "an exact-canonical user string literal must be left verbatim: {out}"
        );
        assert!(
            out.contains(r#"const rel = "./GenericComp.vue";"#),
            "a relative user string literal must be left verbatim: {out}"
        );
        assert!(
            out.contains("// see /ws/src/GenericComp.vue for details"),
            "a line comment must be left verbatim: {out}"
        );
        assert!(
            out.contains("/* block: /ws/src/GenericComp.vue */"),
            "a block comment must be left verbatim: {out}"
        );
        assert!(
            out.contains("const tmpl = `/ws/src/GenericComp.vue`;"),
            "a template literal must be left verbatim: {out}"
        );
        assert!(
            out.contains("const nested = `path is ${other} for /ws/src/GenericComp.vue`;"),
            "a template literal with an interpolation must be left verbatim: {out}"
        );

        // The stub must appear EXACTLY once — only the one real specifier was
        // rewritten, none of the literal/comment/template occurrences leaked.
        assert_eq!(
            out.matches(stub).count(),
            1,
            "exactly one occurrence (the real import) must be rewritten to the stub: {out}"
        );
    }

    /// TSC-VALIDATION-LOWERING SCOPE: the lowering touches ONLY generated carrier
    /// MODULE SPECIFIERS, never user source. When the generated TSX contains a
    /// KNOWN carrier path (present in `vue_ts_map`) ONLY in NON-specifier positions —
    /// a user string literal, a `//`/`/* */` comment, a template literal, a JSX text
    /// node, an object-property VALUE — the lowering must return the input
    /// BYTE-FOR-BYTE IDENTICAL (no specifier ⇒ no rewrite ⇒ no mutation). This is the
    /// guard against the forbidden "rewrite user source to paper over binding": a
    /// version that rewrote inside a string/comment/template, or treated a
    /// non-specifier carrier mention as a specifier, would mutate these bytes and
    /// FAIL the exact-equality assertion.
    #[test]
    fn tsc_validation_lowering_leaves_user_source_byte_for_byte_when_no_specifier() {
        let mut map = HashMap::new();
        // `KnownComp.vue` IS a known carrier — so if the lowering wrongly treated any
        // of the NON-specifier occurrences below as a specifier, it would rewrite
        // them to this stub, mutating the bytes.
        map.insert(
            "/ws/src/KnownComp.vue".to_string(),
            PathBuf::from("/tmp/KnownComp_xyz.vue.ts"),
        );

        // A file that MENTIONS the known carrier path in every non-specifier
        // position but contains NO module specifier at all.
        let user_source = r#"const label = "/ws/src/KnownComp.vue is the widget";
// import note: /ws/src/KnownComp.vue (do not touch)
/* block ref: /ws/src/KnownComp.vue */
const route = { path: "/ws/src/KnownComp.vue", lazy: true };
const tmpl = `render /ws/src/KnownComp.vue here`;
const jsxish = <div>/ws/src/KnownComp.vue</div>;
const ident = KnownComp_vue_thing;
"#;

        let out = lower_tsc_validation_carrier_specifiers(user_source, &map);

        // BYTE-FOR-BYTE identical — the strongest negative assertion: not one byte of
        // user source moved, and the stub never leaked into a non-specifier position.
        assert_eq!(
            out, user_source,
            "user source with NO module specifier must be returned byte-for-byte identical \
             (the lowering must never rewrite a carrier path that is not a specifier)"
        );
        assert!(
            !out.contains("/tmp/KnownComp_xyz.vue.ts"),
            "the stub must NEVER appear — no specifier position existed to lower"
        );
    }

    /// CONTEXT-SCOPING (Part 1), per specifier shape: each genuine specifier shape
    /// is rewritten, while a same-text NON-specifier string literal on the next
    /// line is left verbatim. Covers default, `import type … from`,
    /// `import * as … from`, `export * from`, `export { … } from`, dynamic
    /// `import("…")` (whitespace-tolerant), and side-effect `import "…"`.
    #[test]
    fn lower_tsc_validation_carrier_specifiers_covers_specifier_shapes() {
        let mut map = HashMap::new();
        map.insert(
            "/ws/src/C.vue".to_string(),
            PathBuf::from("/tmp/C_abc.vue.ts"),
        );
        let stub = "/tmp/C_abc.vue.ts";

        // Each input pairs a real specifier with a same-text non-specifier literal.
        let shapes: &[&str] = &[
            // default import
            "import C from \"/ws/src/C.vue\";\nconst a = \"/ws/src/C.vue\";",
            // import type { X } from
            "import type { P } from \"/ws/src/C.vue\";\nconst b = \"/ws/src/C.vue\";",
            // import { type X } from
            "import { type P } from \"/ws/src/C.vue\";\nconst b2 = \"/ws/src/C.vue\";",
            // import * as ns from
            "import * as NS from \"/ws/src/C.vue\";\nconst c = \"/ws/src/C.vue\";",
            // export * from
            "export * from \"/ws/src/C.vue\";\nconst d = \"/ws/src/C.vue\";",
            // export type * from
            "export type * from \"/ws/src/C.vue\";\nconst d2 = \"/ws/src/C.vue\";",
            // export { X } from
            "export { default } from \"/ws/src/C.vue\";\nconst e = \"/ws/src/C.vue\";",
            // export type { X } from
            "export type { P } from \"/ws/src/C.vue\";\nconst e2 = \"/ws/src/C.vue\";",
            // dynamic import("x")
            "const f = import(\"/ws/src/C.vue\");\nconst g = \"/ws/src/C.vue\";",
            // dynamic import ( "x" ) — whitespace-tolerant
            "const h = import ( \"/ws/src/C.vue\" );\nconst i = \"/ws/src/C.vue\";",
            // side-effect import "x"
            "import \"/ws/src/C.vue\";\nconst j = \"/ws/src/C.vue\";",
        ];

        for input in shapes {
            let out = lower_tsc_validation_carrier_specifiers(input, &map);
            // The specifier was rewritten exactly once; the trailing non-specifier
            // literal `"/ws/src/C.vue"` survives verbatim, so the original carrier
            // path still appears exactly once (the literal) and the stub once.
            assert_eq!(
                out.matches(stub).count(),
                1,
                "specifier shape must rewrite exactly the specifier to the stub: \
                 input={input:?} out={out:?}"
            );
            assert_eq!(
                out.matches("\"/ws/src/C.vue\"").count(),
                1,
                "the same-text non-specifier literal must survive verbatim: \
                 input={input:?} out={out:?}"
            );
        }
    }

    /// LEADING-CONTEXT GUARD: a `.import(…)` member-access call is NOT a dynamic
    /// import — `import` is a property name preceded by `.`, so its string argument
    /// is an ordinary value, not a module specifier, and must be copied verbatim
    /// (never rewritten to the carrier stub), even when the bare carrier path IS in
    /// `vue_ts_map`. The negative control in the same test confirms a GENUINE
    /// `import(…)` (no leading `.`) still rewrites, so the guard did not over-correct.
    #[test]
    fn lower_tsc_validation_carrier_specifiers_skips_member_access_import_call() {
        let mut map = HashMap::new();
        map.insert(
            "/ws/src/GenericComp.vue".to_string(),
            PathBuf::from("/tmp/GenericComp_abc.vue.ts"),
        );
        let stub = "/tmp/GenericComp_abc.vue.ts";

        // Member-access `.import("x")` — `import` is a property, the prior
        // significant byte is `.`, so the string argument is left verbatim.
        let member = r#"const c = loader.import("/ws/src/GenericComp.vue");"#;
        let out = lower_tsc_validation_carrier_specifiers(member, &map);
        assert!(
            !out.contains(stub),
            "a `.import(\"x\")` member-access call must NOT route its argument to \
             the carrier stub: {out}"
        );
        assert!(
            out.contains(r#"loader.import("/ws/src/GenericComp.vue")"#),
            "the member-access call argument must survive verbatim: {out}"
        );

        // Optional-chaining `?.import("x")` — the prior significant byte is still
        // `.`, so the same guard applies.
        let optional = r#"const c = loader?.import("/ws/src/GenericComp.vue");"#;
        let out_opt = lower_tsc_validation_carrier_specifiers(optional, &map);
        assert!(
            !out_opt.contains(stub),
            "a `?.import(\"x\")` optional-chaining member call must NOT route its \
             argument to the carrier stub: {out_opt}"
        );
        assert!(
            out_opt.contains(r#"loader?.import("/ws/src/GenericComp.vue")"#),
            "the optional-chaining call argument must survive verbatim: {out_opt}"
        );

        // NEGATIVE CONTROL: a genuine dynamic `import("x")` (no leading `.`) is a
        // real specifier position and MUST still rewrite to the stub — the guard
        // only suppresses member-access keywords, not real introducers.
        let genuine = r#"const c = import("/ws/src/GenericComp.vue");"#;
        let out_real = lower_tsc_validation_carrier_specifiers(genuine, &map);
        assert!(
            out_real.contains(&format!(r#"import("{stub}")"#)),
            "a genuine dynamic import specifier must still rewrite to the stub: {out_real}"
        );
        assert!(
            !out_real.contains(r#"import("/ws/src/GenericComp.vue")"#),
            "the genuine dynamic import's original specifier must be gone (rewritten): {out_real}"
        );
    }

    /// LEADING-CONTEXT GUARD, comment-tolerant lookback: the member-access guard
    /// must see the `.` qualifier even when a `//` line comment OR a `/* */` block
    /// comment sits between the `.` and the `import` member name. `prev_significant_byte`
    /// skips BOTH comment forms in reverse, so `loader. // note\n import("x")` is
    /// recognised as a property access (`import` qualified by `.`) and its string
    /// argument is left verbatim — never routed to the carrier stub. The negative
    /// control (a genuine `import("x")` whose only preceding token is a line comment
    /// on its OWN line, with no `.` qualifier) still rewrites, proving the lookback
    /// did not over-suppress real introducers.
    #[test]
    fn lower_tsc_validation_carrier_specifiers_member_access_lookback_skips_line_comments() {
        let mut map = HashMap::new();
        map.insert(
            "/ws/src/GenericComp.vue".to_string(),
            PathBuf::from("/tmp/GenericComp_abc.vue.ts"),
        );
        let stub = "/tmp/GenericComp_abc.vue.ts";

        // Member-access `.import("x")` with an intervening `//` LINE comment between
        // the `.` and `import`. The prior significant byte (skipping the line comment
        // and the newline) is `.`, so `import` is a property — the argument is verbatim.
        let line_comment =
            "const c = loader. // pick the carrier\n  import(\"/ws/src/GenericComp.vue\");";
        let out_line = lower_tsc_validation_carrier_specifiers(line_comment, &map);
        assert!(
            !out_line.contains(stub),
            "a `.import(\"x\")` member call with an intervening // line comment must NOT \
             route its argument to the carrier stub: {out_line}"
        );
        assert!(
            out_line.contains("import(\"/ws/src/GenericComp.vue\")"),
            "the member-access call argument must survive verbatim across a line comment: {out_line}"
        );

        // Member-access `.import("x")` with an intervening `/* */` BLOCK comment.
        let block_comment = "const c = loader. /* pick */ import(\"/ws/src/GenericComp.vue\");";
        let out_block = lower_tsc_validation_carrier_specifiers(block_comment, &map);
        assert!(
            !out_block.contains(stub),
            "a `.import(\"x\")` member call with an intervening /* */ block comment must NOT \
             route its argument to the carrier stub: {out_block}"
        );
        assert!(
            out_block.contains("import(\"/ws/src/GenericComp.vue\")"),
            "the member-access call argument must survive verbatim across a block comment: {out_block}"
        );

        // NEGATIVE CONTROL: a genuine dynamic `import("x")` whose only preceding token
        // is a line comment on its OWN line (no `.` qualifier anywhere before it) is a
        // real specifier position and MUST still rewrite — the lookback skips the
        // comment and lands on a non-`.` byte (the `;`), so the guard does not fire.
        let genuine = "const prev = 1;\n// load it dynamically\nconst c = import(\"/ws/src/GenericComp.vue\");";
        let out_real = lower_tsc_validation_carrier_specifiers(genuine, &map);
        assert!(
            out_real.contains(&format!(r#"import("{stub}")"#)),
            "a genuine dynamic import preceded only by a line comment must still rewrite \
             to the stub: {out_real}"
        );
        assert!(
            !out_real.contains(r#"import("/ws/src/GenericComp.vue")"#),
            "the genuine dynamic import's original specifier must be gone (rewritten): {out_real}"
        );
    }

    /// CROSS-LINE LEXICAL STATE: a multiline TEMPLATE literal whose continuation
    /// line happens to contain `. //` must NOT poison the leading-context lookback
    /// of a GENUINE dynamic import that follows the template. The `//` lives inside
    /// an unterminated template literal carried over from a prior physical line, so
    /// it is template text, NOT a code-level line comment — and the `.` on that line
    /// is template text too, not a member-access qualifier. A line-local backward
    /// heuristic mis-reads the continuation line as starting at code level, treats
    /// `//` as a comment, backs to the `.`, and wrongly suppresses the rewrite. The
    /// lookback must instead reflect the lexical state established by the forward
    /// scan up to the keyword, so the real `import(...)` after the closing backtick
    /// is recognised as a true specifier position and rewritten.
    #[test]
    fn lower_tsc_validation_carrier_specifiers_lookback_respects_multiline_template_state() {
        let mut map = HashMap::new();
        map.insert(
            "/ws/src/GenericComp.vue".to_string(),
            PathBuf::from("/tmp/GenericComp_abc.vue.ts"),
        );
        let stub = "/tmp/GenericComp_abc.vue.ts";

        // A multiline template whose 2nd line is `. // literal text`, then a GENUINE
        // dynamic import. The `.` and `//` are template text (the template opened on
        // line 1 and only closes on line 2 at the backtick), so the import is a real
        // specifier position and MUST rewrite to the stub.
        let multiline =
            "const s = `first\n. // literal text`; import(\"/ws/src/GenericComp.vue\");";
        let out = lower_tsc_validation_carrier_specifiers(multiline, &map);
        assert!(
            out.contains(&format!(r#"import("{stub}")"#)),
            "a genuine dynamic import following a multiline template (whose continuation \
             line contains `. //` as template text) MUST still rewrite to the stub — the \
             `//` is inside the template, not a code comment: {out}"
        );
        assert!(
            !out.contains(r#"import("/ws/src/GenericComp.vue")"#),
            "the genuine dynamic import's original specifier must be gone (rewritten) — the \
             multiline-template `. //` must not suppress the rewrite: {out}"
        );

        // The template body itself is preserved verbatim (no corruption of its text).
        assert!(
            out.contains("`first\n. // literal text`"),
            "the multiline template literal body must survive verbatim: {out}"
        );

        // CROSS-LINE BLOCK COMMENT sibling: a `/* … */` block comment that OPENS on a
        // prior line and only closes after a `. //`-bearing line is comment text, so a
        // genuine import after the block-comment close still rewrites. The line-local
        // heuristic would mis-read the continuation line as code.
        let multiline_block =
            "const s = 1; /* first\n. // still comment */ import(\"/ws/src/GenericComp.vue\");";
        let out_block = lower_tsc_validation_carrier_specifiers(multiline_block, &map);
        assert!(
            out_block.contains(&format!(r#"import("{stub}")"#)),
            "a genuine dynamic import after a multiline /* */ block comment (whose \
             continuation line contains `. //`) MUST still rewrite to the stub: {out_block}"
        );

        // NEGATIVE CONTROL (member access still suppressed): a REAL `.import("x")`
        // member access whose qualifying `.` precedes the keyword on the SAME code
        // level must still be left verbatim — the cross-line fix must not regress the
        // genuine member-access suppression.
        let member = r#"const c = loader.import("/ws/src/GenericComp.vue");"#;
        let out_member = lower_tsc_validation_carrier_specifiers(member, &map);
        assert!(
            !out_member.contains(stub),
            "a genuine `.import(\"x\")` member access must STILL be left verbatim after \
             the cross-line lexical fix: {out_member}"
        );
        assert!(
            out_member.contains(r#"loader.import("/ws/src/GenericComp.vue")"#),
            "the member-access argument must survive verbatim: {out_member}"
        );

        // NEGATIVE CONTROL (escaped backtick inside template): an escaped backtick
        // `\`` does NOT close the template, so a following `. //` and `import(...)`
        // are STILL inside the template — the whole thing is template text, nothing
        // is rewritten and the body is preserved.
        let escaped = "const s = `a\\`b\n. // import(\"/ws/src/GenericComp.vue\")`; const x = 1;";
        let out_escaped = lower_tsc_validation_carrier_specifiers(escaped, &map);
        assert!(
            !out_escaped.contains(stub),
            "an import that is INSIDE a template (kept open by an escaped backtick) must \
             NOT be rewritten — the escaped backtick does not close the template: {out_escaped}"
        );
        assert!(
            out_escaped.contains("import(\"/ws/src/GenericComp.vue\")"),
            "the template body containing the escaped backtick must survive verbatim: {out_escaped}"
        );
    }

    /// EXACT-CANONICAL ONLY (Part 2): with the ambiguous basename fallback removed,
    /// a bare in-project carrier specifier that does NOT exact-hit `vue_ts_map` but
    /// shares a basename with a map entry in a DIFFERENT directory must NOT be
    /// rewritten to that entry's stub. The map holds `/ws/a/Foo.vue`; a specifier
    /// for `/ws/b/Foo.vue` is an unknown carrier → `None` (left bare for the
    /// `*.vue` wildcard shim), never the `/ws/a` stub.
    #[test]
    fn carrier_virtual_import_target_no_basename_fallback() {
        let mut map = HashMap::new();
        map.insert(
            "/ws/a/Foo.vue".to_string(),
            PathBuf::from("/tmp/Foo_a.vue.ts"),
        );

        // Bare same-basename carrier in a different dir → unknown → None.
        assert!(
            carrier_virtual_import_target("/ws/b/Foo.vue", &map).is_none(),
            "a same-basename bare carrier in a different dir must NOT route to the \
             other dir's stub (no basename fallback); it must be None"
        );

        // A suffixed same-basename carrier in a different dir → unknown →
        // CarrierPath (strip the suffix back to the bare carrier for the shim),
        // NOT the other dir's stub.
        match carrier_virtual_import_target("/ws/b/Foo.vue.tsx", &map) {
            Some(Rewrite::CarrierPath(len)) => assert_eq!(
                &"/ws/b/Foo.vue.tsx"[..len],
                "/ws/b/Foo.vue",
                "a same-basename suffixed carrier in a different dir must strip to \
                 its own bare carrier, never the other dir's stub"
            ),
            other => panic!(
                "same-basename suffixed carrier must be CarrierPath, got {}",
                rewrite_label(&other)
            ),
        }

        // The exact-canonical entry still hits the stub (exact-hit preserved).
        match carrier_virtual_import_target("/ws/a/Foo.vue", &map) {
            Some(Rewrite::Stub(stub)) => assert_eq!(
                stub, "/tmp/Foo_a.vue.ts",
                "the exact-canonical carrier must still resolve to its stub"
            ),
            other => panic!(
                "exact-canonical carrier must be Rewrite::Stub, got {}",
                rewrite_label(&other)
            ),
        }
    }

    #[test]
    fn lower_tsc_validation_carrier_specifiers_preserves_non_carrier_imports() {
        let map = HashMap::new();
        let code = r#"import { ref } from 'vue'
import type { Foo } from './types'"#;

        let result = lower_tsc_validation_carrier_specifiers(code, &map);

        // Non-carrier imports should be untouched.
        assert_eq!(result, code, "non-carrier imports should be unchanged");
    }

    #[test]
    fn lower_tsc_validation_carrier_specifiers_preserves_non_string_occurrences() {
        let map = HashMap::new();
        // A carrier-API suffix not inside quotes (e.g. in a comment) is unchanged.
        let code = "// This references a Foo.vue.verter.ts file\nconst x = 1;";
        let result = lower_tsc_validation_carrier_specifiers(code, &map);
        assert_eq!(
            result, code,
            "non-string carrier-API suffix should be unchanged: {result}"
        );
    }

    #[test]
    fn lower_tsc_validation_carrier_specifiers_handles_double_quotes() {
        let mut map = HashMap::new();
        map.insert(
            "D:/project/Child.vue".to_string(),
            PathBuf::from("C:/tmp/Child_xyz.vue.ts"),
        );

        let code = r#"import Child from "D:/project/Child.vue.verter.ts""#;
        let result = lower_tsc_validation_carrier_specifiers(code, &map);
        assert!(
            result.contains(r#""C:/tmp/Child_xyz.vue.ts""#),
            "double-quoted known carrier-API should be rewritten: {result}"
        );
        assert!(
            !result.contains("D:/project/Child.vue.verter.ts"),
            "original path should be replaced: {result}"
        );
    }

    #[test]
    fn sanitize_component_name_handles_digit_prefix() {
        assert_eq!(sanitize_component_name("404"), "_404");
    }

    #[test]
    fn sanitize_component_name_handles_reserved_word() {
        assert_eq!(sanitize_component_name("default"), "_default");
        assert_eq!(sanitize_component_name("export"), "_export");
    }

    #[test]
    fn sanitize_component_name_handles_special_chars() {
        assert_eq!(sanitize_component_name("my-component"), "my_component");
    }

    // ── dts post-processing tests ──────────────────────────────────

    #[test]
    fn build_dts_rename_map_basic() {
        let root = PathBuf::from("/project");
        let generated = vec![(
            PathBuf::from("/project/src/components/Button.vue"),
            String::new(),
            PathBuf::from("/tmp/abc/Button_a1b2c3d4.tsc.tsx"),
        )];

        let map = build_dts_rename_map(&generated, &root);

        // Positive: correct mapping
        assert_eq!(
            map.get("Button_a1b2c3d4.tsc.tsx.d.ts"),
            Some(&PathBuf::from("src/components/Button.vue.d.ts")),
            "should map tsc.tsx.d.ts to vue.d.ts relative path"
        );
        // Negative: no other entries
        assert_eq!(map.len(), 1, "should have exactly one entry");
    }

    #[test]
    fn build_dts_rename_map_multiple_same_basename() {
        let root = PathBuf::from("/project");
        let hash1 = simple_hash(b"/project/src/A/Button.vue");
        let hash2 = simple_hash(b"/project/src/B/Button.vue");
        let generated = vec![
            (
                PathBuf::from("/project/src/A/Button.vue"),
                String::new(),
                PathBuf::from(format!("/tmp/Button_{hash1:016x}.tsc.tsx")),
            ),
            (
                PathBuf::from("/project/src/B/Button.vue"),
                String::new(),
                PathBuf::from(format!("/tmp/Button_{hash2:016x}.tsc.tsx")),
            ),
        ];

        let map = build_dts_rename_map(&generated, &root);

        assert_eq!(map.len(), 2, "both entries should be present");
        assert_eq!(
            map.get(&format!("Button_{hash1:016x}.tsc.tsx.d.ts")),
            Some(&PathBuf::from("src/A/Button.vue.d.ts"))
        );
        assert_eq!(
            map.get(&format!("Button_{hash2:016x}.tsc.tsx.d.ts")),
            Some(&PathBuf::from("src/B/Button.vue.d.ts"))
        );
    }

    #[test]
    fn compute_relative_path_same_dir() {
        let result = compute_relative_path("/project/src", "/project/src/types");
        assert_eq!(result, "./types", "same dir should start with ./");
        assert!(!result.contains(".."), "should not go up");
    }

    #[test]
    fn compute_relative_path_parent() {
        let result = compute_relative_path("/project/src/components", "/project/src/types/index");
        assert_eq!(result, "../types/index", "should go up one level");
    }

    #[test]
    fn compute_relative_path_deeply_nested() {
        let result = compute_relative_path("/project/src/deep/nested/dir", "/project/lib/other");
        // 4 levels up: dir → nested → deep → src → project, then into lib/other
        assert_eq!(result, "../../../../lib/other");
    }

    #[test]
    fn compute_relative_path_sibling() {
        let result = compute_relative_path("/project/src/a", "/project/src/b/file");
        assert_eq!(result, "../b/file");
    }

    #[test]
    fn rewrite_dts_imports_absolute_to_relative() {
        let content =
            r#"import("D:/project/src/types").Props & import("D:/project/src/utils").Helper"#;
        let target_dir = Path::new("D:/project/src/components");
        let root_dir = Path::new("D:/project");
        let import_map = HashMap::new();

        let result = rewrite_dts_imports(content, target_dir, root_dir, &import_map);

        // Positive: relative paths
        assert!(
            result.contains("../types"),
            "should rewrite to relative: {result}"
        );
        assert!(
            result.contains("../utils"),
            "should rewrite utils to relative: {result}"
        );
        // Negative: no absolute paths
        assert!(
            !result.contains("D:/project"),
            "absolute paths should be removed: {result}"
        );
    }

    #[test]
    fn rewrite_dts_imports_preserves_bare_modules() {
        let content = r#"import("vue").DefineComponent & import("@vueuse/core").UseFn"#;
        let target_dir = Path::new("/project/src");
        let root_dir = Path::new("/project");
        let import_map = HashMap::new();

        let result = rewrite_dts_imports(content, target_dir, root_dir, &import_map);

        assert!(
            result.contains("\"vue\""),
            "bare vue import should be preserved: {result}"
        );
        assert!(
            result.contains("\"@vueuse/core\""),
            "scoped package import should be preserved: {result}"
        );
        // Negative: No path rewriting applied
        assert!(
            !result.contains("./vue"),
            "bare module should not become relative: {result}"
        );
    }

    #[test]
    fn rewrite_dts_imports_cross_directory() {
        let content = r#"import("/project/src/shared/types").Foo"#;
        let target_dir = Path::new("/project/src/components/ui");
        let root_dir = Path::new("/project");
        let import_map = HashMap::new();

        let result = rewrite_dts_imports(content, target_dir, root_dir, &import_map);

        assert!(
            result.contains("../../shared/types"),
            "should compute correct relative path: {result}"
        );
        assert!(
            !result.contains("/project/"),
            "absolute path should be removed: {result}"
        );
    }

    #[test]
    fn rewrite_dts_imports_tsc_tsx_references() {
        let content = r#"import("./Modal_deadbeef01234567.tsc.tsx").ModalProps"#;
        let target_dir = Path::new("/project/src/components");
        let root_dir = Path::new("/project");
        let mut import_map = HashMap::new();
        import_map.insert(
            "Modal_deadbeef01234567.tsc.tsx".to_string(),
            "src/views/Modal.vue".to_string(),
        );

        let result = rewrite_dts_imports(content, target_dir, root_dir, &import_map);

        assert!(
            result.contains("../views/Modal.vue"),
            "should rewrite .tsc.tsx ref to .vue relative path: {result}"
        );
        assert!(
            !result.contains(".tsc.tsx"),
            ".tsc.tsx should not appear in output: {result}"
        );
    }

    #[test]
    fn rewrite_dts_imports_handles_windows_backslash_paths() {
        // On Windows, tsc may emit absolute paths with backslashes.
        let content = r#"import("D:\project\src\types").Props"#;
        let target_dir = Path::new("D:/project/src/components");
        let root_dir = Path::new("D:/project");
        let import_map = HashMap::new();

        let result = rewrite_dts_imports(content, target_dir, root_dir, &import_map);

        // Positive: should be rewritten to relative
        assert!(
            result.contains("../types"),
            "Windows backslash paths should be normalized and rewritten: {result}"
        );
        // Negative: no absolute paths
        assert!(
            !result.contains("D:"),
            "absolute path should be removed: {result}"
        );
        // Negative: no backslashes in output
        assert!(
            !result.contains('\\'),
            "output should use forward slashes: {result}"
        );
    }

    #[test]
    fn postprocess_creates_correct_structure() {
        let temp = tempfile::TempDir::new().unwrap();
        let decl_dir = temp.path().join("dist/types");
        let root_dir = temp.path().join("project");

        // Simulate tsc output: a temp subdir with .tsc.tsx.d.ts files.
        let temp_subdir = decl_dir.join("tmp_abc");
        fs::create_dir_all(&temp_subdir).unwrap();

        let hash = simple_hash(b"project/src/Button.vue");
        let emitted_name = format!("Button_{hash:016x}.tsc.tsx.d.ts");
        let dts_content = r#"export declare const Button: {};"#;
        fs::write(temp_subdir.join(&emitted_name), dts_content).unwrap();

        // Also create vue-shims.d.ts artifact.
        fs::write(decl_dir.join("vue-shims.d.ts"), "declare module '*.vue' {}").unwrap();

        let generated = vec![(
            root_dir.join("src/Button.vue"),
            String::new(),
            PathBuf::from(format!("/tmp/Button_{hash:016x}.tsc.tsx")),
        )];

        postprocess_vue_declarations(&decl_dir, &generated, &root_dir);

        // Positive: correct file created
        let target = decl_dir.join("src/Button.vue.d.ts");
        assert!(target.exists(), "should create src/Button.vue.d.ts");
        let result_content = fs::read_to_string(&target).unwrap();
        assert!(
            result_content.contains("export declare const Button"),
            "content should be preserved"
        );

        // Negative: temp file removed
        assert!(
            !temp_subdir.join(&emitted_name).exists(),
            "original .tsc.tsx.d.ts should be deleted"
        );

        // Negative: vue-shims.d.ts removed
        assert!(
            !decl_dir.join("vue-shims.d.ts").exists(),
            "vue-shims.d.ts should be deleted"
        );
    }

    #[test]
    fn generate_all_tsx_includes_inline_source_map() {
        let temp = tempfile::TempDir::new().unwrap();
        let vue_path = temp.path().join("Test.vue");
        fs::write(
            &vue_path,
            "<script setup lang=\"ts\">\nlet a = 1;\na = {}\n</script>\n<template><div /></template>\n",
        )
        .unwrap();

        let out_dir = temp.path().join("out");
        fs::create_dir_all(&out_dir).unwrap();

        let results = generate_all_tsx(&[vue_path], &out_dir);
        assert_eq!(results.len(), 1, "should produce one TSX file");

        let (_vue, tsx_code, _tsx_path) = &results[0];
        // Positive: inline source map marker present
        assert!(
            tsx_code.contains("//# sourceMappingURL=data:application/json;base64,"),
            "TSX code must include inline source map for error remapping"
        );
    }

    #[test]
    fn generate_all_tsx_source_map_remaps_script_body() {
        let temp = tempfile::TempDir::new().unwrap();
        let vue_content = "<script setup lang=\"ts\">\nlet a = 1;\na = {}\n</script>\n<template><div /></template>\n";
        let vue_path = temp.path().join("Test.vue");
        fs::write(&vue_path, vue_content).unwrap();

        let out_dir = temp.path().join("out");
        fs::create_dir_all(&out_dir).unwrap();

        let results = generate_all_tsx(std::slice::from_ref(&vue_path), &out_dir);
        let (_vue, tsx_code, _tsx_path) = &results[0];

        // Find the line of `a = {}` in the generated TSX.
        let tsx_line_1 = tsx_code
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains("a = {}"))
            .map(|(i, _)| i as u32 + 1)
            .expect("TSX should contain `a = {}`");

        // Remap via source map — should resolve back to the .vue file.
        let (source_name, pos) = crate::error_map::map_tsc_position(tsx_code, tsx_line_1, 1)
            .expect("source map lookup should succeed");

        // The source should be the .vue file path used as `filename`.
        assert!(
            source_name.contains("Test.vue"),
            "source should be Test.vue, got: {source_name}"
        );

        // In the original .vue, `a = {}` is on line 3 (1-indexed).
        // Source map positions are 0-indexed, so line 2.
        assert_eq!(pos.line, 2, "should map to line 3 (0-indexed: 2) in .vue");
    }

    #[test]
    fn write_temp_tsconfig_validation_includes_jsx_option() {
        let temp = tempfile::TempDir::new().unwrap();
        let base_tsconfig = temp.path().join("base-tsconfig.json");
        fs::write(&base_tsconfig, r#"{ "compilerOptions": {} }"#).unwrap();

        let opts = EmitOptions {
            no_emit: true,
            declaration: false,
            declaration_dir: None,
        };

        let result =
            write_temp_tsconfig(temp.path(), &base_tsconfig, &[], &opts, temp.path()).unwrap();
        let content = fs::read_to_string(&result).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Positive: jsx option set for TSX parsing
        let jsx_val = json["compilerOptions"]["jsx"]
            .as_str()
            .expect("jsx compiler option should be set in validation tsconfig");
        assert_eq!(
            jsx_val, "react-jsx",
            "jsx should be react-jsx for Vue TSX type checking"
        );

        // Positive: jsxImportSource set for Vue JSX types
        let jsx_import_source = json["compilerOptions"]["jsxImportSource"]
            .as_str()
            .expect("jsxImportSource should be set");
        assert_eq!(jsx_import_source, "vue", "jsxImportSource should be vue");
    }

    #[test]
    fn write_temp_tsconfig_includes_root_dir() {
        let temp = tempfile::TempDir::new().unwrap();
        let base_tsconfig = temp.path().join("base-tsconfig.json");
        fs::write(&base_tsconfig, r#"{ "compilerOptions": {} }"#).unwrap();

        let root = temp.path().join("my-project");
        fs::create_dir_all(&root).unwrap();

        let opts = EmitOptions {
            no_emit: false,
            declaration: true,
            declaration_dir: Some(temp.path().join("dist")),
        };

        let result = write_temp_tsconfig(temp.path(), &base_tsconfig, &[], &opts, &root).unwrap();
        let content = fs::read_to_string(&result).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        let root_dir_val = json["compilerOptions"]["rootDir"]
            .as_str()
            .expect("rootDir should be a string");
        assert!(
            root_dir_val.contains("my-project"),
            "rootDir should contain the project dir: {root_dir_val}"
        );
        // Negative: no backslashes in rootDir
        assert!(
            !root_dir_val.contains('\\'),
            "rootDir should use forward slashes: {root_dir_val}"
        );
    }

    #[test]
    fn is_vue_jsx_type_gap_error_filters_correctly() {
        use crate::reporter::{Severity, TscDiagnostic};

        let make = |ts_code: u32, msg: &str| TscDiagnostic {
            file: "test.tsx".into(),
            line: 1,
            col: 1,
            severity: Severity::Error,
            ts_code,
            message: msg.into(),
        };

        // children + HTMLAttributes → filter
        assert!(is_vue_jsx_type_gap_error(&make(2322,
            "Type '{ class: string; children: Element[]; }' is not assignable to type 'HTMLAttributes & ReservedProps'.")));

        // children + SVGAttributes → filter
        assert!(is_vue_jsx_type_gap_error(&make(2322,
            "Type '{ children: Element; }' is not assignable to type 'SVGAttributes & ReservedProps'.")));

        // TS2559 with children → filter
        assert!(is_vue_jsx_type_gap_error(&make(
            2559,
            "Type '{ children: string; }' has no properties in common with type 'HTMLAttributes'."
        )));

        // textContent + HTMLAttributes → filter
        assert!(is_vue_jsx_type_gap_error(&make(2322,
            "Type '{ textContent: any; }' is not assignable to type 'HTMLAttributes & ReservedProps'.")));

        // textContent + LabelHTMLAttributes → filter
        assert!(is_vue_jsx_type_gap_error(&make(2322,
            "Type '{ for: any; textContent: any; }' is not assignable to type 'IntrinsicAttributes & LabelHTMLAttributes & ReservedProps'.")));

        // innerHTML + HTMLAttributes → filter
        assert!(is_vue_jsx_type_gap_error(&make(2322,
            "Type '{ innerHTML: string; }' is not assignable to type 'HTMLAttributes & ReservedProps'.")));

        // TS2322 WITHOUT gap prop → keep
        assert!(!is_vue_jsx_type_gap_error(&make(
            2322,
            "Type '{ class: string; }' is not assignable to type 'HTMLAttributes'."
        )));

        // TS2304 (different code) → keep
        assert!(!is_vue_jsx_type_gap_error(&make(
            2304,
            "Cannot find name 'children'."
        )));

        // children on custom component → keep
        assert!(!is_vue_jsx_type_gap_error(&make(
            2322,
            "Type '{ children: string; }' is not assignable to type 'MyComponentProps'."
        )));
    }

    #[test]
    fn is_temp_tsconfig_error_filters_correctly() {
        use crate::reporter::{Severity, TscDiagnostic};

        // Error from temp tsconfig → filter out
        let d = TscDiagnostic {
            file: "/project/.tmpABC/verter-tsc-check.tsconfig.json".into(),
            line: 2,
            col: 3,
            severity: Severity::Error,
            ts_code: 5102,
            message: "Option 'baseUrl' has been removed.".into(),
        };
        assert!(
            is_temp_tsconfig_error(&d),
            "should filter temp tsconfig errors"
        );

        // Error from user's tsconfig → keep
        let d2 = TscDiagnostic {
            file: "/project/tsconfig.json".into(),
            line: 1,
            col: 1,
            severity: Severity::Error,
            ts_code: 5102,
            message: "Option 'baseUrl' has been removed.".into(),
        };
        assert!(
            !is_temp_tsconfig_error(&d2),
            "should not filter user tsconfig errors"
        );

        // Error from source file → keep
        let d3 = TscDiagnostic {
            file: "/project/src/App.vue".into(),
            line: 1,
            col: 1,
            severity: Severity::Error,
            ts_code: 2322,
            message: "Type error".into(),
        };
        assert!(
            !is_temp_tsconfig_error(&d3),
            "should not filter source file errors"
        );
    }

    #[test]
    fn run_declaration_phase_failure_returns_remapped_diagnostics_and_skips_emitted_files() {
        let (_temp, config, tsconfig_path, decl_dir) = create_run_fixture("phase-b-fail");
        // The `--api` typecheck stage is bypassed here (no `--api` engine in the
        // fixture); this exercises the declaration stage directly. See
        // `run_declaration_only`.
        let (diagnostics, emitted_files) = run_declaration_only(
            &config,
            &tsconfig_path,
            &EmitOptions {
                no_emit: false,
                declaration: true,
                declaration_dir: Some(decl_dir.clone()),
            },
        );

        assert_eq!(
            diagnostics.len(),
            1,
            "declaration-phase failures should surface diagnostics"
        );
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.ts_code, 2304, "should preserve TypeScript code");
        assert!(
            diagnostic.message.contains("MissingType"),
            "should preserve checker message: {}",
            diagnostic.message
        );
        assert!(
            diagnostic
                .file
                .replace('\\', "/")
                .ends_with("/src/Test.vue"),
            "diagnostic should remap back to the vue file: {}",
            diagnostic.file
        );
        assert!(
            emitted_files.is_empty(),
            "declaration-phase failure must not report emitted files"
        );
        assert!(
            !decl_dir.join("src/Test.vue.d.ts").exists(),
            "declaration-phase failure must not postprocess vue declarations"
        );
    }

    #[test]
    fn run_declaration_phase_success_postprocesses_vue_declarations() {
        let (_temp, config, tsconfig_path, decl_dir) = create_run_fixture("phase-b-success");
        // `--api` typecheck stage bypassed (no engine in the fixture); exercise the
        // declaration stage directly. See `run_declaration_only`.
        let (diagnostics, emitted_files) = run_declaration_only(
            &config,
            &tsconfig_path,
            &EmitOptions {
                no_emit: false,
                declaration: true,
                declaration_dir: Some(decl_dir.clone()),
            },
        );

        let target = decl_dir.join("src/Test.vue.d.ts");
        assert!(
            diagnostics.is_empty(),
            "successful declaration run should not add diagnostics"
        );
        assert!(
            target.exists(),
            "should postprocess .tsc.tsx output into .vue.d.ts"
        );
        assert!(
            emitted_files.iter().any(|path| path == &target),
            "emitted files should include the final .vue.d.ts output"
        );
    }

    /// Regression: when no CLI --declarationDir or --outDir is provided, the
    /// output dir should be resolved from tsconfig.json compilerOptions.
    /// This mirrors the main.rs fallback chain:
    ///   cli.declaration_dir → cli.out_dir → config.declaration_dir → config.out_dir
    /// The generated tsconfig must explicitly set `noEmitOnError: false` to override
    /// the parent tsconfig's potential `noEmitOnError: true`. Otherwise, tsc won't emit
    /// any `.d.ts` files when the project has type errors, even for non-erroring files.
    #[test]
    fn write_temp_tsconfig_declaration_sets_no_emit_on_error_false() {
        let temp = tempfile::TempDir::new().unwrap();
        let base_tsconfig = temp.path().join("base-tsconfig.json");
        fs::write(&base_tsconfig, r#"{ "compilerOptions": {} }"#).unwrap();

        let opts = EmitOptions {
            no_emit: false,
            declaration: true,
            declaration_dir: Some(temp.path().join("dist")),
        };

        let result =
            write_temp_tsconfig(temp.path(), &base_tsconfig, &[], &opts, temp.path()).unwrap();
        let content = fs::read_to_string(&result).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        let co = &json["compilerOptions"];
        // Positive: noEmitOnError must be explicitly false
        assert_eq!(
            co["noEmitOnError"], false,
            "declaration tsconfig should set noEmitOnError: false to ensure emission despite errors"
        );
    }

    /// When the declaration stage has errors but tsc still emits some .d.ts
    /// files, post-processing must still run to rename the emitted files to .vue.d.ts.
    /// Previously, post-processing was skipped entirely on error, leaving 0 .vue.d.ts files.
    #[test]
    fn run_declaration_phase_with_errors_still_postprocesses_emitted_files() {
        // Create fixture with mock tsc that reports an error AND emits a .d.ts file
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("project");
        let src_dir = root.join("src");
        let vue_path = src_dir.join("Test.vue");
        let tsconfig_path = root.join("tsconfig.json");
        let decl_dir = root.join("dist").join("types");

        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            &vue_path,
            r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ props.msg }}</div></template>
"#,
        )
        .unwrap();
        fs::write(
            &tsconfig_path,
            r#"{
  "compilerOptions": {
    "strict": true
  },
  "files": ["src/Test.vue"]
}"#,
        )
        .unwrap();

        // Write a mock tsc that both emits a .d.ts AND reports an error (exit 1).
        // This mimics real tsc behavior: errors in some files don't prevent emission of others.
        write_mock_tsc_error_with_emit(&root, &decl_dir);

        let config = load_tsconfig(&tsconfig_path).expect("test tsconfig should load");
        // `--api` typecheck stage bypassed (no engine in the fixture); exercise the
        // declaration stage directly. See `run_declaration_only`.
        let (diagnostics, _emitted_files) = run_declaration_only(
            &config,
            &tsconfig_path,
            &EmitOptions {
                no_emit: false,
                declaration: true,
                declaration_dir: Some(decl_dir.clone()),
            },
        );

        // Positive: diagnostics should be reported
        assert!(
            !diagnostics.is_empty(),
            "should report diagnostics from the error"
        );

        // Positive: .vue.d.ts file should still be created despite errors
        let target = decl_dir.join("src/Test.vue.d.ts");
        assert!(
            target.exists(),
            "should postprocess .vue.d.ts even when tsc reports errors, found: {:?}",
            collect_dts_files(&decl_dir)
        );
    }

    // ── Cross-component type resolution tests ─────────────────────

    #[test]
    fn generate_public_api_stubs_produces_in_memory_stub_carrier() {
        let temp = tempfile::TempDir::new().unwrap();
        let src_dir = temp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let child_vue = src_dir.join("Child.vue");
        fs::write(
            &child_vue,
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
        )
        .unwrap();

        let host = VerterHost::new_standalone(HostConfig::default());
        let canonical_id = child_vue.to_string_lossy().replace('\\', "/");
        let source = fs::read_to_string(&child_vue).unwrap();
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical_id.clone()),
                input_id: canonical_id.clone(),
                source: std::sync::Arc::<str>::from(source),
                file_language: FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();

        let out_dir = temp.path().join("out");
        fs::create_dir_all(&out_dir).unwrap();

        let (stub_files, vue_ts_map) =
            generate_public_api_stubs(&host, std::slice::from_ref(&child_vue), &out_dir);

        // Positive: exactly one in-memory `.vue.ts` stub carrier.
        assert_eq!(stub_files.len(), 1, "should generate one stub carrier");
        let (stub_path, stub_content) = &stub_files[0];

        // The stub is VIRTUAL (in-memory): rooted at `base_dir` with a
        // `Name_<hash>.vue.ts` basename, and NOT written to disk.
        assert!(
            !stub_path.exists(),
            "stub carrier must be in-memory, not written to disk: {}",
            stub_path.display()
        );
        assert!(
            stub_path.starts_with(&out_dir),
            "stub path should be rooted at base_dir: {}",
            stub_path.display()
        );
        assert!(
            stub_path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with(".vue.ts"))
                .unwrap_or(false),
            "stub basename should end with .vue.ts: {}",
            stub_path.display()
        );
        assert!(
            stub_content.contains("export default"),
            "stub should contain export default: {stub_content}"
        );

        // Positive: map entry should exist (canonical .vue → virtual stub path).
        assert!(
            vue_ts_map.contains_key(&canonical_id),
            "vue_ts_map should have entry for the .vue file"
        );

        // Negative: stub should not contain raw .vue.ts import paths.
        assert!(
            !stub_content.contains(".vue.ts"),
            "stub should not contain .vue.ts import paths: {stub_content}"
        );
    }

    #[test]
    fn lower_tsc_validation_carrier_specifiers_maps_known_paths() {
        let mut map = HashMap::new();
        map.insert(
            "D:/project/src/Child.vue".to_string(),
            PathBuf::from("C:/tmp/out/Child_abc.vue.ts"),
        );

        // Post-`rewrite_relative_imports` the carrier-API specifier carries the
        // canonical carrier path plus the reserved `.verter.ts` suffix.
        let code = r#"import('D:/project/src/Child.vue.verter.ts')['default']"#;
        let result = lower_tsc_validation_carrier_specifiers(code, &map);

        // Positive: should rewrite to temp path
        assert!(
            result.contains("C:/tmp/out/Child_abc.vue.ts"),
            "should rewrite to temp stub path: {result}"
        );
        // Negative: original carrier-API path should be gone
        assert!(
            !result.contains("D:/project/src/Child.vue.verter.ts"),
            "original carrier-API path should be replaced: {result}"
        );
    }

    #[test]
    fn lower_tsc_validation_carrier_specifiers_preserves_unknown() {
        let map = HashMap::new(); // empty — no known paths

        let code = r#"import('D:/node_modules/some-lib/Comp.vue.verter.ts')['default']"#;
        let result = lower_tsc_validation_carrier_specifiers(code, &map);

        // Unknown carrier-API path → strip `.verter.ts` back to the carrier path
        // (fallback to the `*.vue` wildcard shim).
        assert!(
            result.contains("Comp.vue'"),
            "unknown carrier-API path should strip back to the carrier path: {result}"
        );
        assert!(
            !result.contains(".verter.ts"),
            "unknown carrier-API path should not remain as-is: {result}"
        );
    }

    #[test]
    fn lower_tsc_validation_carrier_specifiers_handles_from_syntax() {
        let mut map = HashMap::new();
        map.insert(
            "D:/project/src/Child.vue".to_string(),
            PathBuf::from("C:/tmp/out/Child_abc.vue.ts"),
        );

        let code = r#"import type { Props } from 'D:/project/src/Child.vue.verter.ts'"#;
        let result = lower_tsc_validation_carrier_specifiers(code, &map);

        // Positive: should rewrite from-syntax imports too
        assert!(
            result.contains("C:/tmp/out/Child_abc.vue.ts"),
            "from-syntax carrier-API import should be rewritten: {result}"
        );
        // Negative: original path should be gone
        assert!(
            !result.contains("D:/project/src/Child.vue.verter.ts"),
            "original path should be replaced: {result}"
        );
    }

    #[test]
    fn lower_tsc_validation_carrier_specifiers_ignores_non_carrier_imports() {
        let map = HashMap::new();

        let code = r#"import type { Foo } from 'D:/project/src/types.ts'"#;
        let result = lower_tsc_validation_carrier_specifiers(code, &map);

        // Non-carrier imports should remain unchanged.
        assert_eq!(
            result, code,
            "non-carrier imports should be unchanged: {result}"
        );
    }

    #[test]
    fn generate_all_tsc_accepts_shared_host() {
        let temp = tempfile::TempDir::new().unwrap();
        let vue_path = temp.path().join("Test.vue");
        fs::write(
            &vue_path,
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>
"#,
        )
        .unwrap();

        let host = VerterHost::new_standalone(HostConfig::default());
        let canonical_id = vue_path.to_string_lossy().replace('\\', "/");
        let source = fs::read_to_string(&vue_path).unwrap();
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical_id.clone()),
                input_id: canonical_id,
                source: std::sync::Arc::<str>::from(source),
                file_language: FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();

        let out_dir = temp.path().join("tsc_out");
        fs::create_dir_all(&out_dir).unwrap();

        let results = generate_all_tsc(&host, &[vue_path], &out_dir);

        // Positive: should produce output
        assert!(
            !results.is_empty(),
            "generate_all_tsc with shared host should produce output"
        );
        // Positive: generated file should exist
        let (_, _, tsc_path) = &results[0];
        assert!(tsc_path.exists(), "generated .tsc.tsx should exist on disk");
    }

    /// The declaration stage (`generate_all_tsc`) routes through ONE
    /// `get_public_api_batch`, NOT a per-file `get_public_api` loop.
    ///
    /// DISCRIMINATION — two independent properties:
    ///  1. **O(1)-not-per-N store-view reads.** `host.provenance()
    ///     .store_view_from_host_reads` (a per-`VerterHost` atomic bumped in the
    ///     `HostStoreView::from_host` chokepoint) is reachable from this crate.
    ///     A per-file loop takes ≥ N reads (each macro-bearing render re-reads
    ///     the store view); the batch collapses to ONE per-batch fixed-view
    ///     capture. A WARM declaration stage of N files must read `< N` (and
    ///     `>= 1`, so a dead counter cannot trivially satisfy the bound). This
    ///     is exactly the cliff the migration removes — a regression to a
    ///     per-file `host.get_public_api(` loop drives this `>= N` → RED.
    ///  2. **Cross-item materialization.** A LATER file (`Parent.vue`) imports an
    ///     EARLIER file's (`Child.vue`) emit interface; its declaration output
    ///     MATERIALIZES the sibling SFC's `(e: 'childEvt', payload: number)`
    ///     signature. A failed cross-item walk drops `payload: number` → RED.
    #[test]
    fn generate_all_tsc_routes_through_batch_o1_reads_and_resolves_cross_item() {
        use std::sync::atomic::Ordering::Relaxed;

        let temp = tempfile::TempDir::new().unwrap();
        // Child.vue exports an emit call-signature interface from its plain
        // `<script>` block (the cross-item dependency).
        let child_path = temp.path().join("Child.vue");
        fs::write(
            &child_path,
            r#"<script lang="ts">
export interface ChildEmits { (e: 'childEvt', payload: number): void }
</script>
<script setup lang="ts">
defineProps<{ a: string }>()
</script>
<template><div /></template>
"#,
        )
        .unwrap();
        // Three consumers import Child's emit interface (a mid-batch cross-item
        // dependency on the FIRST file). N=4 gives the `< N` bound margin.
        let mut vue_files = vec![child_path.clone()];
        for i in 0..3 {
            let consumer_path = temp.path().join(format!("Consumer{i}.vue"));
            fs::write(
                &consumer_path,
                r#"<script setup lang="ts">
import type { ChildEmits } from './Child.vue'
defineEmits<ChildEmits>()
</script>
<template><div /></template>
"#,
            )
            .unwrap();
            vue_files.push(consumer_path);
        }

        let host = VerterHost::new_standalone(HostConfig::default());
        for path in &vue_files {
            let canonical_id = path.to_string_lossy().replace('\\', "/");
            let source = fs::read_to_string(path).unwrap();
            let _ = host
                .upsert(UpsertRequest {
                    canonical_id: Some(canonical_id.clone()),
                    input_id: canonical_id,
                    source: std::sync::Arc::<str>::from(source),
                    file_language: FileLanguage::vue(),
                    aliases: Vec::new(),
                })
                .unwrap();
        }

        let out_dir = temp.path().join("tsc_out");
        fs::create_dir_all(&out_dir).unwrap();

        // Cold pass warms the extract + transitive-dep caches.
        let _cold = generate_all_tsc(&host, &vue_files, &out_dir);

        // WARM pass: measure this host's `from_host` reads in isolation.
        const N: u64 = 4;
        host.provenance()
            .store_view_from_host_reads
            .store(0, Relaxed);
        let results = generate_all_tsc(&host, &vue_files, &out_dir);
        let warm_from_host = host.provenance().store_view_from_host_reads.load(Relaxed);

        assert_eq!(
            results.len(),
            4,
            "all four .vue files produce declaration output"
        );
        assert!(
            warm_from_host >= 1,
            "the warm declaration stage must perform at least one real `from_host` \
             read (the per-batch fixed-view capture), so a dead counter cannot \
             trivially satisfy the bound; observed {warm_from_host}",
        );
        assert!(
            warm_from_host < N,
            "generate_all_tsc must route through ONE `get_public_api_batch` (O(1) \
             `from_host` reads — the single per-batch capture), NOT a per-file \
             `get_public_api` loop (≥ N={N} reads). Observed {warm_from_host}",
        );

        // Cross-item correctness: every consumer's declaration output
        // MATERIALIZES the sibling `Child.vue`'s `ChildEmits` payload.
        for i in 0..3 {
            let consumer_name = format!("Consumer{i}.vue");
            let code = results
                .iter()
                .find(|(p, _, _)| {
                    p.file_name().and_then(|s| s.to_str()) == Some(consumer_name.as_str())
                })
                .map(|(_, code, _)| code.clone())
                .unwrap_or_else(|| panic!("{consumer_name} declaration output present"));
            assert!(
                code.contains("payload: number") && code.contains("'childEvt'"),
                "{consumer_name} declaration output must MATERIALIZE the sibling \
                 SFC's `ChildEmits` `(e: 'childEvt', payload: number)`; got:\n{code}",
            );
        }
    }

    #[test]
    fn run_declaration_with_tsconfig_sourced_output_dir() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("project");
        let src_dir = root.join("src");
        let decl_dir = root.join("dist").join("types");

        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            src_dir.join("Test.vue"),
            r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ props.msg }}</div></template>
"#,
        )
        .unwrap();

        // The tsconfig declares declarationDir — no CLI flags needed.
        let tsconfig_path = root.join("tsconfig.json");
        fs::write(
            &tsconfig_path,
            r#"{
  "compilerOptions": {
    "strict": true,
    "declarationDir": "dist/types"
  },
  "files": ["src/Test.vue"]
}"#,
        )
        .unwrap();
        write_mock_tsc(&root, "phase-b-success");

        let config = load_tsconfig(&tsconfig_path).expect("test tsconfig should load");

        // Verify tsconfig resolution produced the declarationDir
        assert!(
            config.declaration_dir.is_some(),
            "tsconfig should resolve declarationDir"
        );

        // Simulate main.rs fallback: no CLI flags, so use config.declaration_dir
        let effective_dir = config
            .declaration_dir
            .clone()
            .or_else(|| config.out_dir.clone());

        // `--api` typecheck stage bypassed (no engine in the fixture); exercise the
        // declaration stage directly. See `run_declaration_only`.
        let (diagnostics, emitted_files) = run_declaration_only(
            &config,
            &tsconfig_path,
            &EmitOptions {
                no_emit: false,
                declaration: true,
                declaration_dir: effective_dir,
            },
        );

        let target = decl_dir.join("src/Test.vue.d.ts");
        assert!(
            diagnostics.is_empty(),
            "tsconfig-sourced declarationDir should produce no diagnostics: {diagnostics:?}"
        );
        assert!(
            target.exists(),
            "should postprocess .vue.d.ts using tsconfig-sourced declarationDir"
        );
        assert!(
            !emitted_files.is_empty(),
            "emitted_files should not be empty when using tsconfig-sourced output dir"
        );
    }
}
