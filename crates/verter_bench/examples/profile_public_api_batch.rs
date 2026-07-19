//! Public-API batch scaling profiler.
//!
//! Builds a deterministic macro corpus (N cross-file SFCs, each importing a
//! shared `./types`) and calls `host.get_public_api_batch(ids)` ONCE, printing
//! the per-call average µs. Expectation: ~constant per-call as N scales — the
//! verter-tsc public-API stub-gen O(N²) per-call store-view cliff is collapsed
//! onto ONE per-batch fixed view. This is scaling EVIDENCE, not a hard gate.
//!
//! Usage:
//!   cargo run -p verter_bench --example profile_public_api_batch --release
//!
//! Environment variables:
//!   VERTER_PUBLIC_API_BATCH_N — number of SFCs (default 500; the campaign
//!   sweeps 500 / 1000 / 2000 / 4000). Do NOT run 14k in the gate.

use std::sync::Arc;
use std::time::Instant;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

const TYPES_TS: &str = "export interface ButtonProps { label: string; size?: 'sm' | 'md' }\n\
                        export interface ButtonEmits { (e: 'click', payload: number): void }\n";

/// Cross-file owner SFC: `defineProps`/`defineEmits` take IMPORTED type
/// arguments, so each render walks the import graph — the macro-deps path that
/// took the per-call store-view read in the pre-fix scalar loop.
fn owner_sfc(idx: usize) -> String {
    format!(
        "<script setup lang=\"ts\">\n\
         import type {{ ButtonProps, ButtonEmits }} from './types'\n\
         defineProps<ButtonProps>()\n\
         defineEmits<ButtonEmits>()\n\
         const local_{idx} = {idx}\n\
         </script>\n\
         <template><button>{{{{ local_{idx} }}}}</button></template>\n"
    )
}

fn upsert(host: &VerterHost, id: &str, source: &str, lang: FileLanguage) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(id.to_string()),
        input_id: id.to_string(),
        source: Arc::from(source),
        file_language: lang,
        aliases: Vec::new(),
    });
}

fn main() {
    let n: usize = std::env::var("VERTER_PUBLIC_API_BATCH_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);

    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(&host, "/src/types.ts", TYPES_TS, FileLanguage::script_ts());
    let ids: Vec<String> = (0..n)
        .map(|i| {
            let id = format!("/src/Comp{i}.vue");
            upsert(&host, &id, &owner_sfc(i), FileLanguage::vue());
            id
        })
        .collect();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();

    eprintln!("Public-API batch scaling profiler");
    eprintln!("  N = {n}");

    let start = Instant::now();
    let responses = host.get_public_api_batch(&refs);
    let elapsed = start.elapsed();

    let resolved = responses
        .iter()
        .map(|response| response.as_ref().expect("public API projection"))
        .filter(|response| response.is_some())
        .count();
    let per_call_us = elapsed.as_secs_f64() * 1e6 / (n as f64);
    eprintln!("  Resolved:     {resolved}/{n}");
    eprintln!("  Total:        {elapsed:?}");
    eprintln!("  Per-call avg: {per_call_us:.2} µs");

    // Anti-vacuity: a corpus that silently fails to resolve would make the
    // per-call number meaningless. Every macro-bearing SFC must resolve.
    assert_eq!(
        resolved, n,
        "every SFC in the macro corpus must resolve its cross-file public-API surface",
    );
}
