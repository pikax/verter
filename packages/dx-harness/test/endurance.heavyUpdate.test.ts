/** Heavy-update endurance lanes against the real LSP. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  ENDURANCE_LANES,
  heavyUpdateFixture,
  loadEnduranceConfig,
  runHeavyUpdateScenario,
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
  describe.sequential(`endurance: heavy-update [${lane.id}/${config.route}]`, () => {
    let rig: EnduranceRig;
    let receipt: EnduranceReceipt | null = null;
    const fixture = heavyUpdateFixture(lane);

    beforeAll(async () => {
      rig = await materializeRig(fixture.files, config);
    });
    afterAll(async () => {
      await disposeRig(rig);
    });

    it("keeps every edit-to-query response current", async () => {
      receipt = await runHeavyUpdateScenario(scenarioContext(rig, "heavy-update", lane), {
        fixture,
      });
      attestReceipt(receipt);
      expect(receipt.frameworks[lane.framework]?.[lane.mode]).toBeDefined();
      expect(receipt.editsSent).toBeGreaterThanOrEqual(config.heavyUpdateCycles * 5);
      expect(receipt.requestsSent).toBeGreaterThanOrEqual(config.heavyUpdateCycles * 6);
    }, 3_600_000);
  });
}
