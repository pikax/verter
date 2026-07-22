import * as fs from "fs";
import * as path from "path";

import Mocha from "mocha";

/**
 * Mocha entry point for the VS Code acceptance lane.
 *
 * The lane deliberately keeps its own runner rather than reusing
 * `e2e/suite/index.ts`. That runner's root hook opens a fixture-specific
 * `App.vue` and waits on fixture-shaped readiness signals; the acceptance lane
 * runs against arbitrary real projects where no such file exists, and its whole
 * job is to MEASURE readiness rather than to presuppose it.
 *
 * A zero-test execution is refused for the same reason it is refused in the
 * fixture runner: `@vscode/test-electron` can exit 0 on a host that never ran
 * anything, and an acceptance lane that can silently execute nothing is not an
 * acceptance lane.
 */
export async function run(): Promise<void> {
  const mocha = new Mocha({ ui: "tdd", timeout: 600_000, color: true });
  const compiled = path.resolve(__dirname, "./acceptance.test.js");
  if (!fs.existsSync(compiled)) {
    throw new Error(`acceptance suite was not compiled: ${compiled}`);
  }
  mocha.addFile(compiled);

  return new Promise((resolve, reject) => {
    const passedTestIds: string[] = [];
    const pendingTestIds: string[] = [];
    const failedTests: Array<{ id: string; err: string }> = [];

    const runner = mocha.run((failures: number) => {
      const stats = runner.stats ?? { passes: 0, failures: 0, pending: 0 };
      const executed = (stats.passes ?? 0) + (stats.failures ?? 0) + (stats.pending ?? 0);
      const base = process.env.VERTER_E2E_LOG_FILE;
      if (base) {
        try {
          fs.writeFileSync(
            `${base}.runsummary`,
            `${JSON.stringify(
              {
                lane: "acceptance",
                passes: stats.passes ?? 0,
                failures: stats.failures ?? 0,
                pending: stats.pending ?? 0,
                executed,
                passedTestIds,
                pendingTestIds,
                failedTests,
              },
              null,
              2,
            )}\n`,
          );
        } catch {
          /* best-effort */
        }
      }

      if (failures > 0 || (stats.failures ?? 0) > 0) {
        const detail =
          failedTests.map((f) => `${f.id}: ${f.err}`).join(" | ") || "see mocha output";
        reject(
          new Error(
            `${Math.max(failures, stats.failures ?? 0)} acceptance test(s) failed — ${detail}`,
          ),
        );
        return;
      }
      if (executed === 0) {
        reject(new Error("acceptance lane executed 0 tests — vacuous pass refused"));
        return;
      }
      resolve();
    });
    runner.on("pass", (test) => passedTestIds.push(test.title));
    runner.on("pending", (test) => pendingTestIds.push(test.title));
    runner.on("fail", (test, err) =>
      failedTests.push({ id: test.title, err: err instanceof Error ? err.message : String(err) }),
    );
  });
}
