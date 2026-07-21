/** Build-from-scratch endurance lanes against the real LSP. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  ENDURANCE_LANES,
  buildComponentFixture,
  loadEnduranceConfig,
  runBuildComponentScenario,
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
  describe.sequential(`endurance: build-component [${lane.id}/${config.route}]`, () => {
    let rig: EnduranceRig;
    let receipt: EnduranceReceipt | null = null;
    const fixture = buildComponentFixture(lane);

    beforeAll(async () => {
      rig = await materializeRig(fixture.files, config);
    });
    afterAll(async () => {
      await disposeRig(rig);
    });

    it("types props, events, and snippets with completion during typing", async () => {
      receipt = await runBuildComponentScenario(
        scenarioContext(rig, "build-component-from-scratch", lane),
        fixture,
      );
      attestReceipt(receipt);
      expect(receipt.framework).toBe(lane.framework);
      expect(receipt.mode).toBe(lane.mode);
      expect(receipt.frameworks[lane.framework]?.[lane.mode]).toBeDefined();
      expect(receipt.editsSent).toBeGreaterThan(100);
    }, 600_000);
  });
}
