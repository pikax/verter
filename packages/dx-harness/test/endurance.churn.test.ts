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
import { writeFileSync } from "node:fs";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  DEFAULT_ENDURANCE_LANE,
  describeProcessTreeRss,
  describeRetentionReading,
  describeWireBytes,
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
    // The server's stderr is the only record of what it was doing when a run
    // hangs or fails; keep it when asked (VERTER_ENDURANCE_STDERR_FILE).
    const stderrFile = process.env.VERTER_ENDURANCE_STDERR_FILE;
    if (stderrFile) writeFileSync(stderrFile, rig.handle.client.stderr.text());
    await disposeRig(rig);
  });

  it("holds process-tree memory flat across open/edit/query/close cycles", async () => {
    const serverPid = rig.handle.client.process.pid;
    expect(serverPid, "the spawned server must expose a pid to root the tree").toBeDefined();

    const result: ChurnScenarioResult = await runChurnScenario(
      scenarioContext(rig, "churn", lane),
      { serverPid: serverPid!, fixture },
    );

    // The evidence prints BEFORE any verdict is asserted, so a failing run still
    // leaves its readings in the log for whoever has to explain it.
    console.log(
      `[endurance] churn ${result.cyclesCompleted} cycles\n` +
        result.checkpoints
          .map(
            (checkpoint) =>
              `  cycle ${checkpoint.cyclesCompleted}${checkpoint.quiesced ? "" : " (NOT quiesced)"}: ` +
              `${describeProcessTreeRss(checkpoint.sample)}\n` +
              `    ${describeRetentionReading(checkpoint.retention)}
` +
              `    ${describeWireBytes(checkpoint.wireBytes)}`,
          )
          .join("\n") +
        `\n  envelope:  ${result.growth.detail}` +
        `\n  slope:     ${result.slope.detail}` +
        `\n  retention: ${result.retention.detail}`,
    );

    attestReceipt(result.receipt, { requireFinalSanity: true });
    expectRssWithinCeiling(result.receipt, config);
    expect(result.receipt.editsSent).toBeGreaterThanOrEqual(result.cyclesCompleted);

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
    expect(result.quiescedAtBothCheckpoints, "every reading must follow host quiescence").toBe(
      true,
    );
    expect(
      result.growth.pass,
      `process-tree memory grew across ${result.cyclesCompleted} open/edit/query/close cycles: ${result.growth.detail}`,
    ).toBe(true);

    // WSP6-AC1 is a claim about the TRAJECTORY, not the destination: "1000
    // open/edit/close cycles show no unbounded retained-byte slope after
    // quiescence". The envelope above admits a constant per-cycle drip that
    // happens to land inside `baseline * factor + floor`; the per-window slope
    // does not, and it refuses to evaluate a run shorter than the criterion's
    // own 1000 cycles (a shorter run is a smoke lane, never this proof).
    expect(
      result.slope.observable,
      `the retained-byte slope was NOT evaluated: ${result.slope.detail}`,
    ).toBe(true);
    expect(
      result.slope.pass,
      `retained bytes kept growing per cycle after quiescence: ${result.slope.detail}`,
    ).toBe(true);

    // WSP6.1 measures OBJECT LIFETIMES alongside bytes: the host reports its
    // retained set (live/retired artifact versions, captured roots, parse
    // leases) at every quiesced checkpoint, and none of those counters may
    // grow with the number of cycles. WSP6.3: pressure outcomes must not occur
    // on the standard corpus, so the aggregate account's pressure refusals
    // must read zero. An unreported retention is an UNAVAILABLE metric — red,
    // never a narrower pass.
    expect(
      result.retention.observable,
      `the retained-object trend was NOT evaluated: ${result.retention.detail}`,
    ).toBe(true);
    expect(
      result.retention.pass,
      `retained objects accumulated across ${result.cyclesCompleted} cycles: ${result.retention.detail}`,
    ).toBe(true);
  }, 3_600_000);
});
