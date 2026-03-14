import { expect } from "chai";
import * as vscode from "vscode";
import {
  assertLogNotContains,
  waitForExtensionReady,
  waitForFileReady,
  openAndReady,
  openVueFile,
  getAppVuePath,
  measureHover,
  findPosition,
  findNthPosition,
  hoverText,
  waitForHoverMatching,
  FIXTURE_NAME,
  TYPE_PROVIDER,
} from "../helpers";
import { getTimer } from "../timer";

function expectNoHoverDegrade(content: string, messagePrefix: string): void {
  expect(content, `${messagePrefix} should not degrade to any`).to.not.match(/:\s*any\b/);
  expect(content, `${messagePrefix} should not degrade to fallback component shell`).to.not.include(
    "DefineComponent<{}, {}>",
  );
}

suite(`Hover [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady();
    doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);
  });

  test("hover on ref binding in template shows typed result", async function () {
    // TSGO cannot work without tsconfig.json on disk
    if (FIXTURE_NAME === "no-config" && TYPE_PROVIDER === "tsgo") return this.skip();
    const pos = findPosition(doc, "{{ count }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("count (ref)", latencyMs);
    console.log(`    Hover on count: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "ref hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "ref hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "ref hover should mention count").to.include("count");
    expect(content, "ref hover should mention number").to.include("number");
    expectNoHoverDegrade(content, "ref hover");
  });

  test("hover on computed binding in template shows typed result", async function () {
    // TSGO cannot work without tsconfig.json on disk
    if (FIXTURE_NAME === "no-config" && TYPE_PROVIDER === "tsgo") return this.skip();
    const pos = findPosition(doc, "{{ doubled }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("doubled (computed)", latencyMs);
    console.log(`    Hover on doubled: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "computed hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "computed hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "computed hover should mention doubled").to.include("doubled");
    expect(content, "computed hover should mention number").to.include("number");
    expectNoHoverDegrade(content, "computed hover");
  });

  test("hover on prop binding in template shows string type", async function () {
    const pos = findPosition(doc, "{{ title }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("title (prop)", latencyMs);
    console.log(`    Hover on title: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "prop hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "prop hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "prop hover should mention title").to.include("title");
    expect(content, "prop hover should mention string").to.include("string");
    expectNoHoverDegrade(content, "prop hover");
  });

  test("hover on prop member expression shows string type", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "props.title", 6);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("title (prop member)", latencyMs);
    console.log(`    Hover on props.title: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "prop member hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "prop member hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "prop member hover should mention title").to.include("title");
    expect(content, "prop member hover should mention string").to.include("string");
    expectNoHoverDegrade(content, "prop member hover");
  });

  test("hover on component prop attribute shows child prop type", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, 'foo="literal"', 0);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("foo (prop attr)", latencyMs);
    console.log(`    Hover on foo attr: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "prop attr hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "prop attr hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "prop attr hover should mention foo").to.include("foo");
    expect(content, "prop attr hover should mention string").to.include("string");
    expectNoHoverDegrade(content, "prop attr hover");
  });

  test("hover on bound prop name shows child prop type", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, ':bar="count"', 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("bar (v-bind prop)", latencyMs);
    console.log(`    Hover on :bar: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "bound prop hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "bound prop hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "bound prop hover should mention bar").to.include("bar");
    expect(content, "bound prop hover should mention number").to.include("number");
    expectNoHoverDegrade(content, "bound prop hover");
  });

  test("hover on bound prop expression shows typed local", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, ':bar="count"', 6);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("count (v-bind value)", latencyMs);
    console.log(`    Hover on bound count: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "bound expression hover should exist").to.be.greaterThan(0);
    expect(
      hovers[0].contents.length,
      "bound expression hover should have content",
    ).to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "bound expression hover should mention count").to.include("count");
    expect(content, "bound expression hover should mention number").to.include("number");
    expectNoHoverDegrade(content, "bound expression hover");
  });

  test("hover on component tag shows real props and emits", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<MyComp", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("MyComp (component tag)", latencyMs);
    console.log(`    Hover on <MyComp: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "component tag hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "component tag hover should have content").to.be.greaterThan(
      0,
    );

    const content = hoverText(hovers[0]);
    expect(content, "component hover should mention foo").to.include("foo");
    expect(content, "component hover should mention bar").to.include("bar");
    expect(content, "component hover should mention custom").to.include("custom");
    expectNoHoverDegrade(content, "component tag hover");
  });

  test("hover on component event attribute shows payload type", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, '@custom="handleCustom($event)"', 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("custom (event attr)", latencyMs);
    console.log(`    Hover on @custom: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "event attr hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "event attr hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "event attr hover should mention custom").to.include("custom");
    expect(content, "event attr hover should mention payload").to.include("payload");
    expect(content, "event attr hover should mention string").to.include("string");
    expectNoHoverDegrade(content, "event attr hover");
  });

  test("hover on event handler function stays typed", async function () {
    const pos = findPosition(doc, '@click.prevent="increment"', '@click.prevent="'.length);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("increment (event handler)", latencyMs);
    console.log(`    Hover on increment handler: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "event handler hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "event handler hover should have content").to.be.greaterThan(
      0,
    );

    const content = hoverText(hovers[0]);
    expect(content, "event handler hover should mention increment").to.include("increment");
    expectNoHoverDegrade(content, "event handler hover");
  });

  test("hover on slot outlet tag stays meaningful", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const myCompDoc = await openAndReady("src/MyComp.vue");

    const pos = findPosition(myCompDoc, '<slot name="header"', 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(myCompDoc.uri, pos);
    getTimer().recordHover("slot outlet tag", latencyMs);
    console.log(`    Hover on <slot>: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "slot outlet hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "slot outlet hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "slot outlet hover should mention slot").to.match(/\bslot\b/i);
    expect(content, "slot outlet hover should not show generic () any").to.not.include("() any");
    expect(content, "slot outlet hover should not degrade to any").to.not.match(/:\s*any\b/);

    doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);
  });

  test("hover on slot consumer stays meaningful", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<template #header>", 10);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("#header (slot consumer)", latencyMs);
    console.log(`    Hover on #header: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "slot consumer hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "slot consumer hover should have content").to.be.greaterThan(
      0,
    );

    const content = hoverText(hovers[0]);
    expect(content, "slot consumer hover should mention slot").to.match(/\bslot\b/i);
    expect(content, "slot consumer hover should not degrade to any").to.not.match(/:\s*any\b/);
  });

  test("hover on slot name attribute value stays meaningful", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const myCompDoc = await openAndReady("src/MyComp.vue");

    const pos = findPosition(myCompDoc, 'name="header"', 6);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(myCompDoc.uri, pos);
    getTimer().recordHover("header (slot name attr)", latencyMs);
    console.log(`    Hover on slot name header: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "slot name hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "slot name hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "slot name hover should not show generic () any").to.not.include("() any");
    expect(content, "slot name hover should not degrade to any").to.not.match(/:\s*any\b/);

    doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);
  });

  test("hover on v-for local shows resolved type", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "action.disabled", 0);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("action (v-for local)", latencyMs);
    console.log(`    Hover on action: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "v-for local hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "v-for local hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    const hasNamedType = content.includes("Action");
    const hasExpandedType =
      content.includes("label") && content.includes("disabled") && content.includes("handler");
    if (hasNamedType && !hasExpandedType) {
      console.log("    Note: v-for hover shows named type only (Action), not expanded properties");
    } else if (!hasNamedType && hasExpandedType) {
      console.log("    Note: v-for hover shows expanded type only, not named type (Action)");
    }
    expect(hasNamedType || hasExpandedType, `unexpected v-for local hover:\n${content}`).to.equal(
      true,
    );
    expect(content, "v-for local hover should not degrade to any").to.not.match(/:\s*any\b/);
  });

  test("hover on v-for member shows property type", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "action.disabled", 7);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("disabled (v-for member)", latencyMs);
    console.log(`    Hover on action.disabled: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "v-for member hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "v-for member hover should have content").to.be.greaterThan(
      0,
    );

    const content = hoverText(hovers[0]);
    expect(content, "v-for member hover should mention disabled").to.include("disabled");
    expect(content, "v-for member hover should mention boolean").to.include("boolean");
    expect(content, "v-for member hover should not degrade to any").to.not.match(/:\s*any\b/);
  });

  test("hover on nested v-for outer local resolves correctly", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findNthPosition(doc, "user.name", 1, 0);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("user (nested v-for outer)", latencyMs);
    console.log(`    Hover on nested user: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "nested v-for hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "nested v-for hover should have content").to.be.greaterThan(
      0,
    );

    const content = hoverText(hovers[0]);
    const hasNamedType = content.includes("User");
    const hasExpandedType =
      content.includes("name") && content.includes("email") && content.includes("age");
    if (hasNamedType && !hasExpandedType) {
      console.log(
        "    Note: nested v-for hover shows named type only (User), not expanded properties",
      );
    } else if (!hasNamedType && hasExpandedType) {
      console.log("    Note: nested v-for hover shows expanded type only, not named type (User)");
    }
    expect(hasNamedType || hasExpandedType, `unexpected nested v-for hover:\n${content}`).to.equal(
      true,
    );
    expect(content, "nested v-for hover should not degrade to any").to.not.match(/:\s*any\b/);
  });

  test("hover on narrowed variable excludes null", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "selectedUser.name", 0);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("selectedUser (narrowed)", latencyMs);
    console.log(`    Hover on selectedUser: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "narrowed hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "narrowed hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "narrowed hover should mention User").to.include("User");
    expect(content, "narrowed hover should not include null").to.not.include("null");
    expect(content, "narrowed hover should not degrade to any").to.not.match(/:\s*any\b/);
  });

  test("hover on direct .vue import binding shows typed component", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "import MyComp from './MyComp.vue'", 7);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("MyComp (import binding)", latencyMs);
    console.log(`    Hover on imported MyComp: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "import binding hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "import binding hover should have content").to.be.greaterThan(
      0,
    );

    const content = hoverText(hovers[0]);
    expect(content, "import binding hover should mention foo").to.include("foo");
    expect(content, "import binding hover should mention bar").to.include("bar");
    expectNoHoverDegrade(content, "import binding hover");
  });

  test("hover on v-slot local and member are typed", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const slotDoc = await openVueFile("src/TemplateSlotCases.vue");
    const readyPos = findPosition(slotDoc, "{{ outerLabel }}", 3);
    expect(readyPos, "should find slot outer-label probe").to.exist;
    await waitForFileReady(slotDoc, {
      probePosition: readyPos!,
      expectedLabel: "outerLabel",
    });

    const localPos = findPosition(slotDoc, "slotItem.name", 0);
    const memberPos = findPosition(slotDoc, "slotItem.name", 9);
    expect(localPos, "should find slot local").to.exist;
    expect(memberPos, "should find slot member").to.exist;

    const localHovers = await waitForHoverMatching(slotDoc.uri, localPos!, {
      predicate: (hovers) => hovers.length > 0 && hoverText(hovers[0]).includes("slotItem"),
    });
    const memberHovers = await waitForHoverMatching(slotDoc.uri, memberPos!, {
      predicate: (hovers) => hovers.length > 0 && hoverText(hovers[0]).includes("name"),
    });
    const localHover = await measureHover(slotDoc.uri, localPos!);
    const memberHover = await measureHover(slotDoc.uri, memberPos!);

    expect(localHovers.length, "slot local hover should exist").to.be.greaterThan(0);
    expect(memberHovers.length, "slot member hover should exist").to.be.greaterThan(0);
    expect(localHover.hovers.length, "slot local hover should exist").to.be.greaterThan(0);
    expect(memberHover.hovers.length, "slot member hover should exist").to.be.greaterThan(0);

    const localContent = hoverText(localHover.hovers[0]);
    const memberContent = hoverText(memberHover.hovers[0]);

    expect(localContent, "slot local hover should mention slotItem").to.include("slotItem");
    const hasNamedType = localContent.includes("SlotItem");
    const hasExpandedType = localContent.includes("name") && localContent.includes("id");
    expect(
      hasNamedType || hasExpandedType,
      `slot local hover should show named or expanded slot type:\n${localContent}`,
    ).to.equal(true);
    expect(localContent, "slot local hover should not degrade to any").to.not.match(/:\s*any\b/);

    expect(memberContent, "slot member hover should mention name").to.include("name");
    expect(memberContent, "slot member hover should mention string").to.include("string");
    expect(memberContent, "slot member hover should not degrade to any").to.not.match(/:\s*any\b/);
  });

  test("hover survives broken script recovery for earlier template bindings", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const recoveryDoc = await openAndReady("src/TemplateRecovery.vue");

    const pos = findPosition(recoveryDoc, "{{ count }}", 3);
    expect(pos, "should find recovered count usage").to.exist;

    const { hovers, latencyMs } = await measureHover(recoveryDoc.uri, pos!);
    getTimer().recordHover("count (broken script recovery)", latencyMs);
    console.log(`    Hover in TemplateRecovery count: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "recovery hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "recovery hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "recovery hover should mention count").to.include("count");
    expect(content, "recovery hover should mention number").to.include("number");
    expect(content, "recovery hover should not degrade to any").to.not.match(/:\s*any\b/);
  });

  test("hover in template-only file stays meaningful for slot outlets", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const templateOnlyDoc = await openAndReady("src/TemplateOnly.vue");

    const pos = findPosition(templateOnlyDoc, '<slot name="header"', 1);
    expect(pos, "should find template-only slot outlet").to.exist;

    const { hovers } = await measureHover(templateOnlyDoc.uri, pos!);
    expect(hovers.length, "template-only hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content.toLowerCase(), "template-only hover should mention slot").to.include("slot");
    expect(content, "template-only hover should not show generic () any").to.not.include("() any");
  });

  test("hover in JS SFC keeps typed template bindings", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const jsDoc = await openAndReady("src/JsTemplateCases.vue");

    const pos = findPosition(jsDoc, "{{ count }}", 3);
    expect(pos, "should find JS SFC count usage").to.exist;

    const { hovers } = await measureHover(jsDoc.uri, pos!);
    expect(hovers.length, "JS SFC hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "JS SFC hover should mention count").to.include("count");
    expect(content, "JS SFC hover should mention number").to.include("number");
    expect(content, "JS SFC hover should not degrade to any").to.not.match(/:\s*any\b/);
  });

  test("hover on v-model binding shows Ref<string>", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, 'v-model="inputVal"', 9);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    console.log(`    Hover on v-model inputVal: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "v-model hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "v-model hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "v-model hover should mention inputVal").to.include("inputVal");
    // Verter-only hover may show `const inputVal` without inferred type
    if (TYPE_PROVIDER) {
      expect(content, "v-model hover should mention string").to.include("string");
    }
    expectNoHoverDegrade(content, "v-model hover");
  });

  test("hover on v-for destructured param shows string type", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // In: <div v-for="{ name, email } in users" :key="email">{{ name }} ({{ email }})</div>
    // Find the {{ name }} usage which is inside the v-for scope
    const pos = findPosition(doc, "{{ name }} ({{ email }})", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    console.log(`    Hover on destructured name: ${latencyMs}ms, ${hovers.length} result(s)`);

    // Verter-only mode may not provide hover for destructured v-for params
    if (hovers.length === 0 && !TYPE_PROVIDER) {
      console.log("    Verter-only: no hover for destructured v-for param (needs type provider)");
      return;
    }

    expect(hovers.length, "destructured param hover should exist").to.be.greaterThan(0);
    expect(
      hovers[0].contents.length,
      "destructured param hover should have content",
    ).to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "destructured param hover should mention name").to.include("name");
    expect(content, "destructured param hover should mention string").to.include("string");
    expectNoHoverDegrade(content, "destructured param hover");
  });

  test("hover on v-for index shows number type", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // In: <span v-for="(user, idx) in users" :key="idx">
    const pos = findPosition(doc, "(user, idx)", 7);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    console.log(`    Hover on v-for idx: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "v-for index hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "v-for index hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "v-for index hover should mention idx").to.include("idx");
    expect(content, "v-for index hover should mention number").to.include("number");
    expectNoHoverDegrade(content, "v-for index hover");
  });

  test("hover on $event parameter shows Event type", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "handleInput($event)", 12);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    console.log(`    Hover on $event: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "$event hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "$event hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "$event hover should mention Event").to.include("Event");
    expect(content, "$event hover should not be any").to.not.match(/:\s*any\b/);
  });

  test("hover on ref shows Ref<number> not Ref<any>", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // Hover on "count" in the script: const count = ref(0)
    const pos = findPosition(doc, "const count = ref(0)", 6);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    console.log(`    Hover on count decl: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "ref decl hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "ref decl hover should include Ref").to.include("Ref");
    expect(content, "ref decl hover should include number").to.include("number");
    expect(content, "ref decl hover should NOT be Ref<any>").to.not.include("Ref<any>");
    expect(content, "ref decl hover should NOT be unknown").to.not.include("unknown");
  });

  test("hover on computed shows ComputedRef<number>", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "const doubled = computed(", 6);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    console.log(`    Hover on doubled decl: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "computed decl hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "computed decl hover should include ComputedRef or number").to.satisfy(
      (c: string) => c.includes("ComputedRef") || c.includes("number"),
      "expected ComputedRef or number type",
    );
    expect(content, "computed decl hover should NOT be any").to.not.match(/:\s*any\b/);
  });

  test("hover on TypeResolutionCases union type shows exact union", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const trDoc = await openAndReady("src/TypeResolutionCases.vue");

    const pos = findPosition(trDoc, "{{ mixed }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers } = await measureHover(trDoc.uri, pos);
    expect(hovers.length, "union type hover should exist").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "union type hover should mention string").to.include("string");
    expect(content, "union type hover should mention number").to.include("number");
    expectNoHoverDegrade(content, "union type hover");
  });

  // ── Monorepo Fixture ──────────────────────────────────────────

  test("hover on cross-package component tag shows typed props (monorepo)", async function () {
    if (FIXTURE_NAME !== "monorepo") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<SharedComp", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("SharedComp (monorepo)", latencyMs);
    console.log(`    Hover on <SharedComp: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "component tag hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "component tag hover should have content").to.be.greaterThan(
      0,
    );

    const content = hoverText(hovers[0]);
    expect(content, "component hover should mention foo").to.include("foo");
    expect(content, "component hover should mention bar").to.include("bar");
    expectNoHoverDegrade(content, "monorepo component tag hover");
  });

  test("hover on cross-package imported function return (monorepo)", async function () {
    if (FIXTURE_NAME !== "monorepo") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "{{ greeting }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("greeting (monorepo)", latencyMs);
    console.log(`    Hover on greeting: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "greeting hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "greeting hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    console.log(`    greeting hover content: ${content.slice(0, 120)}`);

    // Without a type provider, cross-package imports may degrade to `any`
    if (!TYPE_PROVIDER && content.match(/:\s*any\b/)) {
      console.log(
        "    Verter-only: cross-package import type degrades to any (needs type provider)",
      );
      return;
    }

    const hasGreeting = content.includes("greeting");
    const hasString = content.includes("string");
    const hasHelper = content.includes("helper");
    expect(
      hasGreeting || hasString || hasHelper,
      `greeting hover should mention greeting, string, or helper:\n${content}`,
    ).to.equal(true);
    expectNoHoverDegrade(content, "monorepo greeting hover");
  });

  // ── Composite Paths Fixture ────────────────────────────────────

  test("hover on @/-imported component in composite project (composite-paths)", async function () {
    if (FIXTURE_NAME !== "composite-paths") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<HelloWorld", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("HelloWorld (composite-paths)", latencyMs);
    console.log(`    Hover on <HelloWorld: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "component tag hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "component tag hover should have content").to.be.greaterThan(
      0,
    );

    const content = hoverText(hovers[0]);
    expect(content, "component hover should mention msg").to.include("msg");
    expectNoHoverDegrade(content, "composite-paths component hover");
  });

  // ── Path Aliases Fixture ───────────────────────────────────────

  test("hover on @/-imported component shows typed props (path-aliases)", async function () {
    if (FIXTURE_NAME !== "path-aliases") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<MyComp", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("MyComp (path-aliases)", latencyMs);
    console.log(`    Hover on <MyComp: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "component tag hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "component tag hover should have content").to.be.greaterThan(
      0,
    );

    const content = hoverText(hovers[0]);
    expect(content, "component hover should mention foo").to.include("foo");
    expectNoHoverDegrade(content, "path-aliases component hover");
  });

  // ── Tsconfig References Fixture ────────────────────────────────

  test("hover on component in solution-style project (tsconfig-references)", async function () {
    if (FIXTURE_NAME !== "tsconfig-references") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<MyComp", 1);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("MyComp (tsconfig-references)", latencyMs);
    console.log(`    Hover on <MyComp: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "component tag hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "component tag hover should have content").to.be.greaterThan(
      0,
    );

    const content = hoverText(hovers[0]);
    expect(content, "component hover should mention foo").to.include("foo");
    expectNoHoverDegrade(content, "tsconfig-references component hover");
  });

  // ── No-Config Fixture ──────────────────────────────────────────

  test("hover on ref binding works without tsconfig (no-config)", async function () {
    if (FIXTURE_NAME !== "no-config") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "{{ count }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("count (no-config)", latencyMs);
    console.log(`    Hover on count: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "ref hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "ref hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "ref hover should mention count").to.include("count");
    expectNoHoverDegrade(content, "no-config ref hover");
  });

  // ── Single-File Fixture ────────────────────────────────────────

  test("hover works in single-file project (single-file)", async function () {
    if (FIXTURE_NAME !== "single-file") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "{{ count }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const { hovers, latencyMs } = await measureHover(doc.uri, pos);
    getTimer().recordHover("count (single-file)", latencyMs);
    console.log(`    Hover on count: ${latencyMs}ms, ${hovers.length} result(s)`);

    expect(hovers.length, "ref hover should exist").to.be.greaterThan(0);
    expect(hovers[0].contents.length, "ref hover should have content").to.be.greaterThan(0);

    const content = hoverText(hovers[0]);
    expect(content, "ref hover should mention count").to.include("count");
    expectNoHoverDegrade(content, "single-file ref hover");
  });

  test("hover latency is reasonable", function () {
    const report = getTimer().getReport();
    const samples = report.hover.samples;

    if (samples.length === 0) {
      console.log("    No hover samples collected");
      return;
    }

    const latencies = samples.map((sample) => sample.latencyMs);
    const avg = latencies.reduce((sum, latency) => sum + latency, 0) / latencies.length;
    const max = Math.max(...latencies);

    console.log(
      `    Hover stats: avg=${Math.round(avg)}ms, max=${max}ms, samples=${samples.length}`,
    );

    expect(avg, "Average hover latency should stay under 5s").to.be.lessThan(5_000);
  });

  test("hover scenarios do not log panic markers", function () {
    assertLogNotContains("panicked at", "hover flows should not trigger Rust panics");
    assertLogNotContains("thread 'main' panicked", "hover flows should not crash the server");
  });
});
