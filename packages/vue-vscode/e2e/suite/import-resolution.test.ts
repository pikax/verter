import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openVueFile,
  getAppVuePath,
  sleep,
  FIXTURE_NAME,
} from "../helpers";

suite(`Import Resolution [${FIXTURE_NAME}]`, function () {
  this.timeout(90_000);

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
    await waitForExtensionReady(60_000);
  });

  test("no 'Cannot find module' diagnostics on App.vue", async function () {
    if (!IMPORT_FIXTURES.includes(FIXTURE_NAME)) {
      console.log("    pass (N/A for this fixture)");
      return;
    }
    const doc = await openVueFile(getAppVuePath());
    // Wait for type provider diagnostics to settle
    await sleep(15_000);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    const moduleErrors = diags.filter(
      (d) =>
        d.message.includes("Cannot find module") &&
        (d.code === 2307 || (typeof d.code === "object" && d.code?.value === 2307)),
    );

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
    await sleep(15_000);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    const aliasErrors = diags.filter(
      (d) =>
        d.message.includes("Cannot find module") && d.message.includes("@/"),
    );

    expect(
      aliasErrors,
      `@/ alias errors: ${aliasErrors.map((d) => d.message).join("; ")}`,
    ).to.have.lengthOf(0);
  });

  test(".vue imports resolve without .vue.ts errors", async function () {
    if (FIXTURE_NAME === "no-config" || FIXTURE_NAME === "single-file") {
      console.log("    pass (N/A for this fixture)");
      return;
    }
    const doc = await openVueFile(getAppVuePath());
    await sleep(15_000);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    const vueTsErrors = diags.filter(
      (d) =>
        d.message.includes(".vue.ts") &&
        d.message.includes("Cannot find module"),
    );

    expect(
      vueTsErrors,
      `.vue.ts resolution errors: ${vueTsErrors.map((d) => d.message).join("; ")}`,
    ).to.have.lengthOf(0);
  });
});
