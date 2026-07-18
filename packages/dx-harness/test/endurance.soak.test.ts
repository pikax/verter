/**
 * Endurance scenario 4 — soak.
 *
 * Sustained mixed workload (typing + hover + completion + definition across
 * files) for VERTER_ENDURANCE_SOAK_MS (default 150s). Asserts: no p95
 * degradation trend across time windows (late <= early * factor AND late <=
 * absolute bound), provider alive, ZERO unanswered requests, RSS under the
 * ceiling (skipped with an explicit note on unsupported platforms), and a
 * final full-feature sanity pass after the soak.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  loadEnduranceConfig,
  runSoakScenario,
  SOAK_SCRATCH_PATH,
  SOAK_TYPED_DOC,
  soakProbes,
  soakWorkspace,
  type EnduranceReceipt,
} from "../src/endurance/index.js";
import {
  attestReceipt,
  disposeRig,
  expectRssWithinCeiling,
  materializeRig,
  scenarioContext,
  type EnduranceRig,
} from "./endurance.helpers.js";

const config = loadEnduranceConfig();

describe.sequential(`endurance: soak [${config.route}]`, () => {
  let rig: EnduranceRig;
  let receipt: EnduranceReceipt | null = null;

  beforeAll(async () => {
    const { files, carriers } = soakWorkspace();
    rig = await materializeRig(files, config);
    rig.session.openFile("src/Child.vue");
    rig.session.openFile("src/App.vue");
    for (const carrier of carriers) {
      rig.session.openFile(carrier);
    }
    // The typer's scratch buffer starts empty.
    rig.session.openFile(SOAK_SCRATCH_PATH, "");
  });

  afterAll(async () => {
    await disposeRig(rig);
  });

  it("sustains a mixed workload with bounded latency/RSS and a clean final sanity pass", async () => {
    const { carriers } = soakWorkspace();
    const probes = soakProbes(carriers);
    receipt = await runSoakScenario(scenarioContext(rig, "soak"), {
      probes,
      typingFile: { relativePath: SOAK_SCRATCH_PATH, typedText: SOAK_TYPED_DOC },
    });
    attestReceipt(receipt, { requireFinalSanity: true });
    expectRssWithinCeiling(receipt, config);
    // Absolute p95 bound always applies.
    expect(receipt.latency.overall.p95).toBeLessThanOrEqual(config.p95MaxMs);
    // Degradation trend: only meaningful with >=2 usable windows; a shorter
    // run reports null and is honestly trend-skipped (never vacuously passed).
    if (receipt.degradationCheck) {
      expect(
        receipt.degradationCheck.pass,
        `late-window p95 ${receipt.degradationCheck.lateWindowP95}ms exceeds early-window p95 ` +
          `${receipt.degradationCheck.earlyWindowP95}ms * ${receipt.degradationCheck.factor}`,
      ).toBe(true);
      expect(receipt.degradationCheck.lateWindowP95).toBeLessThanOrEqual(config.p95MaxMs);
    } else {
      console.log(
        "[endurance] soak too short for a two-window trend check — degradation trend skipped explicitly",
      );
    }
  }, 3_600_000);
});
