/**
 * FFI Benchmark Suite — Measures NAPI and WASM boundary overhead.
 *
 * Run: pnpm --filter @verter/benchmark run bench:ffi
 *
 * Groups:
 *  1. FFI roundtrip overhead (no-change, resolve, getVirtualFile)
 *  2. NAPI vs WASM comparison (compile, cached read, no-change)
 *  3. Breakdown timing (FFI cost = JS total - parseDurationMs)
 */
import { Bench } from "tinybench";
import { readFileSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";
import { VerterHost, type HostUpdateResult } from "@verter/native";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const FIXTURE_FILES = [
  "tiny-template.vue",
  "simple-interactive.vue",
  "list-rendering.vue",
  "conditional-heavy.vue",
  "form-component.vue",
  "composition-heavy.vue",
  "template-heavy.vue",
  "kitchen-sink.vue",
];

interface Fixture {
  name: string;
  source: string;
  buffer: Buffer;
  size: number;
}

function loadFixtures(): Fixture[] {
  const dir = join(__dirname, "fixtures");
  return FIXTURE_FILES.map((f) => {
    const source = readFileSync(join(dir, f), "utf-8");
    return {
      name: f.replace(".vue", ""),
      source,
      buffer: Buffer.from(source),
      size: Buffer.byteLength(source),
    };
  });
}

// ---------------------------------------------------------------------------
// Printing helpers
// ---------------------------------------------------------------------------

function pad(s: string, n: number): string {
  return s.padStart(n);
}

function fmtNs(ns: number): string {
  if (ns < 1_000) return `${ns.toFixed(0)} ns`;
  if (ns < 1_000_000) return `${(ns / 1_000).toFixed(2)} µs`;
  return `${(ns / 1_000_000).toFixed(3)} ms`;
}

function printBenchResults(bench: Bench) {
  for (const task of bench.tasks) {
    const r = task.result! as any;
    const lat = r.latency || {};
    const thr = r.throughput || {};
    const meanNs = (lat.mean || 0) * 1_000_000; // ms → ns
    console.log(
      `    ${task.name.padEnd(45)} ${pad(fmtNs(meanNs), 12)}  (${pad((thr.mean || 0).toFixed(0), 8)} ops/s)`,
    );
  }
}

function printHeader(title: string) {
  console.log("");
  console.log("━".repeat(78));
  console.log(`  ${title}`);
  console.log("━".repeat(78));
}

function printSubHeader(title: string) {
  console.log(`\n── ${title} ──`);
}

// ---------------------------------------------------------------------------
// Group 1: NAPI FFI Roundtrip Overhead
// ---------------------------------------------------------------------------

async function benchNapiFfiOverhead(fixtures: Fixture[]) {
  printHeader("GROUP 1 — NAPI FFI ROUNDTRIP OVERHEAD");

  for (const fixture of fixtures) {
    printSubHeader(`${fixture.name} (${(fixture.size / 1024).toFixed(2)} KB)`);

    const host = new VerterHost({ analysisLevel: "none" });

    // Prime: first upsert does full compile
    host.upsert({ inputId: `${fixture.name}.vue`, source: fixture.buffer });

    const bench = new Bench({ time: 2000, warmupIterations: 30 });

    // No-change re-upsert (hash match, skip compile)
    bench.add("napi:upsert:no-change (buffer)", () => {
      host.upsert({ inputId: `${fixture.name}.vue`, source: fixture.buffer });
    });

    // String source (JS wrapper converts to Buffer)
    bench.add("napi:upsert:no-change (string)", () => {
      host.upsert({
        inputId: `${fixture.name}.vue`,
        source: fixture.source as any,
      });
    });

    // Cached getVirtualFile (compiled result already cached)
    bench.add("napi:getVirtualFile (cached)", () => {
      host.getVirtualFile({
        canonicalId: `${fixture.name}.vue`,
        nodeKind: { kind: "main" },
      });
    });

    // Resolve (minimal work)
    bench.add("napi:resolve", () => {
      host.resolve(`${fixture.name}.vue`);
    });

    await bench.run();
    printBenchResults(bench);
  }
}

// ---------------------------------------------------------------------------
// Group 2: NAPI vs WASM Comparison
// ---------------------------------------------------------------------------

async function benchNapiVsWasm(fixtures: Fixture[]) {
  printHeader("GROUP 2 — NAPI vs WASM COMPARISON");

  // Try loading WASM
  let WasmHost: any;
  try {
    const wasm = await import("@verter/wasm");
    await wasm.initialize();
    WasmHost = wasm.Host;
    console.log("  WASM loaded successfully");
  } catch (e) {
    console.log(`  WASM not available (${e instanceof Error ? e.message : e}), skipping WASM benchmarks`);
    return;
  }

  for (const fixture of fixtures) {
    printSubHeader(`${fixture.name} (${(fixture.size / 1024).toFixed(2)} KB)`);

    // NAPI host
    const napiHost = new VerterHost({ analysisLevel: "none" });

    // WASM host
    const wasmHost = new WasmHost({ analysisLevel: "none" });

    const bench = new Bench({ time: 2000, warmupIterations: 10 });
    const fname = `${fixture.name}.vue`;

    // Full compile: NAPI
    bench.add("napi:upsert:compile", () => {
      napiHost.remove(fname);
      napiHost.upsert({ inputId: fname, source: fixture.buffer });
    });

    // Full compile: WASM
    bench.add("wasm:upsert:compile", () => {
      wasmHost.remove(fname);
      wasmHost.upsert({ inputId: fname, source: fixture.source });
    });

    // Prime both for cached operations
    napiHost.upsert({ inputId: fname, source: fixture.buffer });
    wasmHost.upsert({ inputId: fname, source: fixture.source });

    // No-change: NAPI
    bench.add("napi:upsert:no-change", () => {
      napiHost.upsert({ inputId: fname, source: fixture.buffer });
    });

    // No-change: WASM
    bench.add("wasm:upsert:no-change", () => {
      wasmHost.upsert({ inputId: fname, source: fixture.source });
    });

    // Cached getVirtualFile: NAPI
    bench.add("napi:getVirtualFile", () => {
      napiHost.getVirtualFile({
        canonicalId: fname,
        nodeKind: { kind: "main" },
      });
    });

    // Cached getVirtualFile: WASM
    bench.add("wasm:getVirtualFile", () => {
      wasmHost.getVirtualFile({
        canonicalId: fname,
        nodeKind: { kind: "main" },
      });
    });

    await bench.run();
    printBenchResults(bench);
  }
}

// ---------------------------------------------------------------------------
// Group 3: Breakdown Timing
// ---------------------------------------------------------------------------

async function benchBreakdown(fixtures: Fixture[]) {
  printHeader("GROUP 3 — COMPILE BREAKDOWN (NAPI)");
  console.log(
    "  Breakdown: JS total vs Rust parseDurationMs → FFI overhead = total - parse",
  );

  const ITERATIONS = 200;

  for (const fixture of fixtures) {
    printSubHeader(`${fixture.name} (${(fixture.size / 1024).toFixed(2)} KB)`);

    const host = new VerterHost({ analysisLevel: "none" });
    const fname = `${fixture.name}.vue`;

    // Warmup
    for (let i = 0; i < 5; i++) {
      host.remove(fname);
      host.upsert({ inputId: fname, source: fixture.buffer });
    }

    // Measure full compile cycle
    let totalUpsertMs = 0;
    let totalGetVirtualMs = 0;
    let totalParseDurationMs = 0;

    for (let i = 0; i < ITERATIONS; i++) {
      host.remove(fname);

      const t0 = performance.now();
      const result: HostUpdateResult = host.upsert({
        inputId: fname,
        source: fixture.buffer,
      });
      const t1 = performance.now();

      host.getVirtualFile({
        canonicalId: fname,
        nodeKind: { kind: "main" },
      });
      const t2 = performance.now();

      totalUpsertMs += t1 - t0;
      totalGetVirtualMs += t2 - t1;
      totalParseDurationMs += result.parseDurationMs;
    }

    const avgUpsert = totalUpsertMs / ITERATIONS;
    const avgGetVirtual = totalGetVirtualMs / ITERATIONS;
    const avgParseDuration = totalParseDurationMs / ITERATIONS;
    const ffiOverhead = avgUpsert - avgParseDuration;
    const total = avgUpsert + avgGetVirtual;

    console.log(`    Total cycle:          ${pad((total * 1000).toFixed(0), 8)} µs`);
    console.log(`      upsert (JS):        ${pad((avgUpsert * 1000).toFixed(0), 8)} µs`);
    console.log(`        parse (Rust):     ${pad((avgParseDuration * 1000).toFixed(0), 8)} µs  (${((avgParseDuration / avgUpsert) * 100).toFixed(0)}% of upsert)`);
    console.log(`        FFI overhead:     ${pad((ffiOverhead * 1000).toFixed(0), 8)} µs  (${((ffiOverhead / avgUpsert) * 100).toFixed(0)}% of upsert)`);
    console.log(`      getVirtualFile:     ${pad((avgGetVirtual * 1000).toFixed(0), 8)} µs`);
  }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  console.log("━".repeat(78));
  console.log("  VERTER FFI BENCHMARK SUITE");
  console.log("  Measuring NAPI/WASM boundary overhead");
  console.log("━".repeat(78));

  const fixtures = loadFixtures();
  console.log(`\nLoaded ${fixtures.length} fixtures`);

  await benchNapiFfiOverhead(fixtures);
  await benchNapiVsWasm(fixtures);
  await benchBreakdown(fixtures);

  console.log("\n" + "━".repeat(78));
  console.log("  DONE");
  console.log("━".repeat(78));

  process.exit(0);
}

main().catch((error) => {
  console.error("Benchmark failed:", error);
  process.exit(1);
});
