#![allow(dead_code)]
//! Currency per-file cost probe.
//!
//! Measurement-only harness for the path-precise-resolution-currency host tax.
//! Splits the host wall into phases (construct / upsert / analyse+lint /
//! meta / teardown), compares interleaved `upsert+lint` against a two-phase
//! `upsert_all` then `lint_all`, and runs a four-cell import micro-bench.
//!
//! Usage:
//!   cargo run -p verter_bench --profile release-dbg --example currency_phase_probe -- <mode>
//!
//! Modes:
//!   lint      — M1 lint phase split (MemoryWorkspace), interleaved vs two-phase
//!   meta      — M1 component-meta phase split (FilesystemWorkspace)
//!   cells     — M3 four-cell edge micro-bench, N=25/100/200, memory + filesystem
//!   all       — every mode
//!
//! Env:
//!   VERTER_FIXTURES  — fixture dir (default /tmp/vue-benchmarks/fixtures/200)
//!   PROBE_RUNS       — measured runs per cell (default 5)
//!   PROBE_WARMUP     — warmup runs per cell (default 1)

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use verter_diagnostics::{LintConfig, Linter};
use verter_semantic::analysis::types::{AnalysisFlags, ScriptAnalysisSnapshot};
use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::{FileAnalysisSnapshot, FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{FilesystemOptions, FilesystemWorkspace, ProjectGraph, ViteConfigOptions};

// ─────────────────────────── stats ───────────────────────────

#[derive(Clone, Debug, Default)]
struct Series {
    samples: Vec<f64>,
}

impl Series {
    fn push(&mut self, ms: f64) {
        self.samples.push(ms);
    }
    fn median(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut s = self.samples.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = s.len();
        if n % 2 == 1 {
            s[n / 2]
        } else {
            (s[n / 2 - 1] + s[n / 2]) / 2.0
        }
    }
    fn min(&self) -> f64 {
        self.samples.iter().cloned().fold(f64::INFINITY, f64::min)
    }
    fn max(&self) -> f64 {
        self.samples
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max)
    }
    /// Spread as +/- % of median across the observed range.
    fn spread_pct(&self) -> f64 {
        let m = self.median();
        if m <= 0.0 {
            return 0.0;
        }
        ((self.max() - self.min()) / m) * 100.0
    }
    fn fmt(&self) -> String {
        format!(
            "{:>9.2} ms  [{:.2}..{:.2}, ±{:.1}%]",
            self.median(),
            self.min(),
            self.max(),
            self.spread_pct()
        )
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn runs() -> usize {
    std::env::var("PROBE_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
}

fn warmup() -> usize {
    std::env::var("PROBE_WARMUP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

// ─────────────────────────── fixtures ───────────────────────────

struct VueFile {
    id: String,
    source: String,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("VERTER_FIXTURES")
            .unwrap_or_else(|_| "/tmp/vue-benchmarks/fixtures/200".to_string()),
    )
}

fn load_fixture_files(dir: &Path, limit: Option<usize>) -> Vec<VueFile> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read fixtures at {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("vue"))
        .collect();
    names.sort();
    if let Some(n) = limit {
        names.truncate(n);
    }
    names
        .into_iter()
        .map(|p| VueFile {
            id: p.to_string_lossy().replace('\\', "/"),
            source: std::fs::read_to_string(&p).unwrap(),
        })
        .collect()
}

// ─────────────────────────── lint helpers ───────────────────────────

fn script_from_host(analysis: &FileAnalysisSnapshot) -> ScriptAnalysisSnapshot {
    ScriptAnalysisSnapshot {
        imports: analysis.imports.clone(),
        module_references: analysis.module_references.to_vec(),
        bindings: analysis.bindings.clone(),
        macros: analysis.macros.to_vec(),
        macro_type_deps: analysis.macro_type_deps.to_vec(),
        flags: AnalysisFlags::from_bits_truncate(analysis.script_flags),
        vue_api_calls: analysis.vue_api_calls.to_vec(),
        dom_query_calls: analysis.dom_query_calls.to_vec(),
        css_var_manipulations: analysis.css_var_manipulations.to_vec(),
        script_binding_occurrences: analysis.script_binding_occurrences.to_vec(),
        options_api: analysis.options_api.clone(),
        store_usages: analysis.store_usages.to_vec(),
        store_definitions: analysis.store_definitions.to_vec(),
        is_typescript: analysis.is_typescript,
        ..Default::default()
    }
}

fn upsert_one(host: &VerterHost, f: &VueFile) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(f.id.clone()),
        input_id: f.id.clone(),
        source: Arc::from(f.source.as_str()),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    });
}

/// Ordered SFC block facts projected from the registered carrier inventory,
/// mirroring the production NAPI/MCP lint callers. Empty when the file has no
/// registered structure (fail closed).
fn registered_block_facts(
    host: &VerterHost,
    canonical_or_alias: &str,
) -> Vec<verter_diagnostics::SfcBlockFact> {
    host.registered_file_structure_snapshot(canonical_or_alias)
        .map(|(structure, _)| verter_diagnostics::project_block_facts(structure.inventory()))
        .unwrap_or_default()
}

fn lint_one(host: &VerterHost, linter: &Linter, id: &str) -> usize {
    match host.get_analysis(id) {
        Some(snapshot) => {
            let script = script_from_host(&snapshot);
            let blocks = registered_block_facts(host, id);
            linter
                .lint(
                    Some(&script),
                    snapshot.template.as_deref(),
                    &snapshot.styles,
                    &blocks,
                )
                .into_diagnostics()
                .len()
        }
        None => 0,
    }
}

// ─────────────────────────── counters ───────────────────────────

#[derive(Clone, Debug, Default)]
struct Counters {
    rows: Vec<(&'static str, u64)>,
}

fn counters_of(host: &VerterHost) -> Counters {
    use std::sync::atomic::Ordering::Relaxed;
    let p = host.provenance();
    Counters {
        rows: vec![
            ("host_upsert_calls", p.host_upsert_calls.load(Relaxed)),
            ("get_analysis_calls", p.get_analysis_calls.load(Relaxed)),
            (
                "indexed_ready_materializes",
                p.indexed_ready_materializes.load(Relaxed),
            ),
            (
                "indexed_ready_scheduler_snapshot_reuse",
                p.indexed_ready_scheduler_snapshot_reuse.load(Relaxed),
            ),
            ("shallow_state_builds", p.shallow_state_builds.load(Relaxed)),
            ("eval_env_builds", p.eval_env_builds.load(Relaxed)),
            ("sfc_parses", p.sfc_parses.load(Relaxed)),
            ("carrier_parses", p.carrier_parses.load(Relaxed)),
            (
                "vue_script_snapshot_parses",
                p.vue_script_snapshot_parses.load(Relaxed),
            ),
            ("decl_bodies_lowered", p.decl_bodies_lowered.load(Relaxed)),
            ("dep_resolution_calls", p.dep_resolution_calls.load(Relaxed)),
            (
                "import_resolution_cache_hit_count",
                p.import_resolution_cache_hit_count.load(Relaxed),
            ),
            (
                "import_resolution_cache_miss_count",
                p.import_resolution_cache_miss_count.load(Relaxed),
            ),
            (
                "store_view_from_host_reads",
                p.store_view_from_host_reads.load(Relaxed),
            ),
            ("ensure_loaded_calls", p.ensure_loaded_calls.load(Relaxed)),
            (
                "ensure_loaded_work_ns",
                p.ensure_loaded_work_ns.load(Relaxed),
            ),
            (
                "ensure_loaded_wait_ns",
                p.ensure_loaded_wait_ns.load(Relaxed),
            ),
            (
                "scheduler_submit_count",
                p.scheduler_submit_count.load(Relaxed),
            ),
            (
                "native_fs_read_dir_count",
                p.native_fs_read_dir_count.load(Relaxed),
            ),
            (
                "native_fs_read_file_miss_count",
                p.native_fs_read_file_miss_count.load(Relaxed),
            ),
            (
                "component_meta_result_cache_hits",
                p.component_meta_result_cache_hits.load(Relaxed),
            ),
            (
                "component_meta_result_cache_misses",
                p.component_meta_result_cache_misses.load(Relaxed),
            ),
        ],
    }
}

fn print_counters(label: &str, c: &Counters, per: usize) {
    println!("  counters [{label}] (n={per}):");
    for (k, v) in &c.rows {
        if *v == 0 {
            continue;
        }
        if k.ends_with("_ns") {
            println!(
                "    {k:<42} {v:>12}  ({:>8.3} ms total, {:>7.1} µs/file)",
                *v as f64 / 1e6,
                (*v as f64 / 1e3) / per.max(1) as f64
            );
        } else {
            println!(
                "    {k:<42} {v:>12}  ({:>6.2}/file)",
                *v as f64 / per.max(1) as f64
            );
        }
    }
}

#[cfg(feature = "currency_probe")]
fn print_probe(per: usize) {
    let snap = verter_workspace::currency_probe::snapshot();
    if snap.is_empty() {
        return;
    }
    println!("  currency probe (n={per}):");
    println!(
        "    {:<44} {:>10} {:>12} {:>12} {:>12}",
        "site", "calls", "calls/file", "total ms", "µs/file"
    );
    for row in snap {
        if row.name.contains("[tally]") {
            println!(
                "    {:<44} {:>10} {:>12.2} {:>12} {:>12.1}",
                row.name,
                row.calls,
                row.calls as f64 / per.max(1) as f64,
                row.ns,
                row.ns as f64 / row.calls.max(1) as f64
            );
            continue;
        }
        println!(
            "    {:<44} {:>10} {:>12.2} {:>12.3} {:>12.1}",
            row.name,
            row.calls,
            row.calls as f64 / per.max(1) as f64,
            row.ns as f64 / 1e6,
            (row.ns as f64 / 1e3) / per.max(1) as f64
        );
    }
}

#[cfg(not(feature = "currency_probe"))]
fn print_probe(_per: usize) {}

#[cfg(feature = "currency_probe")]
fn reset_probe() {
    verter_workspace::currency_probe::reset();
}

#[cfg(not(feature = "currency_probe"))]
fn reset_probe() {}

// ─────────────────────────── M1: lint ───────────────────────────

#[derive(Default)]
struct LintPhases {
    construct: Series,
    upsert: Series,
    analyse_lint: Series,
    /// `get_analysis` only (subset of `analyse_lint`).
    analysis: Series,
    /// pure `Linter` run (subset of `analyse_lint`).
    linter: Series,
    /// second `get_analysis` pass over the same files (warm).
    analysis_warm: Series,
    teardown: Series,
    total: Series,
}

/// Interleaved: `upsert(f); lint(f)` per file — the shape the JS harness uses.
fn lint_interleaved(files: &[VueFile], phases: &mut LintPhases) -> (usize, Counters) {
    let linter = Linter::new(LintConfig::default());
    let t_all = Instant::now();

    let t0 = Instant::now();
    let host = VerterHost::new_standalone(HostConfig::default());
    phases.construct.push(ms(t0.elapsed()));

    let mut upsert_ns = 0u128;
    let mut lint_ns = 0u128;
    let mut work = 0usize;
    for f in files {
        let t = Instant::now();
        upsert_one(&host, f);
        upsert_ns += t.elapsed().as_nanos();
        let t = Instant::now();
        work += lint_one(&host, &linter, &f.id);
        lint_ns += t.elapsed().as_nanos();
    }
    phases.upsert.push(upsert_ns as f64 / 1e6);
    phases.analyse_lint.push(lint_ns as f64 / 1e6);

    let counters = counters_of(&host);

    let t0 = Instant::now();
    host.close();
    drop(host);
    phases.teardown.push(ms(t0.elapsed()));
    phases.total.push(ms(t_all.elapsed()));
    (work, counters)
}

/// Two-phase: all upserts, then all lints.
fn lint_two_phase(files: &[VueFile], phases: &mut LintPhases) -> (usize, Counters) {
    let linter = Linter::new(LintConfig::default());
    let t_all = Instant::now();

    let t0 = Instant::now();
    let host = VerterHost::new_standalone(HostConfig::default());
    phases.construct.push(ms(t0.elapsed()));

    let t0 = Instant::now();
    for f in files {
        upsert_one(&host, f);
    }
    phases.upsert.push(ms(t0.elapsed()));

    let t0 = Instant::now();
    let mut work = 0usize;
    let mut analysis_ns = 0u128;
    let mut linter_ns = 0u128;
    for f in files {
        let t = Instant::now();
        let snapshot = host.get_analysis(&f.id);
        analysis_ns += t.elapsed().as_nanos();
        if let Some(snapshot) = snapshot {
            let t = Instant::now();
            let script = script_from_host(&snapshot);
            let blocks = registered_block_facts(&host, &f.id);
            work += linter
                .lint(
                    Some(&script),
                    snapshot.template.as_deref(),
                    &snapshot.styles,
                    &blocks,
                )
                .into_diagnostics()
                .len();
            linter_ns += t.elapsed().as_nanos();
        }
    }
    phases.analyse_lint.push(ms(t0.elapsed()));
    phases.analysis.push(analysis_ns as f64 / 1e6);
    phases.linter.push(linter_ns as f64 / 1e6);

    // Warm second pass: same host, same files, `get_analysis` again.
    let t0 = Instant::now();
    for f in files {
        let _ = host.get_analysis(&f.id);
    }
    phases.analysis_warm.push(ms(t0.elapsed()));

    let counters = counters_of(&host);

    let t0 = Instant::now();
    host.close();
    drop(host);
    phases.teardown.push(ms(t0.elapsed()));
    phases.total.push(ms(t_all.elapsed()));
    (work, counters)
}

/// Upsert-only: no analysis/lint at all. Isolates ingest from query.
fn lint_upsert_only(files: &[VueFile], phases: &mut LintPhases) -> (usize, Counters) {
    let t_all = Instant::now();
    let t0 = Instant::now();
    let host = VerterHost::new_standalone(HostConfig::default());
    phases.construct.push(ms(t0.elapsed()));

    let t0 = Instant::now();
    for f in files {
        upsert_one(&host, f);
    }
    phases.upsert.push(ms(t0.elapsed()));
    phases.analyse_lint.push(0.0);

    let counters = counters_of(&host);

    let t0 = Instant::now();
    host.close();
    drop(host);
    phases.teardown.push(ms(t0.elapsed()));
    phases.total.push(ms(t_all.elapsed()));
    (0, counters)
}

fn report_phases(label: &str, p: &LintPhases, n: usize) {
    println!("── {label} (n={n} files) ──");
    println!("  construct      {}", p.construct.fmt());
    println!(
        "  upsert         {}   -> {:.3} ms/file",
        p.upsert.fmt(),
        p.upsert.median() / n as f64
    );
    println!(
        "  analyse+lint   {}   -> {:.3} ms/file",
        p.analyse_lint.fmt(),
        p.analyse_lint.median() / n as f64
    );
    if !p.analysis.samples.is_empty() {
        println!(
            "    · get_analysis(cold) {}   -> {:.3} ms/file",
            p.analysis.fmt(),
            p.analysis.median() / n as f64
        );
        println!(
            "    · Linter (pure)      {}   -> {:.3} ms/file",
            p.linter.fmt(),
            p.linter.median() / n as f64
        );
        println!(
            "    · get_analysis(warm) {}   -> {:.3} ms/file",
            p.analysis_warm.fmt(),
            p.analysis_warm.median() / n as f64
        );
    }
    println!("  teardown       {}", p.teardown.fmt());
    println!(
        "  TOTAL          {}   -> {:.3} ms/file",
        p.total.fmt(),
        p.total.median() / n as f64
    );
}

fn run_lint_mode() {
    let files = load_fixture_files(&fixtures_dir(), None);
    let n = files.len();
    println!("\n===== M1 LINT PHASE SPLIT (MemoryWorkspace) =====");
    println!("fixtures: {} ({n} files)", fixtures_dir().display());
    println!("runs={} warmup={}\n", runs(), warmup());

    for _ in 0..warmup() {
        let mut p = LintPhases::default();
        let _ = lint_interleaved(&files, &mut p);
    }

    let mut inter = LintPhases::default();
    let mut inter_c = Counters::default();
    let mut inter_work = 0;
    for _ in 0..runs() {
        let (w, c) = lint_interleaved(&files, &mut inter);
        inter_work = w;
        inter_c = c;
    }

    let mut two = LintPhases::default();
    let mut two_c = Counters::default();
    let mut two_work = 0;
    for _ in 0..runs() {
        let (w, c) = lint_two_phase(&files, &mut two);
        two_work = w;
        two_c = c;
    }

    let mut only = LintPhases::default();
    let mut only_c = Counters::default();
    for _ in 0..runs() {
        let (_, c) = lint_upsert_only(&files, &mut only);
        only_c = c;
    }

    report_phases("interleaved  upsert(f); lint(f)", &inter, n);
    print_counters("interleaved", &inter_c, n);
    println!();
    report_phases("two-phase    upsert_all; lint_all", &two, n);
    print_counters("two-phase", &two_c, n);
    println!();
    report_phases("upsert-only  (no analysis at all)", &only, n);
    print_counters("upsert-only", &only_c, n);
    println!();
    println!("  lint diagnostics: interleaved={inter_work} two-phase={two_work} (must match)");
    println!(
        "  interleaved TOTAL {:.2} ms vs two-phase TOTAL {:.2} ms  (delta {:+.2} ms, {:+.1}%)",
        inter.total.median(),
        two.total.median(),
        two.total.median() - inter.total.median(),
        (two.total.median() - inter.total.median()) / inter.total.median() * 100.0
    );
    println!(
        "  upsert-bound share (interleaved): upsert {:.1}% / analyse+lint {:.1}%",
        inter.upsert.median() / inter.total.median() * 100.0,
        inter.analyse_lint.median() / inter.total.median() * 100.0
    );
    print_probe(n);
}

// ─────────────────────────── M1: meta ───────────────────────────

/// Mirror the JS harness's `prepareTypecheckDir`: copy the first `count`
/// fixtures into a temp root beside a `tsconfig.json` whose `paths.vue`
/// points at the real `vue` package, so `vue` genuinely resolves.
fn prepare_meta_root(src: &Path, count: usize) -> (tempfile::TempDir, PathBuf, Vec<VueFile>) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let files = load_fixture_files(src, Some(count));
    let mut copied = Vec::with_capacity(files.len());
    let mut names = Vec::with_capacity(files.len());
    for f in &files {
        let name = Path::new(&f.id)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        std::fs::write(root.join(&name), &f.source).unwrap();
        copied.push(VueFile {
            id: root.join(&name).to_string_lossy().replace('\\', "/"),
            source: f.source.clone(),
        });
        names.push(name);
    }
    let aux = "env.d.ts";
    let aux_src = src.join(aux);
    if aux_src.exists() {
        std::fs::copy(&aux_src, root.join(aux)).unwrap();
        names.push(aux.to_string());
    }
    let vue_pkg = std::env::var("VERTER_VUE_PKG")
        .unwrap_or_else(|_| "/tmp/vue-benchmarks/node_modules/vue".to_string());
    let include = names
        .iter()
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        root.join("tsconfig.json"),
        format!(
            "{{\n  \"compilerOptions\": {{\n    \"target\": \"ESNext\",\n    \"module\": \"ESNext\",\n    \"moduleResolution\": \"Bundler\",\n    \"strict\": true,\n    \"noEmit\": true,\n    \"jsx\": \"preserve\",\n    \"paths\": {{ \"vue\": [\"{vue_pkg}\"] }}\n  }},\n  \"include\": [{include}]\n}}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("package.json"),
        "{\n  \"private\": true,\n  \"type\": \"module\",\n  \"name\": \"probe-meta\"\n}\n",
    )
    .unwrap();
    (tmp, root, copied)
}

fn run_meta_mode() {
    let src = fixtures_dir();
    let (_tmp, dir, files) = prepare_meta_root(&src, 100);
    let n = files.len();
    println!("\n===== M1 COMPONENT-META PHASE SPLIT (FilesystemWorkspace) =====");
    println!("fixtures: {} ({n} files)", dir.display());
    println!("runs={} warmup={}\n", runs(), warmup());

    let mut construct = Series::default();
    let mut load = Series::default();
    let mut resolve = Series::default();
    let mut teardown = Series::default();
    let mut total = Series::default();
    let mut counters = Counters::default();
    let mut members = 0usize;

    for i in 0..(runs() + warmup()) {
        let record = i >= warmup();
        let t_all = Instant::now();

        let t0 = Instant::now();
        let ws = FilesystemWorkspace::new(FilesystemOptions {
            roots: vec![dir.to_string_lossy().to_string()],
            eager_preload: false,
        });
        let graph = ProjectGraph::from_workspace_roots(
            &ws,
            &[dir.to_string_lossy().to_string()],
            &ViteConfigOptions::default(),
        );
        ws.set_project_graph(graph.graph);
        let meta_host = ComponentMetaHost::new(HostConfig::default(), Arc::new(ws));
        let session = meta_host.open_session().unwrap();
        let c_ms = ms(t0.elapsed());

        // Phase: load every file into the base project (explicit, so the
        // subsequent query phase is warm-on-source).
        let t0 = Instant::now();
        for f in &files {
            let _ = meta_host.ensure_loaded(&f.id);
        }
        let l_ms = ms(t0.elapsed());

        let t0 = Instant::now();
        let mut m = 0usize;
        for f in &files {
            if let Ok(Some(meta)) = session.get_component_meta(&f.id) {
                m += meta.props.len() + meta.events.len() + meta.slots.len();
            }
        }
        let r_ms = ms(t0.elapsed());

        let c = counters_of(meta_host.host());

        let t0 = Instant::now();
        drop(session);
        meta_host.shutdown();
        drop(meta_host);
        let t_ms = ms(t0.elapsed());
        let all_ms = ms(t_all.elapsed());

        if record {
            construct.push(c_ms);
            load.push(l_ms);
            resolve.push(r_ms);
            teardown.push(t_ms);
            total.push(all_ms);
            counters = c;
            members = m;
        }
    }

    println!("── meta: session construct / ensure_loaded all / get_component_meta all ──");
    println!("  construct      {}", construct.fmt());
    println!(
        "  ensure_loaded  {}   -> {:.3} ms/file",
        load.fmt(),
        load.median() / n as f64
    );
    println!(
        "  meta resolve   {}   -> {:.3} ms/file",
        resolve.fmt(),
        resolve.median() / n as f64
    );
    println!("  teardown       {}", teardown.fmt());
    println!(
        "  TOTAL          {}   -> {:.3} ms/file",
        total.fmt(),
        total.median() / n as f64
    );
    println!("  meta members: {members}");
    print_counters("meta", &counters, n);
    print_probe(n);
}

// ───────────────── R1: napi-shape lint wrapper accounting ─────────────────
//
// The NAPI `lint` entry-point does strictly more per call than the in-process
// probe: it builds a fresh `Linter` (187-rule registry) per call, re-fetches the
// source, rewrites every diagnostic span from UTF-8 bytes to UTF-16 units, and
// materialises an owned result struct (Debug-formatted tags + span kind, cloned
// strings). This mode times each of those steps separately, in-process, so the
// NAPI wall can be split into "extra Rust work the wrapper does" and "the FFI
// crossing itself".

/// Byte offset → UTF-16 unit offset, mirroring `verter_ffi::convert::offset`.
fn byte_offset_to_utf16(source: &str, byte_offset: u32) -> u32 {
    let mut clamped = (byte_offset as usize).min(source.len());
    while clamped > 0 && !source.is_char_boundary(clamped) {
        clamped -= 1;
    }
    source[..clamped].encode_utf16().count() as u32
}

/// Mirrors `NapiLintDiagnostic` construction (owned strings + Debug formats).
#[derive(Debug)]
struct OwnedDiagnostic {
    rule: String,
    category: String,
    severity: String,
    message: String,
    span_start: u32,
    span_end: u32,
    tags: Vec<String>,
    span_kind: String,
}

fn run_napi_shape_mode() {
    let files = load_fixture_files(&fixtures_dir(), None);
    let n = files.len();
    println!("\n===== R1 NAPI-SHAPE LINT WRAPPER ACCOUNTING (MemoryWorkspace) =====");
    println!("fixtures: {} ({n} files)", fixtures_dir().display());
    println!("runs={} warmup={}\n", runs(), warmup());

    let mut construct = Series::default();
    let mut upsert_cold = Series::default();
    let mut upsert_same = Series::default();
    let mut analysis_cold = Series::default();
    let mut analysis_warm = Series::default();
    let mut linter_new = Series::default();
    let mut script_build = Series::default();
    let mut lint_run = Series::default();
    let mut get_source = Series::default();
    let mut utf16 = Series::default();
    let mut owned_build = Series::default();
    let mut diag_count = 0usize;

    for i in 0..(runs() + warmup()) {
        let record = i >= warmup();

        let t0 = Instant::now();
        let host = VerterHost::new_standalone(HostConfig::default());
        let c_ms = ms(t0.elapsed());

        let t0 = Instant::now();
        for f in &files {
            upsert_one(&host, f);
        }
        let u_cold = ms(t0.elapsed());

        // Identical re-upsert: the Rust-side cost when content did not change.
        let t0 = Instant::now();
        for f in &files {
            upsert_one(&host, f);
        }
        let u_same = ms(t0.elapsed());

        // Cold analysis pass.
        let t0 = Instant::now();
        for f in &files {
            let _ = host.get_analysis(&f.id);
        }
        let a_cold = ms(t0.elapsed());

        // Warm analysis pass.
        let t0 = Instant::now();
        let mut snaps = Vec::with_capacity(n);
        for f in &files {
            snaps.push(host.get_analysis(&f.id));
        }
        let a_warm = ms(t0.elapsed());

        // 200x Linter::new (what the NAPI wrapper pays per call).
        let t0 = Instant::now();
        let mut linters = Vec::with_capacity(n);
        for _ in 0..n {
            linters.push(Linter::new(LintConfig::default()));
        }
        let l_new = ms(t0.elapsed());

        // 200x script snapshot projection.
        let t0 = Instant::now();
        let mut scripts = Vec::with_capacity(n);
        for s in &snaps {
            scripts.push(s.as_ref().map(script_from_host));
        }
        let s_build = ms(t0.elapsed());

        // 200x block-fact projection (parallels the script snapshot pass).
        let mut block_facts = Vec::with_capacity(n);
        for f in &files {
            block_facts.push(registered_block_facts(&host, &f.id));
        }

        // 200x pure lint run (hoisted linter, as the probe does).
        let linter = Linter::new(LintConfig::default());
        let t0 = Instant::now();
        let mut all_diags = Vec::with_capacity(n);
        for ((s, script), blocks) in snaps.iter().zip(scripts.iter()).zip(block_facts.iter()) {
            match (s, script) {
                (Some(snapshot), Some(script)) => all_diags.push(
                    linter
                        .lint(
                            Some(script),
                            snapshot.template.as_deref(),
                            &snapshot.styles,
                            blocks,
                        )
                        .into_diagnostics(),
                ),
                _ => all_diags.push(Vec::new()),
            }
        }
        let l_run = ms(t0.elapsed());

        // 200x get_source.
        let t0 = Instant::now();
        let mut sources = Vec::with_capacity(n);
        for f in &files {
            sources.push(host.get_source(&f.id));
        }
        let g_src = ms(t0.elapsed());

        // 200x UTF-16 span rewrite over the produced diagnostics.
        let t0 = Instant::now();
        let mut sink = 0u32;
        for (diags, src) in all_diags.iter().zip(sources.iter()) {
            if let Some(src) = src.as_deref() {
                for d in diags {
                    sink = sink
                        .wrapping_add(byte_offset_to_utf16(src, d.span.start))
                        .wrapping_add(byte_offset_to_utf16(src, d.span.end));
                }
            }
        }
        let u16_ms = ms(t0.elapsed());
        std::hint::black_box(sink);

        // 200x owned-result construction (Debug formats + string clones).
        let t0 = Instant::now();
        let mut owned = Vec::with_capacity(n);
        for diags in &all_diags {
            let mut row = Vec::with_capacity(diags.len());
            for d in diags {
                row.push(OwnedDiagnostic {
                    rule: d.rule.clone(),
                    category: d.category.clone(),
                    severity: format!("{:?}", d.severity),
                    message: d.message.clone(),
                    span_start: d.span.start,
                    span_end: d.span.end,
                    tags: d.tags.iter().map(|t| format!("{:?}", t)).collect(),
                    span_kind: format!("{:?}", d.span_kind),
                });
            }
            owned.push(row);
        }
        let o_ms = ms(t0.elapsed());
        std::hint::black_box(&owned);

        let total_diags: usize = all_diags.iter().map(|d| d.len()).sum();
        std::hint::black_box(&linters);

        host.close();
        drop(host);

        if record {
            construct.push(c_ms);
            upsert_cold.push(u_cold);
            upsert_same.push(u_same);
            analysis_cold.push(a_cold);
            analysis_warm.push(a_warm);
            linter_new.push(l_new);
            script_build.push(s_build);
            lint_run.push(l_run);
            get_source.push(g_src);
            utf16.push(u16_ms);
            owned_build.push(o_ms);
            diag_count = total_diags;
        }
    }

    let row = |label: &str, s: &Series| {
        println!(
            "  {:<28} {}   -> {:>8.1} us/file",
            label,
            s.fmt(),
            s.median() / n as f64 * 1000.0
        );
    };
    println!("── per-step, in-process, {n} files ──");
    row("host construct (1x)", &construct);
    row("upsert x200 (cold)", &upsert_cold);
    row("upsert x200 (identical)", &upsert_same);
    row("get_analysis x200 (cold)", &analysis_cold);
    row("get_analysis x200 (warm)", &analysis_warm);
    row("Linter::new x200", &linter_new);
    row("script snapshot x200", &script_build);
    row("linter.lint x200", &lint_run);
    row("get_source x200", &get_source);
    row("utf16 span rewrite x200", &utf16);
    row("owned result build x200", &owned_build);
    println!("  diagnostics produced: {diag_count}");
}

// ─────────────────────────── M3: four cells ───────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    /// no imports at all
    A,
    /// one bare `vue` import (matches fixtures/200)
    B,
    /// one resolvable relative import
    C,
    /// one MISSING relative import
    D,
}

impl Cell {
    fn label(self) -> &'static str {
        match self {
            Cell::A => "A no-imports        ",
            Cell::B => "B bare `vue`        ",
            Cell::C => "C relative resolvable",
            Cell::D => "D relative missing  ",
        }
    }
}

/// Byte-identical bodies except for the import line, so the delta is the edge.
fn cell_source(cell: Cell, i: usize) -> String {
    let import = match cell {
        Cell::A => String::new(),
        Cell::B => "import { ref } from 'vue'\n".to_string(),
        Cell::C => format!("import {{ helper }} from './helper{i}'\n"),
        Cell::D => format!("import {{ helper }} from './missing{i}'\n"),
    };
    format!(
        "<template>\n  <div class=\"c-{i}\">{{{{ message }}}}</div>\n</template>\n\
         <script setup lang=\"ts\">\n{import}const message = 'Hello {i}'\n</script>\n"
    )
}

fn helper_source(i: usize) -> String {
    format!("export const helper{i} = {i};\nexport const helper = {i};\n")
}

/// One cell run: build host, upsert N files, get_analysis each. Returns
/// (total ms, upsert ms, analysis ms, counters).
fn cell_run_memory(cell: Cell, n: usize) -> (f64, f64, f64, Counters) {
    let host = VerterHost::new_standalone(HostConfig::default());
    let mut items = Vec::with_capacity(n);
    for i in 0..n {
        items.push((format!("/probe/Comp{i}.vue"), cell_source(cell, i)));
    }
    // Helper modules for cell C must exist as host files.
    if cell == Cell::C {
        for i in 0..n {
            let _ = host.upsert(UpsertRequest {
                canonical_id: Some(format!("/probe/helper{i}.ts")),
                input_id: format!("/probe/helper{i}.ts"),
                source: Arc::from(helper_source(i).as_str()),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            });
        }
    }
    let t0 = Instant::now();
    for (id, src) in &items {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(id.clone()),
            input_id: id.clone(),
            source: Arc::from(src.as_str()),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        });
    }
    let up = ms(t0.elapsed());
    let t0 = Instant::now();
    for (id, _) in &items {
        let _ = host.get_analysis(id);
    }
    let an = ms(t0.elapsed());
    let c = counters_of(&host);
    host.close();
    (up + an, up, an, c)
}

fn cell_run_filesystem(cell: Cell, n: usize) -> (f64, f64, f64, Counters) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let mut ids = Vec::with_capacity(n);
    for i in 0..n {
        let p = root.join(format!("Comp{i}.vue"));
        std::fs::write(&p, cell_source(cell, i)).unwrap();
        ids.push(p.to_string_lossy().replace('\\', "/"));
        if cell == Cell::C {
            std::fs::write(root.join(format!("helper{i}.ts")), helper_source(i)).unwrap();
        }
    }
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions {
        roots: vec![root.to_string_lossy().to_string()],
        eager_preload: false,
    }));
    let host = VerterHost::new(HostConfig::default(), ws);
    let sources: Vec<String> = ids
        .iter()
        .map(|id| std::fs::read_to_string(id).unwrap())
        .collect();
    let t0 = Instant::now();
    for (id, src) in ids.iter().zip(sources.iter()) {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(id.clone()),
            input_id: id.clone(),
            source: Arc::from(src.as_str()),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        });
    }
    let up = ms(t0.elapsed());
    let t0 = Instant::now();
    for id in &ids {
        let _ = host.get_analysis(id);
    }
    let an = ms(t0.elapsed());
    let c = counters_of(&host);
    host.close();
    (up + an, up, an, c)
}

fn run_cells_mode() {
    println!("\n===== M3 FOUR-CELL EDGE MICRO-BENCH =====");
    println!("runs={} warmup={}", runs(), warmup());

    for backend in ["memory", "filesystem"] {
        println!("\n── backend: {backend} ──");
        println!(
            "{:<22} {:>5} {:>12} {:>12} {:>12} {:>11} {:>11}",
            "cell", "N", "total ms", "upsert ms", "analysis ms", "µs/file", "Δ vs A µs"
        );
        for n in [25usize, 100, 200] {
            let mut base_us = 0.0f64;
            for cell in [Cell::A, Cell::B, Cell::C, Cell::D] {
                let mut total = Series::default();
                let mut up = Series::default();
                let mut an = Series::default();
                let mut counters = Counters::default();
                for i in 0..(runs() + warmup()) {
                    let (t, u, a, c) = if backend == "memory" {
                        cell_run_memory(cell, n)
                    } else {
                        cell_run_filesystem(cell, n)
                    };
                    if i >= warmup() {
                        total.push(t);
                        up.push(u);
                        an.push(a);
                        counters = c;
                    }
                }
                let us_per_file = total.median() * 1000.0 / n as f64;
                if cell == Cell::A {
                    base_us = us_per_file;
                }
                println!(
                    "{:<22} {:>5} {:>12.2} {:>12.2} {:>12.2} {:>11.1} {:>11.1}",
                    cell.label(),
                    n,
                    total.median(),
                    up.median(),
                    an.median(),
                    us_per_file,
                    us_per_file - base_us
                );
                if n == 200 {
                    let _ = &counters;
                }
            }
            println!();
        }
    }
}

// ─────────────────────────── main ───────────────────────────

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    reset_probe();
    match mode.as_str() {
        "lint" => run_lint_mode(),
        "meta" => run_meta_mode(),
        "cells" => run_cells_mode(),
        "napi-shape" => run_napi_shape_mode(),
        "all" => {
            run_lint_mode();
            reset_probe();
            run_meta_mode();
            reset_probe();
            run_cells_mode();
        }
        other => {
            eprintln!("unknown mode {other}; expected lint|meta|cells|napi-shape|all");
            std::process::exit(2);
        }
    }
}
