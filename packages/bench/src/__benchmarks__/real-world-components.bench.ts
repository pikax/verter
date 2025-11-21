import { bench, beforeAll, describe } from "vitest";
import { URI } from "vscode-uri";
import {
  getLanguageServer as getVolarServer,
  testWorkspacePath as volarWorkspacePath,
} from "../server/volar-server";
import {
  getVerterServer,
  testWorkspacePath as verterWorkspacePath,
} from "../server/verter-server-direct";
import * as fs from "fs";
import * as path from "path";

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
});

describe("Real World Components Benchmarks", () => {
  describe("button.vue - Props in computed (fresh document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/button.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "props.label" and get completions after "props."
    const searchText = "props.label";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("props.", offset) + "props.".length;

    bench("Volar - button.vue props completions (fresh)", async () => {
      const uri = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
      const doc = await volarServer.open(uri, "vue", fileContent);
      
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(doc.uri).fsPath,
          position: targetOffset,
        },
      });

      await volarServer.close(doc.uri);
    });

    bench("Verter - button.vue props completions (fresh)", async () => {
      const uri = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
      const doc = await verterServer.openDocument(uri, "vue", fileContent);
      
      const position = doc.positionAt(targetOffset);
      await verterServer.getCompletions(uri, position);

      await verterServer.closeDocument(uri);
    });
  });

  describe("button.vue - Props in computed (cached document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/button.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "props.label" and get completions after "props."
    const searchText = "props.label";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("props.", offset) + "props.".length;

    const uriVolar = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
    const docVolar = await volarServer.open(uriVolar, "vue", fileContent);

    const uriVerter = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
    const docVerter = await verterServer.openDocument(uriVerter, "vue", fileContent);

    bench("Volar - button.vue props completions (cached)", async () => {
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(docVolar.uri).fsPath,
          position: targetOffset,
        },
      });
    });

    bench("Verter - button.vue props completions (cached)", async () => {
      const position = docVerter.positionAt(targetOffset);
      await verterServer.getCompletions(uriVerter, position);
    });
  });

  describe("button.vue - Action object properties (fresh document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/button.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "action?.label" and get completions after "action?."
    const searchText = "action?.label";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("action?.", offset) + "action?.".length;

    bench("Volar - button.vue action properties (fresh)", async () => {
      const uri = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
      const doc = await volarServer.open(uri, "vue", fileContent);
      
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(doc.uri).fsPath,
          position: targetOffset,
        },
      });

      await volarServer.close(doc.uri);
    });

    bench("Verter - button.vue action properties (fresh)", async () => {
      const uri = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
      const doc = await verterServer.openDocument(uri, "vue", fileContent);
      
      const position = doc.positionAt(targetOffset);
      await verterServer.getCompletions(uri, position);

      await verterServer.closeDocument(uri);
    });
  });

  describe("button.vue - Action object properties (cached document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/button.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "action?.label" and get completions after "action?."
    const searchText = "action?.label";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("action?.", offset) + "action?.".length;

    const uriVolar = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
    const docVolar = await volarServer.open(uriVolar, "vue", fileContent);

    const uriVerter = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
    const docVerter = await verterServer.openDocument(uriVerter, "vue", fileContent);

    bench("Volar - button.vue action properties (cached)", async () => {
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(docVolar.uri).fsPath,
          position: targetOffset,
        },
      });
    });

    bench("Verter - button.vue action properties (cached)", async () => {
      const position = docVerter.positionAt(targetOffset);
      await verterServer.getCompletions(uriVerter, position);
    });
  });

  describe("avatar.vue - Props in computed (fresh document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/avatar.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "props.id" and get completions after "props."
    const searchText = "props.id";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("props.", offset) + "props.".length;

    bench("Volar - avatar.vue props completions (fresh)", async () => {
      const uri = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
      const doc = await volarServer.open(uri, "vue", fileContent);
      
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(doc.uri).fsPath,
          position: targetOffset,
        },
      });

      await volarServer.close(doc.uri);
    });

    bench("Verter - avatar.vue props completions (fresh)", async () => {
      const uri = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
      const doc = await verterServer.openDocument(uri, "vue", fileContent);
      
      const position = doc.positionAt(targetOffset);
      await verterServer.getCompletions(uri, position);

      await verterServer.closeDocument(uri);
    });
  });

  describe("avatar.vue - Props in computed (cached document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/avatar.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "props.id" and get completions after "props."
    const searchText = "props.id";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("props.", offset) + "props.".length;

    const uriVolar = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
    const docVolar = await volarServer.open(uriVolar, "vue", fileContent);

    const uriVerter = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
    const docVerter = await verterServer.openDocument(uriVerter, "vue", fileContent);

    bench("Volar - avatar.vue props completions (cached)", async () => {
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(docVolar.uri).fsPath,
          position: targetOffset,
        },
      });
    });

    bench("Verter - avatar.vue props completions (cached)", async () => {
      const position = docVerter.positionAt(targetOffset);
      await verterServer.getCompletions(uriVerter, position);
    });
  });

  describe("avatar.vue - Ref properties (fresh document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/avatar.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "avatarEl.value" and get completions after "avatarEl."
    const searchText = "avatarEl.value";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("avatarEl.", offset) + "avatarEl.".length;

    bench("Volar - avatar.vue ref properties (fresh)", async () => {
      const uri = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
      const doc = await volarServer.open(uri, "vue", fileContent);
      
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(doc.uri).fsPath,
          position: targetOffset,
        },
      });

      await volarServer.close(doc.uri);
    });

    bench("Verter - avatar.vue ref properties (fresh)", async () => {
      const uri = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
      const doc = await verterServer.openDocument(uri, "vue", fileContent);
      
      const position = doc.positionAt(targetOffset);
      await verterServer.getCompletions(uri, position);

      await verterServer.closeDocument(uri);
    });
  });

  describe("avatar.vue - Ref properties (cached document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/avatar.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "avatarEl.value" and get completions after "avatarEl."
    const searchText = "avatarEl.value";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("avatarEl.", offset) + "avatarEl.".length;

    const uriVolar = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
    const docVolar = await volarServer.open(uriVolar, "vue", fileContent);

    const uriVerter = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
    const docVerter = await verterServer.openDocument(uriVerter, "vue", fileContent);

    bench("Volar - avatar.vue ref properties (cached)", async () => {
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(docVolar.uri).fsPath,
          position: targetOffset,
        },
      });
    });

    bench("Verter - avatar.vue ref properties (cached)", async () => {
      const position = docVerter.positionAt(targetOffset);
      await verterServer.getCompletions(uriVerter, position);
    });
  });

  describe("avatar.vue - HTMLElement properties (fresh document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/avatar.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "avatarEl.value?.offsetHeight" and get completions after "value?."
    const searchText = "avatarEl.value?.offsetHeight";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("value?.", offset) + "value?.".length;

    bench("Volar - avatar.vue HTMLElement properties (fresh)", async () => {
      const uri = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
      const doc = await volarServer.open(uri, "vue", fileContent);
      
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(doc.uri).fsPath,
          position: targetOffset,
        },
      });

      await volarServer.close(doc.uri);
    });

    bench("Verter - avatar.vue HTMLElement properties (fresh)", async () => {
      const uri = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
      const doc = await verterServer.openDocument(uri, "vue", fileContent);
      
      const position = doc.positionAt(targetOffset);
      await verterServer.getCompletions(uri, position);

      await verterServer.closeDocument(uri);
    });
  });

  describe("avatar.vue - HTMLElement properties (cached document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/avatar.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "avatarEl.value?.offsetHeight" and get completions after "value?."
    const searchText = "avatarEl.value?.offsetHeight";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("value?.", offset) + "value?.".length;

    const uriVolar = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
    const docVolar = await volarServer.open(uriVolar, "vue", fileContent);

    const uriVerter = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
    const docVerter = await verterServer.openDocument(uriVerter, "vue", fileContent);

    bench("Volar - avatar.vue HTMLElement properties (cached)", async () => {
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(docVolar.uri).fsPath,
          position: targetOffset,
        },
      });
    });

    bench("Verter - avatar.vue HTMLElement properties (cached)", async () => {
      const position = docVerter.positionAt(targetOffset);
      await verterServer.getCompletions(uriVerter, position);
    });
  });

  describe("icon.vue - Ref element (fresh document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/icon.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "el.value" and get completions after "el."
    const searchText = "el.value";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("el.", offset) + "el.".length;

    bench("Volar - icon.vue ref element (fresh)", async () => {
      const uri = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
      const doc = await volarServer.open(uri, "vue", fileContent);
      
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(doc.uri).fsPath,
          position: targetOffset,
        },
      });

      await volarServer.close(doc.uri);
    });

    bench("Verter - icon.vue ref element (fresh)", async () => {
      const uri = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
      const doc = await verterServer.openDocument(uri, "vue", fileContent);
      
      const position = doc.positionAt(targetOffset);
      await verterServer.getCompletions(uri, position);

      await verterServer.closeDocument(uri);
    });
  });

  describe("icon.vue - Ref element (cached document)", async () => {
    const volarServer = await getVolarServer();
    const verterServer = await getVerterServer();
    const filePath = "components/icon.vue";
    const fileContent = fs.readFileSync(
      path.resolve(volarWorkspacePath, filePath),
      "utf-8"
    );

    // Find "el.value" and get completions after "el."
    const searchText = "el.value";
    const offset = fileContent.indexOf(searchText);
    const targetOffset = fileContent.indexOf("el.", offset) + "el.".length;

    const uriVolar = URI.file(path.resolve(volarWorkspacePath, filePath)).toString();
    const docVolar = await volarServer.open(uriVolar, "vue", fileContent);

    const uriVerter = URI.file(path.resolve(verterWorkspacePath, filePath)).toString();
    const docVerter = await verterServer.openDocument(uriVerter, "vue", fileContent);

    bench("Volar - icon.vue ref element (cached)", async () => {
      await volarServer.tsserver.message({
        seq: volarServer.nextSeq(),
        command: "completions",
        arguments: {
          file: URI.parse(docVolar.uri).fsPath,
          position: targetOffset,
        },
      });
    });

    bench("Verter - icon.vue ref element (cached)", async () => {
      const position = docVerter.positionAt(targetOffset);
      await verterServer.getCompletions(uriVerter, position);
    });
  });
});
