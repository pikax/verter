/**
 * Endurance scenario 2 — heavy-update loops.
 *
 * N (default 200, VERTER_ENDURANCE_HEAVY_UPDATE_CYCLES) edit→query cycles:
 * rename a member, add/remove a prop, break+fix syntax; every post-edit
 * response must reflect the CURRENT content (converging within a hard
 * deadline), with zero unanswered requests and a live provider throughout.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  HEAVY_UPDATE_FILES,
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

describe.sequential(`endurance: heavy-update [${config.route}]`, () => {
  let rig: EnduranceRig;
  let receipt: EnduranceReceipt | null = null;

  beforeAll(async () => {
    rig = await materializeRig(HEAVY_UPDATE_FILES, config);
  });

  afterAll(async () => {
    await disposeRig(rig);
  });

  it("keeps every edit→query response current across all cycles", async () => {
    receipt = await runHeavyUpdateScenario(scenarioContext(rig, "heavy-update"));
    attestReceipt(receipt);
    // 5 edits + ~6 tracked requests per cycle prove the loop really ran.
    expect(receipt.editsSent).toBeGreaterThanOrEqual(config.heavyUpdateCycles * 5);
    expect(receipt.requestsSent).toBeGreaterThanOrEqual(config.heavyUpdateCycles * 6);
  }, 3_600_000);
});
