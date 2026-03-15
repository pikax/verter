import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  waitForTypeProviderSync,
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
    diagnostics
      .map(
        (diagnostic) =>
          `${diagnostic.source ?? "unknown"}:${diagnosticCode(diagnostic)} ${diagnostic.message}`,
      )
      .join("; ");

  const isModuleNotFoundDiagnostic = (diagnostic: vscode.Diagnostic): boolean =>
    diagnostic.message.includes("Cannot find module") && diagnosticCode(diagnostic) === "2307";

  const isVueTsModuleDiagnostic = (diagnostic: vscode.Diagnostic): boolean =>
    isModuleNotFoundDiagnostic(diagnostic) && diagnostic.message.includes(".vue.ts");

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
    await waitForTypeProviderSync();
  });

  test("no 'Cannot find module' diagnostics on App.vue", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (!IMPORT_FIXTURES.includes(FIXTURE_NAME)) {
      console.log("    pass (N/A for this fixture)");
      return;
    }
    const doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);

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
      (d) => d.message.includes("Cannot find module") && d.message.includes("@/"),
    );

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

    await expectNoForbiddenDiagnostics(doc, isVueTsModuleDiagnostic, ".vue.ts resolution errors");
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
      (diagnostic) => isModuleNotFoundDiagnostic(diagnostic) || isTsExtensionDiagnostic(diagnostic),
      "Nested Vue import diagnostics",
    );
  });
});
