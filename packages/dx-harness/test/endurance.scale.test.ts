/** Read-only scale lane over an external or generated four-lane corpus. */
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  ENDURANCE_LANES,
  buildReceipt,
  deriveCorpusProbes,
  disposeWorkspace,
  FailureBag,
  loadEnduranceConfig,
  runRenameCycles,
  runSoakScenario,
  runStormScenario,
  type CorpusLaneSection,
  type CorpusProbeDerivation,
  type EnduranceLane,
  type EnduranceReceipt,
} from "../src/endurance/index.js";
import {
  attestReceipt,
  disposeRig,
  expectRssWithinCeiling,
  scenarioContext,
  spawnRig,
} from "./endurance.helpers.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const config = loadEnduranceConfig();
const SCALE_ENABLED = config.corpusDir !== null || config.syntheticScale;

describe.sequential(`endurance: scale lane [${config.route}]`, () => {
  if (!SCALE_ENABLED) {
    it.skip("scale lane disabled (set VERTER_ENDURANCE_CORPUS_DIR or VERTER_ENDURANCE_SYNTHETIC_SCALE=1)", () => {});
    return;
  }

  let corpusRoot: string;
  let derivation: CorpusProbeDerivation;
  let generatedDir: string | null = null;

  beforeAll(() => {
    corpusRoot = config.corpusDir ?? mkdtempSync(path.join(tmpdir(), "verter-endurance-corpus-"));
    if (config.corpusDir === null) {
      generatedDir = corpusRoot;
      const generator = path.resolve(HERE, "..", "scripts", "generate-endurance-corpus.mjs");
      execFileSync(process.execPath, [generator, corpusRoot, String(config.scaleCorpusFiles)], {
        stdio: "inherit",
      });
    }
    derivation = deriveCorpusProbes(corpusRoot, { maxFiles: config.scaleOpenFiles });
    if (derivation.probes.length < 6) {
      throw new Error(
        `scale lane derived only ${derivation.probes.length} probes from ${corpusRoot}`,
      );
    }
  });

  afterAll(() => {
    if (generatedDir) disposeWorkspace(generatedDir);
  });

  function laneFor(section: CorpusLaneSection): EnduranceLane {
    const lane = ENDURANCE_LANES.find(
      (candidate) => candidate.framework === section.framework && candidate.mode === section.mode,
    );
    if (!lane) throw new Error(`unknown corpus lane ${section.framework}-${section.mode}`);
    return lane;
  }

  it("storms every discovered framework/mode section with lane-scoped receipts", async () => {
    for (const section of derivation.lanes) {
      if (section.probes.length === 0) continue;
      const lane = laneFor(section);
      const rig = await spawnRig(corpusRoot, config, false);
      try {
        for (const file of section.files) rig.session.openFile(file);
        const churnFile = section.files.find((file) =>
          readFileSync(path.join(corpusRoot, file), "utf8").includes("</script>"),
        );
        const stableProbes = section.probes.filter((probe) => probe.relativePath !== churnFile);
        if (stableProbes.length === 0)
          throw new Error(`scale storm has no stable probes for ${lane.id}`);
        const receipt: EnduranceReceipt = await runStormScenario(
          scenarioContext(rig, "scale-storm", lane),
          {
            probes: stableProbes,
            churn: churnFile
              ? {
                  relativePath: churnFile,
                  baseText: readFileSync(path.join(corpusRoot, churnFile), "utf8"),
                }
              : undefined,
          },
        );
        attestReceipt(receipt, { requireFinalSanity: true });
        expect(receipt.frameworks[section.framework]?.[section.mode]).toBeDefined();
        // Scale-storm bound: a real corpus program under storm is heavier than
        // the synthetic-carrier storm (see types.ts `scaleStormP95MaxMs`).
        expect(receipt.latency.overall.p95).toBeLessThanOrEqual(config.scaleStormP95MaxMs);
      } finally {
        await disposeRig(rig);
      }
    }
  }, 3_600_000);

  it("runs overlay-only rename cycles in every framework/mode section", async () => {
    for (const section of derivation.lanes) {
      if (!section.renameTarget)
        throw new Error(`scale rename has no target for ${section.framework}-${section.mode}`);
      const lane = laneFor(section);
      const rig = await spawnRig(corpusRoot, config, false);
      try {
        for (const file of section.files) rig.session.openFile(file);
        const failures = new FailureBag();
        const context = scenarioContext(rig, "scale-heavy-update", lane);
        const startedAtMs = Date.now();
        context.sampler?.start();
        const cycles = Math.min(config.heavyUpdateCycles, 20);
        const finalSanityPass = await runRenameCycles(
          context,
          section.renameTarget.file,
          section.renameTarget.ident,
          cycles,
          failures,
        );
        const receipt = buildReceipt(context, startedAtMs, {
          finalSanityPass,
          failures: failures.list,
        });
        attestReceipt(receipt, { requireFinalSanity: true });
        expect(receipt.requestsSent).toBeGreaterThanOrEqual(cycles * 4);
        expect(receipt.frameworks[section.framework]?.[section.mode]).toBeDefined();
      } finally {
        await disposeRig(rig);
      }
    }
  }, 3_600_000);

  it("soaks every discovered framework/mode section with lane-scoped receipts", async () => {
    for (const section of derivation.lanes) {
      if (section.probes.length === 0) continue;
      const lane = laneFor(section);
      const rig = await spawnRig(corpusRoot, config, false);
      try {
        for (const file of section.files) rig.session.openFile(file);
        const typingPath = section.files.find((file) =>
          readFileSync(path.join(corpusRoot, file), "utf8").includes("</script>"),
        );
        const stableProbes = section.probes.filter((probe) => probe.relativePath !== typingPath);
        if (stableProbes.length === 0)
          throw new Error(`scale soak has no stable probes for ${lane.id}`);
        const receipt: EnduranceReceipt = await runSoakScenario(
          scenarioContext(rig, "scale-soak", lane),
          {
            probes: stableProbes,
            typingFile: typingPath
              ? {
                  relativePath: typingPath,
                  typedText: readFileSync(path.join(corpusRoot, typingPath), "utf8"),
                }
              : undefined,
          },
        );
        attestReceipt(receipt, { requireFinalSanity: true });
        expect(receipt.frameworks[section.framework]?.[section.mode]).toBeDefined();
        expectRssWithinCeiling(receipt, config);
        expect(receipt.latency.overall.p95).toBeLessThanOrEqual(config.p95MaxMs);
        if (receipt.degradationCheck) expect(receipt.degradationCheck.pass).toBe(true);
      } finally {
        await disposeRig(rig);
      }
    }
  }, 3_600_000);
});
