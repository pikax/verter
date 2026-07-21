/** Mixed-workload soak lanes against the real LSP. */
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  ENDURANCE_LANES,
  loadEnduranceConfig,
  runSoakScenario,
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

for (const lane of ENDURANCE_LANES) {
  describe.sequential(`endurance: soak [${lane.id}/${config.route}]`, () => {
    let rig: EnduranceRig;
    let receipt: EnduranceReceipt | null = null;
    const workspace = soakWorkspace(4, lane);

    beforeAll(async () => {
      rig = await materializeRig(workspace.files, config);
      rig.session.openFile(workspace.childPath);
      rig.session.openFile(workspace.appPath);
      for (const carrier of workspace.carriers) rig.session.openFile(carrier);
      rig.session.openFile(workspace.scratchPath, "");
    });
    afterAll(async () => {
      await disposeRig(rig);
    });

    it("keeps latency/RSS bounded with a clean final sanity pass", async () => {
      receipt = await runSoakScenario(scenarioContext(rig, "soak", lane), {
        probes: soakProbes(workspace.carriers, lane),
        typingFile: {
          relativePath: workspace.scratchPath,
          typedText: workspace.typedDocument,
        },
      });
      attestReceipt(receipt, { requireFinalSanity: true });
      expect(receipt.frameworks[lane.framework]?.[lane.mode]).toBeDefined();
      expectRssWithinCeiling(receipt, config);
      expect(receipt.latency.overall.p95).toBeLessThanOrEqual(config.p95MaxMs);
      if (receipt.degradationCheck) {
        const check = receipt.degradationCheck;
        expect(
          check.pass,
          `meaningful degradation: early=${check.earlyWindowP95} late=${check.lateWindowP95} ` +
            `factor=${check.factor} floor=${check.floorMs}`,
        ).toBe(true);
        expect(check.lateWindowP95).toBeLessThanOrEqual(config.p95MaxMs);
      } else {
        console.log(
          "[endurance] soak has fewer than two usable windows; trend check skipped explicitly",
        );
      }
    }, 3_600_000);
  });
}
