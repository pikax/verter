/**
 * Document-lifecycle churn against the real LSP, with process-TREE resident
 * memory compared across two quiesced checkpoints.
 *
 * This is the lane that answers "does a long editing session grow?" at the
 * level a user experiences it — the operating system's figure for the server
 * process AND its descendants, the type-provider engine included — rather than
 * the figure one in-process accountant happens to be asked to charge.
 *
 * One lane (the default vue-ts lane), deliberately: the property under test is
 * the host's document-lifecycle retention, which is carrier-agnostic. Running
 * the framework × mode matrix here would multiply a multi-minute run without
 * discriminating anything the first lane does not.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  DEFAULT_ENDURANCE_LANE,
  describeProcessTreeRss,
  loadEnduranceConfig,
  runChurnScenario,
  churnFixture,
  type ChurnScenarioResult,
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
const lane = DEFAULT_ENDURANCE_LANE;

describe.sequential(`endurance: churn [${lane.id}/${config.route}]`, () => {
  let rig: EnduranceRig;
  const fixture = churnFixture(config.churnCarrierBlocks, lane);

  beforeAll(async () => {
    rig = await materializeRig(fixture.files, config, lane);
  });
  afterAll(async () => {
    await disposeRig(rig);
  });

  it("holds process-tree memory flat across open/edit/query/close cycles", async () => {
    const serverPid = rig.handle.client.process.pid;
    expect(serverPid, "the spawned server must expose a pid to root the tree").toBeDefined();

    const result: ChurnScenarioResult = await runChurnScenario(
      scenarioContext(rig, "churn", lane),
      { serverPid: serverPid!, fixture },
    );

    attestReceipt(result.receipt, { requireFinalSanity: true });
    expectRssWithinCeiling(result.receipt, config);
    expect(result.receipt.editsSent).toBeGreaterThanOrEqual(result.cyclesCompleted);

    console.log(
      `[endurance] churn ${result.cyclesCompleted} cycles\n` +
        `  baseline: ${describeProcessTreeRss(result.baseline)}\n` +
        `  final:    ${describeProcessTreeRss(result.final)}\n` +
        `  verdict:  ${result.growth.detail}`,
    );

    // An UNAVAILABLE metric is a missing proof, not a pass. WSP6-AC1 is a claim
    // about the whole process tree after quiescence; a checkpoint that could not
    // enumerate the tree, could not read a discovered member, or lost a baseline
    // member before the final reading has not produced that evidence — and a
    // root-only or provider-less reading is exactly the shape that stays flat
    // while the type-provider child retains every document version. So the lane
    // goes RED on an unevaluated bound rather than green on a narrower subset.
    expect(
      result.growth.observable,
      `the process-tree growth bound was NOT evaluated on platform ${process.platform}: ` +
        `${result.growth.detail}\n` +
        `  baseline: ${describeProcessTreeRss(result.baseline)}\n` +
        `  final:    ${describeProcessTreeRss(result.final)}`,
    ).toBe(true);
    expect(result.quiescedAtBothCheckpoints, "both readings must follow host quiescence").toBe(
      true,
    );
    expect(
      result.growth.pass,
      `process-tree memory grew across ${result.cyclesCompleted} open/edit/query/close cycles: ${result.growth.detail}`,
    ).toBe(true);
  }, 3_600_000);
});
