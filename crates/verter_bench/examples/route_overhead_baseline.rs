//! B5-direct calibration harness for `B6_COMPILER_ROUTE_OVERHEAD`.
//!
//! Eight in-process sources (four Vue, four Svelte) compiled through
//! `StandaloneCompiler::compile` as `RuntimeClient`, maps off. This is the
//! B5 one-shot leg. Prepared-first / prepared-repeat / batch arms are part
//! of the locked cell and are refused here — those routes do not exist on
//! the B5 tree.
//!
//! ```text
//! cargo build -p verter_bench --release --example route_overhead_baseline
//! ./target/release/examples/route_overhead_baseline --runs 1
//! ```

use std::hint::black_box;
use std::time::Instant;

use sha2::{Digest, Sha256};
use verter_compiler::compile::types::{VueExecutionInputs, VueMacroSemanticInput};
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, RuntimeProductRequest,
    SvelteCompileRequest, VueCompileRequest,
};
use verter_compiler::standalone::{
    DirectCompileOutput, DirectExecutionInputs, StandaloneCompiler, SvelteExecutionInputs,
};

struct CorpusItem {
    id: &'static str,
    filename: &'static str,
    source: &'static str,
    vue: bool,
}

const CORPUS: &[CorpusItem] = &[
    CorpusItem {
        id: "vue-simple",
        filename: "vue-simple.vue",
        source: "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>\n",
        vue: true,
    },
    CorpusItem {
        id: "vue-styled",
        filename: "vue-styled.vue",
        source: "<script setup>\nconst msg = 'hi'\n</script>\n<template><div class=\"x\">{{ msg }}</div></template>\n<style>\n.x { color: red; }\n</style>\n",
        vue: true,
    },
    CorpusItem {
        id: "vue-list",
        filename: "vue-list.vue",
        source: "<script setup>\nconst items = [1, 2, 3]\nfunction onPick(n) { console.log(n) }\n</script>\n<template>\n  <ul>\n    <li v-for=\"n in items\" :key=\"n\" @click=\"onPick(n)\">{{ n }}</li>\n  </ul>\n  <p v-if=\"items.length\">count {{ items.length }}</p>\n</template>\n",
        vue: true,
    },
    CorpusItem {
        id: "vue-computed",
        filename: "vue-computed.vue",
        source: "<script setup>\nimport { ref, computed } from 'vue'\nconst n = ref(1)\nconst doubled = computed(() => n.value * 2)\n</script>\n<template>\n  <button @click=\"n++\">{{ n }} / {{ doubled }}</button>\n</template>\n",
        vue: true,
    },
    CorpusItem {
        id: "svelte-simple",
        filename: "svelte-simple.svelte",
        source: "<script>\n  let count = $state(0);\n</script>\n<button onclick={() => count++}>{count}</button>\n",
        vue: false,
    },
    CorpusItem {
        id: "svelte-styled",
        filename: "svelte-styled.svelte",
        source: "<script>\n  let count = $state(0);\n</script>\n<button onclick={() => count++}>{count}</button>\n<style>\n  button { color: red; }\n</style>\n",
        vue: false,
    },
    CorpusItem {
        id: "svelte-each",
        filename: "svelte-each.svelte",
        source: "<script>\n  let items = $state([1, 2, 3]);\n</script>\n<ul>\n  {#each items as n (n)}\n    <li>{n}</li>\n  {/each}\n</ul>\n",
        vue: false,
    },
    CorpusItem {
        id: "svelte-if",
        filename: "svelte-if.svelte",
        source: "<script>\n  let on = $state(true);\n</script>\n{#if on}\n  <p>yes</p>\n{:else}\n  <p>no</p>\n{/if}\n",
        vue: false,
    },
];

struct PassResult {
    compile_calls: u64,
    artifact_count: u64,
    payload_bytes: u64,
    digest_hex: String,
}

fn request_for(item: &CorpusItem) -> CompileRequest {
    let product = CompileProduct::RuntimeClient(RuntimeProductRequest::default());
    if item.vue {
        CompileRequest::new(
            vec![product],
            FrameworkCompileRequest::Vue(VueCompileRequest::default()),
            None,
            Some(item.filename.to_string()),
            None,
            false,
            false,
        )
        .expect("vue RuntimeClient request constructs")
    } else {
        CompileRequest::new(
            vec![product],
            FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
            None,
            Some(item.filename.to_string()),
            None,
            false,
            false,
        )
        .expect("svelte RuntimeClient request constructs")
    }
}

fn compile_item(item: &CorpusItem) -> DirectCompileOutput {
    let request = request_for(item);
    let vue_exec = VueExecutionInputs::default();
    let vue_macros = VueMacroSemanticInput::Unavailable;
    let svelte_exec = SvelteExecutionInputs {
        css_hash_override: None,
        prepared_styles: Vec::new(),
    };
    let inputs = if item.vue {
        DirectExecutionInputs::Vue {
            execution: &vue_exec,
            macros: &vue_macros,
        }
    } else {
        DirectExecutionInputs::Svelte {
            execution: &svelte_exec,
        }
    };
    StandaloneCompiler
        .compile(item.source, &request, inputs)
        .unwrap_or_else(|err| panic!("{}: {err:?}", item.id))
}

fn run_pass() -> PassResult {
    let mut hasher = Sha256::new();
    let mut compile_calls = 0u64;
    let mut artifact_count = 0u64;
    let mut payload_bytes = 0u64;
    for item in CORPUS {
        let output = compile_item(item);
        compile_calls += 1;
        let artifacts = output.artifacts.artifacts();
        assert_eq!(
            artifacts.len(),
            1,
            "{} must publish exactly one RuntimeClient artifact",
            item.id
        );
        artifact_count += artifacts.len() as u64;
        let code = artifacts[0].code();
        payload_bytes += code.len() as u64;
        hasher.update(item.id.as_bytes());
        hasher.update([0]);
        hasher.update(code.as_bytes());
        hasher.update([0]);
        hasher.update(output.styles.len().to_string().as_bytes());
        hasher.update(*b"\n");
        black_box(code);
        black_box(&output.styles);
    }
    PassResult {
        compile_calls,
        artifact_count,
        payload_bytes,
        digest_hex: hex_of(&hasher.finalize()),
    }
}

fn hex_of(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn parse_args() -> (u32, u32, String) {
    let mut runs = 1u32;
    let mut warmup = 1u32;
    let mut arm = "direct".to_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--runs" => {
                i += 1;
                runs = args
                    .get(i)
                    .expect("--runs needs a value")
                    .parse()
                    .expect("--runs must be a u32");
            }
            "--warmup" => {
                i += 1;
                warmup = args
                    .get(i)
                    .expect("--warmup needs a value")
                    .parse()
                    .expect("--warmup must be a u32");
            }
            "--arm" => {
                i += 1;
                arm = args.get(i).expect("--arm needs a value").clone();
            }
            "--help" | "-h" => {
                eprintln!(
                    "route_overhead_baseline --arm direct --runs 1 --warmup 1\n\
                     prepared-first / prepared-repeat / batch are refused on this tree"
                );
                std::process::exit(0);
            }
            other => panic!("unknown argument: {other}"),
        }
        i += 1;
    }
    (runs, warmup, arm)
}

fn median_ns(samples: &mut [u128]) -> u128 {
    samples.sort_unstable();
    let n = samples.len();
    if n == 0 {
        panic!("no samples");
    }
    if n % 2 == 1 {
        samples[n / 2]
    } else {
        (samples[n / 2 - 1] + samples[n / 2]) / 2
    }
}

fn main() {
    let (runs, warmup, arm) = parse_args();
    match arm.as_str() {
        "direct" => {}
        "prepared-first" | "prepared-repeat" | "batch" => {
            eprintln!(
                "arm {arm} is part of B6_COMPILER_ROUTE_OVERHEAD but is not \
                 present on the B5 tree; refuse rather than invent a stand-in"
            );
            std::process::exit(2);
        }
        other => panic!("unknown arm: {other}"),
    }
    assert!(runs >= 1, "--runs must be >= 1");

    for _ in 0..warmup {
        let warm = run_pass();
        black_box(&warm.digest_hex);
    }

    let mut samples = Vec::with_capacity(runs as usize);
    let mut digest = None;
    let mut compile_calls = 0;
    let mut artifact_count = 0;
    let mut payload_bytes = 0;
    for _ in 0..runs {
        let started = Instant::now();
        let pass = run_pass();
        let elapsed = started.elapsed().as_nanos();
        samples.push(elapsed);
        match &digest {
            None => digest = Some(pass.digest_hex.clone()),
            Some(prev) => assert_eq!(
                prev, &pass.digest_hex,
                "output digest moved between samples"
            ),
        }
        compile_calls = pass.compile_calls;
        artifact_count = pass.artifact_count;
        payload_bytes = pass.payload_bytes;
        assert!(payload_bytes > 0, "RuntimeClient payload must be non-empty");
    }

    println!("sample\twall_ns");
    for (i, ns) in samples.iter().enumerate() {
        println!("{}\t{ns}", i + 1);
    }

    let mut sorted = samples.clone();
    let median = median_ns(&mut sorted);
    let min = *samples.iter().min().expect("samples");
    let max = *samples.iter().max().expect("samples");
    let mean = samples.iter().sum::<u128>() as f64 / samples.len() as f64;
    let var = samples
        .iter()
        .map(|s| {
            let d = *s as f64 - mean;
            d * d
        })
        .sum::<f64>()
        / samples.len() as f64;
    let cv = if mean == 0.0 {
        0.0
    } else {
        100.0 * var.sqrt() / mean
    };

    println!("SUMMARY\tarm=direct");
    println!("SUMMARY\truns={runs}");
    println!("SUMMARY\twarmup={warmup}");
    println!("SUMMARY\tmedian_wall_ns={median}");
    println!("SUMMARY\tmin_wall_ns={min}");
    println!("SUMMARY\tmax_wall_ns={max}");
    println!("SUMMARY\tmean_wall_ns={mean:.0}");
    println!("SUMMARY\tcv_percent={cv:.4}");
    println!("SUMMARY\tcompile_calls={compile_calls}");
    println!("SUMMARY\tartifact_count={artifact_count}");
    println!("SUMMARY\tpayload_bytes={payload_bytes}");
    println!(
        "SUMMARY\toutput_digest={}",
        digest.expect("digest recorded")
    );
    println!("SUMMARY\tcorpus_len={}", CORPUS.len());
}
