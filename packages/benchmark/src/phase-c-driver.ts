/**
 * Phase C telemetry driver.
 *
 * Spawns `_audit-component.ts` once per target component
 * (ChatMessages.vue / Button.vue / Table.vue by default) with the
 * focused-counter JSONL emit enabled, then summarises the per-component
 * counter slice on stdout for the Phase C investigator. The full JSONL
 * is written to `--jsonl-out=...` (default `D:/tmp/phase-c-counters.jsonl`).
 *
 * Audit caps stay at the Rust 10K-per-lane defaults — large enough to
 * carry typical component traffic, small enough to bound any
 * pathological fixture's memory footprint.
 *
 * Per-component dispatch carries an explicit hard timeout (default
 * 5 min; brief calls for 5 min). The child is killed on timeout; the
 * driver still summarises the (possibly empty) JSONL line emitted
 * before the timeout fired.
 */

import { spawn } from "node:child_process";
import { existsSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "..", "..", "..");

interface Args {
  components: string[];
  jsonlOut: string;
  hardTimeoutMs: number;
  scratchDir: string;
}

function parseArgs(argv: string[]): Args {
  const args: Args = {
    components: ["ChatMessages", "Button", "Table"],
    jsonlOut: "D:/tmp/phase-c-counters.jsonl",
    hardTimeoutMs: 300_000,
    scratchDir: "D:/tmp/phase-c-scratch",
  };
  for (const arg of argv) {
    if (arg.startsWith("--components=")) {
      args.components = arg
        .slice("--components=".length)
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      continue;
    }
    if (arg.startsWith("--jsonl-out=")) {
      args.jsonlOut = resolve(arg.slice("--jsonl-out=".length));
      continue;
    }
    if (arg.startsWith("--hard-timeout-ms=")) {
      args.hardTimeoutMs = Number.parseInt(arg.slice("--hard-timeout-ms=".length), 10);
      continue;
    }
    if (arg.startsWith("--scratch=")) {
      args.scratchDir = resolve(arg.slice("--scratch=".length));
      continue;
    }
  }
  return args;
}

async function runOne(
  componentToken: string,
  jsonlPath: string,
  scratchDir: string,
  hardTimeoutMs: number,
): Promise<{ ok: boolean; durationMs: number; killed: boolean; stderr: string }> {
  const auditWorkerPath = resolve(repoRoot, "packages", "benchmark", "src", "_audit-component.ts");
  const auditPath = resolve(scratchDir, `${componentToken}.audit.json`);
  const analysisPath = resolve(scratchDir, `${componentToken}.analysis.json`);

  const env = {
    ...process.env,
    VERTER_COMPONENT_META_AUDIT_PATH: auditPath,
    VERTER_COMPONENT_META_ANALYSIS_PATH: analysisPath,
    VERTER_COMPONENT_META_FOCUSED_JSONL_PATH: jsonlPath,
    NODE_OPTIONS: "--max-old-space-size=8192",
  };

  const startedAt = Date.now();
  const child = spawn(process.execPath, ["--import", "tsx", auditWorkerPath, componentToken], {
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });

  let stderr = "";
  child.stderr.on("data", (chunk: Buffer) => {
    stderr += chunk.toString("utf-8");
  });
  child.stdout.on("data", (chunk: Buffer) => {
    process.stdout.write(`  [${componentToken}] ${chunk.toString("utf-8")}`);
  });

  let killed = false;
  const timer = setTimeout(() => {
    killed = true;
    try {
      child.kill("SIGKILL");
    } catch {
      // ignore
    }
  }, hardTimeoutMs);

  return new Promise((resolveFn) => {
    child.on("exit", (code) => {
      clearTimeout(timer);
      const durationMs = Date.now() - startedAt;
      const ok = code === 0 && !killed;
      resolveFn({ ok, durationMs, killed, stderr });
    });
  });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  // Ensure scratch dir exists.
  try {
    await import("node:fs").then((fs) => fs.mkdirSync(args.scratchDir, { recursive: true }));
  } catch {
    // ignore
  }
  // Reset the JSONL output (atomic — each line is one component).
  if (existsSync(args.jsonlOut)) {
    unlinkSync(args.jsonlOut);
  }
  writeFileSync(args.jsonlOut, "", "utf-8");
  console.log(
    `Phase C driver: hard_timeout=${args.hardTimeoutMs}ms components=${args.components.join(",")}`,
  );
  console.log(`JSONL output: ${args.jsonlOut}`);

  const summary: Array<{
    component: string;
    durationMs: number;
    ok: boolean;
    killed: boolean;
    counters: Record<string, number> | null;
  }> = [];

  for (const component of args.components) {
    console.log(`\n=== Running ${component} ===`);
    const { ok, durationMs, killed, stderr } = await runOne(
      component,
      args.jsonlOut,
      args.scratchDir,
      args.hardTimeoutMs,
    );
    let counters: Record<string, number> | null = null;
    try {
      const lines = readFileSync(args.jsonlOut, "utf-8").trim().split("\n").filter(Boolean);
      // Find the LATEST line that matches this component (so a killed
      // child that never emitted does not pick up a previous
      // component's slice).
      const wantedName = `${component}.vue`;
      for (let i = lines.length - 1; i >= 0; i--) {
        const parsed = JSON.parse(lines[i]) as {
          component?: string;
          counters?: Record<string, number>;
        };
        if (parsed.component === wantedName && parsed.counters) {
          counters = parsed.counters;
          break;
        }
      }
    } catch (error) {
      console.error(`  [parse] ${error instanceof Error ? error.message : String(error)}`);
    }
    if (!ok && stderr) {
      console.error(`  [stderr] ${stderr.trimEnd().slice(0, 400)}`);
    }
    summary.push({ component, durationMs, ok, killed, counters });
    console.log(
      `  result: ok=${ok} killed=${killed} duration=${durationMs}ms ${counters ? "JSONL emitted" : "JSONL EMPTY"}`,
    );
  }

  console.log(`\n\n=== Summary ===`);
  console.log(`${"Component".padEnd(15)} ${"DurationMs".padEnd(12)} ${"Ok".padEnd(6)} ${"Killed"}`);
  for (const row of summary) {
    console.log(
      `${row.component.padEnd(15)} ${row.durationMs.toString().padEnd(12)} ${row.ok.toString().padEnd(6)} ${row.killed.toString()}`,
    );
  }
  console.log(`\n=== Counter slice (focused) ===`);
  if (summary.every((s) => s.counters === null)) {
    console.log("(no counter data — all components killed or failed before JSONL emit)");
    return;
  }
  const keys = Array.from(
    new Set(summary.flatMap((s) => (s.counters ? Object.keys(s.counters) : []))),
  );
  const header = ["Counter", ...summary.map((s) => s.component)].join("\t");
  console.log(header);
  for (const key of keys) {
    const cells = [key, ...summary.map((s) => (s.counters ? String(s.counters[key] ?? 0) : "—"))];
    console.log(cells.join("\t"));
  }
  console.log(`\nFull JSONL: ${args.jsonlOut}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(2);
});
