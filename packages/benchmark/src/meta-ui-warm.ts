/**
 * Meta-UI WARM-vs-COLD batch CPU.
 *
 * The companion to `meta-ui-saturation` (which is cold-only). This drives
 * `getComponentMetaBatch` three times on the SAME session — cold, then
 * warm, then warm2 — and reports CPU/wall/cores for each plus the
 * warm/cold CPU ratio. The warm ratio is the metric the validation-
 * snapshot + proof-sidecar work targets: a repeated warm batch should
 * approach O(N) cache lookups (ratio -> small) rather than re-paying the
 * per-query resolution cost (ratio -> 1).
 *
 *   pnpm --filter @verter/benchmark bench:meta:ui:warm -- --exclude=chat
 *
 * Requires the meta-UI corpus (`bench:meta:ui:setup`) and the built
 * native binding (`pnpm run build:native`). Developer diagnostic; not a
 * unit test.
 */
import { existsSync } from "node:fs";
import { availableParallelism } from "node:os";
import { dirname, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath, pathToFileURL } from "node:url";

import { buildCheckerConfig, parseMetaUiBenchArgs, prepareMetaUiProject } from "./meta-ui-bench.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "../../..");

const CHECKER_OPTIONS: Record<string, unknown> = {
  forceUseTs: true,
  schema: { literalBooleanSchema: true },
  runtimeMode: "dedicated",
};

interface MetaResult {
  props?: unknown[];
  events?: unknown[];
  slots?: unknown[];
  exposed?: unknown[];
}
interface MetaSessionLike {
  updateFile(filePath: string, source: string): void;
  getComponentMeta(filePath: string): Promise<MetaResult | null | undefined>;
  getComponentMetaBatch(filePaths: string[]): Promise<Array<MetaResult | null | undefined>>;
  close(): void;
}

function hasSurface(meta: MetaResult | null | undefined): boolean {
  if (!meta) return false;
  return Boolean(
    meta.props?.length || meta.events?.length || meta.slots?.length || meta.exposed?.length,
  );
}

function parseExcludes(argv: string[]): string[] {
  const tokens: string[] = [];
  for (const arg of argv) {
    if (arg.startsWith("--exclude=")) {
      for (const t of arg.slice("--exclude=".length).split(",")) {
        const tok = t.trim().toLowerCase();
        if (tok) tokens.push(tok);
      }
    }
  }
  return tokens;
}

async function measure(
  label: string,
  total: number,
  fn: () => Promise<number>,
): Promise<{
  label: string;
  wallMs: number;
  cpuMs: number;
  cores: number;
  resolved: number;
  total: number;
}> {
  globalThis.gc?.();
  const cpu0 = process.cpuUsage();
  const t0 = performance.now();
  const resolved = await fn();
  const wallMs = performance.now() - t0;
  const cpu1 = process.cpuUsage(cpu0);
  const cpuMs = (cpu1.user + cpu1.system) / 1000;
  return { label, wallMs, cpuMs, cores: wallMs > 0 ? cpuMs / wallMs : 0, resolved, total };
}

async function loadMeta(): Promise<{
  openComponentMetaSession: (
    config: { root: string; config: Record<string, unknown> },
    checkerOptions?: Record<string, unknown>,
  ) => Promise<MetaSessionLike>;
  shutdownMetaRuntime: () => void;
}> {
  const sourceEntry = resolve(repoRoot, "packages", "component-meta", "src", "index.ts");
  if (!existsSync(sourceEntry)) throw new Error("component-meta source not found");
  return (await import(pathToFileURL(sourceEntry).href)) as never;
}

async function main(): Promise<void> {
  const argv = process.argv.slice(2);
  const args = parseMetaUiBenchArgs(argv);
  const prepared = prepareMetaUiProject(args);
  const cores = availableParallelism();
  const excludes = parseExcludes(argv);
  const kept = prepared.componentSnapshots.filter(
    (c: { relativePath: string }) =>
      !excludes.some((tok) => c.relativePath.toLowerCase().includes(tok)),
  );
  const checkerConfig = buildCheckerConfig(prepared, kept);
  const { openComponentMetaSession, shutdownMetaRuntime } = await loadMeta();
  const allPaths = kept.map((c: { absolutePath: string }) => c.absolutePath);

  console.log(`\nwarm-vs-cold batch — ${kept.length} components, ${cores} cores\n`);

  const session = await openComponentMetaSession(
    { root: prepared.uiRoot, config: checkerConfig },
    CHECKER_OPTIONS,
  );
  for (const c of kept) session.updateFile(c.absolutePath, c.transformedSource);

  const runBatch = async (label: string) =>
    measure(
      label,
      allPaths.length,
      async () => (await session.getComponentMetaBatch(allPaths)).filter(hasSurface).length,
    );

  const cold = await runBatch("cold ");
  const warm = await runBatch("warm ");
  const warm2 = await runBatch("warm2");
  session.close();
  shutdownMetaRuntime();

  for (const m of [cold, warm, warm2]) {
    console.log(
      `  ${m.label}  wall=${(m.wallMs / 1000).toFixed(2)}s  cpu=${(m.cpuMs / 1000).toFixed(2)}s  cores=${m.cores.toFixed(2)}x  resolved=${m.resolved}/${m.total}`,
    );
  }
  const ratio = warm.cpuMs / cold.cpuMs;
  console.log(
    `\n  WARM/COLD cpu ratio = ${ratio.toFixed(3)}  (warm CPU reduction ${((1 - ratio) * 100).toFixed(0)}%)`,
  );
  console.log(`  warm2/cold = ${(warm2.cpuMs / cold.cpuMs).toFixed(3)}  (stability check)\n`);
}

void main();
