/**
 * Component Meta Benchmark: @verter/component-meta vs vue-component-meta (Volar)
 *
 * § 1  Per-file benchmark (tinybench, 20 warmup + 100 iterations)
 * § 2  In-memory stress test (10K files)
 * § 3  Disk-based stress test (5K files)
 * § 4  Correctness comparison
 * § 5  JSON output (--json flag)
 *
 * Usage:
 *   node --import tsx src/meta-bench.ts
 *   node --import tsx src/meta-bench.ts --json
 */
import { Bench } from "tinybench";
import { readFileSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { formatDuration, formatBytes } from "./utils/stats.js";
import {
  compareMeta,
  generateMetaReport,
  type SimplifiedMeta,
  type MetaComparisonResult,
  type MetaBenchmarkReport,
} from "./meta-bench-utils.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const _require = createRequire(import.meta.url);
const JSON_MODE = process.argv.includes("--json");

// ─── Fixture Loading ────────────────────────────────────────────────────────

const FIXTURE_NAMES = [
  "bare-template.vue",
  "single-prop.vue",
  "typed-props.vue",
  "slots-and-emits.vue",
  "models-and-expose.vue",
  "options-api.vue",
  "jsdoc-heavy.vue",
  "kitchen-sink-meta.vue",
];

interface Fixture {
  name: string;
  filename: string;
  source: string;
  size: number;
  absPath: string;
}

const FIXTURES_DIR = join(__dirname, "meta-fixtures");
const TSCONFIG_PATH = join(FIXTURES_DIR, "tsconfig.json");

const fixtures: Fixture[] = FIXTURE_NAMES.map((filename) => {
  const absPath = resolve(FIXTURES_DIR, filename);
  const source = readFileSync(absPath, "utf-8");
  return {
    name: filename.replace(".vue", ""),
    filename,
    source,
    size: Buffer.byteLength(source, "utf-8"),
    absPath,
  };
});

// ─── Column formatter ───────────────────────────────────────────────────────

const col = (s: string | number, w: number, right = true) =>
  right ? String(s).padStart(w) : String(s).padEnd(w);

// ─── Initialize Checkers ────────────────────────────────────────────────────

function log(msg: string) {
  if (!JSON_MODE) process.stdout.write(msg);
}
function logln(msg: string) {
  if (!JSON_MODE) console.log(msg);
}

logln("\n" + "=".repeat(74));
logln(" Component Meta Benchmark: Verter vs Volar (vue-component-meta)");
logln("=".repeat(74));

// Initialize Verter checker — use createRequire to avoid ESM/CJS conflicts
log("\n  Initializing Verter checker...");
const verterInitStart = performance.now();
const { createChecker: createVerterChecker } = _require("@verter/component-meta/compat");
const verterChecker = createVerterChecker(TSCONFIG_PATH);
const verterInitMs = performance.now() - verterInitStart;
logln(` done (${formatDuration(verterInitMs)})`);

// Initialize Volar checker
log("  Initializing Volar checker...");
const volarInitStart = performance.now();
let volarChecker: any;
try {
  const volarMeta = _require("vue-component-meta");
  volarChecker = volarMeta.createChecker(TSCONFIG_PATH);
} catch (e: any) {
  console.error(`\n  Failed to initialize Volar checker: ${e.message}`);
  console.error("  Install vue-component-meta: pnpm add -D vue-component-meta");
  process.exit(1);
}
const volarInitMs = performance.now() - volarInitStart;
logln(` done (${formatDuration(volarInitMs)})`);

logln(
  `\n  Init time — Verter: ${formatDuration(verterInitMs)}, Volar: ${formatDuration(volarInitMs)}`,
);

// ─── Helper: extract simplified meta ────────────────────────────────────────

function toSimplifiedMeta(meta: any): SimplifiedMeta {
  return {
    props: (meta.props ?? []).map((p: any) => ({
      name: p.name,
      type: p.type ?? "",
      required: p.required ?? false,
    })),
    events: (meta.events ?? []).map((e: any) => ({
      name: e.name,
      type: e.type ?? "",
    })),
    slots: (meta.slots ?? []).map((s: any) => ({
      name: s.name,
      type: s.type ?? "",
    })),
  };
}

// ═══════════════════════════════════════════════════════════════════════════
// § 1  Per-file benchmark (tinybench)
// ═══════════════════════════════════════════════════════════════════════════

logln("\n" + "-".repeat(74));
logln(" S 1  Per-file: Verter vs Volar, same fixtures, same thread");
logln("-".repeat(74));

logln(
  `\n  ${col("Fixture", 24, false)}${col("Size", 8)}  ${col("Verter", 10)}  ${col("Volar", 10)}  ${col("Speedup", 10)}`,
);
logln("  " + "-".repeat(66));

interface PerFileResult {
  fixture: string;
  size: number;
  verterMs: number;
  volarMs: number;
  comparison: MetaComparisonResult;
}

const perFileResults: PerFileResult[] = [];

for (const f of fixtures) {
  const bench = new Bench({ warmupIterations: 20, iterations: 100 });

  bench.add("verter", () => {
    verterChecker.getComponentMeta(f.absPath);
  });
  bench.add("volar", () => {
    volarChecker.getComponentMeta(f.absPath);
  });

  await bench.run();

  const verterMs = (bench.getTask("verter")!.result! as any).latency?.mean || 0;
  const volarMs = (bench.getTask("volar")!.result! as any).latency?.mean || 0;

  // Correctness: get meta from both and compare
  const verterMeta = toSimplifiedMeta(verterChecker.getComponentMeta(f.absPath));
  const volarMeta = toSimplifiedMeta(volarChecker.getComponentMeta(f.absPath));
  const comparison = compareMeta(verterMeta, volarMeta);

  perFileResults.push({
    fixture: f.name,
    size: f.size,
    verterMs,
    volarMs,
    comparison,
  });

  const speedup = (volarMs / verterMs).toFixed(1) + "x";
  logln(
    `  ${col(f.name, 24, false)}${col(formatBytes(f.size), 8)}  ${col(formatDuration(verterMs), 10)}  ${col(formatDuration(volarMs), 10)}  ${col(speedup, 10)}`,
  );
}

const avgSpeedup =
  perFileResults.reduce((s, r) => s + r.volarMs / r.verterMs, 0) / perFileResults.length;

logln("  " + "-".repeat(66));
logln(
  `  ${col("AVERAGE", 24, false)}${col("", 8)}  ${col("", 10)}  ${col("", 10)}  ${col(avgSpeedup.toFixed(1) + "x", 10)}`,
);
logln("");

// ═══════════════════════════════════════════════════════════════════════════
// § 2  In-memory stress test (10K files)
// ═══════════════════════════════════════════════════════════════════════════

logln("-".repeat(74));
logln(" S 2  In-memory stress test — 10,000 files (8 fixtures x 1,250)");
logln("-".repeat(74));

const INMEM_COUNT = 1250;
const INMEM_TOTAL = INMEM_COUNT * fixtures.length;

let verterStressMs = 0;
let volarStressMs = 0;
let inMemVerterFps = 0;
let inMemVolarFps = 0;

// Pre-register all files with Verter (always works)
log("\n  Registering files with Verter...");
const verterRegStart = performance.now();
const inMemFiles: Array<{ name: string; absPath: string }> = [];
for (let i = 0; i < INMEM_COUNT; i++) {
  for (const f of fixtures) {
    const virtualPath = resolve(FIXTURES_DIR, `stress-${String(i).padStart(4, "0")}-${f.filename}`);
    verterChecker.updateFile(virtualPath, f.source);
    inMemFiles.push({ name: f.name, absPath: virtualPath });
  }
}
logln(` done (${formatDuration(performance.now() - verterRegStart)})`);

// Verter stress
log(`  Running Verter (${INMEM_TOTAL} getComponentMeta calls)...`);
const verterStressStart = performance.now();
for (const entry of inMemFiles) {
  verterChecker.getComponentMeta(entry.absPath);
}
verterStressMs = performance.now() - verterStressStart;
inMemVerterFps = Math.round((INMEM_TOTAL / verterStressMs) * 1000);
logln(` done — ${formatDuration(verterStressMs)} (${inMemVerterFps.toLocaleString()} files/s)`);

// Volar stress — may fail/crash on large file counts
try {
  log("  Registering files with Volar...");
  const volarRegStart = performance.now();
  for (const entry of inMemFiles) {
    const fixture = fixtures.find((f) => f.name === entry.name)!;
    volarChecker.updateFile(entry.absPath, fixture.source);
  }
  logln(` done (${formatDuration(performance.now() - volarRegStart)})`);

  log(`  Running Volar  (${INMEM_TOTAL} getComponentMeta calls)...`);
  const volarStressStart = performance.now();
  for (const entry of inMemFiles) {
    volarChecker.getComponentMeta(entry.absPath);
  }
  volarStressMs = performance.now() - volarStressStart;
  inMemVolarFps = Math.round((INMEM_TOTAL / volarStressMs) * 1000);
  logln(` done — ${formatDuration(volarStressMs)} (${inMemVolarFps.toLocaleString()} files/s)`);
} catch (e: any) {
  logln(`  Volar crashed during stress test: ${e.message ?? e}`);
  volarStressMs = -1;
}

if (volarStressMs > 0) {
  const inMemSpeedup = (volarStressMs / verterStressMs).toFixed(1) + "x";
  logln(
    `\n  ${col("Checker", 14, false)}  ${col("Time", 10)}  ${col("Files/s", 10)}  ${col("Speedup", 10)}`,
  );
  logln("  " + "-".repeat(50));
  logln(
    `  ${col("Verter", 14, false)}  ${col(formatDuration(verterStressMs), 10)}  ${col(inMemVerterFps.toLocaleString(), 10)}  ${col(inMemSpeedup, 10)}`,
  );
  logln(
    `  ${col("Volar", 14, false)}  ${col(formatDuration(volarStressMs), 10)}  ${col(inMemVolarFps.toLocaleString(), 10)}  ${col("1.0x", 10)}`,
  );
} else {
  logln(
    `\n  Verter: ${formatDuration(verterStressMs)} (${inMemVerterFps.toLocaleString()} files/s)`,
  );
  logln("  Volar:  crashed (unable to handle 10K virtual files)");
}
logln("");

// Cleanup virtual files
for (const entry of inMemFiles) {
  verterChecker.deleteFile(entry.absPath);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 3  Disk-based stress test (5K files)
// ═══════════════════════════════════════════════════════════════════════════

logln("-".repeat(74));
logln(" S 3  Disk-based stress test — 5,000 files (8 fixtures x 625)");
logln("-".repeat(74));

const DISK_COUNT = 625;
const DISK_TOTAL = DISK_COUNT * fixtures.length;
const TEMP_DIR = join(tmpdir(), "meta-bench-" + Date.now());
mkdirSync(TEMP_DIR, { recursive: true });

// Write files to temp dir
const diskFiles: string[] = [];
for (let i = 0; i < DISK_COUNT; i++) {
  for (const f of fixtures) {
    const filename = `${String(i).padStart(4, "0")}-${f.filename}`;
    const filePath = join(TEMP_DIR, filename);
    writeFileSync(filePath, f.source);
    diskFiles.push(filePath);
  }
}

// Write tsconfig
writeFileSync(
  join(TEMP_DIR, "tsconfig.json"),
  JSON.stringify(
    {
      compilerOptions: {
        target: "ESNext",
        module: "ESNext",
        moduleResolution: "bundler",
        strict: true,
        jsx: "preserve",
        skipLibCheck: true,
      },
      include: ["*.vue"],
    },
    null,
    2,
  ),
);

logln(`\n  Written ${DISK_TOTAL} files to temp dir`);

let verterDiskInitMs = 0;
let volarDiskInitMs = 0;
let verterDiskRunMs = 0;
let volarDiskRunMs = 0;
let diskVerterFps = 0;
let diskVolarFps = 0;

// Initialize fresh Verter checker for disk test
log("  Initializing Verter (disk)...");
const verterDiskStart = performance.now();
const verterDiskChecker = createVerterChecker(join(TEMP_DIR, "tsconfig.json"));
verterDiskInitMs = performance.now() - verterDiskStart;
logln(` done (${formatDuration(verterDiskInitMs)})`);

// Verter scan
log(`  Running Verter (${DISK_TOTAL} files from disk)...`);
const verterDiskRunStart = performance.now();
for (const filePath of diskFiles) {
  verterDiskChecker.getComponentMeta(filePath);
}
verterDiskRunMs = performance.now() - verterDiskRunStart;
diskVerterFps = Math.round((DISK_TOTAL / verterDiskRunMs) * 1000);
logln(` done — ${formatDuration(verterDiskRunMs)} (${diskVerterFps.toLocaleString()} files/s)`);

// Volar disk test — may be slow or crash on 5K files
try {
  log("  Initializing Volar (disk)...");
  const volarDiskStart = performance.now();
  const volarDiskMeta = _require("vue-component-meta");
  const volarDiskChecker = volarDiskMeta.createChecker(join(TEMP_DIR, "tsconfig.json"));
  volarDiskInitMs = performance.now() - volarDiskStart;
  logln(` done (${formatDuration(volarDiskInitMs)})`);

  log(`  Running Volar  (${DISK_TOTAL} files from disk)...`);
  const volarDiskRunStart = performance.now();
  for (const filePath of diskFiles) {
    volarDiskChecker.getComponentMeta(filePath);
  }
  volarDiskRunMs = performance.now() - volarDiskRunStart;
  diskVolarFps = Math.round((DISK_TOTAL / volarDiskRunMs) * 1000);
  logln(` done — ${formatDuration(volarDiskRunMs)} (${diskVolarFps.toLocaleString()} files/s)`);
} catch (e: any) {
  logln(`  Volar crashed during disk stress test: ${e.message ?? e}`);
  volarDiskRunMs = -1;
}

if (volarDiskRunMs > 0) {
  const diskSpeedup = (volarDiskRunMs / verterDiskRunMs).toFixed(1) + "x";
  logln(
    `\n  ${col("Checker", 14, false)}  ${col("Init", 10)}  ${col("Scan", 10)}  ${col("Files/s", 10)}  ${col("Speedup", 10)}`,
  );
  logln("  " + "-".repeat(60));
  logln(
    `  ${col("Verter", 14, false)}  ${col(formatDuration(verterDiskInitMs), 10)}  ${col(formatDuration(verterDiskRunMs), 10)}  ${col(diskVerterFps.toLocaleString(), 10)}  ${col(diskSpeedup, 10)}`,
  );
  logln(
    `  ${col("Volar", 14, false)}  ${col(formatDuration(volarDiskInitMs), 10)}  ${col(formatDuration(volarDiskRunMs), 10)}  ${col(diskVolarFps.toLocaleString(), 10)}  ${col("1.0x", 10)}`,
  );
} else {
  logln(
    `\n  Verter: init ${formatDuration(verterDiskInitMs)}, scan ${formatDuration(verterDiskRunMs)} (${diskVerterFps.toLocaleString()} files/s)`,
  );
  logln("  Volar:  crashed");
}
logln("");

// Cleanup temp dir
try {
  rmSync(TEMP_DIR, { recursive: true, force: true });
} catch {}

// ═══════════════════════════════════════════════════════════════════════════
// § 4  Correctness comparison
// ═══════════════════════════════════════════════════════════════════════════

logln("-".repeat(74));
logln(" S 4  Correctness comparison: structural match of meta output");
logln("-".repeat(74));

logln(
  `\n  ${col("Fixture", 24, false)}${col("Props", 8)}${col("Events", 8)}${col("Slots", 8)}${col("Status", 10)}`,
);
logln("  " + "-".repeat(60));

let allCorrect = true;
for (const r of perFileResults) {
  const { comparison: c } = r;
  const pStatus = c.props.status === "match" ? "OK" : "DIFF";
  const eStatus = c.events.status === "match" ? "OK" : "DIFF";
  const sStatus = c.slots.status === "match" ? "OK" : "DIFF";
  const overall = c.overall === "match" ? "MATCH" : "MISMATCH";

  if (c.overall !== "match") allCorrect = false;

  logln(
    `  ${col(r.fixture, 24, false)}${col(pStatus, 8)}${col(eStatus, 8)}${col(sStatus, 8)}${col(overall, 10)}`,
  );

  // Print details for mismatches
  if (c.props.missing.length > 0) {
    logln(`    Props missing in Verter: ${c.props.missing.join(", ")}`);
  }
  if (c.props.extra.length > 0) {
    logln(`    Props extra in Verter: ${c.props.extra.join(", ")}`);
  }
  if (c.events.missing.length > 0) {
    logln(`    Events missing in Verter: ${c.events.missing.join(", ")}`);
  }
  if (c.events.extra.length > 0) {
    logln(`    Events extra in Verter: ${c.events.extra.join(", ")}`);
  }
  if (c.slots.missing.length > 0) {
    logln(`    Slots missing in Verter: ${c.slots.missing.join(", ")}`);
  }
  if (c.slots.extra.length > 0) {
    logln(`    Slots extra in Verter: ${c.slots.extra.join(", ")}`);
  }

  // Print type diffs as warnings
  for (const cat of ["props", "events", "slots"] as const) {
    for (const diff of c[cat].typeDiffs) {
      logln(`    Type diff (${cat}) "${diff.name}": Verter="${diff.verter}" Volar="${diff.volar}"`);
    }
  }
}

logln("  " + "-".repeat(60));
logln(`  Overall: ${allCorrect ? "ALL MATCH" : "MISMATCHES DETECTED (see warnings above)"}`);
logln("");

// ═══════════════════════════════════════════════════════════════════════════
// § 5  JSON output
// ═══════════════════════════════════════════════════════════════════════════

const report: MetaBenchmarkReport & {
  stress: {
    inMemory: {
      files: number;
      verterMs: number;
      volarMs: number | null;
      verterFps: number;
      volarFps: number | null;
      speedup: number | null;
      volarCrashed: boolean;
    };
    disk: {
      files: number;
      verterInitMs: number;
      volarInitMs: number | null;
      verterMs: number;
      volarMs: number | null;
      verterFps: number;
      volarFps: number | null;
      speedup: number | null;
      volarCrashed: boolean;
    };
  };
  init: { verterMs: number; volarMs: number };
} = {
  ...generateMetaReport(perFileResults),
  stress: {
    inMemory: {
      files: INMEM_TOTAL,
      verterMs: verterStressMs,
      volarMs: volarStressMs > 0 ? volarStressMs : null,
      verterFps: inMemVerterFps,
      volarFps: volarStressMs > 0 ? inMemVolarFps : null,
      speedup: volarStressMs > 0 ? volarStressMs / verterStressMs : null,
      volarCrashed: volarStressMs <= 0,
    },
    disk: {
      files: DISK_TOTAL,
      verterInitMs: verterDiskInitMs,
      volarInitMs: volarDiskRunMs > 0 ? volarDiskInitMs : null,
      verterMs: verterDiskRunMs,
      volarMs: volarDiskRunMs > 0 ? volarDiskRunMs : null,
      verterFps: diskVerterFps,
      volarFps: volarDiskRunMs > 0 ? diskVolarFps : null,
      speedup: volarDiskRunMs > 0 ? volarDiskRunMs / verterDiskRunMs : null,
      volarCrashed: volarDiskRunMs <= 0,
    },
  },
  init: {
    verterMs: verterInitMs,
    volarMs: volarInitMs,
  },
};

if (JSON_MODE) {
  console.log(JSON.stringify(report, null, 2));
} else {
  logln("=".repeat(74));
  logln(
    ` Summary: avg ${avgSpeedup.toFixed(1)}x speedup, ${allCorrect ? "all correct" : "mismatches detected"}`,
  );
  logln("=".repeat(74));
  logln("");
}
