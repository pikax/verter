import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  waitForFileReady,
  openVueFile,
  getAppVuePath,
  FIXTURE_NAME,
  TYPE_PROVIDER,
  waitForNoDiagnosticsMatching,
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

  const diagnosticCode = (diagnostic: vscode.Diagnostic): string => {
    const code = typeof diagnostic.code === "object" ? diagnostic.code?.value : diagnostic.code;
    return String(code ?? "");
  };

  const formatDiagnostics = (diagnostics: vscode.Diagnostic[]): string =>
    diagnostics.map((diagnostic) => `${diagnostic.source ?? "unknown"}:${diagnosticCode(diagnostic)} ${diagnostic.message}`).join("; ");

  const isModuleNotFoundDiagnostic = (diagnostic: vscode.Diagnostic): boolean =>
    diagnostic.message.includes("Cannot find module") &&
    diagnosticCode(diagnostic) === "2307";

  const isVueTsModuleDiagnostic = (diagnostic: vscode.Diagnostic): boolean =>
    isModuleNotFoundDiagnostic(diagnostic) &&
    diagnostic.message.includes(".vue.ts");

  const isTsExtensionDiagnostic = (diagnostic: vscode.Diagnostic): boolean =>
    diagnosticCode(diagnostic) === "5097" ||
    diagnostic.message.includes("allowImportingTsExtensions");

  const expectNoForbiddenDiagnostics = async (
    doc: vscode.TextDocument,
    predicate: (diagnostic: vscode.Diagnostic) => boolean,
    label: string,
  ) => {
    const settledDiagnostics = await waitForNoDiagnosticsMatching(doc.uri, {
      predicate,
      timeoutMs: 15_000,
    });
    const forbiddenDiagnostics = settledDiagnostics.filter(predicate);
    expect(
      forbiddenDiagnostics,
      `${label}: ${formatDiagnostics(forbiddenDiagnostics)}`,
    ).to.have.lengthOf(0);
  };

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

    // TSGO CANARY: composite-paths with TSGO should have module errors because
    // TSGO cannot resolve path aliases from referenced tsconfigs (upstream limitation).
    // If this test starts FAILING, TSGO has fixed the limitation — update auto-mode
    // detection in main.rs and remove this canary.
    if (FIXTURE_NAME === "composite-paths" && TYPE_PROVIDER === "tsgo") {
      const diags = vscode.languages.getDiagnostics(doc.uri);
      const moduleErrors = diags.filter(isModuleNotFoundDiagnostic);
      expect(
        moduleErrors.length,
        "TSGO CANARY: composite-paths @/ aliases should fail on TSGO (known upstream limitation). " +
          "If this fails, TSGO may have fixed composite tsconfig path resolution — " +
          "update auto-mode detection in main.rs and remove this canary.",
      ).to.be.greaterThan(0);
      return;
    }

    await expectNoForbiddenDiagnostics(
      doc,
      isModuleNotFoundDiagnostic,
      "Expected no TS2307 module resolution diagnostics",
    );
  });

  test("@/ path alias imports resolve without errors", async function () {
    if (!TYPE_PROVIDER) return this.skip();
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

    // TSGO CANARY: composite-paths .vue.ts errors are caused by unresolved @/ aliases
    // (same root cause as the path alias canary above).
    if (FIXTURE_NAME === "composite-paths" && TYPE_PROVIDER === "tsgo") {
      const diags = vscode.languages.getDiagnostics(doc.uri);
      const vueTsErrors = diags.filter(isVueTsModuleDiagnostic);
      // Don't assert error count — .vue.ts errors here are a side effect of the
      // @/ alias limitation, not a separate .vue.ts resolution issue.
      console.log(
        `    TSGO canary: ${vueTsErrors.length} .vue.ts error(s) expected (composite path alias limitation)`,
      );
      return;
    }

    await expectNoForbiddenDiagnostics(
      doc,
      isVueTsModuleDiagnostic,
      ".vue.ts resolution errors",
    );
  });

  test(".vue imports do not trigger TS5097 .ts-extension diagnostics", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME === "no-config" || FIXTURE_NAME === "single-file") {
      console.log("    pass (N/A for this fixture)");
      return;
    }
    const doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);

    await expectNoForbiddenDiagnostics(
      doc,
      isTsExtensionDiagnostic,
      "Unexpected TS5097 .ts-extension diagnostics",
    );
  });

  test("nested Vue-to-Vue imports resolve in component files", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "single-project") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const doc = await openVueFile("src/WrappedButton.vue");
    await waitForFileReady(doc);

    await expectNoForbiddenDiagnostics(
      doc,
      (diagnostic) =>
        isModuleNotFoundDiagnostic(diagnostic) || isTsExtensionDiagnostic(diagnostic),
      "Nested Vue import diagnostics",
    );
  });
});
