import * as fs from "fs";
import * as os from "os";
import { join } from "path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { clearRunArtifacts, enforceRunSummary, runSummaryPath } from "./runSummaryOracle";

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
  it("deletes the stale log and run summary before a run", () => {
    fs.writeFileSync(logFile, "stale log\n");
    fs.writeFileSync(runSummaryPath(logFile), "{}");

    clearRunArtifacts(logFile);

    expect(fs.existsSync(logFile)).toBe(false);
    expect(fs.existsSync(runSummaryPath(logFile))).toBe(false);
  });

  it("is a no-op when the artifacts are absent", () => {
    expect(() => clearRunArtifacts(logFile)).not.toThrow();
  });
});

describe("enforceRunSummary stale/missing robustness", () => {
  it("refuses a current run with no fresh summary after deleting a stale green summary", async () => {
    writeSummary({ failures: 0, executed: 3 });
    clearRunArtifacts(logFile);

    await expect(
      enforceRunSummary(logFile, "editor-owned-project@shared-tsgo", { pollMs: 0 }),
    ).rejects.toThrow(/no run summary|vacuous pass refused/i);
  });

  it("demonstrates that an uncleared stale green summary would be accepted", async () => {
    writeSummary({ failures: 0, executed: 3 });

    await expect(
      enforceRunSummary(logFile, "editor-owned-project@shared-tsgo", { pollMs: 0 }),
    ).resolves.toBeUndefined();
  });

  it("refuses a missing summary for every fixture", async () => {
    await expect(enforceRunSummary(logFile, "single-project@tsgo", { pollMs: 0 })).rejects.toThrow(
      /no run summary|vacuous pass refused/i,
    );
  });
});

describe("enforceRunSummary result semantics", () => {
  it("throws on any reported test failure", async () => {
    writeSummary({ failures: 2, executed: 5, rootHookError: null });
    await expect(
      enforceRunSummary(logFile, "editor-owned-project@shared-tsgo", { pollMs: 0 }),
    ).rejects.toThrow(/2 test\(s\) failed/);
  });

  it("surfaces a root-hook error alongside the failure count", async () => {
    writeSummary({ failures: 1, executed: 0, rootHookError: "boom in beforeAll" });
    await expect(
      enforceRunSummary(logFile, "editor-owned-project@shared-tsgo", { pollMs: 0 }),
    ).rejects.toThrow(/root hook error: boom in beforeAll/);
  });

  it("refuses a zero-test execution", async () => {
    writeSummary({ failures: 0, executed: 0 });
    await expect(
      enforceRunSummary(logFile, "editor-owned-project@shared-tsgo", { pollMs: 0 }),
    ).rejects.toThrow(/executed 0 tests/);
  });

  it("passes a clean summary with executed tests", async () => {
    writeSummary({ failures: 0, executed: 4 });
    await expect(
      enforceRunSummary(logFile, "editor-owned-project@shared-tsgo", { pollMs: 0 }),
    ).resolves.toBeUndefined();
  });
});
