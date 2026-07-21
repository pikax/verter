/**
 * Provider-recommendation route behavior (tsgo-preferred flip, B8): the
 * end-to-end spot check that the server's structured recommendation reaches
 * the real extension and renders per-route — shown once on tsserver-family
 * serving, never on the preferred tsgo/shared-tsgo routes. Runs for both
 * framework parity fixtures, so each framework gets the spot check on every
 * provider route in the matrix.
 */
import { strict as assert } from "node:assert";

import {
  FIXTURE_NAME,
  TYPE_PROVIDER,
  readTestLog,
  waitForTypeProviderSync,
} from "../../../helpers";
import { ensureParityReady } from "../../../lib/parityHarness";

const RECOMMENDATION_LOG_MARKER = "Provider recommendation:";

suite(`Provider recommendation [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    if (FIXTURE_NAME !== "vue-parity" && FIXTURE_NAME !== "svelte-parity") {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    await ensureParityReady(FIXTURE_NAME === "svelte-parity" ? "src/App.svelte" : "src/App.vue");
  });

  test("shared.provider-recommendation.route-behavior", async function () {
    this.timeout(60_000);
    if (
      TYPE_PROVIDER !== "tsserver" &&
      TYPE_PROVIDER !== "tsgo" &&
      TYPE_PROVIDER !== "shared-tsgo"
    ) {
      throw new Error(
        `TEST_DEFECT: parity route must pin a type provider, got ${JSON.stringify(TYPE_PROVIDER)}`,
      );
    }
    // Deterministic point strictly after the server's typeProviderStatus
    // notification (the carrier of the structured recommendation) was
    // handled by the extension.
    await waitForTypeProviderSync();
    const log = readTestLog();

    if (TYPE_PROVIDER === "tsserver") {
      const line = log.split("\n").find((entry) => entry.includes(RECOMMENDATION_LOG_MARKER));
      assert.ok(line, "tsserver-family serving must surface the provider recommendation notice");
      assert.ok(
        line.includes("tsgo") || line.includes("TSGO"),
        `the recommendation must name the preferred tsgo provider: ${line}`,
      );
      assert.ok(
        line.includes("TS6133"),
        `the recommendation must disclose the known TS6133 quick-fix gap honestly: ${line}`,
      );
      assert.ok(
        !line.includes("barrel"),
        `the retired barrel-typing claim must stay retired: ${line}`,
      );
    } else {
      assert.ok(
        !log.includes(RECOMMENDATION_LOG_MARKER),
        `the preferred ${TYPE_PROVIDER} route must never be nagged with a provider recommendation`,
      );
    }
  });
});
