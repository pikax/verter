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

describe("Completions Benchmark: Volar vs Verter", () => {
  describe("Script setup completions", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    bench("Volar - Script setup completions", async () => {
      const server = volarServer;
      const fileName = "fixture.vue";
      const content = `<script setup lang="ts">
const msg = 'Hello World'
$|
</script>`;

      const [contentBefore, contentAfter] = content.split("|");
      const position = {
        line: contentBefore.split("\n").length - 1,
        character: contentBefore.split("\n").pop()!.length,
      };

      const fullContent = contentBefore + contentAfter;
      const uri = URI.file(`${volarWorkspacePath}/${fileName}`).toString();

      const doc = await server.open(uri, "vue", fullContent);
      const offset = doc.offsetAt(position);
      await server.tsserver.message({
        seq: server.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(doc.uri).fsPath,
          position: offset,
        },
      });

      await server.close(doc.uri);
    });

    bench("Verter - Script setup completions", async () => {
      const server = verterServer;
      const fileName = "fixture.vue";
      const content = `<script setup lang="ts">
const msg = 'Hello World'
$|
</script>`;

      const [contentBefore, contentAfter] = content.split("|");
      const position = {
        line: contentBefore.split("\n").length - 1,
        character: contentBefore.split("\n").pop()!.length,
      };

      const fullContent = contentBefore + contentAfter;
      const uri = URI.file(`${verterWorkspacePath}/${fileName}`).toString();

      await server.openDocument(uri, "vue", fullContent);
      await server.getCompletions(uri, position);
      await server.closeDocument(uri);
    });
  });

  describe("TypeScript completions in script", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();

    bench(
      "Volar - TypeScript completions",
      async () => {
        const server = volarServer;
        const fileName = "fixture.vue";
        const content = `<script setup lang="ts">
const msg = 'Hello World'
msg.|
</script>`;

        const [contentBefore, contentAfter] = content.split("|");
        const position = {
          line: contentBefore.split("\n").length - 1,
          character: contentBefore.split("\n").pop()!.length,
        };

        const fullContent = contentBefore + contentAfter;
        const uri = URI.file(`${volarWorkspacePath}/${fileName}`).toString();

        const doc = await server.open(uri, "vue", fullContent);
        const offset = doc.offsetAt(position);
        await server.tsserver.message({
          seq: server.nextSeq(),
          command: "completions",
          arguments: {
            file: URI.parse(doc.uri).fsPath,
            position: offset,
          },
        });

        await server.close(doc.uri);
      },
      { time: 5000 }
    );

    bench(
      "Verter - TypeScript completions",
      async () => {
        const server = verterServer;
        const fileName = "fixture.vue";
        const content = `<script setup lang="ts">
const msg = 'Hello World'
msg.|
</script>`;

        const [contentBefore, contentAfter] = content.split("|");
        const position = {
          line: contentBefore.split("\n").length - 1,
          character: contentBefore.split("\n").pop()!.length,
        };

        const fullContent = contentBefore + contentAfter;
        const uri = URI.file(`${verterWorkspacePath}/${fileName}`).toString();

        await server.openDocument(uri, "vue", fullContent);
        await server.getCompletions(uri, position);
        await server.closeDocument(uri);
      },
      { time: 5000 }
    );
  });

  describe("Auto import component", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();

    bench(
      "Volar - Auto import component",
      async () => {
        const server = volarServer;
        const fileName = "tsconfigProject/fixture.vue";
        const content = `<script setup lang="ts">
import componentFor|
</script>`;

        const [contentBefore, contentAfter] = content.split("|");
        const position = {
          line: contentBefore.split("\n").length - 1,
          character: contentBefore.split("\n").pop()!.length,
        };

        const fullContent = contentBefore + contentAfter;
        const uri = URI.file(`${volarWorkspacePath}/${fileName}`).toString();

        const doc = await server.open(uri, "vue", fullContent);
        const offset = doc.offsetAt(position);
        await server.tsserver.message({
          seq: server.nextSeq(),
          command: "completions",
          arguments: {
            file: URI.parse(doc.uri).fsPath,
            position: offset,
          },
        });

        await server.close(doc.uri);
      },
      { time: 5000 }
    );

    bench(
      "Verter - Auto import component",
      async () => {
        const server = verterServer;
        const fileName = "tsconfigProject/fixture.vue";
        const content = `<script setup lang="ts">
import componentFor|
</script>`;

        const [contentBefore, contentAfter] = content.split("|");
        const position = {
          line: contentBefore.split("\n").length - 1,
          character: contentBefore.split("\n").pop()!.length,
        };

        const fullContent = contentBefore + contentAfter;
        const uri = URI.file(`${verterWorkspacePath}/${fileName}`).toString();

        await server.openDocument(uri, "vue", fullContent);
        await server.getCompletions(uri, position);
        await server.closeDocument(uri);
      },
      { time: 5000 }
    );
  });

  describe.only("template completion", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();

    bench("Volar", async () => {
      const fileName = "fixtureTemplate.vue";
      const content = `<template>
    <div>{{ mess| }}</div>
</template>
<script setup lang="ts">
const message = 'Hello';
</script>`;
      const [contentBefore, contentAfter] = content.split("|");
      const position = {
        line: contentBefore.split("\n").length - 1,
        character: contentBefore.split("\n").pop()!.length,
      };

      const fullContent = contentBefore + contentAfter;
      const uri = URI.file(`${volarWorkspacePath}/${fileName}`).toString();

      const doc = await volarServer.open(
        URI.parse(uri).fsPath,
        "vue",
        fullContent
      );
      const offset = doc.offsetAt(position);
      const rr = await volarServer.tsserver.message({
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
      const server = verterServer;
      const fileName = "fixture.vue";
      const content = `<template>
    <div>{{ mess| }}</div>
</template>
<script setup lang="ts">
const message = 'Hello';
</script>`;

      const [contentBefore, contentAfter] = content.split("|");
      const position = {
        line: contentBefore.split("\n").length - 1,
        character: contentBefore.split("\n").pop()!.length,
      };

      const fullContent = contentBefore + contentAfter;
      const uri = URI.file(`${verterWorkspacePath}/${fileName}`).toString();

      await server.openDocument(uri, "vue", fullContent);
      const c = await server.getCompletions(uri, position);
      await server.closeDocument(uri);
    });
  });

  describe("Complex TypeScript inference", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();

    bench(
      "Volar - Complex TypeScript inference",
      async () => {
        const server = volarServer;
        const fileName = "fixture.vue";
        const content = `<script setup lang="ts">
import { ref } from 'vue'

interface User {
  id: number;
  name: string;
  email: string;
}

const user = ref<User>({ id: 1, name: 'John', email: 'john@example.com' })
user.value.|
</script>`;

        const [contentBefore, contentAfter] = content.split("|");
        const position = {
          line: contentBefore.split("\n").length - 1,
          character: contentBefore.split("\n").pop()!.length,
        };

        const fullContent = contentBefore + contentAfter;
        const uri = URI.file(`${volarWorkspacePath}/${fileName}`).toString();

        const doc = await server.open(uri, "vue", fullContent);
        const offset = doc.offsetAt(position);
        const r = await server.tsserver.message({
          seq: server.nextSeq(),
          command: "completions",
          arguments: {
            file: URI.parse(doc.uri).fsPath,
            position: offset,
          },
        });

        console.log(r);

        await server.close(doc.uri);
      },
      { time: 5000 }
    );

    bench(
      "Verter - Complex TypeScript inference",
      async () => {
        const server = verterServer;
        const fileName = "fixture.vue";
        const content = `<script setup lang="ts">
import { ref } from 'vue'

interface User {
  id: number;
  name: string;
  email: string;
}

const user = ref<User>({ id: 1, name: 'John', email: 'john@example.com' })
user.value.|
</script>`;

        const [contentBefore, contentAfter] = content.split("|");
        const position = {
          line: contentBefore.split("\n").length - 1,
          character: contentBefore.split("\n").pop()!.length,
        };

        const fullContent = contentBefore + contentAfter;
        const uri = URI.file(`${verterWorkspacePath}/${fileName}`).toString();

        await server.openDocument(uri, "vue", fullContent);
        const r = await server.getCompletions(uri, position);
        console.log(r);
        await server.closeDocument(uri);
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
        const server = volarServer;
        const files = Array.from({ length: 5 }, (_, i) => ({
          name: `file${i}.vue`,
          content: `<script setup lang="ts">
const msg${i} = 'Hello ${i}'
msg${i}.|
</script>`,
        }));

        for (const file of files) {
          const [contentBefore, contentAfter] = file.content.split("|");
          const position = {
            line: contentBefore.split("\n").length - 1,
            character: contentBefore.split("\n").pop()!.length,
          };

          const fullContent = contentBefore + contentAfter;
          const uri = URI.file(`${volarWorkspacePath}/${file.name}`).toString();

          const doc = await server.open(uri, "vue", fullContent);
          const offset = doc.offsetAt(position);
          await server.tsserver.message({
            seq: server.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(doc.uri).fsPath,
              position: offset,
            },
          });

          await server.close(doc.uri);
        }
      },
      { time: 10000 }
    );

    bench(
      "Verter - Open and complete 5 files",
      async () => {
        const server = verterServer;
        const files = Array.from({ length: 5 }, (_, i) => ({
          name: `file${i}.vue`,
          content: `<script setup lang="ts">
const msg${i} = 'Hello ${i}'
msg${i}.|
</script>`,
        }));

        for (const file of files) {
          const [contentBefore, contentAfter] = file.content.split("|");
          const position = {
            line: contentBefore.split("\n").length - 1,
            character: contentBefore.split("\n").pop()!.length,
          };

          const fullContent = contentBefore + contentAfter;
          const uri = URI.file(
            `${verterWorkspacePath}/${file.name}`
          ).toString();

          await server.openDocument(uri, "vue", fullContent);
          await server.getCompletions(uri, position);
          await server.closeDocument(uri);
        }
      },
      { time: 10000 }
    );
  });
});
