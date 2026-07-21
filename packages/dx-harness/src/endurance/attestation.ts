/**
 * Attestation (non-vacuity) for endurance runs.
 *
 * Every scenario emits a JSON receipt proving the run actually drove traffic:
 * request counters, latency percentiles (overall + per-window), max RSS,
 * provider liveness, and the post-load sanity verdict. Receipts are written
 * to `VERTER_ENDURANCE_RECEIPT` (a `.json` file path, or a directory that
 * receives `<scenario>-<route>-<ts>.json`) — otherwise to a temp file — AND
 * logged, so a green-but-idle run is impossible to mistake for coverage.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import type { EnduranceReceipt } from "./types.js";

/** Resolve the receipt destination; creates parent directories as needed. */
export function receiptDestination(receipt: EnduranceReceipt, envPath: string | null): string {
  if (envPath) {
    if (envPath.endsWith(".json")) {
      mkdirSync(path.dirname(path.resolve(envPath)), { recursive: true });
      return path.resolve(envPath);
    }
    const dir = path.resolve(envPath);
    mkdirSync(dir, { recursive: true });
    return path.join(
      dir,
      `${receipt.scenario}-${receipt.framework}-${receipt.mode}-${receipt.route}-${Date.now()}.json`,
    );
  }
  return path.join(
    tmpdir(),
    `verter-endurance-receipt-${receipt.scenario}-${receipt.framework}-${receipt.mode}-${receipt.route}-${Date.now()}.json`,
  );
}

/** Write the receipt to its destination and log the full JSON. Returns the path. */
export function writeReceipt(receipt: EnduranceReceipt, envPath: string | null): string {
  const destination = receiptDestination(receipt, envPath);
  const json = JSON.stringify(receipt, null, 2);
  writeFileSync(destination, json);
  console.log(`[endurance] receipt (${receipt.scenario} / ${receipt.route}) → ${destination}`);
  console.log(json);
  return destination;
}

/** The gates every spec asserts on top of scenario-specific checks. */
export function receiptCoreFailures(receipt: EnduranceReceipt): string[] {
  const failures: string[] = [];
  if (receipt.requestsSent <= 0) failures.push("requestsSent must be > 0 (vacuous run)");
  if (receipt.requestsUnanswered !== 0) {
    failures.push(`requestsUnanswered must be 0, got ${receipt.requestsUnanswered}`);
  }
  if (receipt.requestsErrored !== 0) {
    failures.push(`requestsErrored must be 0, got ${receipt.requestsErrored}`);
  }
  if (
    receipt.requestsSent !==
    receipt.requestsAnswered +
      receipt.requestsCancelled +
      receipt.requestsErrored +
      receipt.requestsUnanswered
  ) {
    failures.push("request counters violate sent === answered+cancelled+errored+unanswered");
  }
  if (!receipt.providerAliveAtEnd) failures.push("providerAliveAtEnd must be true");
  if (receipt.providerProcess.pid === null)
    failures.push("provider child/relay PID was not attested");
  if (receipt.restartCount !== 0) {
    failures.push(`restartCount must be 0, got ${receipt.restartCount}`);
  }
  // reloadProjects is the DESIGNED tsserver cold-miss recovery: singleflight +
  // 2s cooldown, so a healthy bounded lane sees AT MOST one genuine recovery
  // event. Zero is the norm; ONE is the mechanism working as designed (all
  // requests answered, final sanity green); MORE than one is the D2 storm
  // class the recovery bound exists to prevent — fail hard there.
  if (receipt.reloadProjectsCount > 1) {
    failures.push(
      `reloadProjectsCount must be at most 1 (one designed recovery event), got ${receipt.reloadProjectsCount}`,
    );
  }
  const laneSection = receipt.frameworks[receipt.framework]?.[receipt.mode];
  if (!laneSection) {
    failures.push(`missing ${receipt.framework}/${receipt.mode} receipt section`);
  } else if (
    laneSection.requestsSent !== receipt.requestsSent ||
    laneSection.requestsUnanswered !== receipt.requestsUnanswered ||
    laneSection.editsSent !== receipt.editsSent
  ) {
    failures.push(`${receipt.framework}/${receipt.mode} receipt section disagrees with totals`);
  }
  return failures;
}
