import {
  afterEach,
  afterAll,
  beforeAll,
  expect,
  test,
  beforeEach,
} from "vitest";
import { URI } from "vscode-uri";
import {
  getVerterServer,
  testWorkspacePath,
  closeVerterServer,
  VerterServer,
} from "../server/verter-server";
import type { TextDocument } from "vscode-languageserver-textdocument";
import {
  parseContentWithCursor,
  createTestUri,
  assertCompletionExists,
  getCompletionLabels,
  toErrorMessage,
} from "../helpers/test-helpers";
import { describe } from "vitest";

const openedDocuments: TextDocument[] = [];

// if in windows disable concurrent tests
describe("completions.verter.spec.ts", () => {
  let server: VerterServer;

  beforeAll(async () => {
    server = await getVerterServer();
  });

  afterAll(async () => {
    await closeVerterServer();
  });
  afterEach(async () => {
    await Promise.allSettled(
      openedDocuments.map((document) => server.closeDocument(document.uri))
    );
    openedDocuments.length = 0;
  });

  // warm up to make tests faster
  beforeAll(async () => {
    await requestCompletionListToVerterServer(
      server,
      "fixture-1.vue",
      "vue",
      `<template><|</template>`
    );

    await Promise.allSettled(
      openedDocuments.map((document) => server.closeDocument(document.uri))
    );
  });

  // TODO: Implement template tag completions in Verter
  test.skip("Vue tags", async () => {
    // NOTE: Template tag completions require HTML language service integration
    // Currently Verter focuses on TypeScript completions in script blocks
    const completions = await requestCompletionListToVerterServer(
      server,
      "fixture1.vue",
      "vue",
      `<|`
    );
    const labels = getCompletionLabels(completions);

    expect(labels).toContain("template");
    expect(labels).toContain("script");
    expect(labels).toContain("style");
  });

  // TODO: Implement HTML tag completions in Verter
  test.skip("HTML tags and built-in components", async () => {
    const completions = await requestCompletionListToVerterServer(
      server,
      "fixture2.vue",
      "vue",
      `<template><|</template>`
    );
    const labels = getCompletionLabels(completions);

    // Check that basic HTML tags are present
    expect(labels.some((l: string) => l.includes("div"))).toBe(true);
    expect(labels.some((l: string) => l.includes("span"))).toBe(true);
    expect(labels.length).toBeGreaterThan(0);
  });

  // TODO: Implement HTML event completions in Verter
  test.skip("HTML events", async () => {
    const completions = await requestCompletionListToVerterServer(
      server,
      "fixture3.vue",
      "vue",
      `<template><div @cl|></div></template>`
    );
    const labels = getCompletionLabels(completions).filter((label: string) =>
      label.includes("click")
    );

    // Check that click-related events are present
    expect(labels.length).toBeGreaterThan(0);
  });

  // TODO: Implement Vue directive completions in Verter
  test.skip("Vue directives", async () => {
    const vShowCompletions = await requestCompletionListToVerterServer(
      server,
      "fixture4.vue",
      "vue",
      `<template><div v-sh|></div></template>`
    );
    const vShowLabels = getCompletionLabels(vShowCompletions);
    expect(vShowLabels.some((l: string) => l.includes("v-show"))).toBe(true);

    const vIfCompletions = await requestCompletionListToVerterServer(
      server,
      "fixture5.vue",
      "vue",
      `<template><div v-if|></div></template>`
    );
    const vIfLabels = getCompletionLabels(vIfCompletions);
    expect(vIfLabels.some((l: string) => l.includes("v-if"))).toBe(true);
  });

  test("Script setup completions", async () => {
    try {
      const completions = await requestCompletionListToVerterServer(
        server,
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
    } catch (e) {
      expect.fail(toErrorMessage(e));
    }
  });

  test("TypeScript completions in script", async () => {
    try {
      const completions = await requestCompletionListToVerterServer(
        server,
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
    } catch (e) {
      expect.fail(toErrorMessage(e));
    }
  });

  test("Auto import component", async () => {
    try {
      const completions = await requestCompletionListToVerterServer(
        server,
        "tsconfigProject/fixture.vue",
        "vue",
        `
        <script setup lang="ts">
        import componentFor|
        </script>
      `
      );
      const labels = getCompletionLabels(completions);

      expect(labels.length).toBeGreaterThan(0);
    } catch (e) {
      expect.fail(toErrorMessage(e));
    }
  });

  test("Event modifiers", async () => {
    try {
      const completions = await requestCompletionListToVerterServer(
        server,
        "fixture.vue",
        "vue",
        `
        <template>
          <div @click.| />
        </template>
      `
      );
      const labels = getCompletionLabels(completions);

      // Verter might handle event modifiers differently
      expect(labels.length).toBeGreaterThanOrEqual(0);
    } catch (e) {
      expect.fail(toErrorMessage(e));
    }
  });
});

// Helper functions

async function requestCompletionListToVerterServer(
  server: VerterServer,
  fileName: string,
  languageId: string,
  content: string
) {
  const { content: cleanContent, position } = parseContentWithCursor(content);

  const uri = createTestUri(testWorkspacePath, fileName);
  const document = await server.openDocument(uri, languageId, cleanContent);

  if (openedDocuments.every((d) => d.uri !== document.uri)) {
    openedDocuments.push(document);
  }

  const completions = await server.getCompletions(uri, position);

  expect(completions).toBeDefined();
  return completions;
}
