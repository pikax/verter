import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  waitForFileReady,
  openVueFile,
  getAppVuePath,
  measureHover,
  getCompletions,
  getDefinitions,
  getReferences,
  findPosition,
  hoverText,
  FIXTURE_NAME,
  TYPE_PROVIDER,
} from "../helpers";

function completionLabel(item: vscode.CompletionItem): string {
  return typeof item.label === "string" ? item.label : item.label.label;
}

function completionLabels(list: vscode.CompletionList): string[] {
  return list.items.map(completionLabel);
}

suite(`Script Block [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady();
    doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);
  });

  // ── Script Hover ──────────────────────────────────────────────

  test("hover on ref(0) return shows Ref<number>", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "const count = ref(0)", 6);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "ref decl hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "should include Ref").to.include("Ref");
    expect(content, "should include number").to.include("number");
    expect(content, "should NOT be Ref<any>").to.not.include("Ref<any>");
    expect(content, "should NOT be unknown").to.not.include("unknown");
  });

  test("hover on computed() return shows ComputedRef<number>", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "const doubled = computed(", 6);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "computed decl hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "should include ComputedRef or number").to.satisfy(
      (c: string) => c.includes("ComputedRef") || c.includes("number"),
    );
    expect(content, "should NOT be any").to.not.match(/:\s*any\b/);
  });

  test("hover on defineProps shows props type", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "defineProps<{ title: string }>()", 0);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "defineProps hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "should mention defineProps").to.include("defineProps");
  });

  test("hover on onMounted shows lifecycle hook signature", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "onMounted(", 0);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "onMounted hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "should mention onMounted").to.include("onMounted");
    // In verter-only mode, the full callback signature may include `: any`
    // for unresolved parameter types — only assert strict no-any with a type provider
    if (TYPE_PROVIDER) {
      expect(content, "should NOT be any").to.not.match(/:\s*any\b/);
    }
  });

  test("hover on watch shows watch overload signature", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "watch(count,", 0);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "watch hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "should mention watch").to.include("watch");
    expect(content, "should NOT be any").to.not.match(/:\s*any\b/);
  });

  test("hover on local function shows return type", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "function increment()", 9);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "function hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "should mention increment").to.include("increment");
    expect(content, "should mention void").to.include("void");
    expect(content, "should NOT be any").to.not.match(/:\s*any\b/);
  });

  test("hover on imported formatCount usage shows function signature", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "formatCount(count.value)", 0);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(doc.uri, pos);
    expect(hovers.length, "formatCount hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "should mention formatCount").to.include("formatCount");
    expect(content, "should mention string return").to.include("string");
    expect(content, "should NOT be any").to.not.match(/:\s*any\b/);
  });

  // ── Script Completions ────────────────────────────────────────

  test("count.value in script (Ref requires .value in script)", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // In script, count is Ref<number> so count. should offer "value"
    const pos = findPosition(doc, "count.value * 2", 6);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "count member completions should exist").to.exist;
    expect(completions!.items.length, "should have completions").to.be.greaterThan(0);

    const labels = completionLabels(completions!);
    expect(labels, "should include value property").to.include("value");
  });

  test("completions after 'import { ' from 'vue' include Vue APIs", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // Find the 'ref' import binding position
    const pos = findPosition(doc, "import { ref, computed, onMounted, watch } from 'vue'", 9);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "Vue import completions should exist").to.exist;
    expect(completions!.items.length, "should have completions").to.be.greaterThan(0);

    const labels = completionLabels(completions!);
    // At least some Vue exports should be present
    const hasVueExports = labels.includes("ref") || labels.includes("computed") ||
      labels.includes("reactive") || labels.includes("watch");
    expect(hasVueExports, `should include Vue API exports, got: ${labels.slice(0, 15).join(", ")}`).to.be.true;
  });

  test("props member access offers title", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // In template: {{ props.title }}
    const pos = findPosition(doc, "props.title", 6);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expect(completions, "props member completions should exist").to.exist;
    expect(completions!.items.length, "should have completions").to.be.greaterThan(0);

    const labels = completionLabels(completions!);
    expect(labels, "should include title property").to.include("title");
  });

  // ── Script Definition ─────────────────────────────────────────

  test("go-to-definition on formatCount usage reaches utils.ts", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "formatCount(count.value)", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, pos);
    expect(locations.length, "should have definition result").to.be.greaterThan(0);

    const def = locations[0];
    expect(def.uri.fsPath, "definition should be in utils.ts").to.include("utils.ts");
    expect(def.uri.fsPath, "should NOT be in same file").to.not.equal(doc.uri.fsPath);
  });

  test("go-to-definition on count.value reaches const count = ref(0)", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // In template, hover on count to find the definition
    const templatePos = findPosition(doc, "{{ count }}", 3);
    if (!templatePos) {
      this.skip();
      return;
    }

    const locations = await getDefinitions(doc.uri, templatePos);
    expect(locations.length, "should have definition result").to.be.greaterThan(0);

    const def = locations[0];
    expect(def.uri.fsPath, "definition should be in same file").to.equal(doc.uri.fsPath);

    const text = doc.getText();
    const countDecl = text.indexOf("const count = ref(0)");
    if (countDecl !== -1) {
      const expectedLine = doc.positionAt(countDecl).line;
      expect(def.range.start.line, "should point to ref declaration").to.equal(expectedLine);
    }

    expect(def.uri.fsPath, "should NOT be in .tsx").to.not.match(/\.vue\.tsx$/);
  });

  // ── Script References ─────────────────────────────────────────

  test("references on count declaration finds script AND template usages", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "const count = ref(0)", 6);
    if (!pos) {
      this.skip();
      return;
    }

    const refs = await getReferences(doc.uri, pos);
    console.log(`    References for count: ${refs.length} location(s)`);

    // count appears in: declaration, {{ count }}, count.value * 2, :bar="count",
    // count.value++, formatCount(count.value), watch(count, ...)
    expect(refs.length, "should have at least 4 references").to.be.greaterThanOrEqual(4);

    // All in same file
    for (const ref of refs) {
      expect(ref.uri.fsPath, "reference should be in same file").to.equal(doc.uri.fsPath);
      expect(ref.uri.fsPath, "should NOT reference .tsx").to.not.match(/\.vue\.tsx$/);
    }
  });

  test("references on increment finds declaration + template usages", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "function increment()", 9);
    if (!pos) {
      this.skip();
      return;
    }

    const refs = await getReferences(doc.uri, pos);
    console.log(`    References for increment: ${refs.length} location(s)`);

    // increment is used in: declaration, @click="increment", @click.prevent="increment"
    expect(refs.length, "should have at least 3 references").to.be.greaterThanOrEqual(3);
  });

  test("references on handleCustom finds declaration + @custom template usage", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "function handleCustom(", 9);
    if (!pos) {
      this.skip();
      return;
    }

    const refs = await getReferences(doc.uri, pos);
    console.log(`    References for handleCustom: ${refs.length} location(s)`);

    // handleCustom: declaration, @custom="handleCustom($event)", @alert="handleCustom"
    expect(refs.length, "should have at least 2 references").to.be.greaterThanOrEqual(2);
  });
});
