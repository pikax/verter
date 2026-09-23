/**
 * Close-cost lane (WSP6, review item C): what a document close costs on a
 * realistically large workspace.
 *
 * `release_canonical` walks the live semantic node set to find what the
 * closing document rooted or bound (an O(live nodes) scan paid at the close,
 * instead of a canonical→node reverse index charged to every hot intern).
 * This lane measures that choice where it matters: on the WSP equal-work
 * synthetic SFC slice (2615 generated Vue files, the PrimeVue-equivalent
 * workload of WSP1B) it opens, edits, hovers and closes corpus documents and
 * records, per close, the server's own release receipt (`retention.lastRelease`:
 * wall time, nodes scanned, nodes released, physical slots before/after, the
 * wait behind in-flight computations) together with the exact heap and the
 * retained-object counters read just before the close and once the release
 * landed.
 *
 * Opt in with `VERTER_ENDURANCE_CLOSE_COST=1`. `VERTER_ENDURANCE_CLOSE_COST_CORPUS_DIR`
 * points at an existing workspace root instead of generating the slice;
 * `VERTER_ENDURANCE_CLOSE_COST_FILES` bounds the documents driven (default 40);
 * `VERTER_ENDURANCE_CLOSE_COST_OUT` names a JSON receipt to write.
 */
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { GET_STATISTICS_METHOD } from "../src/core/startupGate.js";
import {
  deriveCorpusProbes,
  disposeWorkspace,
  loadEnduranceConfig,
  stageEnduranceFixtureDependencies,
  type CorpusProbeDerivation,
  type EnduranceProbe,
} from "../src/endurance/index.js";
import {
  describeRetentionReading,
  extractRetentionReading,
  type LastReleaseReading,
  type RetentionReading,
} from "../src/endurance/scenarios/churn.js";
import { disposeRig, spawnRig, type EnduranceRig } from "./endurance.helpers.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "..", "..", "..");
const config = loadEnduranceConfig();
const ENABLED = process.env.VERTER_ENDURANCE_CLOSE_COST === "1";
const FILE_BUDGET = Number(process.env.VERTER_ENDURANCE_CLOSE_COST_FILES ?? "40");
/** WSP1B: the PrimeVue-equivalent slice of the synthetic-15k corpus. */
const SLICE = { count: 2615, modules: 80, composite: 8 } as const;

interface CloseCostSample {
  readonly relativePath: string;
  readonly hoverMs: number;
  readonly before: RetentionReading;
  readonly after: RetentionReading;
  readonly release: LastReleaseReading;
  /** Wall time from the close notification to the release landing. */
  readonly settleMs: number;
}

function generateSlice(): string {
  const root = mkdtempSync(path.join(tmpdir(), "verter-close-cost-"));
  const corpus = path.join(root, "corpus");
  const generator = path.join(REPO_ROOT, "test-corpora/perf/synthetic-15k/generator/generate.mjs");
  execFileSync(
    process.execPath,
    [
      generator,
      "--out",
      corpus,
      "--count",
      String(SLICE.count),
      "--modules",
      String(SLICE.modules),
      "--composite",
      String(SLICE.composite),
      "--quiet",
    ],
    { cwd: REPO_ROOT, stdio: "inherit" },
  );
  writeFileSync(
    path.join(root, "tsconfig.json"),
    `${JSON.stringify(
      {
        compilerOptions: {
          target: "ESNext",
          module: "ESNext",
          moduleResolution: "Bundler",
          strict: true,
        },
        include: ["corpus/**/*.vue", "corpus/**/*.ts"],
      },
      null,
      2,
    )}\n`,
  );
  mkdirSync(path.join(root, ".verter"), { recursive: true });
  stageEnduranceFixtureDependencies(root, "vue");
  return root;
}

async function readRetention(rig: EnduranceRig): Promise<RetentionReading> {
  const snapshot: unknown = await rig.session.client.sendRequest(
    GET_STATISTICS_METHOD,
    {},
    config.probeTimeoutMs,
  );
  const reading = extractRetentionReading(snapshot);
  if (!reading) throw new Error("the server reported no retention reading");
  return reading;
}

/** Poll until one more release has been applied than `appliedBefore`, or give up. */
async function settleRelease(
  rig: EnduranceRig,
  appliedBefore: number,
  deadlineMs: number,
): Promise<{ reading: RetentionReading; settleMs: number }> {
  const started = performance.now();
  for (;;) {
    const reading = await readRetention(rig);
    if (reading.releasesApplied > appliedBefore) {
      return { reading, settleMs: performance.now() - started };
    }
    if (performance.now() - started > deadlineMs) {
      throw new Error(
        `the close's release did not land within ${deadlineMs}ms: ` +
          `${describeRetentionReading(reading)}`,
      );
    }
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

function summarize(samples: readonly CloseCostSample[]): string {
  const micros = samples.map((s) => s.release.elapsedMicros).sort((a, b) => a - b);
  const pick = (q: number) => micros[Math.min(micros.length - 1, Math.floor(q * micros.length))];
  const heapDelta = samples.map((s) =>
    s.before.heapInUseBytes !== null && s.after.heapInUseBytes !== null
      ? s.after.heapInUseBytes - s.before.heapInUseBytes
      : null,
  );
  const lines = [
    `close cost over ${samples.length} closes: release wall time p50 ${(pick(0.5) / 1000).toFixed(2)}ms ` +
      `p95 ${(pick(0.95) / 1000).toFixed(2)}ms max ${(micros[micros.length - 1] / 1000).toFixed(2)}ms; ` +
      `wait behind computations max ${(Math.max(...samples.map((s) => s.release.waitMicros)) / 1000).toFixed(2)}ms; ` +
      `settle max ${Math.max(...samples.map((s) => s.settleMs)).toFixed(0)}ms`,
    `  live nodes scanned ${Math.min(...samples.map((s) => s.release.nodesScanned))}..${Math.max(...samples.map((s) => s.release.nodesScanned))}, ` +
      `released per close ${Math.min(...samples.map((s) => s.release.nodesReleased))}..${Math.max(...samples.map((s) => s.release.nodesReleased))}, ` +
      `slots ${samples[0]?.release.storageSlotsBefore}→${samples[samples.length - 1]?.release.storageSlotsAfter}`,
    `  exact heap delta across a close: ${heapDelta
      .filter((d): d is number => d !== null)
      .map((d) => `${(d / 1024).toFixed(0)}KiB`)
      .join(" ")}`,
  ];
  for (const s of samples) {
    lines.push(
      `  ${s.relativePath}: hover ${s.hoverMs.toFixed(0)}ms; release took ${(s.release.elapsedMicros / 1000).toFixed(2)}ms ` +
        `(wait ${(s.release.waitMicros / 1000).toFixed(2)}ms, settle ${s.settleMs.toFixed(0)}ms) scanned ${s.release.nodesScanned} ` +
        `released ${s.release.nodesReleased} slots ${s.release.storageSlotsBefore}→${s.release.storageSlotsAfter} ` +
        `memo -${s.release.memoEntriesEvicted}; nodes ${s.before.semanticNodes}→${s.after.semanticNodes} ` +
        `memo ${s.before.semanticMemoEntries}→${s.after.semanticMemoEntries} ` +
        `heap ${s.before.heapInUseBytes === null ? "n/a" : (s.before.heapInUseBytes / 1048576).toFixed(1)}→` +
        `${s.after.heapInUseBytes === null ? "n/a" : (s.after.heapInUseBytes / 1048576).toFixed(1)}MiB`,
    );
  }
  return lines.join("\n");
}

describe.sequential(`endurance: close cost on the WSP equal-work slice [${config.route}]`, () => {
  if (!ENABLED) {
    it.skip("close-cost lane disabled (set VERTER_ENDURANCE_CLOSE_COST=1)", () => {});
    return;
  }
  let root: string;
  let generated = false;
  let derivation: CorpusProbeDerivation;
  let rig: EnduranceRig;

  beforeAll(async () => {
    const existing = process.env.VERTER_ENDURANCE_CLOSE_COST_CORPUS_DIR;
    if (existing) {
      root = path.resolve(existing);
    } else {
      root = generateSlice();
      generated = true;
    }
    derivation = deriveCorpusProbes(root, { maxFiles: FILE_BUDGET });
    rig = await spawnRig(root, config, false);
  }, 1_800_000);

  afterAll(async () => {
    const stderrFile = process.env.VERTER_ENDURANCE_STDERR_FILE;
    if (stderrFile) writeFileSync(stderrFile, rig.handle.client.stderr.text());
    if (rig) await disposeRig(rig);
    if (generated) disposeWorkspace(root);
  });

  it("releases a closed document's semantic substrate at a bounded cost", async () => {
    const probes: EnduranceProbe[] = derivation.lanes
      .flatMap((section) => section.probes)
      .filter((probe) => probe.kind === "hover");
    const byFile = new Map<string, EnduranceProbe>();
    for (const probe of probes)
      if (!byFile.has(probe.relativePath)) byFile.set(probe.relativePath, probe);
    const files = [...byFile.keys()].slice(0, FILE_BUDGET);
    expect(files.length, "the slice must yield hoverable corpus documents").toBeGreaterThan(0);

    const samples: CloseCostSample[] = [];
    const failures: string[] = [];
    let cycle = 0;
    for (const relativePath of files) {
      cycle += 1;
      const probe = byFile.get(relativePath)!;
      const text = rig.session.textOf(relativePath);
      rig.session.openFile(relativePath, text);
      rig.session.changeFile(
        relativePath,
        text.replace("</script>", `// close-cost ${cycle}\n</script>`),
      );
      const hoverStarted = performance.now();
      const outcome = await rig.session.runProbe(
        { ...probe, informational: true, label: `close-cost ${cycle} hover` },
        config.probeTimeoutMs,
      );
      const hoverMs = performance.now() - hoverStarted;
      if (outcome.classification !== "answered") {
        failures.push(`${relativePath}: hover settled as ${outcome.classification}`);
      }
      const before = await readRetention(rig);
      rig.session.closeFile(relativePath);
      const { reading: after, settleMs } = await settleRelease(rig, before.releasesApplied, 30_000);
      if (!after.lastRelease) {
        failures.push(`${relativePath}: no release receipt after the close`);
        continue;
      }
      samples.push({ relativePath, hoverMs, before, after, release: after.lastRelease, settleMs });
    }

    const summary = summarize(samples);
    console.log(`[endurance] close cost\n${summary}`);
    const out = process.env.VERTER_ENDURANCE_CLOSE_COST_OUT;
    if (out) {
      writeFileSync(
        out,
        `${JSON.stringify({ root: generated ? "generated WSP1B slice" : root, slice: SLICE, samples }, null, 2)}\n`,
      );
    }
    expect(failures, "every close must land a release receipt").toEqual([]);
    const last = samples[samples.length - 1]!.after;
    expect(last.deferredReleases, "no release may stay queued once the lane is idle").toBe(0);
  }, 3_600_000);
});
