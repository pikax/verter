/**
 * Negative public-type assertion: non-test imports must not see script-setup
 * internals that are only meant for the testing API surface
 * (`Foo.vue.__verter_test.ts` / exposeBindingsTesting).
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  ensureParityReady,
  hoverTextAt,
  failProduct,
  assertTsExpectErrorFileHolds,
} from "../../../lib/parityHarness";

function onlyVueParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "vue-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

suite(`Vue public surface (negative) [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(20_000);
    onlyVueParity(this);
    await ensureParityReady("src/App.vue");
  });

  test("vue.public-surface.no-secret-internal-on-component-hover", async function () {
    onlyVueParity(this);
    // Hover on the component tag / import in a non-test consumer path.
    // The public hover must not advertise secretInternal.
    const text = await hoverTextAt({
      file: "src/features/ExposePublicConsumer.ts",
      token: "ExposePublic",
      occurrence: 0,
    });
    if (/\bsecretInternal\b/.test(text)) {
      failProduct(
        "vue.public-surface.no-secret-internal-on-component-hover",
        "ISSUE-vue-public-surface-leak",
        `public hover leaked secretInternal: ${text}`,
      );
    }
    // Soft positive: something typed returned
    if (text.trim().length === 0) {
      failProduct(
        "vue.public-surface.no-secret-internal-on-component-hover",
        "ISSUE-vue-public-surface-empty",
        "empty hover on public component import",
      );
    }
  });

  test("vue.public-surface.consumer-source-documents-negative", async function () {
    onlyVueParity(this);
    await assertTsExpectErrorFileHolds("src/features/ExposePublicConsumer.ts", 1);
  });
});
