/** Exact Svelte 5 runes interpolation must remain diagnostic-clean after sync. */
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertHoverNeedles,
  ensureParityReady,
  failParityGap,
} from "../../../lib/parityHarness";

const FILE = "src/diagnostics/StateInterpolation.svelte";

suite(`Svelte state interpolation diagnostic [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    if (FIXTURE_NAME !== "svelte-parity") {
      throw new Error("TEST_DEFECT: Svelte diagnostic suite loaded for wrong fixture");
    }
    await ensureParityReady(FILE);
  });

  test("svelte.diagnostic.state-interpolation-clean", async function () {
    try {
      await assertCleanErrors(FILE);
      await assertHoverNeedles(
        { file: FILE, token: "svelteTsTitle", occurrence: 1 },
        ["svelteTsTitle", "string"],
        { forbidAny: true, forbidUnknown: true },
      );
    } catch (error) {
      failParityGap(
        this,
        "svelte.diagnostic.state-interpolation-clean",
        "ISSUE-svelte-state-interpolation-diagnostic",
        `Valid Svelte TypeScript state interpolation reported a diagnostic: ${String(error)}`,
        "provider-gap",
      );
    }
  });
});
