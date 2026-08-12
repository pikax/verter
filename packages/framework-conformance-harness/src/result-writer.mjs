// Atomic result publication. Every harness run that produces a result
// artifact (coverage report, comparison run) goes through this module so
// "no partial artifact publication on success or refusal" holds uniformly,
// not per-caller.
//
// Contract: `runAtomic` invokes `work()`, which may accumulate partial state
// internally, but nothing reaches disk until `work()` RETURNS. If `work()`
// throws at any point — including after producing 999 of 1000 results — the
// target path is left exactly as it was before the call: no file, no
// truncated file, no directory listing showing an in-progress artifact. The
// temp-file + rename step only happens after `work()` has already returned
// successfully, so a mid-flight failure has nothing to rename.

import { mkdirSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

export class PartialResultError extends Error {
  constructor(cause) {
    super(`atomic result publication refused: ${cause.message ?? cause}`);
    this.name = "PartialResultError";
    this.cause = cause;
  }
}

/**
 * @param {string} outPath
 * @param {() => object} work MUST be synchronous and MUST NOT itself write
 *   to `outPath` — its return value is the sole thing ever written.
 * @returns {object} the value `work()` returned, mirrored to disk
 */
export function runAtomic(outPath, work) {
  let result;
  try {
    result = work();
  } catch (error) {
    // Nothing was ever written for this attempt — the caller sees the
    // thrown error and the target path is untouched.
    throw new PartialResultError(error);
  }
  mkdirSync(dirname(outPath), { recursive: true });
  const tmpPath = `${outPath}.tmp-${process.pid}-${process.hrtime.bigint() % 1_000_000n}`;
  writeFileSync(tmpPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
  try {
    renameSync(tmpPath, outPath);
  } catch (error) {
    try {
      unlinkSync(tmpPath);
    } catch {
      /* best-effort cleanup */
    }
    throw error;
  }
  return result;
}
