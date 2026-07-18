//! Host-backed parallel SFC compilation.
//!
//! Bundler/runtime output only. Returns the assembled Main virtual
//! file (script + template render fn). IDE TSX and TSC type-extract
//! batch surfaces are out of scope here; they would land as separate
//! `ide_many` / `public_api_many` entry points.
//!
//! ## Four-stage batch
//!
//! 1. **Stage A — short-circuit empty input.** Empty input returns
//!    immediately; no thread pool is constructed.
//! 2. **Stage B — group + selective upsert.** Group by `canonical_id`.
//!    Reject groups with conflicting source (every entry for that id
//!    receives a duplicate-conflict error). For non-conflicting unique
//!    groups, skip the upsert when the scheduler already holds
//!    byte-identical source (preserves warm `compile_slot` cache).
//!    Submit the deduped per-canonical source updates as ONE atomic
//!    batch through the shared upsert engine
//!    [`VerterHost::upsert_many_with_priority`] (one
//!    `Scheduler::submit_batch_atomic` + one `wait_batch`, with
//!    per-canonical post-commit on the calling thread) at the
//!    caller-configured priority.
//! 3. **Stage C — compile each unique canonical group exactly once.**
//!    Call [`VerterHost::get_virtual_file`] for `Main`. Per-input panic
//!    isolation is owned by the host batch coordinator's generic catch
//!    boundary (a codegen panic in one input becomes an error
//!    `CompileBatchEntry` for that slot via `compile_panic_entry`,
//!    leaving siblings intact). The cache-hit determination and mode
//!    metadata come back on the response, decided at the single
//!    classification site. Read/process-once invariant: same
//!    canonical+profile is never compiled twice within one batch even
//!    if the input list contains duplicates.
//! 4. **Stage D — fan out.** For each original input position, look up
//!    the result for that canonical and clone its `Arc<str>` payloads
//!    (refcount-only, no string copy).
//!
//! Stage B is a single atomic submission on the calling thread: it does
//! NOT fan out through the host batch coordinator. The deduped
//! per-canonical source updates go to the scheduler as ONE
//! `Scheduler::submit_batch_atomic`, followed by ONE `wait_batch`, with
//! the per-canonical post-commit running on the calling thread. The
//! scheduler's own CPU/IO pools execute the parse/analysis work; the
//! caller thread only submits, waits, and commits.
//!
//! Stage C is the parallel stage, and it alone fans out through the host
//! batch coordinator ([`VerterHost::batch_coordinator`] →
//! [`crate::host_batch_coordinator::HostBatchCoordinator::run_batch`]),
//! the single host-side coordination rule shared with the component-meta
//! batch path. The coordinator installs on the host-owned
//! [`verter_scheduler::HostCpuPool`], built once at host construction
//! with an 8 MiB worker stack so the stack guard applies to every code
//! path (no fall-through to Rayon's global pool with its 1 MiB Windows
//! default). `run_batch` is synchronous and the stages are sequential:
//! Stage B fully completes before the Stage-C coordinator is even
//! acquired.
//!
//! The coordinator pool's workers register as
//! [`verter_scheduler::caller_kind::CallerKind::External`], so when a
//! Stage-C compile worker blocks on a scheduler completion handle the
//! host worker parks on the condvar rather than inline-executing
//! scheduler CPU tasks. Running Stage C's waits on the coordinator pool
//! (never the scheduler's stage pool) eliminates the deadlock class
//! where a saturated scheduler CPU pool could starve `compile_many`'s
//! compile/collect/order/finalise phase.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use verter_scheduler::stage::Priority;

use crate::hash::hash_16;
use crate::types::{
    CompileCacheMode, CompileProfile, DowngradeReason, HostDiagnostic, HostError, HostSeverity,
    UpsertRequest, VirtualNodeKind, VirtualQuery,
};
use crate::VerterHost;

/// Test-only sentinel: any input with this canonical id panics inside
/// [`VerterHost::compile_one_in_batch`]'s worker body, so the panic
/// unwinds through the host batch coordinator's generic catch boundary
/// exactly like a real codegen panic. Used by the
/// `compile_many_isolates_panics` test to verify the production catch
/// path (the coordinator boundary + `compile_panic_entry` conversion),
/// not just the test scaffolding.
#[cfg(test)]
pub(crate) const PANIC_INJECT_SENTINEL: &str = "/__compile_panic_inject__.vue";

/// One file in a batch compile call.
#[derive(Debug, Clone)]
pub struct CompileBatchInput {
    pub canonical_id: String,
    pub source: Arc<str>,
    /// Caller-requested compile cache mode for this input. `None`
    /// inherits the batch default ([`CompileBatchOptions::default_mode`]),
    /// which in turn defaults to [`CompileCacheMode::Session`].
    pub requested_mode: Option<CompileCacheMode>,
    /// Explicit component id for scoped-style / HMR identity, threaded
    /// into this input's [`CompileProfile::component_id`] on the
    /// [`CompileManyTarget::RuntimeRender`] lane. This is PER-INPUT (not
    /// batch-level): scoped-style / HMR identity is a property of the
    /// component, not the build, and a real batch mixes components with
    /// distinct ids. `None` lets codegen auto-generate the id. Consumed
    /// ONLY by the RuntimeRender lane; the HostBacked lane's profile is
    /// unchanged.
    pub component_id: Option<String>,
}

/// Result for a single original input position. `cache_hit` is `true`
/// iff this input was served from a warm cache slot (the fact-validated
/// session slot OR the content-addressed store), as decided by the
/// single mode classifier and surfaced on the compile response.
#[derive(Clone)]
pub struct CompileBatchEntry {
    pub canonical_id: String,
    pub code: Arc<str>,
    pub source_map: Option<Arc<str>>,
    /// The compiled `Main` module language (`"ts"` / `"js"` / `"jsx"`),
    /// derived identically across both lanes. `None` on an error/panic
    /// outcome. Bundler consumers (vite sub-request routing) read it.
    pub lang: Option<String>,
    pub errors: Vec<String>,
    /// Non-fatal WARNING-severity diagnostics surfaced on a SUCCESSFUL
    /// compile, kept separate from the fatal `errors`. Populated by the
    /// [`CompileManyTarget::RuntimeRender`] lane's soft-macro contract: an
    /// unresolved imported macro type renders successfully (the compiler
    /// degrades the type to `Unknown`) and reports the diagnostic here
    /// instead of aborting. Always empty on the `HostBacked` lane and on
    /// any fatal outcome.
    pub diagnostics: Vec<HostDiagnostic>,
    pub duration_ms: f64,
    pub cache_hit: bool,
    /// The compile cache mode the caller requested for this input.
    pub requested_mode: CompileCacheMode,
    /// The compile cache mode the runtime actually ran under (equals
    /// `requested_mode` unless an explicit `Content` request downgraded
    /// to `Stateless`).
    pub actual_mode: CompileCacheMode,
    /// The highest-priority reason the requested mode was constrained,
    /// or `None` when no reason fired.
    pub downgrade_reason: Option<DowngradeReason>,
}

/// The batch-level compiler-visible render profile for the
/// [`CompileManyTarget::RuntimeRender`] lane.
///
/// These fields are output-affecting and uniform across a single bundler
/// build (a build is entirely dev OR prod, client OR SSR, one runtime
/// module, one delimiter set), so they live on the batch options rather
/// than per-input. Per-component identity (`component_id`) is separate — it
/// rides on [`CompileBatchInput`].
///
/// This carries EVERY output-affecting field the render lane feeds into
/// `RuntimeCompileOptions`, so the RuntimeRender lane reproduces the
/// caller's build profile byte-for-byte against the HostBacked
/// `get_virtual_file` path (which builds its profile from the same JS
/// `HostCompileProfile`). Omitting a field would silently drop it — e.g.
/// without `source_map` a production build would lose its source maps.
/// Optional fields keep the same "absent = `CompileProfile` default"
/// semantics as the FFI profile conversion, so the render profile also
/// HASHES identically to the profile the caller stored request-time block /
/// style overrides under (`apply_block_overrides`). Fields NOT here are
/// handled elsewhere: `component_id` is per-input, the compile target is
/// fixed (runtime render — no TSX), and the TSX-only knobs
/// (`embed_ambient_types` / `conditional_root_narrowing` / `strict_slots`)
/// do not affect the runtime `Main` and default identically on both lanes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileBatchRenderProfile {
    /// Codegen filename override (component-name extraction, scope-id
    /// derivation, source-map `source`/`file`). `None` falls back to the
    /// canonical id, exactly like an absent `CompileProfile::filename`.
    pub filename: Option<String>,
    /// Production codegen — strips dev-only code (`__file`, HMR).
    pub is_production: bool,
    /// Server-side render function selection.
    pub ssr: bool,
    /// TS type-stripping (plain-JS output).
    pub force_js: bool,
    /// Vapor-mode codegen.
    pub force_vapor: bool,
    /// Emit a source map alongside the `Main` module.
    pub source_map: bool,
    /// Preserve template comments in the render output. TRI-STATE: `None`
    /// keeps the compiler default (`!is_production` — dev preserves,
    /// prod strips), exactly like an absent `CompileProfile::comments`.
    /// Collapsing an absent value to `false` would strip comments from
    /// dev builds.
    pub comments: Option<bool>,
    /// HMR injection strategy the host-side main-module assembly emits.
    pub hmr_strategy: crate::types::HmrStrategy,
    /// Runtime module import specifier (e.g. `"vue"`).
    pub runtime_module_name: Option<String>,
    /// Types module import specifier.
    pub types_module_name: Option<String>,
    /// Custom template interpolation delimiters (default `{{ }}`).
    pub delimiters: Option<(String, String)>,
    /// Custom-element tag names (affect template codegen).
    pub custom_elements: Option<Vec<String>>,
    /// SSR asset-collection module id (root-relative, supplied by the
    /// bundler plugin). See [`crate::types::CompileProfile::ssr_module_id`].
    pub ssr_module_id: Option<String>,
}

/// Caller-configurable batch options.
///
/// `priority = None` defaults to [`Priority::Background`] (yields to
/// concurrent interactive work). Callers with no concurrent interactive
/// work (benchmarks, CI cold-start measurement) should pass
/// [`Priority::Interactive`]. Worker count is fixed at host
/// construction time via [`crate::HostConfig::host_cpu_threads`] —
/// the host-owned CPU pool is not resized per call.
#[derive(Default, Clone, Debug)]
pub struct CompileBatchOptions {
    pub priority: Option<Priority>,
    /// Default compile cache mode applied to inputs whose
    /// [`CompileBatchInput::requested_mode`] is `None`. `None` resolves
    /// to [`CompileCacheMode::Session`] (the host default).
    pub default_mode: Option<CompileCacheMode>,
}

/// The compile lane a [`VerterHost::compile_many`] batch runs under.
///
/// The lane is ALWAYS explicit — it is never inferred from the node kind,
/// the file, or the caller. One shared runtime substrate
/// (`compile_bundle` + `assemble_vue_main_module`), two lanes:
///
/// - [`CompileManyTarget::HostBacked`] runs the full Stage-C session
///   wrapper (`compile_entry`): cache-mode classification, the
///   fact-observation tracer, warm-hit consult, and session/content
///   publish. This is the path IDE / analysis / TSC / type-resolution
///   consumers rely on. Its output is byte-for-byte unchanged.
/// - [`CompileManyTarget::RuntimeRender`] runs a render-only lane that
///   produces the SAME `Main` bytes through the SAME shared substrate but
///   drops the per-file wrapper overhead (source re-clone, cache-mode
///   classification, the unconditional dependency/semantic-axis sync, and
///   the store-view/overlay/resolver-context construction on simple
///   files). Cross-file-macro files still resolve `external_types`
///   through the ONE shared resolver so their render output stays
///   byte-identical. An unresolved imported macro type degrades to a
///   warning instead of a fatal error on this lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileManyTarget {
    /// The full session-wrapper path (`compile_entry`). Byte-for-byte
    /// unchanged; used by every IDE / analysis / TSC / type-resolution
    /// consumer.
    HostBacked,
    /// The render-only bundler lane. Same substrate + same host-side
    /// `Main` assembly, without the per-file session-wrapper overhead. The
    /// [`CompileBatchRenderProfile`] is REQUIRED — carried on the variant so
    /// the lane is fail-closed by construction: you cannot request a runtime
    /// render without supplying the output-affecting build profile, and the
    /// host never substitutes a hidden preset for it.
    RuntimeRender { profile: CompileBatchRenderProfile },
}

/// Bundler-default compile profile preset: production codegen, no SSR, no
/// HMR. This is the EXPLICIT fallback the `RuntimeRender` lane uses when
/// [`CompileBatchOptions::render_profile`] is `None` — it is NOT hidden
/// policy for every `compile_many` call. When a caller supplies a
/// [`CompileBatchRenderProfile`], the lane builds its profile from that
/// instead (see [`render_base_profile`]).
pub fn compile_profile_for_bundler() -> CompileProfile {
    CompileProfile {
        is_production: true,
        ssr: false,
        ..CompileProfile::default()
    }
}

/// Build the batch-level base `CompileProfile` for the `RuntimeRender`
/// lane from the REQUIRED [`CompileBatchRenderProfile`] carried on
/// [`CompileManyTarget::RuntimeRender`]. Every output-affecting field is
/// taken from it (reproducing the caller's build profile byte-for-byte
/// against the HostBacked `get_virtual_file` path); there is no
/// preset-substitution fallback — the lane is fail-closed by construction
/// (the profile cannot be absent). `component_id` is per-input and set
/// later, so it is left `None` here. The compile target stays the default
/// bundler target (no TSX); the TSX-only knobs keep their defaults (which
/// match the HostBacked path, whose profile also defaults them).
///
/// Absent-field semantics mirror the FFI profile conversion
/// (`ffi_profile_to_host`) field-for-field — `comments: None` stays `None`
/// (the compiler default `!is_production`), an absent runtime module name
/// keeps the `CompileProfile` default (`Some("vue")`). This is
/// hash-load-bearing: the resulting profile must produce the SAME
/// `compile_profile_hash` as the `CompileProfile` built from the same JS
/// `HostCompileProfile`, because request-time block / style overrides
/// (`apply_block_overrides`) are stored under that hash and the render
/// lane consumes them through it.
fn render_base_profile(rp: &CompileBatchRenderProfile) -> CompileProfile {
    let mut profile = CompileProfile {
        filename: rp.filename.clone(),
        is_production: rp.is_production,
        ssr: rp.ssr,
        force_js: rp.force_js,
        force_vapor: rp.force_vapor,
        source_map: rp.source_map,
        comments: rp.comments,
        hmr_strategy: rp.hmr_strategy,
        types_module_name: rp.types_module_name.clone(),
        delimiters: rp.delimiters.clone(),
        custom_elements: rp.custom_elements.clone(),
        ssr_module_id: rp.ssr_module_id.clone(),
        ..CompileProfile::default()
    };
    if let Some(name) = &rp.runtime_module_name {
        profile.runtime_module_name = Some(name.clone());
    }
    profile
}

impl VerterHost {
    /// Host-backed parallel SFC batch compile.
    ///
    /// See module-level docs for the four-stage algorithm. Returns
    /// one [`CompileBatchEntry`] per input, in the original input
    /// order. Output ordering is fixed by Stage D, not by Stage B/C's
    /// (non-deterministic) HashMap iteration.
    ///
    /// Per-input panic isolation: if `get_virtual_file` panics for one
    /// input, only that input's entry receives a `compiler panic: ...`
    /// error; the rest of the batch completes normally.
    pub fn compile_many(
        &self,
        inputs: Vec<CompileBatchInput>,
        options: CompileBatchOptions,
        target: CompileManyTarget,
    ) -> Vec<CompileBatchEntry> {
        // ── short-circuit empty input ──
        // No pool is constructed. Tested by
        // `compile_many_with_zero_inputs`.
        if inputs.is_empty() {
            return Vec::new();
        }

        // The batch-level base profile. `HostBacked` keeps the byte-frozen
        // bundler preset (its output is byte-unchanged); `RuntimeRender`
        // builds from the REQUIRED render profile carried on the variant
        // (reproducing the build's dev/prod/ssr/force_js/vapor/source-map/
        // comments/hmr/runtime-module/delimiters/custom-elements). Per-input
        // `component_id` is layered on later, on the RuntimeRender lane only.
        let profile = match &target {
            CompileManyTarget::HostBacked => compile_profile_for_bundler(),
            CompileManyTarget::RuntimeRender { profile } => render_base_profile(profile),
        };

        // Boundary canonicalization: pin every input's `canonical_id` to the
        // SAME host identity `upsert` stores under and every read path
        // (`render_only_main` / `get_virtual_file`) resolves to, BEFORE the
        // per-canonical keying below. Stage B's grouping, `group_errors`,
        // `seen_compile_keys`, and the Stage-D output map are all keyed on
        // `input.canonical_id`; the shared upsert engine returns its
        // `UpsertOutcome` under the resolved canonical, and the Stage-C read
        // resolves through `resolve_alias_or_canonical`. Normalizing here
        // (alias map + `canonicalize_id`) is the single point that keeps all
        // four key spaces coherent, so a caller-supplied variant spelling —
        // e.g. a bundler's upper-case Windows drive id `C:/...` against the
        // canonical `c:/...` — cannot desync the upsert-key, the read-key,
        // and the error/output maps into two identities. This mirrors the
        // upsert chokepoint (`resolve_upsert_canonical`) at the batch
        // boundary; Rust callers can still hand `compile_many` a raw id, so
        // the guard lives here regardless of any FFI/TS-side normalization.
        let inputs: Vec<CompileBatchInput> = inputs
            .into_iter()
            .map(|mut input| {
                input.canonical_id = self.resolve_alias_or_canonical(&input.canonical_id);
                input
            })
            .collect();

        let priority = options.priority.unwrap_or(Priority::Background);
        // Batch default cache mode; a per-input `requested_mode` overrides
        // it. `None` on both resolves to the host default `Session`.
        let default_mode = options.default_mode.unwrap_or(CompileCacheMode::Session);

        // ── group + selective upsert ──
        // HashMap iteration is non-deterministic, but we only iterate
        // it for parallel-independent upserts and probe-keys — never
        // for a position-sensitive output. Output order is fixed in
        // Stage D by iterating `inputs` (the caller's order).
        let mut groups: HashMap<String, Vec<&CompileBatchInput>> =
            HashMap::with_capacity(inputs.len());
        for input in &inputs {
            groups
                .entry(input.canonical_id.clone())
                .or_default()
                .push(input);
        }

        // Per-canonical errors discovered in Stage B (duplicate-source
        // conflicts and upsert failures). Surfaced to every original
        // input position belonging to that canonical in Stage D.
        // Source conflicts and upsert failures are properties of the
        // canonical's source, not of the requested mode, so this map is
        // keyed per-canonical and applies to every mode of that canonical.
        let mut group_errors: HashMap<String, String> = HashMap::new();
        let mut canonical_to_upsert: Vec<&CompileBatchInput> = Vec::new();
        // Compile dedup is keyed by the full compile IDENTITY: `(canonical,
        // effective requested_mode, effective component_id)`. The requested
        // mode is part of the identity (a different mode is a genuinely
        // distinct compile with distinct routing and cache side-effects). On
        // the RuntimeRender lane `component_id` is ALSO output-affecting (it
        // is the scoped-style / HMR id and is per-input, not per-build), so
        // two inputs that share a canonical+mode but carry different
        // component ids are DISTINCT compiles and must each run — otherwise
        // one compiled result would be fanned to both and emit the wrong
        // scope id. On the HostBacked lane the effective component id is
        // always `None` (its profile carries `component_id: None`), so this
        // key reduces to the `(canonical, mode)` identity on that lane. The
        // effective mode is
        // `input.requested_mode.unwrap_or(default_mode)`, matching the
        // per-input profile built in `compile_one_in_batch`.
        let key_component_id = |input: &CompileBatchInput| -> Option<String> {
            match &target {
                CompileManyTarget::RuntimeRender { .. } => input.component_id.clone(),
                CompileManyTarget::HostBacked => None,
            }
        };
        let mut seen_compile_keys: HashSet<(String, CompileCacheMode, Option<String>)> =
            HashSet::new();
        let mut canonical_to_compile: Vec<&CompileBatchInput> = Vec::new();
        for (canonical_id, group) in &groups {
            let first = group[0];
            let conflict = group
                .iter()
                .skip(1)
                .any(|other| other.source.as_bytes() != first.source.as_bytes());
            if conflict {
                group_errors.insert(
                    canonical_id.clone(),
                    "duplicate canonical_id with conflicting source in batch".to_string(),
                );
                continue;
            }
            // One upsert per canonical (source is mode-independent).
            if self.scheduler_source_differs_from(canonical_id, &first.source) {
                canonical_to_upsert.push(first);
            }
            // One compile per distinct `(canonical, effective mode, effective
            // component_id)`.
            for input in group {
                let effective_mode = input.requested_mode.unwrap_or(default_mode);
                if seen_compile_keys.insert((
                    canonical_id.clone(),
                    effective_mode,
                    key_component_id(input),
                )) {
                    canonical_to_compile.push(input);
                }
            }
        }

        // ── Stage-B upsert: ONE atomic batch ──
        // Every per-canonical source update is admitted as a SINGLE
        // `Scheduler::submit_batch_atomic` + one `wait_batch` driven by the
        // shared upsert engine. `canonical_to_upsert` is already deduped to
        // one entry per canonical (conflicting-source duplicates were
        // diverted to `group_errors` above), so it is the exact index space
        // for the engine — which captures the calling thread's request
        // context once, asserts canonical uniqueness, submits the whole
        // batch under one DAG-lock acquisition, then runs each canonical's
        // post-commit on this thread after the single wait. Upsert errors
        // fold into `group_errors`, surfaced to every original input
        // position for that canonical in Stage D.
        let upsert_requests: Vec<UpsertRequest> = canonical_to_upsert
            .iter()
            .map(|input| UpsertRequest {
                canonical_id: Some(input.canonical_id.clone()),
                input_id: input.canonical_id.clone(),
                source: Arc::clone(&input.source),
                file_language: verter_language::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .collect();
        for outcome in self.upsert_many_with_priority(upsert_requests, priority) {
            if let Err(e) = outcome.result {
                group_errors
                    .entry(outcome.canonical_id)
                    .or_insert_with(|| format!("upsert failed: {e}"));
            }
        }

        // ── compile each UNIQUE canonical group exactly once ──
        // Stage C fans the parallel `get_virtual_file` calls out through
        // the host batch coordinator — the single host-side coordination
        // rule. The coordinator installs on the host-owned coordinator pool
        // (built once at host construction with an 8 MiB worker stack;
        // workers register as `CallerKind::External`, so the coordinator
        // never inline-executes scheduler CPU tasks while blocked on a
        // completion handle). The outer wait therefore runs on the
        // coordinator pool, never on the scheduler's stage pool. The
        // coordinator is acquired HERE (not before Stage B) because Stage B
        // no longer fans out: it issues one atomic submit + one wait.
        //
        // `run_batch` is synchronous: this block doesn't begin until the
        // Stage-B `wait_batch` above has returned. The
        // `compile_one_call_count` test-only counter on `VerterHost` is
        // incremented at the top of `compile_one_in_batch` to make the
        // read-once invariant directly observable.
        //
        let coordinator = self.batch_coordinator();
        // Per-input panic isolation is owned by the coordinator's generic
        // catch boundary: a codegen panic in one input is caught there
        // and handed to this policy's `on_item_panic`, which renders it
        // into an error `CompileBatchEntry` for that slot (the domain
        // conversion). Sibling inputs are unaffected and `compile_many`
        // still returns one entry per input. `compile_many` performs no
        // scheduler submission accounting (`scheduler: None`).
        let compile_policy = crate::host_batch_coordinator::BatchPolicy {
            scheduler: None,
            label: "compile_many",
            on_item_panic: &|panic: crate::host_batch_coordinator::BatchItemPanic<
                '_,
                &CompileBatchInput,
            >| {
                let input = panic.item;
                let effective_mode = input.requested_mode.unwrap_or(default_mode);
                let entry = compile_panic_entry(input, effective_mode, &panic.message());
                (
                    (
                        input.canonical_id.clone(),
                        effective_mode,
                        key_component_id(input),
                    ),
                    entry,
                )
            },
        };
        let compiled: HashMap<(String, CompileCacheMode, Option<String>), CompileBatchEntry> =
            coordinator
                .run_batch(&canonical_to_compile, &compile_policy, |input| {
                    let pre_err = group_errors.get(&input.canonical_id).cloned();
                    let entry =
                        self.compile_one_in_batch(input, &profile, default_mode, &target, pre_err);
                    let effective_mode = input.requested_mode.unwrap_or(default_mode);
                    (
                        (
                            input.canonical_id.clone(),
                            effective_mode,
                            key_component_id(input),
                        ),
                        entry,
                    )
                })
                .into_iter()
                .collect();

        // ── fan out to original input order ──
        // For canonicals that errored in Stage B (duplicate-source
        // conflict) or Stage C (compile/host error / panic), every
        // original input position receives the same error entry.
        // Otherwise each position receives the entry compiled for ITS OWN
        // `(canonical, effective requested_mode)` group, so two positions
        // that share a canonical but requested different modes each carry
        // their own requested / actual mode and downgrade reason. Cloning a
        // `CompileBatchEntry` is refcount-only on the `Arc<str>` payloads —
        // no string allocation.
        inputs
            .iter()
            .map(|input| {
                if let Some(err) = group_errors.get(&input.canonical_id) {
                    // Stage B failed before compile, so the request never
                    // ran: report the requested mode unchanged, no reason.
                    let requested = input.requested_mode.unwrap_or(default_mode);
                    return CompileBatchEntry {
                        canonical_id: input.canonical_id.clone(),
                        code: Arc::from(""),
                        source_map: None,
                        lang: None,
                        errors: vec![err.clone()],
                        diagnostics: Vec::new(),
                        duration_ms: 0.0,
                        cache_hit: false,
                        requested_mode: requested,
                        actual_mode: requested,
                        downgrade_reason: None,
                    };
                }
                let effective_mode = input.requested_mode.unwrap_or(default_mode);
                compiled
                    .get(&(
                        input.canonical_id.clone(),
                        effective_mode,
                        key_component_id(input),
                    ))
                    .cloned()
                    .expect(
                        "stage C compiled every non-error (canonical, mode, component_id) group",
                    )
            })
            .collect()
    }

    /// True iff the scheduler holds source for `canonical_id` whose
    /// `whole_hash` matches `hash_16(source.as_bytes())`. Inverted by
    /// the caller to decide whether an upsert is needed.
    fn scheduler_source_differs_from(&self, canonical_id: &str, source: &Arc<str>) -> bool {
        use crate::host_executor::HostSourceData;
        let snap = match self.scheduler.try_get_source(canonical_id) {
            Some(s) => s,
            None => return true,
        };
        let hd = match snap.downcast_data::<HostSourceData>() {
            Some(h) => h,
            None => return true,
        };
        hash_16(source.as_bytes()) != hd.parse.whole_hash
    }

    /// Per-input compile worker. The `precomputed_error` slot is
    /// `Some(...)` when Stage B already failed for this canonical
    /// (duplicate-source conflict or upsert error) — the compile is
    /// short-circuited but the test-only call counter is still
    /// incremented at the top.
    fn compile_one_in_batch(
        &self,
        input: &CompileBatchInput,
        profile: &CompileProfile,
        default_mode: CompileCacheMode,
        target: &CompileManyTarget,
        precomputed_error: Option<String>,
    ) -> CompileBatchEntry {
        // Test-only: increment the call counter at the VERY TOP of the
        // function so every call site is observed, including the
        // precomputed-error short-circuit. Production builds compile
        // this branch out completely; see field doc on
        // `VerterHost::compile_one_call_count`.
        #[cfg(test)]
        self.compile_one_call_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Test-only: record the caller-kind tag of the worker
        // running this `compile_one_in_batch`. Workers running on
        // `HostCpuPool` MUST report `External` (the dual-pool
        // isolation invariant); a regression that ran `compile_many`
        // on the scheduler's CPU pool would record `CpuWorker`
        // instead. Read by
        // `compile_many_workers_carry_host_cpu_pool_id` (secondary
        // caller-kind canary alongside the primary pool-id token
        // assertion).
        #[cfg(test)]
        {
            let tag: u8 = match verter_scheduler::caller_kind::CallerKind::current() {
                verter_scheduler::caller_kind::CallerKind::External => 1,
                verter_scheduler::caller_kind::CallerKind::Driver => 2,
                verter_scheduler::caller_kind::CallerKind::CpuWorker => 3,
                verter_scheduler::caller_kind::CallerKind::IoWorker => 4,
                verter_scheduler::caller_kind::CallerKind::Inline => 5,
            };
            self.compile_one_caller_kind_tag
                .store(tag, std::sync::atomic::Ordering::Relaxed);
            // Record the host-CPU-pool identity token of this worker.
            // The discriminator: a worker running on *this host's*
            // host pool reports `Some(host.host_cpu_pool().pool_id())`;
            // a regression that re-routes `compile_many` onto a
            // per-call Rayon pool or any other `External`-defaulting
            // thread reports `None` (no `start_handler` installed the
            // token). Stored as `usize` with `usize::MAX` reserved as
            // the "unobserved / None" sentinel so the field stays
            // lock-free.
            let token_repr = verter_scheduler::host_cpu_pool_token().unwrap_or(usize::MAX);
            self.compile_one_host_cpu_pool_token
                .store(token_repr, std::sync::atomic::Ordering::Relaxed);
        }

        let start = Instant::now();

        // Effective requested mode for this input, and the per-input
        // profile that carries it into `get_virtual_file`.
        let requested_mode = input.requested_mode.unwrap_or(default_mode);
        let per_input_profile = CompileProfile {
            requested_mode,
            // Per-input scoped-style / HMR identity. Only the RuntimeRender
            // lane threads it (it is a per-component, not per-build, axis);
            // the HostBacked lane keeps the preset's `component_id` (`None`)
            // so its profile — and output — is byte-unchanged.
            component_id: match target {
                CompileManyTarget::RuntimeRender { .. } => input.component_id.clone(),
                CompileManyTarget::HostBacked => profile.component_id.clone(),
            },
            ..profile.clone()
        };

        if let Some(err) = precomputed_error {
            return CompileBatchEntry {
                canonical_id: input.canonical_id.clone(),
                code: Arc::from(""),
                source_map: None,
                lang: None,
                errors: vec![err],
                diagnostics: Vec::new(),
                duration_ms: start.elapsed().as_secs_f64() * 1000.0,
                cache_hit: false,
                requested_mode,
                actual_mode: requested_mode,
                downgrade_reason: None,
            };
        }

        // Per-input panic isolation is owned by the host batch
        // coordinator's generic catch boundary (see `compile_many`'s
        // `compile_policy.on_item_panic`). This worker does NOT wrap its
        // own `catch_unwind`: a codegen panic propagates to the
        // coordinator, which catches it and renders the error
        // `CompileBatchEntry` via `compile_panic_entry`. Centralizing the
        // catch keeps one coordination rule for every batch client.
        //
        // Test-only panic injection — fired in the worker so it unwinds
        // through the coordinator's catch exactly like a real codegen
        // panic. Production builds compile this branch out completely.
        #[cfg(test)]
        if input.canonical_id == PANIC_INJECT_SENTINEL {
            panic!("synthetic panic for compile_many_isolates_panics test");
        }

        // Route by the explicit lane. `RuntimeRender` runs the render-only
        // lane (same shared substrate + host-side `Main` assembly, without
        // the per-file session-wrapper overhead); `HostBacked` runs the
        // full session wrapper via `get_virtual_file`.
        if matches!(target, CompileManyTarget::RuntimeRender { .. }) {
            return self.compile_one_runtime_render(
                input,
                &per_input_profile,
                requested_mode,
                start,
            );
        }

        let result = self.get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(input.canonical_id.clone()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: per_input_profile.clone(),
        });

        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
        let id_prefix = format!("[{}] ", input.canonical_id);

        match result {
            Ok(response) => {
                let errors: Vec<String> = response
                    .diagnostics
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == HostSeverity::Error)
                    .map(|d| d.message.clone())
                    .collect();
                // The cache-hit determination, actual mode, and downgrade
                // reason are all authoritative on the response (decided at
                // the single classification site inside `get_virtual_file`).
                CompileBatchEntry {
                    canonical_id: input.canonical_id.clone(),
                    code: response.code,
                    source_map: response.source_map,
                    lang: response.lang,
                    errors,
                    // HostBacked never softens a diagnostic to a warning
                    // here — its warnings ride in the response diagnostics
                    // and are not re-surfaced as a distinct success-warning
                    // list. Only the RuntimeRender soft-macro lane populates
                    // `diagnostics`.
                    diagnostics: Vec::new(),
                    duration_ms,
                    cache_hit: response.cache_hit,
                    requested_mode: response.requested_mode,
                    actual_mode: response.actual_mode,
                    downgrade_reason: response.downgrade_reason,
                }
            }
            // CRITICAL: HostError::CompileError carries a
            // DiagnosticsSnapshot. Its `Display` impl collapses to the
            // static "compile error" string, so a `format!("host
            // error: {host_err}")` would lose every diagnostic. Unpack
            // the variant explicitly so all error-severity diagnostics
            // reach `errors: Vec<String>`. Tested by
            // `compile_many_compile_error_preserves_all_diagnostics`.
            //
            // The compile-failure payload also carries the mode metadata
            // decided at classification time. A compile that errored after
            // a downgrade (e.g. a `Content` request floored to `Stateless`)
            // must report the mode it actually ran under, not the requested
            // mode — so the error entry mirrors the success entry's mode
            // surface instead of resetting to the request.
            Err(HostError::CompileError(failure)) => {
                let mut errors: Vec<String> = failure
                    .diagnostics
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == HostSeverity::Error)
                    .map(|d| format!("{id_prefix}{}", d.message))
                    .collect();
                if errors.is_empty() {
                    errors.push(format!("{id_prefix}compile error (no diagnostic messages)"));
                }
                CompileBatchEntry {
                    canonical_id: input.canonical_id.clone(),
                    code: Arc::from(""),
                    source_map: None,
                    lang: None,
                    errors,
                    diagnostics: Vec::new(),
                    duration_ms,
                    cache_hit: false,
                    requested_mode: failure.requested_mode,
                    actual_mode: failure.actual_mode,
                    downgrade_reason: failure.downgrade_reason,
                }
            }
            Err(host_err) => CompileBatchEntry {
                canonical_id: input.canonical_id.clone(),
                code: Arc::from(""),
                source_map: None,
                lang: None,
                errors: vec![format!("{id_prefix}host error: {host_err}")],
                diagnostics: Vec::new(),
                duration_ms,
                cache_hit: false,
                requested_mode,
                actual_mode: requested_mode,
                downgrade_reason: None,
            },
        }
    }

    /// The [`CompileManyTarget::RuntimeRender`] per-file worker: a
    /// render-only compile onto the shared runtime substrate that produces
    /// byte-identical `Main` output to the `HostBacked` wrapper without the
    /// per-file session-wrapper overhead.
    fn compile_one_runtime_render(
        &self,
        input: &CompileBatchInput,
        per_input_profile: &CompileProfile,
        requested_mode: CompileCacheMode,
        start: Instant,
    ) -> CompileBatchEntry {
        let id_prefix = format!("[{}] ", input.canonical_id);
        match self.render_only_main(&input.canonical_id, per_input_profile) {
            Ok(render) => CompileBatchEntry {
                canonical_id: input.canonical_id.clone(),
                code: render.code,
                source_map: render.source_map,
                lang: render.lang,
                errors: Vec::new(),
                diagnostics: render.diagnostics,
                duration_ms: start.elapsed().as_secs_f64() * 1000.0,
                // The render lane consults no host cache node, so a render
                // is never a "warm hit" and always reports `false`. The
                // mode axis is carried for wire-shape parity with the
                // HostBacked entry; the render lane runs under no cache mode.
                cache_hit: false,
                requested_mode,
                actual_mode: requested_mode,
                downgrade_reason: None,
            },
            Err(HostError::CompileError(failure)) => {
                let mut errors: Vec<String> = failure
                    .diagnostics
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == HostSeverity::Error)
                    .map(|d| format!("{id_prefix}{}", d.message))
                    .collect();
                if errors.is_empty() {
                    errors.push(format!("{id_prefix}compile error (no diagnostic messages)"));
                }
                CompileBatchEntry {
                    canonical_id: input.canonical_id.clone(),
                    code: Arc::from(""),
                    source_map: None,
                    lang: None,
                    errors,
                    diagnostics: Vec::new(),
                    duration_ms: start.elapsed().as_secs_f64() * 1000.0,
                    cache_hit: false,
                    requested_mode,
                    actual_mode: requested_mode,
                    downgrade_reason: None,
                }
            }
            Err(host_err) => CompileBatchEntry {
                canonical_id: input.canonical_id.clone(),
                code: Arc::from(""),
                source_map: None,
                lang: None,
                errors: vec![format!("{id_prefix}host error: {host_err}")],
                diagnostics: Vec::new(),
                duration_ms: start.elapsed().as_secs_f64() * 1000.0,
                cache_hit: false,
                requested_mode,
                actual_mode: requested_mode,
                downgrade_reason: None,
            },
        }
    }
}

/// Render a caught per-input compile panic into the error
/// `CompileBatchEntry` for that slot. The host batch coordinator owns
/// the generic `catch_unwind`; this is the domain conversion
/// `compile_many` supplies through its `BatchPolicy::on_item_panic`, so
/// a panicking input produces a one-error entry (prefixed with the
/// canonical id and `"compiler panic:"`) without aborting the batch or
/// poisoning sibling inputs.
fn compile_panic_entry(
    input: &CompileBatchInput,
    effective_mode: CompileCacheMode,
    message: &str,
) -> CompileBatchEntry {
    CompileBatchEntry {
        canonical_id: input.canonical_id.clone(),
        code: Arc::from(""),
        source_map: None,
        lang: None,
        errors: vec![format!(
            "[{}] compiler panic: {}",
            input.canonical_id, message
        )],
        diagnostics: Vec::new(),
        duration_ms: 0.0,
        cache_hit: false,
        requested_mode: effective_mode,
        actual_mode: effective_mode,
        downgrade_reason: None,
    }
}
