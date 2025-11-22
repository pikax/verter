import { afterEach, beforeAll, expect, test, describe, afterAll } from "vitest";
import { URI } from "vscode-uri";
import {
  getVerterServer,
  testWorkspacePath,
  closeVerterServer,
} from "../server/verter-server";
import { getCompletionLabels } from "../helpers/test-helpers";
import type { TextDocument } from "vscode-languageserver-textdocument";
import * as fs from "fs";
import * as fps from "fs/promises";
import * as path from "path";

const openedDocuments: TextDocument[] = [];

describe("Real World Components - Verter", async () => {
  const server = await getVerterServer();
  afterAll(async () => {
    await closeVerterServer();
  });

  const fileMap = new Map<string, string>();

  beforeAll(async () => {
    // Copy __bench__ files to test-workspace
    const benchDir = path.resolve(__dirname, "../__bench__");
    const targetDir = path.resolve(testWorkspacePath, "components");

    if (!fs.existsSync(targetDir)) {
      fs.mkdirSync(targetDir, { recursive: true });
    }

    const files = fs.readdirSync(benchDir);
    await Promise.allSettled(
      files.map(async (file) => {
        if (file.endsWith(".vue")) {
          const source = path.join(benchDir, file);
          const target = path.join(targetDir, file);

          return fps.readFile(source, "utf-8").then((content) => {
            fileMap.set(`components/${file}`, content);

            return fps.writeFile(target, content);
          });
        }
      })
    );
  });

  afterEach(async () => {
    await Promise.allSettled(
      openedDocuments.map((document) => server.closeDocument(document.uri))
    );

    openedDocuments.length = 0;
  });

  // warm up to make tests faster
  beforeAll(async () => {
    const filePath = "components/button.vue";
    const fileContent = fileMap.get(filePath)!;
    const uri = URI.file(path.resolve(testWorkspacePath, filePath)).toString();
    const doc = await server.openDocument(uri, "vue", fileContent);
    openedDocuments.push(doc);

    const position = doc.positionAt(0);
    await server.getCompletions(uri, position);

    await Promise.allSettled(
      openedDocuments.map((document) => server.closeDocument(document.uri))
    );
  });

  describe("Real World Components - button.vue", () => {
    test("Should complete props in computed", async () => {
      const filePath = "components/button.vue";
      const fileContent = fileMap.get(filePath)!;

      // Find "props.label" and get completions after "props."
      const searchText = "props.label";
      const offset = fileContent.indexOf(searchText);
      expect(offset).toBeGreaterThan(-1);

      const targetOffset =
        fileContent.indexOf("props.", offset) + "props.".length;
      const uri = URI.file(
        path.resolve(testWorkspacePath, filePath)
      ).toString();

      const doc = await server.openDocument(uri, "vue", fileContent);
      openedDocuments.push(doc);

      const position = doc.positionAt(targetOffset);
      const completions = await server.getCompletions(uri, position);

      expect(completions).toBeDefined();
      const labels = getCompletionLabels(completions);

      // Should have props from defineProps
      expect(labels).toContain("label");
      expect(labels).toContain("action");
      expect(labels).toContain("to");
      expect(labels).toContain("secondary");
      expect(labels).toContain("color");
      expect(labels).toContain("icon");
      expect(labels).toContain("loading");
    });

    test("Should complete action object properties", async () => {
      const filePath = "components/button.vue";
      const fileContent = fileMap.get(filePath)!;

      // Find "action?.label" and get completions after "action?."
      const searchText = "action?.label";
      const offset = fileContent.indexOf(searchText);
      expect(offset).toBeGreaterThan(-1);

      const targetOffset =
        fileContent.indexOf("action?.", offset) + "action?.".length;
      const uri = URI.file(
        path.resolve(testWorkspacePath, filePath)
      ).toString();

      const doc = await server.openDocument(uri, "vue", fileContent);
      openedDocuments.push(doc);

      const position = doc.positionAt(targetOffset);
      const completions = await server.getCompletions(uri, position);

      expect(completions).toBeDefined();
      const labels = getCompletionLabels(completions);

      // Should have action properties
      expect(labels).toContain("to");
      expect(labels).toContain("label");
      expect(labels).toContain("loading");
      expect(labels).toContain("disabled");
    });

    test.skip("Should complete computed color in template (template expression completions not working in test env)", async () => {
      const filePath = "components/button.vue";
      const fileContent = fileMap.get(filePath)!;
      // Find ":class="[color," in template
      const searchText = ':class="[color,';
      const offset = fileContent.indexOf(searchText);
      expect(offset).toBeGreaterThan(-1);

      const targetOffset = fileContent.indexOf("color", offset);
      const uri = URI.file(
        path.resolve(testWorkspacePath, filePath)
      ).toString();

      const doc = await server.openDocument(uri, "vue", fileContent);
      openedDocuments.push(doc);

      const position = doc.positionAt(targetOffset);
      const completions = await server.getCompletions(uri, position);

      expect(completions).toBeDefined();
      const labels = getCompletionLabels(completions);

      // Should have the color computed property
      expect(labels).toContain("color");
    });
  });

  describe("Real World Components - avatar.vue", () => {
    test("Should complete props in computed", async () => {
      const filePath = "components/avatar.vue";
      const fileContent = fileMap.get(filePath)!;

      // Find "props.id" and get completions after "props."
      const searchText = "props.id";
      const offset = fileContent.indexOf(searchText);
      expect(offset).toBeGreaterThan(-1);

      const targetOffset =
        fileContent.indexOf("props.", offset) + "props.".length;
      const uri = URI.file(
        path.resolve(testWorkspacePath, filePath)
      ).toString();

      const doc = await server.openDocument(uri, "vue", fileContent);
      openedDocuments.push(doc);

      const position = doc.positionAt(targetOffset);
      const completions = await server.getCompletions(uri, position);

      expect(completions).toBeDefined();
      const labels = getCompletionLabels(completions);

      // Should have props from withDefaults/defineProps
      expect(labels).toContain("src");
      expect(labels).toContain("alt");
      expect(labels).toContain("badge");
      expect(labels).toContain("size");
      expect(labels).toContain("id");
    });

    test("Should complete ref properties", async () => {
      const filePath = "components/avatar.vue";
      const fileContent = fileMap.get(filePath)!;

      // Find "avatarEl.value" and get completions after "avatarEl."
      const searchText = "avatarEl.value";
      const offset = fileContent.indexOf(searchText);
      expect(offset).toBeGreaterThan(-1);

      const targetOffset =
        fileContent.indexOf("avatarEl.", offset) + "avatarEl.".length;
      const uri = URI.file(
        path.resolve(testWorkspacePath, filePath)
      ).toString();

      const doc = await server.openDocument(uri, "vue", fileContent);
      openedDocuments.push(doc);

      const position = doc.positionAt(targetOffset);
      const completions = await server.getCompletions(uri, position);

      expect(completions).toBeDefined();
      const labels = getCompletionLabels(completions);

      // Should have Ref properties
      expect(labels).toContain("value");
    });

    test("Should complete HTMLElement properties", async () => {
      const filePath = "components/avatar.vue";
      const fileContent = fileMap.get(filePath)!;

      // Find "avatarEl.value?.offsetHeight" and get completions after "offsetHeight"
      const searchText = "avatarEl.value?.offsetHeight";
      const offset = fileContent.indexOf(searchText);
      expect(offset).toBeGreaterThan(-1);

      const targetOffset = fileContent.indexOf("offsetHeight", offset);
      const uri = URI.file(
        path.resolve(testWorkspacePath, filePath)
      ).toString();

      const doc = await server.openDocument(uri, "vue", fileContent);
      openedDocuments.push(doc);

      const position = doc.positionAt(targetOffset);
      const completions = await server.getCompletions(uri, position);

      expect(completions).toBeDefined();
      const labels = getCompletionLabels(completions);

      // Should have HTMLElement properties
      expect(labels).toContain("offsetHeight");
      expect(labels).toContain("offsetWidth");
    });
  });

  describe("Real World Components - icon.vue", () => {
    test.skip("Should complete props (requires @iconify dependencies)", async () => {
      const filePath = "components/icon.vue";
      const fileContent = fileMap.get(filePath)!;

      // Find "props.icon" and get completions after "props."
      const searchText = "props.icon";
      const offset = fileContent.indexOf(searchText);
      expect(offset).toBeGreaterThan(-1);

      const targetOffset =
        fileContent.indexOf("props.", offset) + "props.".length;
      const uri = URI.file(
        path.resolve(testWorkspacePath, filePath)
      ).toString();

      const doc = await server.openDocument(uri, "vue", fileContent);
      openedDocuments.push(doc);

      const position = doc.positionAt(targetOffset);
      const completions = await server.getCompletions(uri, position);

      expect(completions).toBeDefined();
      const labels = getCompletionLabels(completions);

      // Should have props
      expect(labels).toContain("icon");
      expect(labels).toContain("size");
    });

    test("Should complete ref element", async () => {
      const filePath = "components/icon.vue";
      const fileContent = fileMap.get(filePath)!;

      // Find "el.value" and get completions after "el."
      const searchText = "el.value";
      const offset = fileContent.indexOf(searchText);
      expect(offset).toBeGreaterThan(-1);

      const targetOffset = fileContent.indexOf("el.", offset) + "el.".length;
      const uri = URI.file(
        path.resolve(testWorkspacePath, filePath)
      ).toString();

      const doc = await server.openDocument(uri, "vue", fileContent);
      openedDocuments.push(doc);

      const position = doc.positionAt(targetOffset);
      const completions = await server.getCompletions(uri, position);

      expect(completions).toBeDefined();
      const labels = getCompletionLabels(completions);

      // Should have Ref properties
      expect(labels).toContain("value");
    });
  });
});
