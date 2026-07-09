import * as fs from "fs";
import * as os from "os";
import { join } from "path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  clearRunArtifacts,
  d1MarkerPath,
  enforceRunSummary,
  runSummaryPath,
} from "./runSummaryOracle";

// Each test uses a fresh temp dir + log-file base so the real-FS sidecars never collide.
let dir: string;
let logFile: string;

beforeEach(() => {
  dir = fs.mkdtempSync(join(os.tmpdir(), "verter-oracle-"));
  logFile = join(dir, "verter-e2e-fixture.log");
});
afterEach(() => {
  fs.rmSync(dir, { recursive: true, force: true });
});

function writeSummary(summary: Record<string, unknown>): void {
  fs.writeFileSync(runSummaryPath(logFile), JSON.stringify(summary));
}

describe("clearRunArtifacts", () => {
  it("deletes BOTH the run-summary sidecar and the D1 marker before a run", () => {
    fs.writeFileSync(runSummaryPath(logFile), "{}");
    fs.writeFileSync(d1MarkerPath(logFile), "[D1] stale\n");
    expect(fs.existsSync(runSummaryPath(logFile))).toBe(true);
    expect(fs.existsSync(d1MarkerPath(logFile))).toBe(true);

    clearRunArtifacts(logFile);

    expect(fs.existsSync(runSummaryPath(logFile))).toBe(false);
    expect(fs.existsSync(d1MarkerPath(logFile))).toBe(false);
  });

  it("is a no-op (never throws) when the sidecars are absent", () => {
    expect(() => clearRunArtifacts(logFile)).not.toThrow();
  });
});

describe("enforceRunSummary — F8 stale/missing robustness", () => {
  // THE discriminating F8 case: a STALE green summary from a prior run + a current
  // zero-exit crash that writes no fresh summary must NOT false-green. The delete-before-run
  // removes the stale summary, so a D1/narrowed run with no fresh summary is refused. Without
  // `clearRunArtifacts`, the stale `{failures:0, executed:3}` would be read and PASS.
  it("STALE green summary + crash (no fresh summary) ⇒ refused after clearRunArtifacts (no false green)", async () => {
    writeSummary({ failures: 0, executed: 3 }); // a stale green from a PRIOR run
    clearRunArtifacts(logFile); // delete-before-run
    // The current run crashed at exit 0 and wrote NO fresh summary → must be refused.
    await expect(
      enforceRunSummary(logFile, "d1@tsgo", { refuseVacuous: true, pollMs: 0 }),
    ).rejects.toThrow(/no run summary|vacuous pass refused/i);
  });

  it("proves the guard discriminates: the SAME stale green WITHOUT the clear false-greens", async () => {
    writeSummary({ failures: 0, executed: 3 }); // stale green left in place (no clear)
    // Without the delete-before-run the stale summary is read and passes — this is exactly
    // the false-green the clear defends against, pinned here so the fix cannot silently regress.
    await expect(
      enforceRunSummary(logFile, "d1@tsgo", { refuseVacuous: true, pollMs: 0 }),
    ).resolves.toBeUndefined();
  });

  it("a MISSING summary for a D1/narrowed run throws (vacuous pass refused)", async () => {
    await expect(
      enforceRunSummary(logFile, "d1@tsgo", { refuseVacuous: true, pollMs: 0 }),
    ).rejects.toThrow(/no run summary|vacuous pass refused/i);
  });

  it("a MISSING summary for a NON-vacuous-refused run passes (legacy full-matrix behaviour preserved)", async () => {
    await expect(
      enforceRunSummary(logFile, "single-project@tsgo", { refuseVacuous: false, pollMs: 0 }),
    ).resolves.toBeUndefined();
  });
});

describe("enforceRunSummary — failure + zero-execution semantics", () => {
  it("throws on any reported test failure regardless of refuseVacuous", async () => {
    writeSummary({ failures: 2, executed: 5, rootHookError: null });
    await expect(
      enforceRunSummary(logFile, "d1@tsgo", { refuseVacuous: false, pollMs: 0 }),
    ).rejects.toThrow(/2 test\(s\) failed/);
  });

  it("surfaces a root-hook error alongside the failure count", async () => {
    writeSummary({ failures: 1, executed: 0, rootHookError: "boom in beforeAll" });
    await expect(
      enforceRunSummary(logFile, "d1@tsgo", { refuseVacuous: true, pollMs: 0 }),
    ).rejects.toThrow(/root hook error: boom in beforeAll/);
  });

  it("refuses a 0-test execution when refuseVacuous (D1/narrowed)", async () => {
    writeSummary({ failures: 0, executed: 0 });
    await expect(
      enforceRunSummary(logFile, "d1@tsgo", { refuseVacuous: true, pollMs: 0 }),
    ).rejects.toThrow(/executed 0 tests/);
  });

  it("allows a 0-test execution when NOT refuseVacuous (a non-narrowed, non-D1 fixture)", async () => {
    writeSummary({ failures: 0, executed: 0 });
    await expect(
      enforceRunSummary(logFile, "single-project@tsgo", { refuseVacuous: false, pollMs: 0 }),
    ).resolves.toBeUndefined();
  });

  it("passes a clean summary with executed>0 even under refuseVacuous", async () => {
    writeSummary({ failures: 0, executed: 4 });
    await expect(
      enforceRunSummary(logFile, "d1@tsgo", { refuseVacuous: true, pollMs: 0 }),
    ).resolves.toBeUndefined();
  });
});
