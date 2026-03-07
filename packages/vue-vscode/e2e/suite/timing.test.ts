import { expect } from "chai";
import {
  waitForExtensionReady,
  readTestLog,
  parseStartupTiming,
  isLspReady,
  FIXTURE_NAME,
} from "../helpers";
import { getTimer } from "../timer";

suite(`Startup Timing [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  suiteSetup(async function () {
    await waitForExtensionReady();
  });

  test("measures activation → ready time", function () {
    expect(isLspReady(), "LSP should reach ready state").to.be.true;

    const timing = parseStartupTiming();

    if (timing.deltaMs !== undefined) {
      getTimer().recordStartup(timing.deltaMs);
      console.log(`    Startup time: ${timing.deltaMs}ms`);

      // Generous bound — LSP startup includes binary launch, workspace scan, type provider init
      expect(
        timing.deltaMs,
        "Startup should complete within 60s",
      ).to.be.lessThan(60_000);
    } else {
      console.log("    Warning: Could not parse timing markers from log");
    }
  });

  test("type provider status is logged", function () {
    expect(isLspReady(), "LSP should reach ready state").to.be.true;

    const log = readTestLog();

    const hasTypeProvider = log.includes("Type provider");
    const hasVerterOnly = log.includes("verter-only mode");
    const hasTsgo = log.includes("TSGO");
    const hasTsserver = log.includes("tsserver");

    expect(
      hasTypeProvider || hasVerterOnly || hasTsgo || hasTsserver,
      "Log should indicate type provider status",
    ).to.be.true;

    // Record which provider was used
    if (hasTsserver) {
      getTimer().recordTypeProvider("tsserver");
    } else if (hasTsgo) {
      getTimer().recordTypeProvider("tsgo");
    } else {
      getTimer().recordTypeProvider("verter-only");
    }

    console.log(
      `    Type provider: ${hasTsserver ? "tsserver" : hasTsgo ? "tsgo" : "verter-only"}`,
    );
  });
});
