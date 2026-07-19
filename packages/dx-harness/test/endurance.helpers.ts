/**
 * Shared rig for endurance specs: config → workspace → spawned verter-lsp →
 * instrumented session + RSS sampler, plus receipt assertions. Each spec runs
 * one provider route (VERTER_ENDURANCE_PROVIDER, default tsgo); CI runs the
 * suite once per route.
 */
import { expect } from "vitest";

import {
  disposeWorkspace,
  EnduranceSession,
  LatencyRecorder,
  loadEnduranceConfig,
  materializeWorkspace,
  receiptCoreFailures,
  RequestTracker,
  RssSampler,
  spawnEnduranceLsp,
  writeReceipt,
  type EnduranceConfig,
  type EnduranceLspHandle,
  type EnduranceLane,
  type EnduranceReceipt,
  type ScenarioContext,
  type WorkspaceFiles,
} from "../src/endurance/index.js";

export interface EnduranceRig {
  readonly config: EnduranceConfig;
  readonly workspaceRoot: string;
  readonly handle: EnduranceLspHandle;
  readonly session: EnduranceSession;
  readonly sampler: RssSampler | null;
  /** True when the rig owns workspaceRoot (a temp dir it must clean up). */
  readonly ownsWorkspace: boolean;
}

export function scenarioContext(
  rig: EnduranceRig,
  scenario: string,
  lane: EnduranceLane,
): ScenarioContext {
  return {
    scenario,
    route: rig.config.route,
    lane,
    session: rig.session,
    config: rig.config,
    sampler: rig.sampler,
    providerAttestation: () => rig.handle.providerAttestation(),
  };
}

/** Spawn a rig against an EXISTING root (scale lane; nothing is written there). */
export async function spawnRig(
  workspaceRoot: string,
  config: EnduranceConfig,
  ownsWorkspace: boolean,
): Promise<EnduranceRig> {
  const handle = await spawnEnduranceLsp(config.route, workspaceRoot);
  try {
    const tracker = new RequestTracker();
    const recorder = new LatencyRecorder(config.windowMs);
    const session = new EnduranceSession(handle.client, handle.workspaceRoot, {
      config,
      recorder,
      tracker,
    });
    const pid = handle.client.process.pid;
    const sampler = pid !== undefined ? new RssSampler(pid, config.rssSampleMs) : null;
    return { config, workspaceRoot: handle.workspaceRoot, handle, session, sampler, ownsWorkspace };
  } catch (error) {
    await handle.dispose();
    throw error;
  }
}

/** Materialize a synthetic workspace into a temp dir and spawn a rig on it. */
export async function materializeRig(
  files: WorkspaceFiles,
  config: EnduranceConfig,
): Promise<EnduranceRig> {
  const workspaceRoot = materializeWorkspace(files);
  try {
    return await spawnRig(workspaceRoot, config, true);
  } catch (error) {
    disposeWorkspace(workspaceRoot);
    throw error;
  }
}

export async function disposeRig(rig: EnduranceRig): Promise<void> {
  rig.sampler?.stop();
  await rig.handle.dispose();
  if (rig.ownsWorkspace) disposeWorkspace(rig.workspaceRoot);
}

/** Write the receipt, then hard-assert the non-vacuity gates + scenario failures. */
export function attestReceipt(
  receipt: EnduranceReceipt,
  options: { requireFinalSanity?: boolean } = {},
): void {
  writeReceipt(receipt, process.env.VERTER_ENDURANCE_RECEIPT ?? null);
  const problems: string[] = [...receiptCoreFailures(receipt), ...receipt.failures];
  if (options.requireFinalSanity && receipt.finalSanityPass !== true) {
    problems.push(`finalSanityPass must be true, got ${String(receipt.finalSanityPass)}`);
  }
  expect(problems, `endurance receipt problems:\n${problems.join("\n")}`).toEqual([]);
  expect(receipt.requestsSent).toBeGreaterThan(0);
  expect(receipt.requestsUnanswered).toBe(0);
  expect(receipt.providerAliveAtEnd).toBe(true);
  expect(receipt.restartCount).toBe(0);
  // At most ONE designed singleflight recovery event per lane (see attestation.ts).
  expect(receipt.reloadProjectsCount).toBeLessThanOrEqual(1);
  const section = receipt.frameworks[receipt.framework]?.[receipt.mode];
  expect(section, `missing ${receipt.framework}/${receipt.mode} receipt section`).toBeDefined();
  expect(section?.requestsSent).toBe(receipt.requestsSent);
  expect(section?.requestsUnanswered).toBe(0);
}

/** RSS ceiling check — skipped with an explicit note when the platform can't read it. */
export function expectRssWithinCeiling(receipt: EnduranceReceipt, config: EnduranceConfig): void {
  if (!receipt.rssSupported || receipt.maxRssBytes === null) {
    console.log(
      `[endurance] RSS sampling unsupported on ${process.platform} — RSS assertion skipped (explicitly, not vacuously)`,
    );
    return;
  }
  expect(
    receipt.maxRssBytes,
    `max RSS ${receipt.maxRssBytes} exceeds ceiling ${config.rssMaxBytes}`,
  ).toBeLessThanOrEqual(config.rssMaxBytes);
}
