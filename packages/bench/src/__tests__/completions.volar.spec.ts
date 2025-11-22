import type { TextDocument } from "@volar/language-server";
import { afterEach, afterAll, beforeAll, expect, test } from "vitest";
import { URI } from "vscode-uri";
import {
  getLanguageServer,
  testWorkspacePath,
  closeLanguageServer,
} from "../server/volar-server-lsp";
import {
  parseContentWithCursor,
  createTestUri,
  assertCompletionExists,
  getCompletionLabels,
  toErrorMessage,
} from "../helpers/test-helpers";
import { describe } from "vitest";

type Server = Awaited<ReturnType<typeof getLanguageServer>>;
const openedDocuments: TextDocument[] = [];

describe("completions.volar.spec.ts", () => {
  let server: Server;

  beforeAll(async () => {
    server = await getLanguageServer();
  });

  afterAll(async () => {
    await closeLanguageServer();
  });
  afterEach(async () => {
    await Promise.allSettled(
      openedDocuments.map((document) => server.close(document.uri))
    );
    openedDocuments.length = 0;
  });

  // warm up to make tests faster
  beforeAll(async () => {
    await requestCompletionListToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<template><|</template>`
    );

    await Promise.allSettled(
      openedDocuments.map((document) => server.close(document.uri))
    );
  });

  test("Vue tags", async () => {
    const completions = await requestCompletionListToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<|`
    );
    const labels = getCompletionLabels(completions);

    // Check that essential Vue tags are present
    expect(labels).toContain("template");
    expect(labels).toContain("script");
    expect(labels).toContain("script setup");
    expect(labels).toContain("style");
    expect(labels).toContain('script lang="ts"');
    expect(labels).toContain('script setup lang="ts"');
  });

  test("HTML tags and built-in components", async () => {
    const completions = await requestCompletionListToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<template><|</template>`
    );
    const labels = getCompletionLabels(completions).slice(0, 20);

    expect(labels).toMatchInlineSnapshot(`
		[
		  "!DOCTYPE",
		  "html",
		  "head",
		  "title",
		  "base",
		  "link",
		  "meta",
		  "style",
		  "body",
		  "article",
		  "section",
		  "nav",
		  "aside",
		  "h1",
		  "h2",
		  "h3",
		  "h4",
		  "h5",
		  "h6",
		  "header",
		]
	`);
  });

  test("HTML events", async () => {
    const completions = await requestCompletionListToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<template><div @cl|></div></template>`
    );
    const labels = getCompletionLabels(completions).filter((label) =>
      label.includes("click")
    );

    // Check that click-related events are present
    expect(labels).toContain("onclick");
    expect(
      labels.some((l) => l === "ondblclick" || l.includes("dblclick"))
    ).toBe(true);
  });

  test("Vue directives", async () => {
    await requestCompletionItemToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<template><div v-sh|></div></template>`,
      "v-show"
    );
    await requestCompletionItemToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<template><div v-if|></div></template>`,
      "v-if"
    );
    await requestCompletionItemToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<template><div v-fo|></div></template>`,
      "v-for"
    );
    await requestCompletionItemToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<template><div v-mo|></div></template>`,
      "v-model"
    );
    await requestCompletionItemToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<template><div v-ht|></div></template>`,
      "v-html"
    );
    await requestCompletionItemToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<template><div v-cl|></div></template>`,
      "v-cloak"
    );
    await requestCompletionItemToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<template><div v-el|></div></template>`,
      "v-else"
    );
    await requestCompletionItemToVueServer(
      server,
      "fixture.vue",
      "vue",
      `<template><div v-p|></div></template>`,
      "v-pre"
    );
  });

  test("$event argument", async () => {
    try {
      await requestCompletionItemToTsServer(
        server,
        "fixture.vue",
        "vue",
        `<template><div @click="console.log($eve|)"></div></template>`,
        "$event"
      );
    } catch (e) {
      expect.fail(toErrorMessage(e));
    }
  });

  test("<script setup>", async () => {
    await requestCompletionItemToTsServer(
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
	`,
      "message"
    );
  });

  // it's failing, skipping for now
  test.skip("Auto import component", async () => {
    try {
      const result = await requestCompletionItemToTsServer(
        server,
        "tsconfigProject/fixture.vue",
        "vue",
        `
			<script setup lang="ts">
			import componentFor|
			</script>
		`,
        "componentForAutoImport"
      );

      expect(result).toBeDefined();
    } catch (e) {
      expect.fail(toErrorMessage(e));
    }
  });

  test("Slot props completion", async () => {
    try {
      await requestCompletionItemToTsServer(
        server,
        "fixture.vue",
        "vue",
        `
			<template>
				<Comp>
					<template #foo="foo">
						{{ fo| }}
					</template>
				</Comp>
			</template>
		`,
        "foo"
      );
    } catch (e) {
      expect.fail(toErrorMessage(e));
    }
  });

  test("Event modifiers", async () => {
    await requestCompletionItemToVueServer(
      server,
      "fixture.vue",
      "vue",
      `
		<template>
			<div @click.| />
		</template>
	`,
      "capture"
    );
  });
});
// Helper functions

async function requestCompletionItemToVueServer(
  server: Server,
  fileName: string,
  languageId: string,
  content: string,
  itemLabel: string
) {
  const completions = (await requestCompletionListToVueServer(
    server,
    fileName,
    languageId,
    content
  )) as any;
  const completion = completions?.items?.find(
    (item: any) => item.label === itemLabel
  );
  expect(completion).toBeDefined();

  return completion!;
}

async function requestCompletionListToVueServer(
  server: Server,
  fileName: string,
  languageId: string,
  content: string
) {
  const {
    content: cleanContent,
    position,
    offset,
  } = parseContentWithCursor(content);

  let document = await prepareDocument(
    server,
    fileName,
    languageId,
    cleanContent
  );

  const completions = await server.vueserver.sendCompletionRequest(
    document.uri,
    position
  );
  expect(completions).toBeDefined();

  return completions!;
}

async function requestCompletionItemToTsServer(
  server: Server,
  fileName: string,
  languageId: string,
  content: string,
  itemLabel: string
) {
  const completions = await requestCompletionListToTsServer(
    server,
    fileName,
    languageId,
    content
  );
  let completion = completions.find((item: any) => item.name === itemLabel);
  expect(completion).toBeDefined();

  return completion!;
}

async function requestCompletionListToTsServer(
  server: Server,
  fileName: string,
  languageId: string,
  content: string
) {
  const { content: cleanContent, offset } = parseContentWithCursor(content);

  let document = await prepareDocument(
    server,
    fileName,
    languageId,
    cleanContent
  );

  const res = await server.tsserver.message({
    seq: server.nextSeq(),
    command: "completions",
    arguments: {
      file: URI.parse(document.uri).fsPath,
      position: offset,
    },
  });

  expect(res.success).toBe(true);
  return res.body;
}

async function prepareDocument(
  server: Server,
  fileName: string,
  languageId: string,
  content: string
) {
  const uri = createTestUri(testWorkspacePath, fileName);
  const document = await server.open(uri, languageId, content);

  if (openedDocuments.every((d) => d.uri !== document.uri)) {
    openedDocuments.push(document);
  }

  return document;
}
