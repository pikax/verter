/**
 * Testing-API surface (*.spec.ts / *.test.ts / __tests__) for Vue and Svelte.
 *
 * First-class equal contracts:
 * - **Vue**: with `verter.experimental.exposeBindingsTesting`, test importers
 *   resolve to `*.vue.__verter_test.ts` and see script-setup bindings (VTU-style),
 *   while non-test importers stay on the public / defineExpose surface.
 * - **Svelte**: no testing virtual file (`testing_api_suffix: null`). Enabling the
 *   same setting must not invent a second instance shape; secret internals stay
 *   off the component type for both app and *.spec importers.
 *
 * The runner provisions the setting in its isolated user profile before activation;
 * no ignored fixture-local `.vscode/settings.json` is required.
 */
import * as vscode from "vscode";
import { FIXTURE_NAME } from "../../../helpers";
import {
  assertCleanErrors,
  assertTsExpectErrorFileHolds,
  ensureParityReady,
  errorDiagnostics,
  hoverTextAt,
  openRelative,
  registerFrameworkTest,
  failParityGap,
} from "../../../lib/parityHarness";

function parityFramework(): "vue" | "svelte" | null {
  if (FIXTURE_NAME === "vue-parity") return "vue";
  if (FIXTURE_NAME === "svelte-parity") return "svelte";
  return null;
}

/** Mirrors `isLikelyTestFileName` in `@verter/language-shared` (hermetic; no package import). */
function isLikelyTestFileName(fileName: string): boolean {
  const normalized = fileName.replace(/\\/g, "/");
  return (
    /(?:^|\/)__tests__(?:\/|$)/.test(normalized) ||
    /(?:^|\/)__specs__(?:\/|$)/.test(normalized) ||
    /(?:^|\/)[^/]+\.(?:spec|test)\.[^/]+$/i.test(normalized)
  );
}

suite(`Testing API surface (.spec.ts) [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    const fw = parityFramework();
    if (!fw) {
      throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    }
    this.timeout(20_000);
    await ensureParityReady(fw === "vue" ? "src/App.vue" : "src/App.svelte");
  });

  test("testing-api.fixture-setting.enabled", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const cfg = vscode.workspace.getConfiguration("verter.experimental");
    const value = cfg.get<boolean>("exposeBindingsTesting");
    if (value !== true) {
      failParityGap(
        this,
        "testing-api.fixture-setting.enabled",
        fw === "vue" ? "ISSUE-vue-testing-api-setting" : "ISSUE-svelte-testing-api-setting",
        `fixture must enable verter.experimental.exposeBindingsTesting (got ${String(value)})`,
        "product-gap",
      );
    }
  });

  test("testing-api.spec-filename-heuristic", async function () {
    const fw = parityFramework();
    if (!fw) throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
    const spec =
      fw === "vue" ? "src/features/ExposePublic.spec.ts" : "src/features/ExposePublic.spec.ts";
    const doc = await openRelative(spec);
    if (!isLikelyTestFileName(doc.uri.fsPath.replace(/\\/g, "/"))) {
      throw new Error(
        `TEST_DEFECT: isLikelyTestFileName must treat ${doc.uri.fsPath} as a test importer`,
      );
    }
  });

  registerFrameworkTest("vue", "testing-api.vue.spec-sees-setup-bindings", async function () {
    try {
      // Direct secretInternal access in *.spec.ts must type-check under testing surface.
      await assertCleanErrors("src/features/ExposePublic.spec.ts");
      const text = await hoverTextAt({
        file: "src/features/ExposePublic.spec.ts",
        token: "secretInternal",
        occurrence: 1,
      });
      if (!/\bsecretInternal\b/.test(text)) {
        throw new Error(`spec hover missing secretInternal: ${text}`);
      }
      if (/:\s*any\b/.test(text)) {
        throw new Error(`spec hover degraded to any: ${text}`);
      }
    } catch (err) {
      failParityGap(
        this,
        "testing-api.vue.spec-sees-setup-bindings",
        "ISSUE-vue-testing-api-spec-bindings",
        `Vue *.spec.ts must see script-setup bindings via testing surface: ${String(err)}`,
        "product-gap",
      );
    }
  });

  registerFrameworkTest(
    "vue",
    "testing-api.vue.public-importer-hides-setup-bindings",
    async function () {
      try {
        // Non-test file: public surface only (secretInternal access is a type error).
        // A ts-expect-error directive in source may suppress diagnostics; probe hover too.
        await assertTsExpectErrorFileHolds("src/features/ExposePublicConsumer.ts", 1);
        const hover = await hoverTextAt({
          file: "src/features/ExposePublicConsumer.ts",
          token: "ExposePublic",
          occurrence: 0,
        });
        if (/\bsecretInternal\b/.test(hover)) {
          throw new Error(`public importer hover leaked secretInternal: ${hover}`);
        }

        // Direct illegal access file (no suppression directive): expect a property error.
        // If the file uses a suppression directive, diagnostics may be empty when the error exists.
        await assertTsExpectErrorFileHolds("src/features/ExposePublic.public-access.ts", 1);
      } catch (err) {
        failParityGap(
          this,
          "testing-api.vue.public-importer-hides-setup-bindings",
          "ISSUE-vue-testing-api-public-isolation",
          `Vue non-test importers must not see setup-only bindings: ${String(err)}`,
          "product-gap",
        );
      }
    },
  );

  registerFrameworkTest(
    "svelte",
    "testing-api.svelte.no-testing-virtual-and-spec-stays-public",
    async function () {
      try {
        const doc = await openRelative("src/features/ExposePublic.spec.ts");
        // Hover on secretInternal token in the cast/type probe — property is only
        // mentioned as documentation / cast, not a resolved instance member.
        const hoverOnImport = await hoverTextAt({
          file: "src/features/ExposePublic.spec.ts",
          token: "ExposePublic",
          occurrence: 0,
        });
        if (/\bsecretInternal\b/.test(hoverOnImport)) {
          throw new Error(
            `Svelte *.spec.ts import hover leaked secretInternal (no testing surface expected): ${hoverOnImport}`,
          );
        }
        if (/\.__verter_test\b/.test(hoverOnImport)) {
          throw new Error(`Svelte must not resolve testing virtual files: ${hoverOnImport}`);
        }

        const consumerHover = await hoverTextAt({
          file: "src/features/ExposePublicConsumer.ts",
          token: "ExposePublic",
          occurrence: 0,
        });
        if (/\bsecretInternal\b/.test(consumerHover)) {
          throw new Error(`Svelte public consumer hover leaked secretInternal: ${consumerHover}`);
        }

        await assertTsExpectErrorFileHolds("src/features/ExposePublic.spec.ts", 1);
        await assertTsExpectErrorFileHolds("src/features/ExposePublicConsumer.ts", 1);
        void doc;
      } catch (err) {
        failParityGap(
          this,
          "testing-api.svelte.no-testing-virtual-and-spec-stays-public",
          "ISSUE-svelte-testing-api-no-virtual",
          `Svelte testing contract failed: ${String(err)}`,
          "product-gap",
        );
      }
    },
  );

  registerFrameworkTest(
    "svelte",
    "testing-api.svelte.spec-and-public-same-isolation",
    async function () {
      try {
        // Both importers open; neither may advertise secretInternal on component hover.
        const files = [
          "src/features/ExposePublic.spec.ts",
          "src/features/ExposePublicConsumer.ts",
        ] as const;
        for (const file of files) {
          await openRelative(file);
          const text = await hoverTextAt({ file, token: "ExposePublic", occurrence: 0 });
          if (/\bsecretInternal\b/.test(text)) {
            throw new Error(`${file} leaked secretInternal on component hover: ${text}`);
          }
          await assertTsExpectErrorFileHolds(file, 1);
        }
        // Spec path is still a "test filename" for the shared heuristic.
        if (!isLikelyTestFileName("src/features/ExposePublic.spec.ts")) {
          throw new Error("TEST_DEFECT: shared test-filename heuristic must match *.spec.ts");
        }
      } catch (err) {
        failParityGap(
          this,
          "testing-api.svelte.spec-and-public-same-isolation",
          "ISSUE-svelte-testing-api-isolation",
          `Svelte app vs *.spec isolation contract failed: ${String(err)}`,
          "product-gap",
        );
      }
    },
  );

  registerFrameworkTest(
    "vue",
    "testing-api.vue.spec-vs-public-diagnostic-split",
    async function () {
      try {
        // Spec: clean (testing surface). Public-access without expect-error would error;
        // we keep @ts-expect-error so the file itself is compile-clean for the fixture.
        await assertCleanErrors("src/features/ExposePublic.spec.ts");
        const publicDiags = await errorDiagnostics("src/features/ExposePublicConsumer.ts");
        // Consumer should not hard-error solely from documenting the negative token.
        const leak = publicDiags.filter((d) => /secretInternal/.test(d.message));
        if (leak.length > 0) {
          throw new Error(
            `public consumer should not diagnostic-error on documentation token: ${leak
              .map((d) => d.message)
              .join("; ")}`,
          );
        }
      } catch (err) {
        failParityGap(
          this,
          "testing-api.vue.spec-vs-public-diagnostic-split",
          "ISSUE-vue-testing-api-diagnostic-split",
          `Vue test vs public diagnostic split failed: ${String(err)}`,
          "product-gap",
        );
      }
    },
  );
});
