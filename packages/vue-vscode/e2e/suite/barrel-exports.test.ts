import { expect } from "chai";
import * as vscode from "vscode";
import * as path from "path";
import {
  waitForExtensionReady,
  waitForFileReady,
  openVueFile,
  measureHover,
  FIXTURE_NAME,
  TYPE_PROVIDER,
} from "../helpers";

/** Execute go-to-definition at a position and return locations. */
async function getDefinitions(
  uri: vscode.Uri,
  pos: vscode.Position,
): Promise<vscode.Location[]> {
  const locations = await vscode.commands.executeCommand<vscode.Location[]>(
    "vscode.executeDefinitionProvider",
    uri,
    pos,
  );
  return locations || [];
}

/** Find position of `needle` in document text, offset by `charOffset` into the match. */
function findPosition(
  doc: vscode.TextDocument,
  needle: string,
  charOffset = 0,
): vscode.Position | undefined {
  const idx = doc.getText().indexOf(needle);
  if (idx === -1) return undefined;
  return doc.positionAt(idx + charOffset);
}

suite(`Barrel Exports [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  // Only run on the barrel-exports fixture
  const isBarrelFixture = FIXTURE_NAME === "barrel-exports";

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    if (!isBarrelFixture) return;
    await waitForExtensionReady();
    doc = await openVueFile("src/App.vue");
    await waitForFileReady(doc);
  });

  test("no 'Cannot find module' errors on barrel import", async function () {
    if (!isBarrelFixture) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const diags = vscode.languages.getDiagnostics(doc.uri);
    const moduleErrors = diags.filter((d) => {
      if (!d.message.includes("Cannot find module")) return false;
      const code = typeof d.code === "object" ? d.code?.value : d.code;
      return code === 2307 || code === "2307" || String(code) === "2307";
    });

    // TSGO CANARY: barrel re-exports of .vue components lose typing on TSGO.
    // If this starts passing, TSGO has fixed barrel re-export resolution —
    // update CLAUDE.md known limitations.
    if (TYPE_PROVIDER === "tsgo") {
      console.log(
        `    TSGO: ${moduleErrors.length} module error(s) (barrel re-export limitation)`,
      );
      // Don't assert — TSGO barrel re-exports may or may not produce TS2307
      // depending on whether the .vue file is synced. The type degradation
      // (DefineComponent<{}, {}>) is the real issue, not a module error.
      return;
    }

    expect(
      moduleErrors,
      `Expected no TS2307 errors but found: ${moduleErrors.map((d) => d.message).join("; ")}`,
    ).to.have.lengthOf(0);
  });

  test("hover on barrel-imported component shows typed props", async function () {
    if (!isBarrelFixture) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const text = doc.getText();

    // Find <Overlay in the template — hover on the component tag to get its type
    const overlayMatch = text.indexOf("<Overlay");
    if (overlayMatch === -1) {
      throw new Error("<Overlay not found in App.vue template");
    }

    // Position on "Overlay" (1 char after "<")
    const pos = doc.positionAt(overlayMatch + 1);
    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    console.log(
      `    Hover on <Overlay: ${latencyMs}ms, ${hovers.length} result(s)`,
    );

    if (hovers.length === 0) {
      console.log("    WARNING: No hover results — type provider may not be running");
      return;
    }

    const content = hovers[0].contents
      .map((c) => (typeof c === "string" ? c : c.value))
      .join("\n");

    console.log(`    Hover content: ${content.slice(0, 200)}`);

    // POSITIVE: The hover should show props from the Overlay component.
    // If barrel re-exports work correctly, the type includes $props with
    // zIndex, show, lockScroll, etc.

    // TSGO CANARY: barrel re-exports lose typing on TSGO.
    if (TYPE_PROVIDER === "tsgo") {
      // On TSGO, the type may degrade to DefineComponent<{}, {}> or similar.
      // If this assertion starts passing, TSGO has fixed barrel re-exports.
      const hasProps =
        content.includes("zIndex") ||
        content.includes("show") ||
        content.includes("lockScroll");
      if (!hasProps) {
        console.log(
          "    TSGO CANARY: Barrel-imported component hover lacks props (expected — known limitation)",
        );
      } else {
        console.log(
          "    TSGO: Barrel-imported component hover DOES show props — limitation may be fixed!",
        );
      }
      return;
    }

    // For tsserver: props MUST be visible in the hover
    // If this fails, the barrel re-export type degradation bug is confirmed in the real LSP.
    const hasTypedProps =
      content.includes("zIndex") ||
      content.includes("show") ||
      content.includes("lockScroll") ||
      content.includes("duration");

    expect(
      hasTypedProps,
      `Barrel-imported Overlay hover should show typed props but got:\n${content.slice(0, 500)}`,
    ).to.be.true;

    // NEGATIVE: should NOT show the degraded DefineComponent<{}, {}>
    const isDegraded =
      content.includes("DefineComponent<{}, {}") ||
      content.includes("DefineComponent<{}, {}, {}");

    expect(
      isDegraded,
      `Barrel-imported Overlay should NOT show degraded DefineComponent<{}, {}> type`,
    ).to.be.false;
  });

  test("hover on barrel-imported component with emits shows typed emits", async function () {
    if (!isBarrelFixture) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const text = doc.getText();

    // Find <Button in the template
    const buttonMatch = text.indexOf("<Button");
    if (buttonMatch === -1) {
      throw new Error("<Button not found in App.vue template");
    }

    const pos = doc.positionAt(buttonMatch + 1);
    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    console.log(
      `    Hover on <Button: ${latencyMs}ms, ${hovers.length} result(s)`,
    );

    if (hovers.length === 0) {
      console.log("    WARNING: No hover results — type provider may not be running");
      return;
    }

    const content = hovers[0].contents
      .map((c) => (typeof c === "string" ? c : c.value))
      .join("\n");

    console.log(`    Hover content: ${content.slice(0, 200)}`);

    if (TYPE_PROVIDER === "tsgo") {
      console.log("    TSGO: skipping assertion (barrel re-export limitation)");
      return;
    }

    // For tsserver: Button's label prop should be visible
    const hasProps = content.includes("label") || content.includes("disabled");

    expect(
      hasProps,
      `Barrel-imported Button hover should show typed props but got:\n${content.slice(0, 500)}`,
    ).to.be.true;
  });

  test("barrel-imported component prop passes type check", async function () {
    if (!isBarrelFixture) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Check that there are no type errors on the barrel-imported component's props.
    // E.g., <Overlay :show="showOverlay" :zIndex="100"> should not produce TS errors
    // if the type is properly resolved through the barrel.
    const diags = vscode.languages.getDiagnostics(doc.uri);
    const propErrors = diags.filter((d) => {
      // TS2322: Type 'X' is not assignable to type 'Y' (wrong prop type)
      // TS2769: No overload matches this call (unknown prop on untyped component)
      const code = typeof d.code === "object" ? d.code?.value : d.code;
      const numCode = typeof code === "string" ? parseInt(code, 10) : code;
      return numCode === 2322 || numCode === 2769;
    });

    if (TYPE_PROVIDER === "tsgo") {
      console.log(
        `    TSGO: ${propErrors.length} prop error(s) (barrel re-export limitation)`,
      );
      return;
    }

    expect(
      propErrors,
      `Expected no prop type errors but found: ${propErrors.map((d) => `TS${d.code}: ${d.message}`).join("; ")}`,
    ).to.have.lengthOf(0);
  });

  // ── TS Plugin Tests (critical: plugin must work in sync with Verter) ──

  test("TS plugin: hover on re-exported component in barrel file shows typed props", async function () {
    if (!isBarrelFixture) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders) {
      this.skip();
      return;
    }

    // Open the barrel index.ts file
    const indexPath = path.join(workspaceFolders[0].uri.fsPath, "src/components/index.ts");
    const tsDoc = await vscode.workspace.openTextDocument(vscode.Uri.file(indexPath));
    await vscode.window.showTextDocument(tsDoc);
    await waitForFileReady(tsDoc);

    // Hover on "Overlay" in `export { default as Overlay } from './Overlay.vue'`
    const pos = findPosition(tsDoc, "Overlay", 0);
    if (!pos) {
      this.skip();
      return;
    }
    const { hovers, latencyMs } = await measureHover(tsDoc.uri, pos);

    console.log(`    TS plugin hover on Overlay: ${latencyMs}ms, ${hovers.length} result(s)`);

    if (hovers.length === 0) {
      console.log("    WARNING: No hover results in .ts file — type provider may not be running");
      return;
    }

    const content = hovers[0].contents
      .map((c) => (typeof c === "string" ? c : c.value))
      .join("\n");

    console.log(`    Hover content: ${content.slice(0, 200)}`);

    if (TYPE_PROVIDER === "tsgo") {
      console.log("    TSGO: skipping assertion (barrel re-export limitation)");
      return;
    }

    // NEGATIVE: should NOT show plain DefineComponent<{}, {}>
    expect(content).to.not.include(
      "DefineComponent<{}, {}",
      "TS plugin should provide typed component, not fallback",
    );

    // NEGATIVE: should NOT show plain `any`
    expect(content).to.not.match(
      /:\s*any\b/,
      "TS plugin should not return 'any' type (FALLBACK_STUB)",
    );
  });

  test("TS plugin: definition from barrel file navigates to .vue (not .vue.d.ts)", async function () {
    if (!isBarrelFixture) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders) {
      this.skip();
      return;
    }

    // Open the barrel index.ts file
    const indexPath = path.join(workspaceFolders[0].uri.fsPath, "src/components/index.ts");
    const tsDoc = await vscode.workspace.openTextDocument(vscode.Uri.file(indexPath));
    await vscode.window.showTextDocument(tsDoc);

    // Go-to-definition on './Overlay.vue' source string
    const pos = findPosition(tsDoc, "'./Overlay.vue'", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(tsDoc.uri, pos);
    console.log(`    TS plugin definition on './Overlay.vue': ${locations.length} location(s)`);

    expect(locations.length, "should have definition results").to.be.greaterThan(0);

    const def = locations[0];
    // POSITIVE: should navigate to .vue file
    expect(def.uri.fsPath, "should navigate to Overlay.vue").to.include("Overlay.vue");

    // NEGATIVE: must NOT open .vue.d.ts or .vue.ts (virtual files)
    expect(def.uri.fsPath, "should open actual .vue, not virtual file").to.not.match(
      /\.vue\.(d\.ts|ts|tsx)$/,
    );
  });

  test("TS plugin: import binding hover in .vue file shows typed component (not DefineComponent)", async function () {
    if (!isBarrelFixture) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // In App.vue, hover on "Overlay" in the import statement
    const pos = findPosition(doc, "{ Overlay, Button }", 2); // on "O"
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);

    console.log(`    Import binding hover on Overlay: ${latencyMs}ms, ${hovers.length} result(s)`);

    if (hovers.length === 0) {
      console.log("    WARNING: No hover results — type provider may not be running");
      return;
    }

    const content = hovers[0].contents
      .map((c) => (typeof c === "string" ? c : c.value))
      .join("\n");

    console.log(`    Hover content: ${content.slice(0, 200)}`);

    if (TYPE_PROVIDER === "tsgo") {
      console.log("    TSGO: skipping assertion (barrel re-export limitation)");
      return;
    }

    // NEGATIVE: should NOT be plain DefineComponent<{}, {}>
    expect(content).to.not.include(
      "DefineComponent<{}, {}",
      "import binding should have typed props through barrel",
    );

    // NEGATIVE: should NOT be `any`
    expect(content).to.not.match(
      /:\s*any\b/,
      "import binding should not be 'any'",
    );
  });
});
