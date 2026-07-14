/**
 * OFFLINE Verter-vs-vize benchmark — BOTH axes (manager-run, NOT CI).
 *
 * This is an OPT-IN, env-gated (`VIZE_PATH`) informational comparison the
 * manager runs and reports to the CTO. It is NEVER a `cargo`/CI test and is
 * never imported by the gate — CI is 100% self-referential and vize-free. The
 * comparison runs on the COMMITTED HERMETIC Verter corpus (the synthetic-15k
 * corpus, manifest-verified) so a corpus change can never silently move the
 * number.
 *
 * Two axes:
 *  - AXIS A (compiler throughput) — the legitimate apples-to-apples number:
 *    Verter's NATIVE compiler (`compileMany`) vs vize `compileSfcBatch`,
 *    per-fixture + aggregate codegen throughput (files/sec) + the Verter/vize
 *    ratio.
 *  - AXIS B (typecheck — INFORMATIONAL ONLY): Verter+tsgo carrier typecheck vs
 *    vize `typeCheckBatch`. This is NOT apples-to-apples and is reported with
 *    the caveat explicit: vize's typeCheck is AST-based shallow analysis
 *    (getTypeCheckCapabilities mode 'ast-based'), NOT a real TS typecheck.
 *
 * Usage:
 *   VIZE_PATH=/path/to/vize node --import tsx src/perf/vize-bench.ts
 *     [--smoke] [--axis a|b|both] [--out <file>]
 */
import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { ensureCorpus, type EnsuredCorpus } from "./corpus.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const VERTER_ROOT = resolve(__dirname, "..", "..", "..", "..");
const IS_WIN = process.platform === "win32";
const EXE = IS_WIN ? ".exe" : "";
const require = createRequire(import.meta.url);

interface VizeNative {
  compileSfc: (src: string, opts: { filename: string }) => { code: string };
  compileSfcBatch: (
    pattern: string,
    opts?: { threads?: number },
  ) => { success: number; failed: number; timeMs: number; inputBytes: number; outputBytes: number };
  typeCheckBatch?: (
    pattern: string,
    opts?: { threads?: number },
  ) => { timeMs: number; [k: string]: unknown };
  getTypeCheckCapabilities?: () => { mode: string; description: string; notes?: string[] };
}

function loadVize(): { vize: VizeNative; vizePath: string } {
  const vizePath = process.env.VIZE_PATH;
  if (!vizePath) {
    console.error(
      "VIZE_PATH is not set. This offline bench is OPT-IN and env-gated.\n" +
        "  Set VIZE_PATH to the vize repo root (e.g. VIZE_PATH=/path/to/vize) and re-run.\n" +
        "  vize is an OFFLINE-INFORMATIONAL reference only — never a CI requirement.",
    );
    process.exit(2);
  }
  const nativePath = join(vizePath, "npm", "vize-native", "index.js");
  if (!existsSync(nativePath)) {
    console.error(`vize native binding not found at ${nativePath}`);
    process.exit(2);
  }
  const vize = require(nativePath) as VizeNative;
  return { vize, vizePath };
}

interface BenchOptions {
  smoke: boolean;
  axis: "a" | "b" | "both";
  out?: string;
}
function parseArgs(argv: string[]): BenchOptions {
  const o: BenchOptions = { smoke: false, axis: "both" };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--smoke") o.smoke = true;
    else if (a === "--axis") o.axis = argv[++i] as "a" | "b" | "both";
    else if (a === "--out") o.out = argv[++i];
  }
  return o;
}

function collectVue(dir: string): string[] {
  const out: string[] = [];
  const walk = (d: string) => {
    for (const e of readdirSync(d, { withFileTypes: true, encoding: "utf-8" })) {
      if (e.name.startsWith(".")) continue;
      const p = join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.name.endsWith(".vue")) out.push(p);
    }
  };
  walk(dir);
  out.sort();
  return out;
}

// ── AXIS A — compiler throughput (apples-to-apples) ────────────────────────
interface AxisAResult {
  fileCount: number;
  totalBytes: number;
  verter: { timeMs: number; filesPerSec: number };
  vize: { timeMs: number; filesPerSec: number; success: number; failed: number };
  /** Verter/vize throughput ratio (>1 ⇒ Verter faster). */
  throughputRatio: number;
}

function runAxisA(corpus: EnsuredCorpus, vize: VizeNative, threads: number): AxisAResult {
  const files = collectVue(corpus.dir);
  const sources = files.map((f) => ({ canonicalId: f, source: readFileSync(f) }));
  const totalBytes = sources.reduce((s, f) => s + f.source.length, 0);

  // Verter native compiler via compileMany (the host-backed batch path).
  const { VerterHost } = require(join(VERTER_ROOT, "packages", "native", "dist", "index.js"));
  const host = new VerterHost({ devMode: false, analysisLevel: "none", hostCpuThreads: threads });
  const vt0 = performance.now();
  host.compileMany(sources, { priority: "interactive", defaultMode: "session" });
  const verterMs = performance.now() - vt0;

  // vize compileSfcBatch over the same corpus (glob; reads from disk).
  const glob = join(corpus.dir, "**", "*.vue").replace(/\\/g, "/");
  const vz = vize.compileSfcBatch(glob, { threads });

  return {
    fileCount: files.length,
    totalBytes,
    verter: { timeMs: verterMs, filesPerSec: (files.length / verterMs) * 1000 },
    vize: {
      timeMs: vz.timeMs,
      filesPerSec: (vz.success / vz.timeMs) * 1000,
      success: vz.success,
      failed: vz.failed,
    },
    throughputRatio: vz.timeMs > 0 ? vz.timeMs / verterMs : 0,
  };
}

// ── AXIS B — typecheck (INFORMATIONAL; not apples-to-apples) ───────────────
interface AxisBResult {
  caveat: string;
  vizeMode: string;
  verterTsc: { timeMs: number; diagnostics: number } | { skipped: string };
  vize: { timeMs: number } | { skipped: string };
}

function runAxisB(corpus: EnsuredCorpus, vize: VizeNative, threads: number): AxisBResult {
  const caps = vize.getTypeCheckCapabilities?.();
  const caveat =
    "INFORMATIONAL ONLY — vize typeCheck is AST-based shallow analysis " +
    `(getTypeCheckCapabilities mode '${caps?.mode ?? "ast-based"}': ` +
    `${caps?.description ?? "AST-based type analysis (no TypeScript compiler required)"}), ` +
    "NOT a real TS typecheck. Verter+tsgo runs a real whole-project carrier typecheck. " +
    "This is NOT apples-to-apples; reported because the number was requested.";

  // Verter+tsgo carrier typecheck via verter-tsc (the TSC-surface binary).
  const verterTscBin = (() => {
    for (const p of [
      join(VERTER_ROOT, "target", "release", `verter-tsc${EXE}`),
      join(VERTER_ROOT, "target", "debug", `verter-tsc${EXE}`),
    ]) {
      if (existsSync(p)) return p;
    }
    return null;
  })();
  let verterTsc: AxisBResult["verterTsc"];
  if (verterTscBin) {
    const t0 = performance.now();
    const r = spawnSync(verterTscBin, ["-b", corpus.rootTsconfig, "--noEmit"], {
      cwd: corpus.dir,
      timeout: 30 * 60 * 1000,
      encoding: "utf-8",
      windowsHide: true,
    });
    const ms = performance.now() - t0;
    const out = String(r.stdout ?? "") + String(r.stderr ?? "");
    verterTsc = { timeMs: ms, diagnostics: (out.match(/error TS\d+:/g) ?? []).length };
  } else {
    verterTsc = { skipped: "verter-tsc binary not found" };
  }

  // vize typeCheckBatch over the same corpus.
  let vizeRes: AxisBResult["vize"];
  if (vize.typeCheckBatch) {
    const glob = join(corpus.dir, "**", "*.vue").replace(/\\/g, "/");
    const vz = vize.typeCheckBatch(glob, { threads });
    vizeRes = { timeMs: vz.timeMs };
  } else {
    vizeRes = { skipped: "vize typeCheckBatch not available" };
  }

  return { caveat, vizeMode: caps?.mode ?? "ast-based", verterTsc, vize: vizeRes };
}

async function main(): Promise<void> {
  const options = parseArgs(process.argv);
  const { vize, vizePath } = loadVize();
  const threads = (await import("node:os")).availableParallelism();
  const corpus = await ensureCorpus(
    options.smoke
      ? { config: { fileCount: 200, moduleCount: 20, importsPerFile: 6, compositeModuleCount: 4 } }
      : {},
  );

  console.log("\n" + "═".repeat(82));
  console.log(" OFFLINE Verter vs vize — informational (NOT CI; vize is offline-only)");
  console.log("═".repeat(82));
  console.log(`  vize path   : ${vizePath}`);
  console.log(
    `  corpus hash : ${corpus.contentHash}${corpus.isGateCorpus ? " (gate corpus)" : " (SMOKE slice)"}`,
  );
  console.log(`  threads     : ${threads}`);
  console.log("─".repeat(82));

  const report: Record<string, unknown> = {
    timestamp: new Date().toISOString(),
    vizePath,
    corpusHash: corpus.contentHash,
    threads,
  };

  if (options.axis === "a" || options.axis === "both") {
    const a = runAxisA(corpus, vize, threads);
    report.axisA = a;
    console.log("\n[AXIS A] compiler throughput (apples-to-apples)");
    console.log(`  files       : ${a.fileCount} (${(a.totalBytes / (1024 * 1024)).toFixed(1)} MB)`);
    console.log(
      `  Verter      : ${a.verter.timeMs.toFixed(0)}ms  ${a.verter.filesPerSec.toFixed(0)} files/s`,
    );
    console.log(
      `  vize        : ${a.vize.timeMs.toFixed(0)}ms  ${a.vize.filesPerSec.toFixed(0)} files/s (${a.vize.success} ok, ${a.vize.failed} failed)`,
    );
    console.log(
      `  Verter/vize : ${a.throughputRatio.toFixed(2)}x ${a.throughputRatio >= 1 ? "(Verter faster)" : "(vize faster)"}`,
    );
  }

  if (options.axis === "b" || options.axis === "both") {
    const b = runAxisB(corpus, vize, threads);
    report.axisB = b;
    console.log("\n[AXIS B] typecheck (INFORMATIONAL)");
    console.log(`  WARNING: ${b.caveat}`);
    console.log(
      `  Verter+tsgo : ${"timeMs" in b.verterTsc ? `${b.verterTsc.timeMs.toFixed(0)}ms (${b.verterTsc.diagnostics} diagnostics)` : `skipped — ${b.verterTsc.skipped}`}`,
    );
    console.log(
      `  vize        : ${"timeMs" in b.vize ? `${b.vize.timeMs.toFixed(0)}ms (mode='${b.vizeMode}', shallow)` : `skipped — ${b.vize.skipped}`}`,
    );
  }

  console.log("\n" + "═".repeat(82) + "\n");
  if (options.out) {
    const { writeFileSync } = await import("node:fs");
    writeFileSync(options.out, JSON.stringify(report, null, 2));
  }
}

const invokedDirectly = process.argv[1]?.replace(/\\/g, "/").endsWith("perf/vize-bench.ts");
if (invokedDirectly) {
  main().catch((e) => {
    console.error(e);
    process.exit(1);
  });
}
