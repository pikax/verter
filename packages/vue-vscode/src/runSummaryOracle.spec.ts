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

  it("includes failedTests detail in the failure message", async () => {
    writeSummary({
      failures: 1,
      executed: 2,
      failedTests: [{ id: "vue.foo", err: "PRODUCT_GAP ISSUE-x: boom" }],
    });
    await expect(enforceRunSummary(logFile, "vue-parity@tsserver", { pollMs: 0 })).rejects.toThrow(
      /PRODUCT_GAP ISSUE-x: boom/,
    );
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

  it("refuses pending tests even when the run has no named capability manifest", async () => {
    writeSummary({
      failures: 0,
      executed: 2,
      passedTestIds: ["activation starts"],
      pendingTestIds: ["hover fixture token is absent"],
    });

    await expect(
      enforceRunSummary(logFile, "single-project@tsserver", { pollMs: 0 }),
    ).rejects.toThrow(/pending test.*hover fixture token is absent/i);
  });

  it("refuses pending tests for a required capability contract", async () => {
    writeSummary({
      failures: 0,
      executed: 2,
      passedTestIds: ["vue.ts.definition"],
      pendingTestIds: ["vue.ts.rename"],
    });

    await expect(
      enforceRunSummary(logFile, "vue-contract@tsserver", {
        pollMs: 0,
        requiredTestIds: ["vue.ts.definition", "vue.ts.rename"],
      }),
    ).rejects.toThrow(/pending test.*vue\.ts\.rename/i);
  });

  it("refuses any passed ID outside the complete required inventory", async () => {
    writeSummary({
      failures: 0,
      executed: 3,
      passedTestIds: [
        "vue.clean-diagnostics.daily",
        "vue.definition.markup-to-script",
        "vue.extra.optional",
      ],
      pendingTestIds: [],
    });

    await expect(
      enforceRunSummary(logFile, "vue-parity@tsserver", {
        pollMs: 0,
        requiredTestIds: ["vue.clean-diagnostics.daily", "vue.definition.markup-to-script"],
      }),
    ).rejects.toThrow(/unexpected: vue\.extra\.optional/);
  });

  it("refuses a missing required ID", async () => {
    writeSummary({
      failures: 0,
      executed: 1,
      passedTestIds: ["vue.extra.optional"],
      pendingTestIds: [],
    });

    await expect(
      enforceRunSummary(logFile, "vue-parity@tsgo", {
        pollMs: 0,
        requiredTestIds: ["vue.clean-diagnostics.daily"],
      }),
    ).rejects.toThrow(/missing: vue\.clean-diagnostics\.daily/);
  });

  it("refuses a missing or duplicate required capability contract ID", async () => {
    writeSummary({
      failures: 0,
      executed: 3,
      passedTestIds: ["vue.ts.definition", "vue.ts.definition", "vue.ts.rename"],
      pendingTestIds: [],
    });

    await expect(
      enforceRunSummary(logFile, "vue-contract@tsgo", {
        pollMs: 0,
        requiredTestIds: ["vue.ts.definition", "vue.ts.rename", "vue.ts.references"],
      }),
    ).rejects.toThrow(/duplicate.*vue\.ts\.definition.*missing.*vue\.ts\.references/i);
  });

  it("accepts exact required capability contract coverage", async () => {
    writeSummary({
      failures: 0,
      executed: 2,
      passedTestIds: ["svelte.js.definition", "svelte.js.rename"],
      pendingTestIds: [],
    });

    await expect(
      enforceRunSummary(logFile, "svelte-contract@shared-tsgo", {
        pollMs: 0,
        requiredTestIds: ["svelte.js.definition", "svelte.js.rename"],
      }),
    ).resolves.toBeUndefined();
  });

  it("attests the exact fixture and loaded suite-file inventory", async () => {
    writeSummary({
      failures: 0,
      executed: 2,
      fixture: "svelte-parity",
      loadedFiles: ["parity/svelte/daily.test.js", "parity/svelte/matrix.test.js"],
    });

    await expect(
      enforceRunSummary(logFile, "svelte-parity@tsgo", {
        expectedFixture: "svelte-parity",
        pollMs: 0,
        requiredLoadedFiles: ["parity/svelte/daily.test.js", "parity/svelte/matrix.test.js"],
      }),
    ).resolves.toBeUndefined();
  });

  it("refuses a run summary produced by a different provider route", async () => {
    writeSummary({
      failures: 0,
      executed: 1,
      fixture: "svelte-parity",
      typeProvider: "tsgo",
      passedTestIds: ["svelte.clean-diagnostics.daily"],
      pendingTestIds: [],
    });

    await expect(
      enforceRunSummary(logFile, "svelte-parity@shared-tsgo", {
        expectedFixture: "svelte-parity",
        expectedTypeProvider: "shared-tsgo",
        pollMs: 0,
      }),
    ).rejects.toThrow(/provider route mismatch/i);
  });

  it("refuses missing, unexpected, duplicate, or wrong-fixture run inventory", async () => {
    writeSummary({
      failures: 0,
      executed: 1,
      fixture: "vue-parity",
      loadedFiles: ["parity/vue/daily.test.js", "parity/vue/daily.test.js"],
    });

    await expect(
      enforceRunSummary(logFile, "svelte-parity@shared-tsgo", {
        expectedFixture: "svelte-parity",
        pollMs: 0,
        requiredLoadedFiles: ["parity/svelte/daily.test.js"],
      }),
    ).rejects.toThrow(/fixture mismatch/i);

    writeSummary({
      failures: 0,
      executed: 1,
      fixture: "svelte-parity",
      loadedFiles: ["parity/svelte/daily.test.js", "parity/svelte/daily.test.js"],
    });
    await expect(
      enforceRunSummary(logFile, "svelte-parity@shared-tsgo", {
        expectedFixture: "svelte-parity",
        pollMs: 0,
        requiredLoadedFiles: ["parity/svelte/matrix.test.js"],
      }),
    ).rejects.toThrow(/duplicate paths/i);
  });
});
