/**
 * Endurance scenario 1 — build-a-component-from-scratch.
 *
 * Keystroke-level typing of two SFCs from empty buffers against the REAL
 * verter-lsp (route from VERTER_ENDURANCE_PROVIDER), with completion/hover/
 * definition asserted at realistic mid-typing points. Zero unanswered
 * requests; provider alive at end; JSON attestation receipt emitted.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  BUILD_COMPONENT_FILES,
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

describe.sequential(`endurance: build-component-from-scratch [${config.route}]`, () => {
  let rig: EnduranceRig;
  let receipt: EnduranceReceipt | null = null;

  beforeAll(async () => {
    rig = await materializeRig(BUILD_COMPONENT_FILES, config);
  });

  afterAll(async () => {
    await disposeRig(rig);
  });

  it("types two SFCs from scratch with asserted mid-typing probes", async () => {
    receipt = await runBuildComponentScenario(scenarioContext(rig, "build-component-from-scratch"));
    attestReceipt(receipt);
    // Keystroke-level typing must have produced substantial edit traffic.
    expect(receipt.editsSent).toBeGreaterThan(100);
  }, 600_000);
});
