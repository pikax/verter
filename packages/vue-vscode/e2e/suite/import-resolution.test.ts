import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  waitForFileReady,
  openVueFile,
  getAppVuePath,
  FIXTURE_NAME,
  TYPE_PROVIDER,
} from "../helpers";

suite(`Import Resolution [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  // Fixtures that exercise import resolution (path aliases or project references)
  const IMPORT_FIXTURES = [
    "composite-paths",
    "path-aliases",
    "tsconfig-references",
    "single-project",
    "tsconfig-extends",
    "monorepo",
  ];

  suiteSetup(async function () {
    await waitForExtensionReady();
  });

  test("no 'Cannot find module' diagnostics on App.vue", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (!IMPORT_FIXTURES.includes(FIXTURE_NAME)) {
      console.log("    pass (N/A for this fixture)");
      return;
    }
    const doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    const moduleErrors = diags.filter((d) => {
      if (!d.message.includes("Cannot find module")) return false;
      // TS2307 code can be number, string, or {value: number} depending on provider
      const code = typeof d.code === "object" ? d.code?.value : d.code;
      return code === 2307 || code === "2307" || String(code) === "2307";
    });

    // TSGO CANARY: composite-paths with TSGO should have module errors because
    // TSGO cannot resolve path aliases from referenced tsconfigs (upstream limitation).
    // If this test starts FAILING, TSGO has fixed the limitation — update auto-mode
    // detection in main.rs and remove this canary.
    if (FIXTURE_NAME === "composite-paths" && TYPE_PROVIDER === "tsgo") {
      expect(
        moduleErrors.length,
        "TSGO CANARY: composite-paths @/ aliases should fail on TSGO (known upstream limitation). " +
          "If this fails, TSGO may have fixed composite tsconfig path resolution — " +
          "update auto-mode detection in main.rs and remove this canary.",
      ).to.be.greaterThan(0);
      return;
    }

    expect(
      moduleErrors,
      `Expected no TS2307 errors but found: ${moduleErrors.map((d) => d.message).join("; ")}`,
    ).to.have.lengthOf(0);
  });

  test("@/ path alias imports resolve without errors", async function () {
    if (FIXTURE_NAME !== "composite-paths" && FIXTURE_NAME !== "path-aliases") {
      console.log("    pass (N/A for this fixture)");
      return;
    }
    const doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    const aliasErrors = diags.filter(
      (d) =>
        d.message.includes("Cannot find module") && d.message.includes("@/"),
    );

    // TSGO CANARY: same as above — @/ aliases should fail on TSGO for composite-paths.
    if (FIXTURE_NAME === "composite-paths" && TYPE_PROVIDER === "tsgo") {
      expect(
        aliasErrors.length,
        "TSGO CANARY: @/ alias imports should fail on TSGO for composite-paths. " +
          "If this fails, TSGO may have fixed composite tsconfig path resolution.",
      ).to.be.greaterThan(0);
      return;
    }

    expect(
      aliasErrors,
      `@/ alias errors: ${aliasErrors.map((d) => d.message).join("; ")}`,
    ).to.have.lengthOf(0);
  });

  test(".vue imports resolve without .vue.ts errors", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME === "no-config" || FIXTURE_NAME === "single-file") {
      console.log("    pass (N/A for this fixture)");
      return;
    }
    const doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    const vueTsErrors = diags.filter(
      (d) =>
        d.message.includes(".vue.ts") &&
        d.message.includes("Cannot find module"),
    );

    // TSGO CANARY: composite-paths .vue.ts errors are caused by unresolved @/ aliases
    // (same root cause as the path alias canary above).
    if (FIXTURE_NAME === "composite-paths" && TYPE_PROVIDER === "tsgo") {
      // Don't assert error count — .vue.ts errors here are a side effect of the
      // @/ alias limitation, not a separate .vue.ts resolution issue.
      console.log(
        `    TSGO canary: ${vueTsErrors.length} .vue.ts error(s) expected (composite path alias limitation)`,
      );
      return;
    }

    expect(
      vueTsErrors,
      `.vue.ts resolution errors: ${vueTsErrors.map((d) => d.message).join("; ")}`,
    ).to.have.lengthOf(0);
  });
});
