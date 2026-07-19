/** Hover/definition storm endurance lanes against the real LSP. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  ENDURANCE_LANES,
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

for (const lane of ENDURANCE_LANES) {
  describe.sequential(`endurance: storm [${lane.id}/${config.route}]`, () => {
    let rig: EnduranceRig;
    let receipt: EnduranceReceipt | null = null;
    const workspace = stormWorkspace(undefined, lane);

    beforeAll(async () => {
      rig = await materializeRig(workspace.files, config);
      for (const carrier of workspace.carriers) rig.session.openFile(carrier);
    });
    afterAll(async () => {
      await disposeRig(rig);
    });

    it("sustains mixed traffic with current post-storm answers", async () => {
      receipt = await runStormScenario(scenarioContext(rig, "hover-definition-storm", lane), {
        probes: carrierStormProbes(workspace.carriers, lane),
        churn: {
          relativePath: workspace.carriers[0],
          baseText: workspace.files[workspace.carriers[0]],
        },
      });
      attestReceipt(receipt, { requireFinalSanity: true });
      expect(receipt.frameworks[lane.framework]?.[lane.mode]).toBeDefined();
      expect(receipt.requestsSent).toBeGreaterThanOrEqual(50);
      expect(receipt.latency.overall.p95).toBeLessThanOrEqual(config.stormP95MaxMs);
    }, 3_600_000);
  });
}
