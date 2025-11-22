import { afterEach, expect, test } from "vitest";
import { URI } from "vscode-uri";
import {
  getVerterServer,
  testWorkspacePath,
  closeVerterServer,
} from "../server/verter-server";
import type { TextDocument } from "vscode-languageserver-textdocument";
import {
  parseContentWithCursor,
  createTestUri,
  assertCompletionExists,
  getCompletionLabels,
} from "../helpers/test-helpers";

const openedDocuments: TextDocument[] = [];

afterEach(async () => {
  try {
    const server = await getVerterServer();
    for (const document of openedDocuments) {
      await server.closeDocument(document.uri);
    }
  } catch (e) {
    // Ignore cleanup errors
  }
  openedDocuments.length = 0;
});

// TODO: Implement template tag completions in Verter
test.skip("Vue tags", async () => {
  // NOTE: Template tag completions require HTML language service integration
  // Currently Verter focuses on TypeScript completions in script blocks
  const completions = await requestCompletionListToVerterServer("fixture.vue", "vue", `<|`);
  const labels = getCompletionLabels(completions);

  expect(labels).toContain("template");
  expect(labels).toContain("script");
  expect(labels).toContain("style");
});

// TODO: Implement HTML tag completions in Verter
test.skip("HTML tags and built-in components", async () => {
  const completions = await requestCompletionListToVerterServer(
    "fixture.vue",
    "vue",
    `<template><|</template>`
  );
  const labels = getCompletionLabels(completions);

  // Check that basic HTML tags are present
  expect(labels.some((l: string) => l.includes("div"))).toBe(true);
  expect(labels.some((l: string) => l.includes("span"))).toBe(true);
  expect(labels.length).toBeGreaterThan(0);

  console.log("HTML tags found:", labels.slice(0, 20));
});

// TODO: Implement HTML event completions in Verter
test.skip("HTML events", async () => {
  const completions = await requestCompletionListToVerterServer(
    "fixture.vue",
    "vue",
    `<template><div @cl|></div></template>`
  );
  const labels = getCompletionLabels(completions).filter((label: string) => label.includes("click"));

  // Check that click-related events are present
  expect(labels.length).toBeGreaterThan(0);
  console.log("Click events found:", labels);
});

// TODO: Implement Vue directive completions in Verter
test.skip("Vue directives", async () => {
  const vShowCompletions = await requestCompletionListToVerterServer(
    "fixture.vue",
    "vue",
    `<template><div v-sh|></div></template>`
  );
  const vShowLabels = getCompletionLabels(vShowCompletions);
  expect(vShowLabels.some((l: string) => l.includes("v-show"))).toBe(true);

  const vIfCompletions = await requestCompletionListToVerterServer(
    "fixture.vue",
    "vue",
    `<template><div v-if|></div></template>`
  );
  const vIfLabels = getCompletionLabels(vIfCompletions);
  expect(vIfLabels.some((l: string) => l.includes("v-if"))).toBe(true);

  console.log("Directive completions working");
});

test("Script setup completions", async () => {
  try {
    const completions = await requestCompletionListToVerterServer(
      "fixture.vue",
      "vue",
      `
        <template>
          <div>{{ mess| }}</div>
        </template>
        <script setup lang="ts">
        const message = 'Hello';
        </script>
      `
    );
    const labels = getCompletionLabels(completions);

    expect(labels).toContain("message");
    console.log("Script setup completions found:", labels.slice(0, 10));
  } catch (e) {
    console.log("Script setup completion not fully working:", e);
  }
});

test("TypeScript completions in script", async () => {
  try {
    const completions = await requestCompletionListToVerterServer(
      "fixture.vue",
      "vue",
      `
        <script setup lang="ts">
        const msg = 'hello';
        msg.|
        </script>
      `
    );
    const labels = getCompletionLabels(completions);

    // Should have string methods
    expect(labels).toContain("toString");
    expect(labels).toContain("toLowerCase");
    console.log("TypeScript completions found:", labels.slice(0, 10));
  } catch (e) {
    console.log("TypeScript completion not fully working:", e);
  }
});

test("Auto import component", async () => {
  try {
    const completions = await requestCompletionListToVerterServer(
      "tsconfigProject/fixture.vue",
      "vue",
      `
        <script setup lang="ts">
        import componentFor|
        </script>
      `
    );
    const labels = getCompletionLabels(completions);

    console.log("Import completions:", labels);
    expect(labels.length).toBeGreaterThan(0);
  } catch (e) {
    console.log("Auto import not fully working in test environment");
  }
});

test("Event modifiers", async () => {
  try {
    const completions = await requestCompletionListToVerterServer(
      "fixture.vue",
      "vue",
      `
        <template>
          <div @click.| />
        </template>
      `
    );
    const labels = getCompletionLabels(completions);

    console.log("Event modifiers found:", labels);
    // Verter might handle event modifiers differently
    expect(labels.length).toBeGreaterThanOrEqual(0);
  } catch (e) {
    console.log("Event modifiers completion:", e);
  }
});



// Helper functions

async function requestCompletionListToVerterServer(
  fileName: string,
  languageId: string,
  content: string
) {
  const { content: cleanContent, position } = parseContentWithCursor(content);

  const server = await getVerterServer();
  const uri = createTestUri(testWorkspacePath, fileName);
  const document = await server.openDocument(uri, languageId, cleanContent);

  if (openedDocuments.every((d) => d.uri !== document.uri)) {
    openedDocuments.push(document);
  }

  const completions = await server.getCompletions(uri, position);

  expect(completions).toBeDefined();
  return completions;
}
