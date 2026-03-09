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

  test("measures activation to ready time", function () {
    expect(isLspReady(), "LSP should reach ready state").to.be.true;

    const timing = parseStartupTiming();

    if (timing.activationToReadyMs !== undefined) {
      getTimer().recordStartup(timing.activationToReadyMs);
      console.log(`    Startup time: ${timing.activationToReadyMs}ms`);

      expect(
        timing.activationToReadyMs,
        "Startup should complete within 60s",
      ).to.be.lessThan(60_000);
    } else {
      console.log("    Warning: Could not parse timing markers from log");
    }
  });

  test("type provider status is logged", function () {
    expect(isLspReady(), "LSP should reach ready state").to.be.true;

    const log = readTestLog();
    const timing = parseStartupTiming();

    expect(
      log.includes("Type provider") || timing.providerKind === "verter-only",
      "Log should indicate type provider status",
    ).to.be.true;

    const providerKind = timing.providerKind ?? "verter-only";
    getTimer().recordTypeProvider(providerKind, timing.typeProviderStartedMs);

    console.log(`    Type provider: ${providerKind}`);
  });
});
