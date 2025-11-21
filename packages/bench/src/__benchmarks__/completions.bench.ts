import { bench, describe } from "vitest";
import { URI } from "vscode-uri";
import {
  getLanguageServer as getVolarServer,
  testWorkspacePath as volarWorkspacePath,
} from "../server/volar-server";
import {
  getVerterServer,
  testWorkspacePath as verterWorkspacePath,
} from "../server/verter-server-direct";
import {
  parseContentWithCursor,
  createTestUri,
} from "../helpers/test-helpers";

describe("Completions Benchmark: Volar vs Verter", () => {
  describe("Script setup completions", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const testContent = `<script setup lang="ts">
const msg = 'Hello World'
$|
</script>`;

    bench("Volar - Script setup completions", async () => {
      const fileName = "fixture.vue";
      const { content, position, offset } = parseContentWithCursor(testContent);
      const uri = createTestUri(volarWorkspacePath, fileName);

      const doc = await volarServer.open(uri, "vue", content);
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(doc.uri).fsPath,
          position: offset,
        },
      });

      await volarServer.close(doc.uri);
    });

    bench("Verter - Script setup completions", async () => {
      const fileName = "fixture.vue";
      const { content, position } = parseContentWithCursor(testContent);
      const uri = createTestUri(verterWorkspacePath, fileName);

      await verterServer.openDocument(uri, "vue", content);
      await verterServer.getCompletions(uri, position);
      await verterServer.closeDocument(uri);
    });
  });

  describe("TypeScript completions in script", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const testContent = `<script setup lang="ts">
const msg = 'Hello World'
msg.|
</script>`;

    bench(
      "Volar - TypeScript completions",
      async () => {
        const fileName = "fixture.vue";
        const { content, position, offset } = parseContentWithCursor(testContent);
        const uri = createTestUri(volarWorkspacePath, fileName);

        const doc = await volarServer.open(uri, "vue", content);
        await volarServer.tsserver.message({
          seq: volarServer.nextSeq(),
          command: "completions",
          arguments: {
            file: URI.parse(doc.uri).fsPath,
            position: offset,
          },
        });

        await volarServer.close(doc.uri);
      },
      { time: 5000 }
    );

    bench(
      "Verter - TypeScript completions",
      async () => {
        const fileName = "fixture.vue";
        const { content, position } = parseContentWithCursor(testContent);
        const uri = createTestUri(verterWorkspacePath, fileName);

        await verterServer.openDocument(uri, "vue", content);
        await verterServer.getCompletions(uri, position);
        await verterServer.closeDocument(uri);
      },
      { time: 5000 }
    );
  });

  describe("Auto import component", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const testContent = `<script setup lang="ts">
import componentFor|
</script>`;

    bench(
      "Volar - Auto import component",
      async () => {
        const fileName = "tsconfigProject/fixture.vue";
        const { content, position, offset } = parseContentWithCursor(testContent);
        const uri = createTestUri(volarWorkspacePath, fileName);

        const doc = await volarServer.open(uri, "vue", content);
        await volarServer.tsserver.message({
          seq: volarServer.nextSeq(),
          command: "completions",
          arguments: {
            file: URI.parse(doc.uri).fsPath,
            position: offset,
          },
        });

        await volarServer.close(doc.uri);
      },
      { time: 5000 }
    );

    bench(
      "Verter - Auto import component",
      async () => {
        const fileName = "tsconfigProject/fixture.vue";
        const { content, position } = parseContentWithCursor(testContent);
        const uri = createTestUri(verterWorkspacePath, fileName);

        await verterServer.openDocument(uri, "vue", content);
        await verterServer.getCompletions(uri, position);
        await verterServer.closeDocument(uri);
      },
      { time: 5000 }
    );
  });

  describe.only("template completion", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const testContent = `<template>
    <div>{{ mess| }}</div>
</template>
<script setup lang="ts">
const message = 'Hello';
</script>`;

    bench("Volar", async () => {
      const fileName = "fixtureTemplate.vue";
      const { content, position, offset } = parseContentWithCursor(testContent);
      const uri = createTestUri(volarWorkspacePath, fileName);

      const doc = await volarServer.open(URI.parse(uri).fsPath, "vue", content);
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(doc.uri).fsPath,
          position: offset,
        },
      });
      await volarServer.close(doc.uri);
    });

    bench("Verter", async () => {
      const fileName = "fixture.vue";
      const { content, position } = parseContentWithCursor(testContent);
      const uri = createTestUri(verterWorkspacePath, fileName);

      await verterServer.openDocument(uri, "vue", content);
      await verterServer.getCompletions(uri, position);
      await verterServer.closeDocument(uri);
    });
  });

  describe("Complex TypeScript inference", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const testContent = `<script setup lang="ts">
import { ref } from 'vue'

interface User {
  id: number;
  name: string;
  email: string;
}

const user = ref<User>({ id: 1, name: 'John', email: 'john@example.com' })
user.value.|
</script>`;

    bench(
      "Volar - Complex TypeScript inference",
      async () => {
        const fileName = "fixture.vue";
        const { content, position, offset } = parseContentWithCursor(testContent);
        const uri = createTestUri(volarWorkspacePath, fileName);

        const doc = await volarServer.open(uri, "vue", content);
        const r = await volarServer.tsserver.message({
          seq: volarServer.nextSeq(),
          command: "completions",
          arguments: {
            file: URI.parse(doc.uri).fsPath,
            position: offset,
          },
        });

        console.log(r);

        await volarServer.close(doc.uri);
      },
      { time: 5000 }
    );

    bench(
      "Verter - Complex TypeScript inference",
      async () => {
        const fileName = "fixture.vue";
        const { content, position } = parseContentWithCursor(testContent);
        const uri = createTestUri(verterWorkspacePath, fileName);

        await verterServer.openDocument(uri, "vue", content);
        const r = await verterServer.getCompletions(uri, position);
        console.log(r);
        await verterServer.closeDocument(uri);
      },
      { time: 5000 }
    );
  });

  describe("Multiple file operations", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();

    bench(
      "Volar - Open and complete 5 files",
      async () => {
        const files = Array.from({ length: 5 }, (_, i) => ({
          name: `file${i}.vue`,
          content: `<script setup lang="ts">
const msg${i} = 'Hello ${i}'
msg${i}.|
</script>`,
        }));

        for (const file of files) {
          const { content, position, offset } = parseContentWithCursor(file.content);
          const uri = createTestUri(volarWorkspacePath, file.name);

          const doc = await volarServer.open(uri, "vue", content);
          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(doc.uri).fsPath,
              position: offset,
            },
          });

          await volarServer.close(doc.uri);
        }
      },
      { time: 10000 }
    );

    bench(
      "Verter - Open and complete 5 files",
      async () => {
        const files = Array.from({ length: 5 }, (_, i) => ({
          name: `file${i}.vue`,
          content: `<script setup lang="ts">
const msg${i} = 'Hello ${i}'
msg${i}.|
</script>`,
        }));

        for (const file of files) {
          const { content, position } = parseContentWithCursor(file.content);
          const uri = createTestUri(verterWorkspacePath, file.name);

          await verterServer.openDocument(uri, "vue", content);
          await verterServer.getCompletions(uri, position);
          await verterServer.closeDocument(uri);
        }
      },
      { time: 10000 }
    );
  });
});
