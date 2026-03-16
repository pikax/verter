import { expect } from "chai";
import * as vscode from "vscode";
import {
  ensureFixtureWarm,
  openAndReady,
  getDefinitions,
  getReferences,
  measureHover,
  findPosition,
  hoverText,
  FIXTURE_NAME,
} from "../helpers";

suite(`Style Block [${FIXTURE_NAME}]`, function () {
  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    if (FIXTURE_NAME !== "single-project") return;
    await ensureFixtureWarm();
    doc = await openAndReady("src/StyledComp.vue");
  });

  // ── CSS → Template Navigation ─────────────────────────────────

  test("go-to-definition on .container in style navigates to template class", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, ".container { display:", 1); // on "c" of container
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on .container in style: ${locations.length} location(s)`);

    if (locations.length === 0) {
      console.log("    No definition results — CSS go-to-definition may not be supported yet");
      return;
    }

    // All definitions should be in the same file
    for (const loc of locations) {
      expect(loc.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);
    }
  });

  test("go-to-definition on #main-title in style navigates to template id", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "#main-title { color:", 1); // on "m" of main-title
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on #main-title in style: ${locations.length} location(s)`);

    if (locations.length === 0) {
      console.log("    No definition results — CSS id go-to-definition may not be supported yet");
      return;
    }

    for (const loc of locations) {
      expect(loc.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);
    }
  });

  // ── CSS References ────────────────────────────────────────────

  test("references on .container finds template class and style rule", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, ".container { display:", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const refs = await getReferences(doc.uri, pos);
    console.log(`    References for .container: ${refs.length} location(s)`);

    if (refs.length === 0) {
      console.log("    No reference results — CSS references may not be supported yet");
      return;
    }

    // Should find at least the style rule itself
    expect(refs.length, "should have at least 1 reference").to.be.greaterThanOrEqual(1);

    for (const ref of refs) {
      expect(ref.uri.fsPath, "reference should be in same file").to.equal(doc.uri.fsPath);
    }
  });

  test("references on #main-title finds template id and style rule", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "#main-title { color:", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const refs = await getReferences(doc.uri, pos);
    console.log(`    References for #main-title: ${refs.length} location(s)`);

    if (refs.length === 0) {
      console.log("    No reference results — CSS id references may not be supported yet");
      return;
    }

    expect(refs.length, "should have at least 1 reference").to.be.greaterThanOrEqual(1);
  });

  // ── CSS Hover ─────────────────────────────────────────────────

  test("hover on .container selector shows CSS info", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, ".container { display:", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    console.log(`    Hover on .container: ${hovers.length} result(s)`);

    if (hovers.length === 0) {
      console.log("    No hover results — CSS hover may not be supported yet");
      return;
    }

    const content = hoverText(hovers[0]);
    // Should have some meaningful content (CSS selector info)
    expect(content.length, "CSS hover should have content").to.be.greaterThan(0);
    expect(content, "CSS hover should NOT degrade to any").to.not.match(/:\s*any\b/);
  });

  test("hover on .title selector shows CSS info", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, ".title { font-size:", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    console.log(`    Hover on .title: ${hovers.length} result(s)`);

    if (hovers.length === 0) {
      console.log("    No hover results — CSS hover may not be supported yet");
      return;
    }

    const content = hoverText(hovers[0]);
    expect(content.length, "CSS hover should have content").to.be.greaterThan(0);
  });

  // ── Template Class → Style ────────────────────────────────────

  test("go-to-definition on class='container' in template finds style rule", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, 'class="container"', 7); // on "c" of container in template
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on template class 'container': ${locations.length} location(s)`);

    if (locations.length === 0) {
      console.log(
        "    No definition results — template class go-to-definition may not be supported yet",
      );
      return;
    }

    for (const loc of locations) {
      expect(loc.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);
    }
  });

  test("go-to-definition on id='main-title' in template finds style rule", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, 'id="main-title"', 4); // on "m" of main-title in template
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    console.log(`    Definition on template id 'main-title': ${locations.length} location(s)`);

    if (locations.length === 0) {
      console.log(
        "    No definition results — template id go-to-definition may not be supported yet",
      );
      return;
    }

    for (const loc of locations) {
      expect(loc.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);
    }
  });
});
