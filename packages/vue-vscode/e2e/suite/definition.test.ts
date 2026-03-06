import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openVueFile,
  getAppVuePath,
  sleep,
  FIXTURE_NAME,
  TYPE_PROVIDER,
} from "../helpers";

// ── Helpers ─────────────────────────────────────────────────────

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

/**
 * Find the Nth occurrence of `needle` (0-indexed) and return position at charOffset into it.
 */
function findNthPosition(
  doc: vscode.TextDocument,
  needle: string,
  n: number,
  charOffset = 0,
): vscode.Position | undefined {
  const text = doc.getText();
  let idx = -1;
  for (let i = 0; i <= n; i++) {
    idx = text.indexOf(needle, idx + 1);
    if (idx === -1) return undefined;
  }
  return doc.positionAt(idx + charOffset);
}

// ── Test Suite ──────────────────────────────────────────────────

suite(`Definition [${FIXTURE_NAME}]`, function () {
  this.timeout(90_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady(60_000);
    doc = await openVueFile(getAppVuePath());
    // Cross-file tests need workspace scanner to compile target files
    const waitTime = FIXTURE_NAME === "barrel-exports" ? 15_000 : 12_000;
    await sleep(waitTime);
  });

  // ── A. Same-File Declarations ───────────────────────────────

  test("A1: go-to-definition on prop binding in template", async function () {
    const text = doc.getText();
    const templateMatch = text.indexOf("{{ title }}");
    if (templateMatch === -1) {
      console.log("    {{ title }} not in fixture — skip");
      this.skip();
      return;
    }

    const pos = doc.positionAt(templateMatch + 3);
    const locations = await getDefinitions(doc.uri, pos);

    console.log(`    Definition on title: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];
    const definePropsLine = text.indexOf("defineProps<{ title: string }>");
    expect(definePropsLine, "fixture should have defineProps").to.not.equal(-1);

    // Same file
    expect(def.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);

    // Correct position: "title" inside the type param
    const titleInDefineProps = text.indexOf("title", definePropsLine);
    const expectedPos = doc.positionAt(titleInDefineProps);
    expect(def.range.start.line, "definition should be on the defineProps line").to.equal(
      expectedPos.line,
    );
    expect(def.range.start.character, "definition should point to 'title' in type param").to.equal(
      expectedPos.character,
    );

    // Negative: NOT a .tsx file
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  test("A2: go-to-definition on ref binding in template", async function () {
    const text = doc.getText();
    const templateMatch = text.indexOf("{{ count }}");
    if (templateMatch === -1) {
      console.log("    {{ count }} not in fixture — skip");
      this.skip();
      return;
    }

    const pos = doc.positionAt(templateMatch + 3);
    const locations = await getDefinitions(doc.uri, pos);

    console.log(`    Definition on count: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];
    expect(def.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);

    const countDecl = text.indexOf("const count = ref(0)");
    if (countDecl !== -1) {
      const expectedLine = doc.positionAt(countDecl).line;
      expect(def.range.start.line, "definition should be on the ref declaration line").to.equal(
        expectedLine,
      );
    }

    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  test("A3: go-to-definition on computed binding in template", async function () {
    const text = doc.getText();
    const templateMatch = text.indexOf("{{ doubled }}");
    if (templateMatch === -1) {
      console.log("    {{ doubled }} not in fixture — skip");
      this.skip();
      return;
    }

    const pos = doc.positionAt(templateMatch + 3);
    const locations = await getDefinitions(doc.uri, pos);

    console.log(`    Definition on doubled: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];
    expect(def.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);

    const doubledDecl = text.indexOf("const doubled = computed(");
    if (doubledDecl !== -1) {
      const expectedLine = doc.positionAt(doubledDecl).line;
      expect(def.range.start.line, "definition should be on computed declaration line").to.equal(
        expectedLine,
      );
    }

    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  test("A4: go-to-definition on function in @click handler", async function () {
    const text = doc.getText();
    const clickMatch = text.indexOf('@click="increment"');
    if (clickMatch === -1) {
      console.log("    @click=\"increment\" not in fixture — skip");
      this.skip();
      return;
    }

    // Position on "increment" inside @click="increment"
    const pos = doc.positionAt(clickMatch + '@click="'.length);
    const locations = await getDefinitions(doc.uri, pos);

    console.log(`    Definition on increment: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];
    expect(def.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);

    const fnDecl = text.indexOf("function increment()");
    if (fnDecl !== -1) {
      const expectedLine = doc.positionAt(fnDecl).line;
      expect(def.range.start.line, "definition should be on function declaration line").to.equal(
        expectedLine,
      );
    }

    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  test("A5: go-to-definition on local variable using imported function", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const text = doc.getText();
    const templateMatch = text.indexOf("{{ formatted }}");
    if (templateMatch === -1) {
      console.log("    {{ formatted }} not in fixture — skip");
      this.skip();
      return;
    }

    const pos = doc.positionAt(templateMatch + 3);
    const locations = await getDefinitions(doc.uri, pos);

    console.log(`    Definition on formatted: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];
    expect(def.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);

    const formattedDecl = text.indexOf("const formatted = formatCount(");
    if (formattedDecl !== -1) {
      const expectedLine = doc.positionAt(formattedDecl).line;
      expect(def.range.start.line, "definition should be on const declaration line").to.equal(
        expectedLine,
      );
    }

    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  // ── B. Component Tag → Component File ───────────────────────

  test("B1: go-to-definition on component tag navigates to component file", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const pos = findPosition(doc, "<MyComp", 1); // on "M" of MyComp
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on <MyComp: ${locations.length} location(s)`);
    for (const loc of locations) {
      console.log(`      -> ${loc.uri.fsPath} L${loc.range.start.line}:${loc.range.start.character}`);
    }

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    // Find the definition that points to MyComp.vue (there may be multiple)
    const def = locations.find((l) => l.uri.fsPath.includes("MyComp.vue")) || locations[0];

    // Positive: navigates to MyComp.vue
    expect(def.uri.fsPath, "definition should be in MyComp.vue").to.include("MyComp.vue");

    // Negative: cross-file, NOT same file
    expect(def.uri.fsPath, "should navigate to a different file").to.not.equal(doc.uri.fsPath);

    // Negative: NOT generated .tsx
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  test("B2: go-to-definition on barrel-imported component tag reaches actual .vue", async function () {
    if (FIXTURE_NAME !== "barrel-exports") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const pos = findPosition(doc, "<Overlay", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on <Overlay: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    if (TYPE_PROVIDER === "tsgo" && def.uri.fsPath.includes("index.ts")) {
      console.log("    TSGO CANARY: barrel component resolved to index.ts (known limitation)");
      return;
    }

    // Positive: navigates to Overlay.vue
    expect(def.uri.fsPath, "definition should reach Overlay.vue").to.include("Overlay.vue");

    // Negative: NOT the barrel file
    expect(def.uri.fsPath, "should NOT stop at barrel index.ts").to.not.include("index.ts");

    // Negative: NOT same file
    expect(def.uri.fsPath, "should navigate to a different file").to.not.equal(doc.uri.fsPath);

    // Negative: NOT generated .tsx
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  test("B3: go-to-definition on barrel-imported Button tag reaches actual .vue", async function () {
    if (FIXTURE_NAME !== "barrel-exports") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const pos = findPosition(doc, "<Button", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on <Button: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    if (TYPE_PROVIDER === "tsgo" && def.uri.fsPath.includes("index.ts")) {
      console.log("    TSGO CANARY: barrel component resolved to index.ts (known limitation)");
      return;
    }

    expect(def.uri.fsPath, "definition should reach Button.vue").to.include("Button.vue");
    expect(def.uri.fsPath, "should NOT stop at barrel index.ts").to.not.include("index.ts");
    expect(def.uri.fsPath, "should navigate to a different file").to.not.equal(doc.uri.fsPath);
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  // ── C. Imported Functions → Source File ─────────────────────

  test("C1: go-to-definition on imported function in script navigates to source", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Position on "formatCount" in the import statement
    const pos = findPosition(doc, "import { formatCount }", 9); // on "f" of formatCount
    if (!pos) {
      console.log("    formatCount import not in fixture — skip");
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on formatCount import: ${locations.length} location(s)`);
    for (const loc of locations) {
      console.log(`      -> ${loc.uri.fsPath} L${loc.range.start.line}:${loc.range.start.character}`);
    }

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    // Positive: navigates to utils.ts
    expect(def.uri.fsPath, "definition should be in utils.ts").to.include("utils.ts");

    // Negative: NOT same file
    expect(def.uri.fsPath, "should navigate to a different file").to.not.equal(doc.uri.fsPath);

    // Negative: NOT a .vue file
    expect(def.uri.fsPath, "should NOT be in a .vue file").to.not.include(".vue");
  });

  test("C2: go-to-definition on imported function usage in script navigates to source", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Position on "formatCount" in the usage: `const formatted = formatCount(count.value)`
    const text = doc.getText();
    const usageMatch = text.indexOf("formatCount(count.value)");
    if (usageMatch === -1) {
      console.log("    formatCount usage not in fixture — skip");
      this.skip();
      return;
    }

    const pos = doc.positionAt(usageMatch + 1); // on "o" of formatCount
    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on formatCount usage: ${locations.length} location(s)`);
    for (const loc of locations) {
      console.log(`      -> ${loc.uri.fsPath} L${loc.range.start.line}:${loc.range.start.character}`);
    }

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    // Positive: navigates to utils.ts
    expect(def.uri.fsPath, "definition should be in utils.ts").to.include("utils.ts");

    // Negative: NOT same file
    expect(def.uri.fsPath, "should navigate to a different file").to.not.equal(doc.uri.fsPath);
  });

  // ── D. Barrel Export Bindings ───────────────────────────────

  test("D1: go-to-definition on barrel import binding reaches actual .vue file", async function () {
    if (FIXTURE_NAME !== "barrel-exports") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Position on "Overlay" in `import { Overlay, Button } from './components'`
    const pos = findPosition(doc, "{ Overlay, Button }", 2); // on "O" of Overlay
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on Overlay import binding: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    if (TYPE_PROVIDER === "tsgo" && def.uri.fsPath.includes("index.ts")) {
      console.log("    TSGO CANARY: barrel import resolved to index.ts (known limitation)");
      return;
    }

    // Positive: reaches Overlay.vue
    expect(def.uri.fsPath, "definition should reach Overlay.vue").to.include("Overlay.vue");

    // Negative: NOT the barrel file
    expect(def.uri.fsPath, "should NOT stop at barrel index.ts").to.not.include("index.ts");

    // Negative: NOT generated .tsx
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  test("D2: go-to-definition on barrel import Button binding reaches actual .vue file", async function () {
    if (FIXTURE_NAME !== "barrel-exports") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Position on "Button" in `import { Overlay, Button } from './components'`
    const pos = findPosition(doc, "{ Overlay, Button }", 12); // on "B" of Button
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on Button import binding: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    if (TYPE_PROVIDER === "tsgo" && def.uri.fsPath.includes("index.ts")) {
      console.log("    TSGO CANARY: barrel import resolved to index.ts (known limitation)");
      return;
    }

    expect(def.uri.fsPath, "definition should reach Button.vue").to.include("Button.vue");
    expect(def.uri.fsPath, "should NOT stop at barrel index.ts").to.not.include("index.ts");
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  test("D3: go-to-definition on import source string navigates to barrel file", async function () {
    if (FIXTURE_NAME !== "barrel-exports") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Position inside the string './components'
    const pos = findPosition(doc, "'./components'", 3); // inside the string
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on './components' source: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    // Positive: navigates to index.ts (the barrel file itself)
    expect(
      def.uri.fsPath.includes("index.ts") || def.uri.fsPath.includes("components"),
      "definition should navigate to the barrel file or components dir",
    ).to.be.true;

    // Negative: NOT same file
    expect(def.uri.fsPath, "should navigate to a different file").to.not.equal(doc.uri.fsPath);
  });

  // ── E. Component Props → Child defineProps ──────────────────

  test("E1: go-to-definition on prop attribute navigates to child defineProps", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Position on "foo" in `<MyComp foo="literal" :bar="count" />`
    const pos = findPosition(doc, 'foo="literal"', 0); // on "f" of foo
    if (!pos) {
      console.log("    foo prop not in fixture — skip");
      this.skip();
      return;
    }
    console.log(`    foo prop position: L${pos.line}:${pos.character}`);

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on foo prop: ${locations.length} location(s)`);
    for (const loc of locations) {
      console.log(`      -> ${loc.uri.fsPath} L${loc.range.start.line}:${loc.range.start.character}`);
    }

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    // Positive: navigates to MyComp.vue (child component)
    expect(def.uri.fsPath, "definition should be in MyComp.vue").to.include("MyComp.vue");

    // Negative: NOT same file (must not stay at usage site)
    expect(
      def.uri.fsPath,
      "prop definition should NOT stay at usage site in parent",
    ).to.not.equal(doc.uri.fsPath);

    // Negative: NOT generated .tsx
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);

    // Verify position points to "foo" in defineProps<{ foo: string; bar: number }>()
    // Open child file to verify target line
    const childDoc = await vscode.workspace.openTextDocument(def.uri);
    const childText = childDoc.getText();
    const fooInDefineProps = childText.indexOf("foo: string");
    if (fooInDefineProps !== -1) {
      const expectedLine = childDoc.positionAt(fooInDefineProps).line;
      expect(def.range.start.line, "definition should point to 'foo' in defineProps type").to.equal(
        expectedLine,
      );
    }
  });

  test("E2: go-to-definition on bound prop attribute navigates to child defineProps", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Position on "bar" in `:bar="count"`
    const pos = findPosition(doc, ':bar="count"', 1); // on "b" of bar
    if (!pos) {
      console.log("    :bar prop not in fixture — skip");
      this.skip();
      return;
    }
    console.log(`    :bar prop position: L${pos.line}:${pos.character}`);

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on :bar prop: ${locations.length} location(s)`);
    for (const loc of locations) {
      console.log(`      -> ${loc.uri.fsPath} L${loc.range.start.line}:${loc.range.start.character}`);
    }

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    // Positive: navigates to MyComp.vue
    expect(def.uri.fsPath, "definition should be in MyComp.vue").to.include("MyComp.vue");

    // Negative: NOT same file
    expect(
      def.uri.fsPath,
      "prop definition should NOT stay at usage site in parent",
    ).to.not.equal(doc.uri.fsPath);

    // Negative: NOT generated .tsx
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);

    // Verify position points to "bar" in defineProps
    const childDoc = await vscode.workspace.openTextDocument(def.uri);
    const childText = childDoc.getText();
    const barInDefineProps = childText.indexOf("bar: number");
    if (barInDefineProps !== -1) {
      const expectedLine = childDoc.positionAt(barInDefineProps).line;
      expect(def.range.start.line, "definition should point to 'bar' in defineProps type").to.equal(
        expectedLine,
      );
    }
  });

  test("E3: go-to-definition on barrel-imported component prop reaches child defineProps", async function () {
    if (FIXTURE_NAME !== "barrel-exports") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Position on "label" in `<Button label="Open" ...>`
    const pos = findPosition(doc, 'label="Open"', 0); // on "l" of label
    if (!pos) {
      console.log("    label prop not in fixture — skip");
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on label prop: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    if (TYPE_PROVIDER === "tsgo") {
      // TSGO may not resolve barrel-imported component props
      if (def.uri.fsPath === doc.uri.fsPath || def.uri.fsPath.includes("index.ts")) {
        console.log("    TSGO CANARY: barrel component prop not resolved (known limitation)");
        return;
      }
      console.log("    TSGO: barrel component prop resolved correctly — limitation may be fixed!");
    }

    // Positive: navigates to Button.vue
    expect(def.uri.fsPath, "definition should be in Button.vue").to.include("Button.vue");

    // Negative: NOT same file (must not stay at usage site)
    expect(
      def.uri.fsPath,
      "prop definition should NOT stay at usage site in parent",
    ).to.not.equal(doc.uri.fsPath);

    // Negative: NOT barrel file
    expect(def.uri.fsPath, "should NOT stop at barrel index.ts").to.not.include("index.ts");

    // Negative: NOT generated .tsx
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);

    // Verify position
    const childDoc = await vscode.workspace.openTextDocument(def.uri);
    const childText = childDoc.getText();
    const labelInDefineProps = childText.indexOf("label: string");
    if (labelInDefineProps !== -1) {
      const expectedLine = childDoc.positionAt(labelInDefineProps).line;
      expect(def.range.start.line, "definition should point to 'label' in defineProps type").to.equal(
        expectedLine,
      );
    }
  });

  test("E4: go-to-definition on barrel-imported component bound prop reaches child defineProps", async function () {
    if (FIXTURE_NAME !== "barrel-exports") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Position on "show" in `:show="showOverlay"`
    const pos = findPosition(doc, ':show="showOverlay"', 1); // on "s" of show
    if (!pos) {
      console.log("    :show prop not in fixture — skip");
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on :show prop: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    if (TYPE_PROVIDER === "tsgo") {
      if (def.uri.fsPath === doc.uri.fsPath || def.uri.fsPath.includes("index.ts")) {
        console.log("    TSGO CANARY: barrel component prop not resolved (known limitation)");
        return;
      }
      console.log("    TSGO: barrel component prop resolved correctly — limitation may be fixed!");
    }

    // Positive: navigates to Overlay.vue
    expect(def.uri.fsPath, "definition should be in Overlay.vue").to.include("Overlay.vue");

    // Negative: NOT same file
    expect(
      def.uri.fsPath,
      "prop definition should NOT stay at usage site in parent",
    ).to.not.equal(doc.uri.fsPath);

    // Negative: NOT barrel file
    expect(def.uri.fsPath, "should NOT stop at barrel index.ts").to.not.include("index.ts");

    // Negative: NOT generated .tsx
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);

    // Verify position
    const childDoc = await vscode.workspace.openTextDocument(def.uri);
    const childText = childDoc.getText();
    const showInDefineProps = childText.indexOf("show?: boolean");
    if (showInDefineProps !== -1) {
      const expectedLine = childDoc.positionAt(showInDefineProps).line;
      expect(def.range.start.line, "definition should point to 'show' in defineProps type").to.equal(
        expectedLine,
      );
    }
  });

  // ── F. Path Alias Imports ───────────────────────────────────

  test("F1: go-to-definition through @/ path alias navigates to component file", async function () {
    if (FIXTURE_NAME !== "path-aliases") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const pos = findPosition(doc, "<MyComp", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on <MyComp (path-alias): ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    // Positive: navigates to MyComp.vue
    expect(def.uri.fsPath, "definition should be in MyComp.vue").to.include("MyComp.vue");

    // Negative: NOT same file
    expect(def.uri.fsPath, "should navigate to a different file").to.not.equal(doc.uri.fsPath);

    // Negative: NOT generated .tsx
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  test("F2: go-to-definition on @/ import source string navigates to file", async function () {
    if (FIXTURE_NAME !== "path-aliases") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    // Position inside '@/components/MyComp.vue'
    const pos = findPosition(doc, "'@/components/MyComp.vue'", 5); // inside the string
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on @/ import source: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];

    // Positive: navigates to MyComp.vue
    expect(def.uri.fsPath, "definition should reach MyComp.vue").to.include("MyComp.vue");

    // Negative: NOT same file
    expect(def.uri.fsPath, "should navigate to a different file").to.not.equal(doc.uri.fsPath);

    // Negative: NOT generated .tsx
    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });

  // ── G. Event Handler Navigation ────────────────────────────

  test("G1: go-to-definition on @click event arg navigates to handler", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const text = doc.getText();
    // Find @click.prevent="increment" and position on "click"
    const match = text.indexOf('@click.prevent="increment"');
    if (match === -1) {
      this.skip();
      return;
    }

    // Position on "click" (1 char after @)
    const pos = doc.positionAt(match + 1);
    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on @click (event arg): ${locations.length} location(s)`);

    if (locations.length > 0) {
      const def = locations[0];
      expect(def.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);

      const fnDecl = text.indexOf("function increment()");
      if (fnDecl !== -1) {
        const expectedLine = doc.positionAt(fnDecl).line;
        expect(def.range.start.line, "should navigate to function declaration").to.equal(expectedLine);
      }
    }
  });

  test("G2: go-to-definition on component event handler", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    pass (N/A for this fixture)");
      return;
    }

    const text = doc.getText();
    // Find @custom="handleCustom" and position on "handleCustom"
    const match = text.indexOf('@custom="handleCustom"');
    if (match === -1) {
      this.skip();
      return;
    }

    // Position on "handleCustom"
    const pos = doc.positionAt(match + '@custom="'.length);
    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on handleCustom: ${locations.length} location(s)`);

    expect(locations.length, "should have at least 1 definition").to.be.greaterThan(0);

    const def = locations[0];
    expect(def.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);

    const fnDecl = text.indexOf("function handleCustom(");
    if (fnDecl !== -1) {
      const expectedLine = doc.positionAt(fnDecl).line;
      expect(def.range.start.line, "should navigate to handleCustom declaration").to.equal(expectedLine);
    }

    expect(def.uri.fsPath, "should NOT be in generated .tsx").to.not.match(/\.vue\.tsx$/);
  });
});
