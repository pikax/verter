//! Work-attribution baseline harness.
//!
//! Fixed in-process corpus (no fixture checkout) through the host;
//! reports `verter_audit::attribution` by logical identity.
//!
//! ```text
//! cargo run -p verter_bench --release --example attribution_baseline
//! cargo run -p verter_bench --release --features attribution \
//!     --example attribution_baseline -- --format tsv
//! ```
//!
//! `--format tsv|json|summary`, `--files N`, `--runs N`.
//!
//! Feature off: timings only (no counter table). Feature on also
//! installs `AttributingAllocator`, so disabled-overhead is measured
//! against the pre-instrumentation tree, not against the enabled arm.

use std::sync::Arc;
use std::time::Instant;

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::host_compile::{CompileBatchInput, CompileBatchOptions, CompileManyTarget};
use verter_session::HostConfig;
use verter_workspace::{MemoryOptions, MemoryWorkspace};

/// Heap attribution needs the wrapper installed as THE global allocator,
/// which only a final binary can do. Installed on the measurement arm only,
/// so the disabled arm links the plain system allocator.
#[cfg(feature = "attribution")]
#[global_allocator]
static ALLOC: verter_audit::attribution::AttributingAllocator<std::alloc::System> =
    verter_audit::attribution::AttributingAllocator::new(std::alloc::System);

// corpus

struct SourceFile {
    id: String,
    source: String,
}

/// A shared types module every component imports from, so the corpus
/// exercises cross-file resolution rather than N isolated files.
fn shared_types_module() -> SourceFile {
    SourceFile {
        id: "/bench/types.ts".to_string(),
        // language inferred from the extension by the workspace registry
        source: r#"
export interface Identity {
  id: string;
  label: string;
  createdAt: number;
}

export interface Sizing {
  size?: 'sm' | 'md' | 'lg';
  fullWidth?: boolean;
}

export type Row<T> = {
  value: T;
  identity: Identity;
  meta: Record<string, string>;
};

export type RowKeys = keyof Row<number>;

export interface PanelProps extends Identity, Sizing {
  rows: Row<string>[];
  selected?: Row<string>;
  disabled?: boolean;
}

export type PanelEvents = {
  select: [row: Row<string>];
  dismiss: [];
};
"#
        .to_string(),
    }
}

/// A component that imports the shared types, declares props/emits through
/// macros, and carries a template plus a scoped style block — so one file
/// touches parsing, preparation, resolution, projection, codegen and CSS.
fn component(index: usize) -> SourceFile {
    let source = format!(
        r#"<script setup lang="ts">
import type {{ PanelProps, PanelEvents, Row }} from '/bench/types.ts';

const props = withDefaults(defineProps<PanelProps>(), {{
  size: 'md',
  disabled: false,
}});

const emit = defineEmits<PanelEvents>();

defineSlots<{{
  header(props: {{ title: string }}): unknown;
  row(props: {{ row: Row<string>; index: number }}): unknown;
}}>();

function pick(row: Row<string>) {{
  emit('select', row);
}}

const heading = props.label + ' #{index}';
</script>

<template>
  <section class="panel" :class="{{ wide: props.fullWidth }}">
    <slot name="header" :title="heading" />
    <ul v-if="!props.disabled">
      <li v-for="(row, i) in props.rows" :key="row.identity.id" @click="pick(row)">
        <slot name="row" :row="row" :index="i">{{{{ row.identity.label }}}}</slot>
      </li>
    </ul>
    <button v-else @click="emit('dismiss')">dismiss</button>
  </section>
</template>

<style scoped>
.panel {{ display: flex; flex-direction: column; gap: 4px; }}
.panel.wide {{ width: 100%; }}
.panel li:hover {{ background: #eee; }}
</style>
"#
    );
    SourceFile {
        id: format!("/bench/Panel{index}.vue"),
        source,
    }
}

fn build_corpus(files: usize) -> Vec<SourceFile> {
    let mut corpus = Vec::with_capacity(files + 1);
    corpus.push(shared_types_module());
    for index in 0..files {
        corpus.push(component(index));
    }
    corpus
}

// workload

/// One full pass: fresh host, upsert the corpus, then request component
/// metadata for every component. Returns the wall-clock in milliseconds and
/// the number of components that produced metadata.
fn run_once(corpus: &[SourceFile]) -> (f64, usize) {
    let started = Instant::now();

    let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for file in corpus {
        ws.inject_file(file.id.clone(), Arc::from(file.source.as_str()));
    }

    let meta_host = ComponentMetaHost::new(HostConfig::default(), ws);
    let session = meta_host
        .open_session()
        .expect("opening a component-meta session must succeed");

    for file in corpus {
        let _ = meta_host.ensure_loaded(&file.id);
    }

    let mut resolved = 0usize;
    for file in corpus {
        if !file.id.ends_with(".vue") {
            continue;
        }
        if let Ok(Some(_)) = session.get_component_meta(&file.id) {
            resolved += 1;
        }
    }

    // Compile pass: component-meta never reaches codegen, so without this
    // the rendering / mapping / css / compiled-output domains read zero and
    // the baseline would understate the pipeline.
    let inputs: Vec<CompileBatchInput> = corpus
        .iter()
        .filter(|file| file.id.ends_with(".vue"))
        .map(|file| CompileBatchInput {
            canonical_id: file.id.clone(),
            source: Arc::from(file.source.as_str()),
            requested_mode: None,
            component_id: None,
        })
        .collect();
    let _ = meta_host.host().compile_many(
        inputs,
        CompileBatchOptions::default(),
        CompileManyTarget::HostBacked,
    );

    drop(session);
    meta_host.shutdown();

    (started.elapsed().as_secs_f64() * 1000.0, resolved)
}

// reporting

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

#[cfg(feature = "attribution")]
fn emit_dataset(format: &str) {
    use verter_audit::attribution::{report, snapshot};

    match format {
        "tsv" => print!("{}", report::render_tsv()),
        "json" => print!("{}", report::render_json()),
        _ => {
            let rows = snapshot();
            let totals = report::domain_totals(&rows);
            println!("\n── work by domain ──");
            println!(
                "{:<16}{:>7}{:>14}{:>12}{:>14}{:>14}",
                "domain", "sites", "calls", "ms", "allocs", "net KiB"
            );
            for total in &totals {
                println!(
                    "{:<16}{:>7}{:>14}{:>12.1}{:>14}{:>14}",
                    total.domain.id(),
                    total.sites,
                    total.calls,
                    total.nanos as f64 / 1e6,
                    total.alloc_count,
                    total.net_bytes / 1024,
                );
            }
            println!("\n── top sites by calls ──");
            let mut by_calls = rows.clone();
            by_calls.sort_by(|a, b| b.calls.cmp(&a.calls));
            println!("{:<44}{:>12}{:>14}{:>12}", "site", "calls", "amount", "ms");
            for row in by_calls.iter().take(25) {
                println!(
                    "{:<44}{:>12}{:>14}{:>12.1}",
                    row.id(),
                    row.calls,
                    row.amount,
                    row.nanos as f64 / 1e6,
                );
            }
        }
    }
}

#[cfg(not(feature = "attribution"))]
fn emit_dataset(_format: &str) {
    println!(
        "\nattribution feature is OFF — no counter table exists in this build.\n\
         This arm measures the disabled-instrumentation cost; rebuild with\n\
         `--features attribution` for the dataset."
    );
}

/// Run the workload twice under the same process and compare the determinism
/// digests. Only meaningful with the feature on; without it there is nothing
/// to read, so the check reports as not-applicable.
#[cfg(feature = "attribution")]
fn determinism_check(corpus: &[SourceFile]) {
    use verter_audit::attribution::{reset, snapshot, WorkSite};

    // Digest plus the call count that produced it. A site the workload
    // never reached reports zero calls, and comparing its zero digest
    // against another zero digest agrees unconditionally — so the call
    // count is what separates a real agreement from a vacuous one.
    let sample_of = |site: WorkSite| {
        snapshot()
            .iter()
            .find(|row| row.site == site)
            .map_or((0, 0), |row| (row.digest, row.calls))
    };

    reset();
    let _ = run_once(corpus);
    let first_meta = sample_of(WorkSite::ComponentMetaDigest);
    let first_output = sample_of(WorkSite::CompiledOutputDigest);

    reset();
    let _ = run_once(corpus);
    let second_meta = sample_of(WorkSite::ComponentMetaDigest);
    let second_output = sample_of(WorkSite::CompiledOutputDigest);

    // A site with no recorded observation on either run proves nothing:
    // 0 == 0 holds whatever the pipeline did, so report it as N/A.
    let verdict = |first: (u64, u64), second: (u64, u64)| {
        if first.1 == 0 && second.1 == 0 {
            "N/A (no observations)"
        } else if first.0 == second.0 {
            "AGREE"
        } else {
            "DIVERGED"
        }
    };

    println!("\n── determinism ──");
    println!(
        "component_meta   run1={:>20}  run2={:>20}  {}",
        first_meta.0,
        second_meta.0,
        verdict(first_meta, second_meta)
    );
    println!(
        "compiled_output  run1={:>20}  run2={:>20}  {}",
        first_output.0,
        second_output.0,
        verdict(first_output, second_output)
    );
}

#[cfg(not(feature = "attribution"))]
fn determinism_check(_corpus: &[SourceFile]) {}

// entry

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str, fallback: usize| -> usize {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(fallback)
    };
    let format = args
        .iter()
        .position(|a| a == "--format")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "summary".to_string());

    let files = flag("--files", 40);
    let runs = flag("--runs", 3).max(1);
    let corpus = build_corpus(files);

    let attribution_enabled = cfg!(feature = "attribution");
    eprintln!(
        "corpus: {} files ({} components + 1 shared module), runs: {}, attribution: {}",
        corpus.len(),
        files,
        runs,
        if attribution_enabled { "ON" } else { "OFF" }
    );

    // Warm the process (allocator, lazy statics) before measuring.
    let (_, resolved) = run_once(&corpus);
    eprintln!("warmup resolved {resolved}/{files} components");

    let mut walls = Vec::with_capacity(runs);
    for _ in 0..runs {
        #[cfg(feature = "attribution")]
        verter_audit::attribution::reset();
        let (ms, _) = run_once(&corpus);
        walls.push(ms);
    }

    let best = walls.iter().cloned().fold(f64::INFINITY, f64::min);
    let mid = median(&mut walls.clone());
    println!("wall_median_ms\t{mid:.2}");
    println!("wall_min_ms\t{best:.2}");
    println!("files\t{files}");
    println!("runs\t{runs}");
    println!("attribution\t{}", if attribution_enabled { 1 } else { 0 });

    // The dataset reflects the LAST measured run only (each run resets).
    emit_dataset(&format);
    determinism_check(&corpus);
}
