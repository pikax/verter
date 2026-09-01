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
    writeSummary({ failures: 0, executed: 3, passedTestIds: ["stale pass"] });
    clearRunArtifacts(logFile);

    await expect(
      enforceRunSummary(logFile, "editor-owned-project@shared-tsgo", { pollMs: 0 }),
    ).rejects.toThrow(/no run summary|vacuous pass refused/i);
  });

  it("demonstrates that an uncleared stale green summary would be accepted", async () => {
    writeSummary({ failures: 0, executed: 3, passedTestIds: ["stale pass"] });

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
    writeSummary({ failures: 0, executed: 4, passedTestIds: ["activation starts"] });
    await expect(
      enforceRunSummary(logFile, "editor-owned-project@shared-tsgo", { pollMs: 0 }),
    ).resolves.toBeUndefined();
  });

  it("allows fixture-inapplicable pending rows only when an unmanifested run has a real pass", async () => {
    writeSummary({
      failures: 0,
      executed: 2,
      passedTestIds: ["activation starts"],
      pendingTestIds: ["TS plugin: definition from barrel file navigates to .vue (not .vue.d.ts)"],
    });

    await expect(
      enforceRunSummary(logFile, "single-project@tsserver", { pollMs: 0 }),
    ).resolves.toBeUndefined();

    writeSummary({
      failures: 0,
      executed: 1,
      passedTestIds: [],
      pendingTestIds: ["fixture-specific optional row"],
    });
    await expect(
      enforceRunSummary(logFile, "single-project@tsserver", { pollMs: 0 }),
    ).rejects.toThrow(/no passing test IDs/i);
  });

  it("accepts only the exact explicitly allowed pending manifest", async () => {
    writeSummary({
      failures: 0,
      executed: 2,
      passedTestIds: ["activation starts"],
      pendingTestIds: ["fixture-specific optional row"],
    });

    await expect(
      enforceRunSummary(logFile, "single-project@tsserver", {
        pollMs: 0,
        allowedPendingTestIds: ["fixture-specific optional row"],
      }),
    ).resolves.toBeUndefined();
    await expect(
      enforceRunSummary(logFile, "single-project@tsserver", {
        pollMs: 0,
        allowedPendingTestIds: ["different row"],
      }),
    ).rejects.toThrow(
      /pending.*missing.*different row.*unexpected.*fixture-specific optional row/i,
    );
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
    ).rejects.toThrow(/pending manifest mismatch.*vue\.ts\.rename/i);
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

  it("accepts an exact parity inventory split between passes and skipped product gaps", async () => {
    writeSummary({
      failures: 0,
      executed: 2,
      passedTestIds: ["vue.clean-diagnostics.daily"],
      pendingTestIds: ["vue.definition.markup-to-script"],
      skippedProductGaps: [
        {
          id: "vue.definition.markup-to-script",
          issue: "ISSUE-vue-definition",
        },
      ],
      failedTests: [],
    });

    await expect(
      enforceRunSummary(logFile, "vue-parity@tsserver", {
        pollMs: 0,
        requiredTestIds: ["vue.clean-diagnostics.daily", "vue.definition.markup-to-script"],
        allowedProductGaps: {
          "vue.definition.markup-to-script": "ISSUE-vue-definition",
        },
      }),
    ).resolves.toBeUndefined();
  });

  it("refuses a product-gap failure even when the route manifest allows that row", async () => {
    writeSummary({
      failures: 1,
      executed: 1,
      passedTestIds: [],
      pendingTestIds: [],
      skippedProductGaps: [],
      failedTests: [
        {
          id: "vue.definition.markup-to-script",
          err: "PRODUCT_GAP ISSUE-vue-definition vue.definition.markup-to-script: ran instead of skipping",
          kind: "test",
        },
      ],
    });

    await expect(
      enforceRunSummary(logFile, "vue-parity@tsgo", {
        pollMs: 0,
        requiredTestIds: ["vue.definition.markup-to-script"],
        allowedProductGaps: {
          "vue.definition.markup-to-script": "ISSUE-vue-definition",
        },
      }),
    ).rejects.toThrow(/1 test\(s\) failed.*ran instead of skipping/i);
  });

  it("refuses duplicate or missing parity outcome rows", async () => {
    writeSummary({
      failures: 0,
      executed: 2,
      passedTestIds: ["vue.clean-diagnostics.daily"],
      pendingTestIds: ["vue.clean-diagnostics.daily"],
      skippedProductGaps: [
        {
          id: "vue.clean-diagnostics.daily",
          issue: "ISSUE-clean",
        },
      ],
      failedTests: [],
    });

    await expect(
      enforceRunSummary(logFile, "vue-parity@shared-tsgo", {
        pollMs: 0,
        requiredTestIds: ["vue.clean-diagnostics.daily", "vue.definition.markup-to-script"],
        allowedProductGaps: {
          "vue.clean-diagnostics.daily": "ISSUE-clean",
        },
      }),
    ).rejects.toThrow(/duplicate.*vue\.clean-diagnostics\.daily.*missing.*markup-to-script/i);
  });

  it("refuses root-hook failures even when every recorded test failure is a product gap", async () => {
    writeSummary({
      failures: 1,
      executed: 1,
      rootHookError: "provider warmup failed",
      passedTestIds: [],
      pendingTestIds: ["vue.clean-diagnostics.daily"],
      skippedProductGaps: [{ id: "vue.clean-diagnostics.daily", issue: "ISSUE-clean" }],
      failedTests: [],
    });

    await expect(
      enforceRunSummary(logFile, "vue-parity@tsserver", {
        pollMs: 0,
        requiredTestIds: ["vue.clean-diagnostics.daily"],
        allowedProductGaps: {
          "vue.clean-diagnostics.daily": "ISSUE-clean",
        },
      }),
    ).rejects.toThrow(/root hook error.*provider warmup failed/i);
  });

  it("refuses a hook failure even when it imitates the old product-gap marker", async () => {
    writeSummary({
      failures: 1,
      executed: 1,
      passedTestIds: [],
      pendingTestIds: [],
      skippedProductGaps: [],
      failedTests: [
        {
          id: '"after all" hook',
          err: 'PRODUCT_GAP ISSUE-clean "after all" hook: teardown failed',
          kind: "hook",
        },
      ],
    });

    await expect(
      enforceRunSummary(logFile, "vue-parity@tsserver", {
        pollMs: 0,
        requiredTestIds: ["vue.clean-diagnostics.daily"],
        allowedProductGaps: {
          "vue.clean-diagnostics.daily": "ISSUE-clean",
        },
      }),
    ).rejects.toThrow(/1 test\(s\) failed/i);
  });

  it("refuses missing, unexpected, or wrongly classified product-gap skips", async () => {
    const options = {
      pollMs: 0,
      requiredTestIds: ["vue.clean-diagnostics.daily", "vue.definition.markup-to-script"],
      allowedProductGaps: {
        "vue.definition.markup-to-script": "ISSUE-vue-definition",
      },
    } as const;

    writeSummary({
      failures: 0,
      executed: 2,
      passedTestIds: ["vue.clean-diagnostics.daily"],
      pendingTestIds: [],
      skippedProductGaps: [],
    });
    await expect(enforceRunSummary(logFile, "vue-parity@tsserver", options)).rejects.toThrow(
      /product-gap skip manifest mismatch.*missing.*vue\.definition\.markup-to-script/i,
    );

    writeSummary({
      failures: 0,
      executed: 2,
      passedTestIds: ["vue.clean-diagnostics.daily"],
      pendingTestIds: ["vue.definition.markup-to-script"],
      skippedProductGaps: [
        { id: "vue.definition.markup-to-script", issue: "ISSUE-wrong-classification" },
      ],
    });
    await expect(enforceRunSummary(logFile, "vue-parity@tsserver", options)).rejects.toThrow(
      /product-gap skip manifest mismatch.*issue mismatch/i,
    );

    writeSummary({
      failures: 0,
      executed: 3,
      passedTestIds: ["vue.clean-diagnostics.daily"],
      pendingTestIds: ["vue.definition.markup-to-script", "vue.unapproved-gap"],
      skippedProductGaps: [
        { id: "vue.definition.markup-to-script", issue: "ISSUE-vue-definition" },
        { id: "vue.unapproved-gap", issue: "ISSUE-unapproved" },
      ],
    });
    await expect(enforceRunSummary(logFile, "vue-parity@tsserver", options)).rejects.toThrow(
      /product-gap skip manifest mismatch.*unexpected.*vue\.unapproved-gap/i,
    );
  });

  it("attests the exact fixture and loaded suite-file inventory", async () => {
    writeSummary({
      failures: 0,
      executed: 2,
      passedTestIds: ["svelte.clean-diagnostics.daily"],
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
      passedTestIds: ["vue.clean-diagnostics.daily"],
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
      passedTestIds: ["svelte.clean-diagnostics.daily"],
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
