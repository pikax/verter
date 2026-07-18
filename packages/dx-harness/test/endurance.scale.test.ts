/**
 * Endurance scale lane — storm + heavy-update + soak against an EXTERNAL
 * corpus (VERTER_ENDURANCE_CORPUS_DIR) or a generated synthetic corpus
 * (VERTER_ENDURANCE_SYNTHETIC_SCALE=1). The corpus is used strictly
 * READ-ONLY: files are opened from disk and all edits are in-memory
 * didChange overlays — the harness never writes into the corpus directory.
 * When neither env var is set the lane reports an explicit skip.
 */
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  deriveCorpusProbes,
  disposeWorkspace,
  FailureBag,
  loadEnduranceConfig,
  runRenameCycles,
  runSoakScenario,
  runStormScenario,
  type CorpusProbeDerivation,
  type EnduranceReceipt,
} from "../src/endurance/index.js";
import {
  attestReceipt,
  disposeRig,
  expectRssWithinCeiling,
  scenarioContext,
  spawnRig,
  type EnduranceRig,
} from "./endurance.helpers.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const config = loadEnduranceConfig();
const SCALE_ENABLED = config.corpusDir !== null || config.syntheticScale;

describe.sequential(`endurance: scale lane [${config.route}]`, () => {
  if (!SCALE_ENABLED) {
    it.skip(
      "VERTER_ENDURANCE_CORPUS_DIR unset — scale lane skipped " +
        "(set VERTER_ENDURANCE_CORPUS_DIR to an external project root, or " +
        "VERTER_ENDURANCE_SYNTHETIC_SCALE=1 to generate a synthetic corpus)",
      () => {},
    );
    return;
  }

  let rig: EnduranceRig;
  let derivation: CorpusProbeDerivation;
  let generatedDir: string | null = null;

  beforeAll(async () => {
    let corpusDir = config.corpusDir;
    if (!corpusDir) {
      generatedDir = mkdtempSync(path.join(tmpdir(), "verter-endurance-corpus-"));
      corpusDir = generatedDir;
      const generator = path.resolve(HERE, "..", "scripts", "generate-endurance-corpus.mjs");
      execFileSync(process.execPath, [generator, corpusDir, String(config.scaleCorpusFiles)], {
        stdio: "inherit",
      });
    }
    derivation = deriveCorpusProbes(corpusDir, { maxFiles: config.scaleOpenFiles });
    if (derivation.probes.length < 6) {
      throw new Error(
        `scale lane: only ${derivation.probes.length} probes derivable from ${corpusDir} — corpus too small or atypical`,
      );
    }
    rig = await spawnRig(corpusDir, config, false);
    for (const file of derivation.files) {
      rig.session.openFile(file);
    }
  });

  afterAll(async () => {
    await disposeRig(rig); // ownsWorkspace === false: the corpus is never removed.
    // Retry-hardened removal (Windows transient EBUSY on just-killed children).
    if (generatedDir) disposeWorkspace(generatedDir);
  });

  it("storm against the corpus", async () => {
    const churn = derivation.churnFile
      ? {
          relativePath: derivation.churnFile,
          baseText: readFileSync(path.join(rig.workspaceRoot, derivation.churnFile), "utf8"),
        }
      : undefined;
    const receipt: EnduranceReceipt = await runStormScenario(scenarioContext(rig, "scale-storm"), {
      probes: derivation.probes,
      churn,
    });
    attestReceipt(receipt, { requireFinalSanity: true });
    expect(receipt.latency.overall.p95).toBeLessThanOrEqual(config.stormP95MaxMs);
  }, 3_600_000);

  it("heavy-update rename cycles against a corpus file (overlay-only)", async () => {
    if (!derivation.renameTarget) {
      console.log(
        "[endurance] no rename target derivable from corpus — rename leg skipped explicitly",
      );
      return;
    }
    const failures = new FailureBag();
    const cycles = Math.min(config.heavyUpdateCycles, 20);
    await runRenameCycles(
      scenarioContext(rig, "scale-heavy-update"),
      derivation.renameTarget.file,
      derivation.renameTarget.ident,
      cycles,
      failures,
    );
    expect([...failures.list], `rename-cycle failures:\n${failures.list.join("\n")}`).toEqual([]);
    expect(rig.session.tracker.unanswered).toBe(0);
    expect(rig.session.tracker.errored).toBe(0);
    expect(rig.handle.client.isAlive()).toBe(true);
  }, 3_600_000);

  it("soak against the corpus", async () => {
    const churnText = derivation.churnFile
      ? readFileSync(path.join(rig.workspaceRoot, derivation.churnFile), "utf8")
      : null;
    const receipt: EnduranceReceipt = await runSoakScenario(scenarioContext(rig, "scale-soak"), {
      probes: derivation.probes,
      typingFile:
        derivation.churnFile && churnText !== null
          ? { relativePath: derivation.churnFile, typedText: churnText }
          : undefined,
    });
    attestReceipt(receipt, { requireFinalSanity: true });
    expectRssWithinCeiling(receipt, config);
    expect(receipt.latency.overall.p95).toBeLessThanOrEqual(config.p95MaxMs);
    if (receipt.degradationCheck) {
      expect(receipt.degradationCheck.pass).toBe(true);
      expect(receipt.degradationCheck.lateWindowP95).toBeLessThanOrEqual(config.p95MaxMs);
    } else {
      console.log(
        "[endurance] scale soak too short for a two-window trend check — skipped explicitly",
      );
    }
  }, 3_600_000);
});
