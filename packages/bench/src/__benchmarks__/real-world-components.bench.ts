import { bench, beforeAll, afterAll, describe } from "vitest";
import { URI } from "vscode-uri";
import {
  getLanguageServer as getVolarServer,
  testWorkspacePath as volarWorkspacePath,
  closeLanguageServer,
} from "../server/volar-server-lsp";
import {
  getVerterServer,
  testWorkspacePath as verterWorkspacePath,
  closeVerterServer,
} from "../server/verter-server";
import * as fs from "fs";
import * as path from "path";

describe("Real World Components Benchmarks", () => {
  let volarServer: Awaited<ReturnType<typeof getVolarServer>>;
  let verterServer: Awaited<ReturnType<typeof getVerterServer>>;

  beforeAll(async () => {
    // Copy __bench__ files to both test workspaces
    const benchDir = path.resolve(__dirname, "../__bench__");

    for (const workspacePath of [volarWorkspacePath, verterWorkspacePath]) {
      const targetDir = path.resolve(workspacePath, "components");

      if (!fs.existsSync(targetDir)) {
        fs.mkdirSync(targetDir, { recursive: true });
      }

      const files = fs.readdirSync(benchDir);
      for (const file of files) {
        if (file.endsWith(".vue")) {
          const source = path.join(benchDir, file);
          const target = path.join(targetDir, file);
          fs.copyFileSync(source, target);
        }
      }
    }

    // console.log("Setting up servers for benchmarks...");
    volarServer = await getVolarServer();
    // console.log("Volar server is ready.");
    verterServer = await getVerterServer();
    // console.log("Servers are ready.");
  });

  afterAll(async () => {
    await closeLanguageServer();
    await closeVerterServer();
  });

  describe(
    "button.vue - Props in computed (fresh document)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/button.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "props.label" and get completions after "props."
      const searchText = "props.label";
      const offset = fileContent.indexOf(searchText);
      const targetOffset =
        fileContent.indexOf("props.", offset) + "props.".length;

      const volarURI = URI.file(
        path.resolve(volarWorkspacePath, filePath)
      ).toString();
      const verterURI = URI.file(
        path.resolve(verterWorkspacePath, filePath)
      ).toString();

      afterAll(async () => {
        await volarServer.close(volarURI);
        await verterServer.closeDocument(verterURI);
      });

      bench(
        "Verter - button.vue props completions (fresh)",
        async () => {
          const doc = await verterServer.openDocument(
            verterURI,
            "vue",
            fileContent
          );

          const position = doc.positionAt(targetOffset);
          const c = await verterServer.getCompletions(verterURI, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - button.vue props completions (fresh)",
        async () => {
          const doc = await volarServer.open(volarURI, "vue", fileContent);

          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(doc.uri).fsPath,
              position: targetOffset,
            },
          });

          await volarServer.close(doc.uri);
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "button.vue - Props in computed (cached document)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/button.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "props.label" and get completions after "props."
      const searchText = "props.label";
      const offset = fileContent.indexOf(searchText);
      const targetOffset =
        fileContent.indexOf("props.", offset) + "props.".length;

      let uriVolar: string;
      let docVolar: any;
      let uriVerter: string;
      let docVerter: any;

      beforeAll(async () => {
        volarServer = await getVolarServer();
        verterServer = await getVerterServer();

        uriVolar = URI.file(
          path.resolve(volarWorkspacePath, filePath)
        ).toString();
        docVolar = await volarServer.open(uriVolar, "vue", fileContent);

        uriVerter = URI.file(
          path.resolve(verterWorkspacePath, filePath)
        ).toString();
        docVerter = await verterServer.openDocument(
          uriVerter,
          "vue",
          fileContent
        );
      });

      bench(
        "Verter - button.vue props completions (cached)",
        async () => {
          const position = docVerter.positionAt(targetOffset);
          await verterServer.getCompletions(uriVerter, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - button.vue props completions (cached)",
        async () => {
          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(docVolar.uri).fsPath,
              position: targetOffset,
            },
          });
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "button.vue - Action object properties (fresh)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/button.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "action?.label" and get completions after "action?."
      const searchText = "action?.label";
      const offset = fileContent.indexOf(searchText);
      const targetOffset =
        fileContent.indexOf("action?.", offset) + "action?.".length;

      const volarURI = URI.file(
        path.resolve(volarWorkspacePath, filePath)
      ).toString();
      const verterURI = URI.file(
        path.resolve(verterWorkspacePath, filePath)
      ).toString();

      afterAll(async () => {
        await volarServer.close(volarURI);
        await verterServer.closeDocument(verterURI);
      });

      bench(
        "Verter - button.vue action properties (fresh)",
        async () => {
          const doc = await verterServer.openDocument(
            verterURI,
            "vue",
            fileContent
          );

          const position = doc.positionAt(targetOffset);
          await verterServer.getCompletions(verterURI, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - button.vue action properties (fresh)",
        async () => {
          const doc = await volarServer.open(volarURI, "vue", fileContent);

          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(doc.uri).fsPath,
              position: targetOffset,
            },
          });

          await volarServer.close(doc.uri);
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "button.vue - Action object properties (cached)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/button.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "action?.label" and get completions after "action?."
      const searchText = "action?.label";
      const offset = fileContent.indexOf(searchText);
      const targetOffset =
        fileContent.indexOf("action?.", offset) + "action?.".length;

      let uriVolar: string;
      let docVolar: any;
      let uriVerter: string;
      let docVerter: any;

      beforeAll(async () => {
        volarServer = await getVolarServer();
        verterServer = await getVerterServer();

        uriVolar = URI.file(
          path.resolve(volarWorkspacePath, filePath)
        ).toString();
        docVolar = await volarServer.open(uriVolar, "vue", fileContent);

        uriVerter = URI.file(
          path.resolve(verterWorkspacePath, filePath)
        ).toString();
        docVerter = await verterServer.openDocument(
          uriVerter,
          "vue",
          fileContent
        );
      });

      bench(
        "Verter - button.vue action properties (cached)",
        async () => {
          const position = docVerter.positionAt(targetOffset);
          await verterServer.getCompletions(uriVerter, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - button.vue action properties (cached)",
        async () => {
          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(docVolar.uri).fsPath,
              position: targetOffset,
            },
          });
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "avatar.vue - Props in computed (fresh)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/avatar.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "props.id" and get completions after "props."
      const searchText = "props.id";
      const offset = fileContent.indexOf(searchText);
      const targetOffset =
        fileContent.indexOf("props.", offset) + "props.".length;

      const volarURI = URI.file(
        path.resolve(volarWorkspacePath, filePath)
      ).toString();
      const verterURI = URI.file(
        path.resolve(verterWorkspacePath, filePath)
      ).toString();

      afterAll(async () => {
        await volarServer.close(volarURI);
        await verterServer.closeDocument(verterURI);
      });

      bench(
        "Verter - avatar.vue props completions (fresh)",
        async () => {
          const doc = await verterServer.openDocument(
            verterURI,
            "vue",
            fileContent
          );

          const position = doc.positionAt(targetOffset);
          await verterServer.getCompletions(verterURI, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - avatar.vue props completions (fresh)",
        async () => {
          const doc = await volarServer.open(volarURI, "vue", fileContent);

          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(doc.uri).fsPath,
              position: targetOffset,
            },
          });

          await volarServer.close(doc.uri);
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "avatar.vue - Props in computed (cached)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/avatar.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "props.id" and get completions after "props."
      const searchText = "props.id";
      const offset = fileContent.indexOf(searchText);
      const targetOffset =
        fileContent.indexOf("props.", offset) + "props.".length;

      let uriVolar: string;
      let docVolar: any;
      let uriVerter: string;
      let docVerter: any;

      beforeAll(async () => {
        volarServer = await getVolarServer();
        verterServer = await getVerterServer();

        uriVolar = URI.file(
          path.resolve(volarWorkspacePath, filePath)
        ).toString();
        docVolar = await volarServer.open(uriVolar, "vue", fileContent);

        uriVerter = URI.file(
          path.resolve(verterWorkspacePath, filePath)
        ).toString();
        docVerter = await verterServer.openDocument(
          uriVerter,
          "vue",
          fileContent
        );
      });

      bench(
        "Verter - avatar.vue props completions (cached)",
        async () => {
          const position = docVerter.positionAt(targetOffset);
          await verterServer.getCompletions(uriVerter, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - avatar.vue props completions (cached)",
        async () => {
          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(docVolar.uri).fsPath,
              position: targetOffset,
            },
          });
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "avatar.vue - Ref properties (fresh)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/avatar.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "avatarEl.value" and get completions after "avatarEl."
      const searchText = "avatarEl.value";
      const offset = fileContent.indexOf(searchText);
      const targetOffset =
        fileContent.indexOf("avatarEl.", offset) + "avatarEl.".length;

      const volarURI = URI.file(
        path.resolve(volarWorkspacePath, filePath)
      ).toString();
      const verterURI = URI.file(
        path.resolve(verterWorkspacePath, filePath)
      ).toString();

      afterAll(async () => {
        await volarServer.close(volarURI);
        await verterServer.closeDocument(verterURI);
      });

      bench(
        "Verter - avatar.vue ref properties (fresh)",
        async () => {
          const doc = await verterServer.openDocument(
            verterURI,
            "vue",
            fileContent
          );

          const position = doc.positionAt(targetOffset);
          await verterServer.getCompletions(verterURI, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - avatar.vue ref properties (fresh)",
        async () => {
          const doc = await volarServer.open(volarURI, "vue", fileContent);

          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(doc.uri).fsPath,
              position: targetOffset,
            },
          });

          await volarServer.close(doc.uri);
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "avatar.vue - Ref properties (cached)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/avatar.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "avatarEl.value" and get completions after "avatarEl."
      const searchText = "avatarEl.value";
      const offset = fileContent.indexOf(searchText);
      const targetOffset =
        fileContent.indexOf("avatarEl.", offset) + "avatarEl.".length;

      let uriVolar: string;
      let docVolar: any;
      let uriVerter: string;
      let docVerter: any;

      beforeAll(async () => {
        volarServer = await getVolarServer();
        verterServer = await getVerterServer();

        uriVolar = URI.file(
          path.resolve(volarWorkspacePath, filePath)
        ).toString();
        docVolar = await volarServer.open(uriVolar, "vue", fileContent);

        uriVerter = URI.file(
          path.resolve(verterWorkspacePath, filePath)
        ).toString();
        docVerter = await verterServer.openDocument(
          uriVerter,
          "vue",
          fileContent
        );
      });

      bench(
        "Verter - avatar.vue ref properties (cached)",
        async () => {
          const position = docVerter.positionAt(targetOffset);
          await verterServer.getCompletions(uriVerter, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - avatar.vue ref properties (cached)",
        async () => {
          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(docVolar.uri).fsPath,
              position: targetOffset,
            },
          });
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "avatar.vue - HTMLElement properties (fresh)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/avatar.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "avatarEl.value?.offsetHeight" and get completions after "value?."
      const searchText = "avatarEl.value?.offsetHeight";
      const offset = fileContent.indexOf(searchText);
      const targetOffset =
        fileContent.indexOf("value?.", offset) + "value?.".length;

      const volarURI = URI.file(
        path.resolve(volarWorkspacePath, filePath)
      ).toString();
      const verterURI = URI.file(
        path.resolve(verterWorkspacePath, filePath)
      ).toString();

      afterAll(async () => {
        await volarServer.close(volarURI);
        await verterServer.closeDocument(verterURI);
      });

      bench(
        "Verter - avatar.vue HTMLElement properties (fresh)",
        async () => {
          const doc = await verterServer.openDocument(
            verterURI,
            "vue",
            fileContent
          );

          const position = doc.positionAt(targetOffset);
          await verterServer.getCompletions(verterURI, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - avatar.vue HTMLElement properties (fresh)",
        async () => {
          const doc = await volarServer.open(volarURI, "vue", fileContent);

          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(doc.uri).fsPath,
              position: targetOffset,
            },
          });

          await volarServer.close(doc.uri);
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "avatar.vue - HTMLElement properties (cached)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/avatar.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "avatarEl.value?.offsetHeight" and get completions after "value?."
      const searchText = "avatarEl.value?.offsetHeight";
      const offset = fileContent.indexOf(searchText);
      const targetOffset =
        fileContent.indexOf("value?.", offset) + "value?.".length;

      let uriVolar: string;
      let docVolar: any;
      let uriVerter: string;
      let docVerter: any;

      beforeAll(async () => {
        volarServer = await getVolarServer();
        verterServer = await getVerterServer();

        uriVolar = URI.file(
          path.resolve(volarWorkspacePath, filePath)
        ).toString();
        docVolar = await volarServer.open(uriVolar, "vue", fileContent);

        uriVerter = URI.file(
          path.resolve(verterWorkspacePath, filePath)
        ).toString();
        docVerter = await verterServer.openDocument(
          uriVerter,
          "vue",
          fileContent
        );
      });

      bench(
        "Verter - avatar.vue HTMLElement properties (cached)",
        async () => {
          const position = docVerter.positionAt(targetOffset);
          await verterServer.getCompletions(uriVerter, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - avatar.vue HTMLElement properties (cached)",
        async () => {
          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(docVolar.uri).fsPath,
              position: targetOffset,
            },
          });
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "icon.vue - Ref element (fresh)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/icon.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "el.value" and get completions after "el."
      const searchText = "el.value";
      const offset = fileContent.indexOf(searchText);
      const targetOffset = fileContent.indexOf("el.", offset) + "el.".length;

      const volarURI = URI.file(
        path.resolve(volarWorkspacePath, filePath)
      ).toString();
      const verterURI = URI.file(
        path.resolve(verterWorkspacePath, filePath)
      ).toString();

      afterAll(async () => {
        await volarServer.close(volarURI);
        await verterServer.closeDocument(verterURI);
      });

      bench(
        "Verter - icon.vue ref element (fresh)",
        async () => {
          const doc = await verterServer.openDocument(
            verterURI,
            "vue",
            fileContent
          );

          const position = doc.positionAt(targetOffset);
          await verterServer.getCompletions(verterURI, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - icon.vue ref element (fresh)",
        async () => {
          const doc = await volarServer.open(volarURI, "vue", fileContent);

          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(doc.uri).fsPath,
              position: targetOffset,
            },
          });

          await volarServer.close(doc.uri);
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "icon.vue - Ref element (cached)",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/icon.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      // Find "el.value" and get completions after "el."
      const searchText = "el.value";
      const offset = fileContent.indexOf(searchText);
      const targetOffset = fileContent.indexOf("el.", offset) + "el.".length;

      let uriVolar: string;
      let docVolar: any;
      let uriVerter: string;
      let docVerter: any;

      beforeAll(async () => {
        volarServer = await getVolarServer();
        verterServer = await getVerterServer();

        uriVolar = URI.file(
          path.resolve(volarWorkspacePath, filePath)
        ).toString();
        docVolar = await volarServer.open(uriVolar, "vue", fileContent);

        uriVerter = URI.file(
          path.resolve(verterWorkspacePath, filePath)
        ).toString();
        docVerter = await verterServer.openDocument(
          uriVerter,
          "vue",
          fileContent
        );
      });

      bench(
        "Verter - icon.vue ref element (cached)",
        async () => {
          const position = docVerter.positionAt(targetOffset);
          await verterServer.getCompletions(uriVerter, position);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - icon.vue ref element (cached)",
        async () => {
          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(docVolar.uri).fsPath,
              position: targetOffset,
            },
          });
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );

  describe(
    "Real world editing workflow",
    {
      concurrent: false,
      sequential: true,
    },
    () => {
      const filePath = "components/button.vue";
      const fileContent = fs.readFileSync(
        path.resolve(volarWorkspacePath, filePath),
        "utf-8"
      );

      bench(
        "Verter - complete editing workflow",
        async () => {
          const uri = URI.file(
            path.resolve(verterWorkspacePath, filePath)
          ).toString();

          // 1. Open document
          let doc = await verterServer.openDocument(uri, "vue", fileContent);

          // 2. Get completions after "props." (simulating typing)
          const searchText = "props.label";
          const offset = fileContent.indexOf(searchText);
          const targetOffset =
            fileContent.indexOf("props.", offset) + "props.".length;
          let position = doc.positionAt(targetOffset);
          await verterServer.getCompletions(uri, position);

          // 3. Simulate file edit by closing and reopening with modified content
          await verterServer.closeDocument(uri);
          const edit1Offset = fileContent.indexOf("const color") - 1;
          const edit1Content =
            fileContent.slice(0, edit1Offset) +
            "\n  // Edit 1\n" +
            fileContent.slice(edit1Offset);
          doc = await verterServer.openDocument(uri, "vue", edit1Content);

          // 4. Get completions after "action?." (simulating more typing)
          const actionOffset = fileContent.indexOf("action?.label");
          const actionTargetOffset =
            fileContent.indexOf("action?.", actionOffset) + "action?.".length;
          position = doc.positionAt(actionTargetOffset);
          await verterServer.getCompletions(uri, position);

          // 5. Simulate another edit
          await verterServer.closeDocument(uri);
          const edit2Offset = edit1Content.indexOf("const buttonClasses") - 1;
          const edit2Content =
            edit1Content.slice(0, edit2Offset) +
            "\n  // Edit 2\n" +
            edit1Content.slice(edit2Offset);
          doc = await verterServer.openDocument(uri, "vue", edit2Content);

          // 6. Get completions one more time
          position = doc.positionAt(targetOffset);
          await verterServer.getCompletions(uri, position);

          // 7. Close document
          await verterServer.closeDocument(uri);
        },
        {
          async setup() {
            verterServer = await getVerterServer();
          },
        }
      );

      bench(
        "Volar - complete editing workflow",
        async () => {
          const uri = URI.file(
            path.resolve(volarWorkspacePath, filePath)
          ).toString();

          // 1. Open document
          let doc = await volarServer.open(uri, "vue", fileContent);

          // 2. Get completions after "props." (simulating typing)
          const searchText = "props.label";
          const offset = fileContent.indexOf(searchText);
          const targetOffset =
            fileContent.indexOf("props.", offset) + "props.".length;
          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(doc.uri).fsPath,
              position: targetOffset,
            },
          });

          // 3. Simulate file edit by closing and reopening with modified content
          await volarServer.close(doc.uri);
          const edit1Offset = fileContent.indexOf("const color") - 1;
          const edit1Content =
            fileContent.slice(0, edit1Offset) +
            "\n  // Edit 1\n" +
            fileContent.slice(edit1Offset);
          doc = await volarServer.open(uri, "vue", edit1Content);

          // 4. Get completions after "action?." (simulating more typing)
          const actionOffset = fileContent.indexOf("action?.label");
          const actionTargetOffset =
            fileContent.indexOf("action?.", actionOffset) + "action?.".length;
          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(doc.uri).fsPath,
              position: actionTargetOffset,
            },
          });

          // 5. Simulate another edit
          await volarServer.close(doc.uri);
          const edit2Offset = edit1Content.indexOf("const buttonClasses") - 1;
          const edit2Content =
            edit1Content.slice(0, edit2Offset) +
            "\n  // Edit 2\n" +
            edit1Content.slice(edit2Offset);
          doc = await volarServer.open(uri, "vue", edit2Content);

          // 6. Get completions one more time
          await volarServer.tsserver.message({
            seq: volarServer.nextSeq(),
            command: "completions",
            arguments: {
              file: URI.parse(doc.uri).fsPath,
              position: targetOffset,
            },
          });

          // 7. Close document
          await volarServer.close(doc.uri);
        },
        {
          async setup() {
            volarServer = await getVolarServer();
          },
        }
      );
    }
  );
});
