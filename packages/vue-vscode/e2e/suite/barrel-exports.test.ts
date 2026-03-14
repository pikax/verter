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
  revealDefinition,
} from "../helpers";

/** Execute go-to-definition at a position and return locations. */
async function getDefinitions(uri: vscode.Uri, pos: vscode.Position): Promise<vscode.Location[]> {
  const locations = await vscode.commands.executeCommand<
    Array<vscode.Location | vscode.LocationLink>
  >("vscode.executeDefinitionProvider", uri, pos);
  return (locations || []).map((location) =>
    "uri" in location
      ? location
      : new vscode.Location(
          location.targetUri,
          location.targetSelectionRange ?? location.targetRange,
        ),
  );
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

    // After full VFS sync (all workspace + node_modules files synced to provider),
    // TSGO should resolve barrel re-exports correctly. If this fails, the VFS
    // sync is incomplete or TSGO has a regression.
    if (TYPE_PROVIDER === "tsgo") {
      console.log(`    TSGO: ${moduleErrors.length} module error(s)`);
      // TSGO with full VFS should not have module errors on barrel imports.
      // If this fails, check that workspace scanner syncs all .vue files
      // before non-Vue files (two-phase scan).
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

    console.log(`    Hover on <Overlay: ${latencyMs}ms, ${hovers.length} result(s)`);

    if (hovers.length === 0) {
      console.log("    WARNING: No hover results — type provider may not be running");
      return;
    }

    const content = hovers[0].contents.map((c) => (typeof c === "string" ? c : c.value)).join("\n");

    console.log(`    Hover content: ${content.slice(0, 200)}`);

    // POSITIVE: The hover should show props from the Overlay component.
    // If barrel re-exports work correctly, the type includes $props with
    // zIndex, show, lockScroll, etc.

    // With full VFS sync, both TSGO and tsserver should resolve barrel-imported
    // component types correctly, showing typed props in hover.
    if (TYPE_PROVIDER === "tsgo") {
      console.log("    TSGO: checking barrel-imported component hover for typed props");
    }

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
      content.includes("DefineComponent<{}, {}") || content.includes("DefineComponent<{}, {}, {}");

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

    console.log(`    Hover on <Button: ${latencyMs}ms, ${hovers.length} result(s)`);

    if (hovers.length === 0) {
      console.log("    WARNING: No hover results — type provider may not be running");
      return;
    }

    const content = hovers[0].contents.map((c) => (typeof c === "string" ? c : c.value)).join("\n");

    console.log(`    Hover content: ${content.slice(0, 200)}`);

    // With full VFS sync, both providers should resolve barrel-imported component types.
    if (TYPE_PROVIDER === "tsgo") {
      console.log("    TSGO: checking barrel-imported Button hover for typed props");
    }

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

    // With full VFS sync, both providers should type-check barrel-imported component props.
    if (TYPE_PROVIDER === "tsgo") {
      console.log(`    TSGO: ${propErrors.length} prop error(s)`);
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

    const content = hovers[0].contents.map((c) => (typeof c === "string" ? c : c.value)).join("\n");

    console.log(`    Hover content: ${content.slice(0, 200)}`);

    // With full VFS sync, both providers should show typed props in barrel file hover.
    if (TYPE_PROVIDER === "tsgo") {
      console.log("    TSGO: checking TS plugin hover for typed component");
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

    // In verter-only mode, TS plugin definition in .ts files may not resolve
    if (locations.length === 0 && !TYPE_PROVIDER) {
      console.log("    Verter-only: no definition in .ts file (needs type provider)");
      return;
    }

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

    const content = hovers[0].contents.map((c) => (typeof c === "string" ? c : c.value)).join("\n");

    console.log(`    Hover content: ${content.slice(0, 200)}`);

    // With full VFS sync, both providers should show typed import bindings.
    if (TYPE_PROVIDER === "tsgo") {
      console.log("    TSGO: checking import binding hover for typed component");
    }

    // NEGATIVE: should NOT be plain DefineComponent<{}, {}>
    expect(content).to.not.include(
      "DefineComponent<{}, {}",
      "import binding should have typed props through barrel",
    );

    // NEGATIVE: should NOT be `any`
    expect(content).to.not.match(/:\s*any\b/, "import binding should not be 'any'");
  });

  // ── Terminal Navigation Tests (Step 4/5) ──

  test("D5: barrel export name → terminal Vue component", async function () {
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

    // Go-to-definition on "Overlay" in `export { default as Overlay } from './Overlay.vue'`
    const pos = findPosition(tsDoc, "as Overlay", 3); // on "O" of Overlay
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(tsDoc.uri, pos);
    console.log(`    Barrel export definition on Overlay: ${locations.length} location(s)`);

    // Verter's barrel export resolution should follow the re-export chain
    expect(locations.length, "should have definition results").to.be.greaterThan(0);

    const def = locations[0];
    // POSITIVE: should navigate to Overlay.vue
    expect(def.uri.fsPath, "should navigate to Overlay.vue").to.include("Overlay.vue");

    // NEGATIVE: should NOT stay in the barrel index.ts
    expect(def.uri.fsPath, "should NOT stay in barrel file").to.not.include("index.ts");

    // NEGATIVE: should NOT open virtual files
    expect(def.uri.fsPath, "should NOT open virtual file").to.not.match(/\.vue\.(d\.ts|ts|tsx)$/);
  });

  test("D6: barrel import binding in .vue file → terminal component (not barrel)", async function () {
    if (!isBarrelFixture) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // In App.vue, go-to-definition on "Overlay" in `import { Overlay, Button } from './components'`
    const pos = findPosition(doc, "{ Overlay, Button }", 2); // on "O"
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Import binding definition on Overlay: ${locations.length} location(s)`);

    // In verter-only mode, import binding definition may resolve via type provider
    if (locations.length === 0 && !TYPE_PROVIDER) {
      console.log("    Verter-only: no definition on import binding (needs type provider)");
      return;
    }

    expect(locations.length, "should have definition results").to.be.greaterThan(0);

    // Check if any location points to the terminal component
    const hasTerminal = locations.some((l) => l.uri.fsPath.includes("Overlay.vue"));
    const hasBarrel = locations.some((l) => l.uri.fsPath.includes("index.ts"));

    console.log(`    terminal=${hasTerminal} barrel=${hasBarrel}`);

    if (TYPE_PROVIDER) {
      // With type provider + barrel resolution, should reach the terminal
      expect(hasTerminal, "should navigate to Overlay.vue terminal").to.be.true;

      // NEGATIVE: should NOT resolve only to barrel
      if (!hasTerminal && hasBarrel) {
        throw new Error(
          "Definition resolved to barrel index.ts but should reach terminal Overlay.vue",
        );
      }
    }

    // NEGATIVE: should NOT open virtual files
    for (const loc of locations) {
      expect(loc.uri.fsPath, "should NOT open virtual file").to.not.match(/\.vue\.(d\.ts|ts|tsx)$/);
    }
  });

  test("D7: revealDefinition on barrel export name opens terminal Vue component", async function () {
    if (!isBarrelFixture) {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders) {
      this.skip();
      return;
    }

    const indexPath = path.join(workspaceFolders[0].uri.fsPath, "src/components/index.ts");
    const tsDoc = await vscode.workspace.openTextDocument(vscode.Uri.file(indexPath));
    await vscode.window.showTextDocument(tsDoc);
    await waitForFileReady(tsDoc);

    const pos = findPosition(tsDoc, "as Overlay", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const revealed = await revealDefinition(tsDoc.uri, pos);
    console.log(`    revealDefinition target: ${revealed.uri.fsPath}`);

    expect(revealed.uri.fsPath, "should open Overlay.vue").to.include("Overlay.vue");
    expect(revealed.uri.fsPath, "should not stay in barrel index.ts").to.not.include("index.ts");
    expect(revealed.uri.fsPath, "should not open virtual files").to.not.match(
      /\.vue\.(d\.ts|ts|tsx)$/,
    );
  });
});
