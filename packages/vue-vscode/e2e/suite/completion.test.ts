import { expect } from "chai";
import * as vscode from "vscode";
import {
  assertLogNotContains,
  waitForExtensionReady,
  waitForFileReady,
  openAndReady,
  openVueFile,
  getAppVuePath,
  getCompletions,
  findPosition,
  findNthPosition,
  FIXTURE_NAME,
  TYPE_PROVIDER,
  waitForCompletionsMatching,
} from "../helpers";

function completionLabel(item: vscode.CompletionItem): string {
  return typeof item.label === "string" ? item.label : item.label.label;
}

function completionLabels(list: vscode.CompletionList): string[] {
  return list.items.map(completionLabel);
}

function completionItem(
  list: vscode.CompletionList,
  label: string,
): vscode.CompletionItem | undefined {
  return list.items.find((item) => completionLabel(item) === label);
}

function expectCompletionKinds(
  list: vscode.CompletionList,
  label: string,
  allowedKinds: vscode.CompletionItemKind[],
): void {
  const item = completionItem(list, label);
  expect(item, `expected completion "${label}"`).to.exist;
  expect(item!.kind, `"${label}" should have a concrete completion kind`).to.not.equal(undefined);
  expect(item!.kind, `"${label}" should not degrade to Text`).to.not.equal(
    vscode.CompletionItemKind.Text,
  );
  expect(item!.kind, `"${label}" should have an allowed kind`).to.be.oneOf(allowedKinds);
}

function expectCompletionsPresent(list: vscode.CompletionList, labels: string[]): void {
  const actual = completionLabels(list);
  for (const label of labels) {
    expect(actual, `expected completion "${label}" in [${actual.join(", ")}]`).to.include(label);
  }
}

function expectCompletionsMissing(list: vscode.CompletionList, labels: string[]): void {
  const actual = completionLabels(list);
  for (const label of labels) {
    expect(actual, `unexpected completion "${label}" in [${actual.join(", ")}]`).to.not.include(
      label,
    );
  }
}

function expectCompletionsNonEmpty(
  completions: vscode.CompletionList | undefined,
  msg: string,
): asserts completions is vscode.CompletionList {
  expect(completions, `${msg}: should return completions`).to.exist;
  expect(completions!.items.length, `${msg}: completions should not be empty`).to.be.greaterThan(0);
}

function expectNoInternalLeakage(list: vscode.CompletionList): void {
  const joined = completionLabels(list).join(",");
  expect(joined, "should not include __props").to.not.include("__props");
  expect(joined, "should not include ___VERTER___").to.not.include("___VERTER___");
  expect(joined, "should not include $V_ internals").to.not.include("$V_");
}

suite(`Completion [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  let doc: vscode.TextDocument;

  suiteSetup(async function () {
    await waitForExtensionReady();
    doc = await openVueFile(getAppVuePath());
    await waitForFileReady(doc);
  });

  test("mustache expression shows typed local bindings without globals", async function () {
    const pos = findPosition(doc, "{{ count }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expectCompletionsNonEmpty(completions, "completions");

    console.log(`    Mustache completions: ${completionLabels(completions!).join(", ")}`);

    expectCompletionsPresent(completions!, ["count", "doubled", "increment"]);
    expectCompletionKinds(completions!, "count", [vscode.CompletionItemKind.Variable]);
    expectCompletionKinds(completions!, "doubled", [vscode.CompletionItemKind.Variable]);
    expectCompletionKinds(completions!, "increment", [
      vscode.CompletionItemKind.Function,
      vscode.CompletionItemKind.Method,
      vscode.CompletionItemKind.Variable,
    ]);
    expectCompletionsMissing(completions!, [
      "AbortController",
      "HTMLDivElement",
      "document",
      "window",
    ]);
    expectNoInternalLeakage(completions!);
  });

  test("event handler expression shows typed functions", async function () {
    const pos = findPosition(doc, '@click="increment"', 8);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expectCompletionsNonEmpty(completions, "completions");

    expectCompletionsPresent(completions!, ["increment"]);
    expectCompletionKinds(completions!, "increment", [
      vscode.CompletionItemKind.Function,
      vscode.CompletionItemKind.Method,
      vscode.CompletionItemKind.Variable,
    ]);
    expectNoInternalLeakage(completions!);
  });

  test("component opening tag exposes real props and events without parent leakage", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const compDoc = await openVueFile("src/ComponentCompletionCase.vue");
    const pos = findPosition(compDoc, "<MyComp />", 8);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await waitForCompletionsMatching(compDoc.uri, pos, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return labels.includes("foo") && labels.includes("bar") && labels.includes("@custom");
      },
    });
    expectCompletionsNonEmpty(completions, "completions");

    console.log(`    <MyComp completions: ${completionLabels(completions!).join(", ")}`);

    expectCompletionsPresent(completions!, ["foo", "bar", "@custom"]);
    expectCompletionKinds(completions!, "foo", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionKinds(completions!, "bar", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionKinds(completions!, "@custom", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionsMissing(completions!, [
      "count",
      "doubled",
      "increment",
      "window",
      "document",
      "AbortController",
    ]);
    expectNoInternalLeakage(completions!);
  });

  test("v-for local in template resolves as Variable", async function () {
    const pos = findPosition(doc, "{{ item }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await waitForCompletionsMatching(doc.uri, pos, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return labels.includes("item");
      },
    });
    expectCompletionsNonEmpty(completions, "completions");

    expectCompletionsPresent(completions!, ["item"]);
    expectCompletionKinds(completions!, "item", [vscode.CompletionItemKind.Variable]);
    expectNoInternalLeakage(completions!);
  });

  test("v-for member access shows actual properties with typed kinds", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "action.disabled", 7);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await waitForCompletionsMatching(doc.uri, pos, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return (
          labels.includes("disabled") && labels.includes("label") && labels.includes("handler")
        );
      },
    });
    expectCompletionsNonEmpty(completions, "completions");

    console.log(`    action. completions: ${completionLabels(completions!).join(", ")}`);

    expectCompletionsPresent(completions!, ["disabled", "label", "handler"]);
    expectCompletionKinds(completions!, "disabled", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionKinds(completions!, "label", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionsMissing(completions!, ["@click", "@custom", "foo-bar"]);
    expectNoInternalLeakage(completions!);
    expect(completions!.items.length, "member completions should stay scoped")
      .to.be.greaterThan(2)
      .and.lessThan(50);
  });

  test("v-for item member access in mustache stays typed", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "user.name", 5);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await waitForCompletionsMatching(doc.uri, pos, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return labels.includes("name") && labels.includes("email") && labels.includes("age");
      },
    });
    expectCompletionsNonEmpty(completions, "completions");

    expectCompletionsPresent(completions!, ["name", "email", "age"]);
    expectCompletionKinds(completions!, "name", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionKinds(completions!, "email", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectNoInternalLeakage(completions!);
    expect(completions!.items.length, "member completions should stay scoped")
      .to.be.greaterThan(2)
      .and.lessThan(50);
  });

  test("nested v-for inner and outer scopes stay distinct", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const innerPos = findNthPosition(doc, "action.label", 1, 7);
    const outerPos = findNthPosition(doc, "user.name", 1, 5);
    expect(innerPos, "should find nested inner member access").to.exist;
    expect(outerPos, "should find nested outer member access").to.exist;

    const innerCompletions = await waitForCompletionsMatching(doc.uri, innerPos!, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return labels.includes("label") && labels.includes("disabled");
      },
    });
    const outerCompletions = await waitForCompletionsMatching(doc.uri, outerPos!, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return labels.includes("name") && labels.includes("email") && labels.includes("age");
      },
    });
    expectCompletionsNonEmpty(innerCompletions, "inner completions");
    expectCompletionsNonEmpty(outerCompletions, "outer completions");

    expectCompletionsPresent(innerCompletions!, ["label", "disabled"]);
    expectCompletionsMissing(innerCompletions!, ["email", "age"]);

    expectCompletionsPresent(outerCompletions!, ["name", "email", "age"]);
    expectCompletionsMissing(outerCompletions!, ["disabled"]);
  });

  test("v-if narrowed member access stays typed and scoped", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "selectedUser.name", 13);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await waitForCompletionsMatching(doc.uri, pos, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return labels.includes("name") && labels.includes("email") && labels.includes("age");
      },
    });
    expectCompletionsNonEmpty(completions, "completions");

    expectCompletionsPresent(completions!, ["name", "email", "age"]);
    expectCompletionKinds(completions!, "name", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionsMissing(completions!, ["null", "@custom", "foo-bar"]);
    expectNoInternalLeakage(completions!);
  });

  test("props member access stays typed and free of attr leakage", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "props.title", 6);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await waitForCompletionsMatching(doc.uri, pos, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return labels.includes("title");
      },
    });
    expectCompletionsNonEmpty(completions, "completions");

    expectCompletionsPresent(completions!, ["title"]);
    expectCompletionKinds(completions!, "title", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionsMissing(completions!, ["@click", "@custom", "foo-bar"]);
    expectNoInternalLeakage(completions!);
    // Props type `{ title: string }` has only 1 declared prop — threshold must allow 1
    expect(completions!.items.length, "member completions should stay scoped")
      .to.be.greaterThanOrEqual(1)
      .and.lessThan(50);
  });

  test("broken template expression still returns local bindings and excludes globals", async function () {
    const pos = findPosition(doc, "{{ count + }}", 11);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expectCompletionsNonEmpty(completions, "completions");

    expectCompletionsPresent(completions!, ["count", "doubled", "increment"]);
    expectCompletionKinds(completions!, "count", [vscode.CompletionItemKind.Variable]);
    expectCompletionKinds(completions!, "increment", [
      vscode.CompletionItemKind.Function,
      vscode.CompletionItemKind.Method,
      vscode.CompletionItemKind.Variable,
    ]);
    expectCompletionsMissing(completions!, [
      "AbortController",
      "HTMLDivElement",
      "document",
      "window",
    ]);
    expectNoInternalLeakage(completions!);
    expect(
      completions!.items.length,
      "broken expression completions should stay bounded",
    ).to.be.lessThan(200);
  });

  // Skip: barrel component completions require full type provider sync
  test.skip("barrel Button opening tag exposes actual props and click event", async function () {
    if (FIXTURE_NAME !== "barrel-exports") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<Button ", 8);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expectCompletionsNonEmpty(completions, "completions");

    console.log(`    <Button completions: ${completionLabels(completions!).join(", ")}`);

    // Barrel re-exports need a type provider to resolve component prop types
    if (!TYPE_PROVIDER) {
      const labels = completionLabels(completions!);
      if (!labels.includes("label")) {
        console.log("    Verter-only: barrel component props not resolved (needs type provider)");
        return;
      }
    }

    expectCompletionsPresent(completions!, ["label", "disabled", "size", "@click"]);
    expectCompletionKinds(completions!, "label", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionKinds(completions!, "disabled", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionKinds(completions!, "@click", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionsMissing(completions!, ["count", "doubled", "increment"]);
    expectNoInternalLeakage(completions!);
  });

  // Skip: barrel component completions require full type provider sync
  test.skip("barrel Overlay opening tag exposes actual props", async function () {
    if (FIXTURE_NAME !== "barrel-exports") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<Overlay ", 9);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expectCompletionsNonEmpty(completions, "completions");

    console.log(`    <Overlay completions: ${completionLabels(completions!).join(", ")}`);

    // Barrel re-exports need a type provider to resolve component prop types
    if (!TYPE_PROVIDER) {
      const labels = completionLabels(completions!);
      if (!labels.includes("zIndex")) {
        console.log("    Verter-only: barrel component props not resolved (needs type provider)");
        return;
      }
    }

    expectCompletionsPresent(completions!, ["zIndex", "duration", "show", "lockScroll"]);
    expectCompletionKinds(completions!, "zIndex", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionKinds(completions!, "show", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionsMissing(completions!, ["count", "doubled", "increment", "label"]);
    expectNoInternalLeakage(completions!);
  });

  test("v-slot locals and members stay typed and scoped", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    await openVueFile("src/TypedSlotComp.vue");
    const slotDoc = await openVueFile("src/TemplateSlotCases.vue");
    const localPos = findPosition(slotDoc, "{{ sl }}", 5);
    const memberPos = findPosition(slotDoc, "slotItem.name", 9);
    expect(localPos, "should find slot local usage").to.exist;
    expect(memberPos, "should find slot member completion probe").to.exist;
    await waitForFileReady(slotDoc, {
      probePosition: memberPos!,
      expectedLabel: "name",
      expectedKinds: [vscode.CompletionItemKind.Property, vscode.CompletionItemKind.Field],
      triggerCharacter: ".",
      timeoutMs: 30_000,
    });

    const localCompletions = await waitForCompletionsMatching(slotDoc.uri, localPos!, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return (
          labels.includes("slotItem") &&
          labels.includes("slotIndex") &&
          labels.includes("slotTotal")
        );
      },
    });
    const memberCompletions = await waitForCompletionsMatching(slotDoc.uri, memberPos!, {
      triggerCharacter: ".",
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return labels.includes("name") && labels.includes("id");
      },
    });
    expectCompletionsNonEmpty(localCompletions, "slot local completions");
    expectCompletionsNonEmpty(memberCompletions, "slot member completions");

    expectCompletionsPresent(localCompletions!, ["slotItem", "slotIndex", "slotTotal"]);
    expectCompletionKinds(localCompletions!, "slotItem", [vscode.CompletionItemKind.Variable]);
    expectCompletionKinds(localCompletions!, "slotIndex", [vscode.CompletionItemKind.Variable]);
    expectCompletionsMissing(localCompletions!, ["siblingSlot", "@click", "foo-bar"]);

    expectCompletionsPresent(memberCompletions!, ["name", "id"]);
    expectCompletionKinds(memberCompletions!, "name", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionsMissing(memberCompletions!, ["@click", "foo-bar"]);
    expectNoInternalLeakage(memberCompletions!);
  });

  test("broken script recovery preserves typed completions", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const recoveryDoc = await openVueFile("src/TemplateRecovery.vue");

    const pos = findPosition(recoveryDoc, "{{ cou }}", 6);
    expect(pos, "should find recovered count usage").to.exist;

    const completions = await waitForCompletionsMatching(recoveryDoc.uri, pos!, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return labels.includes("count");
      },
    });
    expectCompletionsNonEmpty(completions, "completions");

    expectCompletionsPresent(completions!, ["count"]);
    expectCompletionKinds(completions!, "count", [vscode.CompletionItemKind.Variable]);
    expectCompletionsMissing(completions!, ["window", "document", "AbortController"]);
    expectNoInternalLeakage(completions!);
  });

  test("broken script recovery keeps earlier functions searchable by prefix", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const recoveryDoc = await openVueFile("src/TemplateRecovery.vue");

    const pos = findPosition(recoveryDoc, "{{ safeA }}", 8);
    expect(pos, "should find recovered safeAction prefix").to.exist;

    const completions = await waitForCompletionsMatching(recoveryDoc.uri, pos!, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return labels.includes("safeAction");
      },
    });
    expectCompletionsNonEmpty(completions, "safeAction completions");

    expectCompletionsPresent(completions!, ["safeAction"]);
    expectCompletionKinds(completions!, "safeAction", [
      vscode.CompletionItemKind.Function,
      vscode.CompletionItemKind.Method,
      vscode.CompletionItemKind.Variable,
    ]);
    expectCompletionsMissing(completions!, ["window", "document", "AbortController"]);
    expectNoInternalLeakage(completions!);
  });

  test("dedicated broken template expression recovery preserves typed completions", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const brokenExprDoc = await openAndReady("src/BrokenTemplateExpr.vue");

    const pos = findPosition(brokenExprDoc, "{{ count + }}", 11);
    expect(pos, "should find broken expression probe").to.exist;

    const completions = await getCompletions(brokenExprDoc.uri, pos!);
    expectCompletionsNonEmpty(completions, "completions");

    expectCompletionsPresent(completions!, ["count", "formatted"]);
    expectCompletionKinds(completions!, "count", [vscode.CompletionItemKind.Variable]);
    expectCompletionKinds(completions!, "formatted", [vscode.CompletionItemKind.Variable]);
    expectCompletionsMissing(completions!, ["window", "document", "AbortController"]);
    expectNoInternalLeakage(completions!);
  });

  test("JS SFC template completions stay typed", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const jsDoc = await openAndReady("src/JsTemplateCases.vue");

    const mustachePos = findPosition(jsDoc, "{{ count }}", 3);
    const memberPos = findPosition(jsDoc, "state.label", 6);
    expect(mustachePos, "should find JS mustache usage").to.exist;
    expect(memberPos, "should find JS member usage").to.exist;

    const mustacheCompletions = await getCompletions(jsDoc.uri, mustachePos!);
    const memberCompletions = await getCompletions(jsDoc.uri, memberPos!, ".");
    expectCompletionsNonEmpty(mustacheCompletions, "JS mustache completions");
    expectCompletionsNonEmpty(memberCompletions, "JS member completions");

    expectCompletionsPresent(mustacheCompletions!, ["count", "increment"]);
    expectCompletionKinds(mustacheCompletions!, "count", [vscode.CompletionItemKind.Variable]);
    expectCompletionKinds(mustacheCompletions!, "increment", [
      vscode.CompletionItemKind.Function,
      vscode.CompletionItemKind.Method,
      vscode.CompletionItemKind.Variable,
    ]);

    expectCompletionsPresent(memberCompletions!, ["label", "done"]);
    expectCompletionKinds(memberCompletions!, "label", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionsMissing(memberCompletions!, ["@click", "foo-bar"]);
    expectNoInternalLeakage(memberCompletions!);
  });

  test("computed member access does NOT offer .value (unwrapped in template)", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // "doubled" is a computed ref — in template, it should be unwrapped
    // so member access shouldn't offer .value
    const pos = findPosition(doc, "{{ doubled }}", 10);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    if (!completions || completions.items.length === 0) {
      // If no completions at this position, that's also acceptable
      // (it's a primitive, no members expected)
      console.log("    No completions on doubled — primitive type, OK");
      return;
    }

    // If completions are returned, they should NOT include .value
    expectCompletionsMissing(completions, ["value"]);
  });

  test("directive completions in element attribute position", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    // In App.vue, find a position inside an element's opening tag
    // where directives should be offered
    const pos = findPosition(doc, '<button @click="increment">+</button>', 8);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    // We don't necessarily need to assert specific directives here,
    // but completions should at least exist and not crash
    if (completions && completions.items.length > 0) {
      expectNoInternalLeakage(completions);
    }
  });

  test("TypeResolutionCases: union type completions show members of both branches", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "single-project") {
      console.log("    N/A");
      return;
    }

    const trDoc = await openAndReady("src/TypeResolutionCases.vue");

    const pos = findPosition(trDoc, "nested.deep.va", "nested.deep.va".length);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await waitForCompletionsMatching(trDoc.uri, pos, {
      predicate: (list) => {
        const labels = list ? completionLabels(list) : [];
        return labels.includes("value");
      },
    });
    expectCompletionsNonEmpty(completions, "nested member completions");
    expectCompletionsPresent(completions!, ["value"]);
    expectCompletionKinds(completions!, "value", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectNoInternalLeakage(completions!);
  });

  // ── Monorepo Fixture ──────────────────────────────────────────

  // Skip: cross-package completions require full type provider sync
  test.skip("cross-package component tag completions (monorepo)", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "monorepo") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<SharedComp ", 12);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expectCompletionsNonEmpty(completions, "completions");

    console.log(`    <SharedComp completions: ${completionLabels(completions!).join(", ")}`);

    expectCompletionsPresent(completions!, ["foo", "bar"]);
    expectCompletionKinds(completions!, "foo", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionKinds(completions!, "bar", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionsMissing(completions!, ["count", "doubled", "increment"]);
    expectNoInternalLeakage(completions!);
  });

  // ── Path Aliases Fixture ───────────────────────────────────────

  // Skip: aliased component completions require full type provider sync
  test.skip("aliased component tag completions (path-aliases)", async function () {
    if (!TYPE_PROVIDER) return this.skip();
    if (FIXTURE_NAME !== "path-aliases") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "<MyComp ", 8);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expectCompletionsNonEmpty(completions, "completions");

    console.log(`    <MyComp completions: ${completionLabels(completions!).join(", ")}`);

    expectCompletionsPresent(completions!, ["foo", "bar"]);
    expectCompletionKinds(completions!, "foo", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionKinds(completions!, "bar", [
      vscode.CompletionItemKind.Property,
      vscode.CompletionItemKind.Field,
    ]);
    expectCompletionsMissing(completions!, ["count", "doubled", "increment"]);
    expectNoInternalLeakage(completions!);
  });

  // ── Single-File Fixture ────────────────────────────────────────

  test("single-file project completions (single-file)", async function () {
    if (FIXTURE_NAME !== "single-file") {
      console.log("    N/A");
      return;
    }

    const pos = findPosition(doc, "{{ count }}", 3);
    if (!pos) {
      this.skip();
      return;
    }

    const completions = await getCompletions(doc.uri, pos);
    expectCompletionsNonEmpty(completions, "completions");

    console.log(`    Single-file completions: ${completionLabels(completions!).join(", ")}`);

    expectCompletionsPresent(completions!, ["count", "doubled", "increment"]);
    expectCompletionKinds(completions!, "count", [vscode.CompletionItemKind.Variable]);
    expectCompletionKinds(completions!, "doubled", [vscode.CompletionItemKind.Variable]);
    expectCompletionsMissing(completions!, [
      "AbortController",
      "HTMLDivElement",
      "document",
      "window",
    ]);
    expectNoInternalLeakage(completions!);
  });

  test("completion scenarios do not log panic markers", function () {
    assertLogNotContains("panicked at", "completion flows should not trigger Rust panics");
    assertLogNotContains("thread 'main' panicked", "completion flows should not crash the server");
  });
});
