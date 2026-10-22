import { LOG_FILE } from "../helpers";
import { recordOwnedProcesses } from "../lib/teardownOwnership";
import { getTimer } from "../timer";

/**
 * Root-level teardown to flush timing data at the end of all test suites.
 * This file should be discovered and loaded by Mocha's test runner.
 * The underscore prefix ensures it sorts last alphabetically.
 */
suiteTeardown(async function () {
  getTimer().flush();
  console.log("\n    Timing report written.");
  // The launcher checks, after the editor exits, that none of these survived.
  await recordOwnedProcesses(LOG_FILE, process.pid);
});
