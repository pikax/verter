/**
 * Endurance scenario 3 — hover/definition storms (D2 reproduction) with a
 * concurrent typer churning the import-chain root.
 *
 * Bounded in-flight workers sustain storm traffic across carrier files for
 * VERTER_ENDURANCE_STORM_MS (default 20s); every request must settle
 * (answered or properly cancelled — a timeout is a silent drop and fails),
 * content stays correct, p95 stays bounded, the provider survives, and
 * hover/definition still answer correctly after the storm.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  carrierStormProbes,
  loadEnduranceConfig,
  runStormScenario,
  stormWorkspace,
  type EnduranceReceipt,
} from "../src/endurance/index.js";
import {
  attestReceipt,
  disposeRig,
  materializeRig,
  scenarioContext,
  type EnduranceRig,
} from "./endurance.helpers.js";

const config = loadEnduranceConfig();

describe.sequential(`endurance: hover-definition-storm [${config.route}]`, () => {
  let rig: EnduranceRig;
  let receipt: EnduranceReceipt | null = null;

  beforeAll(async () => {
    const { files, carriers } = stormWorkspace();
    rig = await materializeRig(files, config);
    for (const carrier of carriers) {
      rig.session.openFile(carrier);
    }
  });

  afterAll(async () => {
    await disposeRig(rig);
  });

  it("sustains a mixed storm with zero dropped requests and correct answers", async () => {
    const { files, carriers } = stormWorkspace();
    const probes = carrierStormProbes(carriers);
    const churn = { relativePath: carriers[0], baseText: files[carriers[0]] };
    receipt = await runStormScenario(scenarioContext(rig, "hover-definition-storm"), {
      probes,
      churn,
    });
    attestReceipt(receipt, { requireFinalSanity: true });
    // Storm traffic must be real: many more requests than probes, and p95 within
    // the per-route bound (tsserver's reflects its single-threaded engine capacity).
    expect(receipt.requestsSent).toBeGreaterThanOrEqual(50);
    expect(receipt.latency.overall.p95).toBeLessThanOrEqual(config.stormP95MaxMs);
  }, 3_600_000);
});
