import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openVueFile,
  getAppVuePath,
  waitForDiagnostics,
  sleep,
  FIXTURE_NAME,
} from "../helpers";
import { getTimer } from "../timer";

suite(`Diagnostics [${FIXTURE_NAME}]`, function () {
  this.timeout(90_000);

  suiteSetup(async function () {
    await waitForExtensionReady(60_000);
  });

  test("extension activates for workspace", async function () {
    const ext = vscode.extensions.getExtension("pikax.verter-vscode");
    expect(ext?.isActive).to.be.true;
  });

  test("opening .vue file does not crash", async function () {
    const doc = await openVueFile(getAppVuePath());
    expect(doc).to.exist;
    expect(doc.languageId).to.equal("vue");
    // Give it time to process without crashing
    await sleep(3_000);
  });

  test("diagnostics API returns for .vue file", async function () {
    const doc = await openVueFile(getAppVuePath());

    // Give the LSP time to process
    await sleep(5_000);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    // We don't assert on count — valid files may have zero diagnostics.
    // The key assertion is that the API call succeeds without error.
    expect(diags).to.be.an("array");

    console.log(
      `    Diagnostics count: ${diags.length}`,
    );
    if (diags.length > 0) {
      const sources = [...new Set(diags.map((d) => d.source || "unknown"))];
      console.log(`    Sources: ${sources.join(", ")}`);
    }
  });

  test("measures time to first diagnostic", async function () {
    const doc = await openVueFile(getAppVuePath());
    const start = Date.now();

    const diags = await waitForDiagnostics(doc.uri, { timeoutMs: 30_000 });
    const elapsed = Date.now() - start;

    const sources = [...new Set(diags.map((d) => d.source || "unknown"))];
    getTimer().recordDiagnostics(elapsed, diags.length, sources);

    console.log(
      `    Time to diagnostics: ${elapsed}ms (${diags.length} diagnostics)`,
    );
  });

  test("diagnostics have valid ranges", async function () {
    const doc = await openVueFile(getAppVuePath());
    await sleep(5_000);

    const diags = vscode.languages.getDiagnostics(doc.uri);
    for (const d of diags) {
      expect(d.range.start.line, "Diagnostic start line should be non-negative").to.be.at.least(0);
      expect(d.range.end.line, "Diagnostic end line should be non-negative").to.be.at.least(0);
      expect(
        d.range.start.isBeforeOrEqual(d.range.end),
        `Diagnostic range should be valid: ${d.message}`,
      ).to.be.true;
    }
  });
});
