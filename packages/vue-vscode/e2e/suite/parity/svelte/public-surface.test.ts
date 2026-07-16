/**
 * Svelte public-surface negative (equal depth to Vue public-surface).
 * Internals must not leak on non-test component import hover.
 */
import { FIXTURE_NAME } from "../../../helpers";
import {
  ensureParityReady,
  failProduct,
  hoverTextAt,
  assertTsExpectErrorFileHolds,
} from "../../../lib/parityHarness";

function onlySvelteParity(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "svelte-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

suite(`Svelte public surface (negative) [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(20_000);
    onlySvelteParity(this);
    await ensureParityReady("src/App.svelte");
  });

  test("svelte.public-surface.no-secret-internal-on-component-hover", async function () {
    onlySvelteParity(this);
    const text = await hoverTextAt({
      file: "src/features/ExposePublicConsumer.ts",
      token: "ExposePublic",
      occurrence: 0,
    });
    if (/\bsecretInternal\b/.test(text)) {
      failProduct(
        "svelte.public-surface.no-secret-internal-on-component-hover",
        "ISSUE-svelte-public-surface-leak",
        `public hover leaked secretInternal: ${text}`,
      );
    }
    if (text.trim().length === 0) {
      failProduct(
        "svelte.public-surface.no-secret-internal-on-component-hover",
        "ISSUE-svelte-public-surface-empty",
        "empty hover on public Svelte component import",
      );
    }
    if (
      !/\bComponent\b/.test(text) ||
      !/\bpublicCount\b/.test(text) ||
      /__Verter\w*|new\s*\(\.\.\.args:\s*any\[\]\)|\bany\b/.test(text)
    ) {
      failProduct(
        "svelte.public-surface.no-secret-internal-on-component-hover",
        "ISSUE-svelte-public-surface-leak",
        `public hover must expose Svelte 5's callable Component contract and public exports: ${text}`,
      );
    }
  });

  test("svelte.public-surface.consumer-documents-negative", async function () {
    onlySvelteParity(this);
    await assertTsExpectErrorFileHolds("src/features/ExposePublicConsumer.ts", 1);
  });
});
