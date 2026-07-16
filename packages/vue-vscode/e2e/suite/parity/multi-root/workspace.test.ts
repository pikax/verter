/**
 * Multi-root VS Code workspace: Vue package + Svelte package side by side.
 */
import * as vscode from "vscode";
import { FIXTURE_NAME } from "../../../helpers";
import { ensureParityReady, failParityGap, workspaceRoot } from "../../../lib/parityHarness";

function onlyMultiRoot(ctx: Mocha.Context): void {
  if (FIXTURE_NAME !== "multi-root-parity")
    throw new Error("TEST_DEFECT: parity suite loaded for an inapplicable fixture");
}

suite(`Multi-root workspace [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    onlyMultiRoot(this);
    // code-workspace mounts pkg-a as folder[0]; open relative to that folder.
    await ensureParityReady("src/App.vue");
  });

  test("multi-root.folders.present", async function () {
    onlyMultiRoot(this);
    try {
      const folders = vscode.workspace.workspaceFolders ?? [];
      if (folders.length < 2) {
        throw new Error(
          `expected >=2 workspace folders, got ${folders.length}: ${folders
            .map((f) => f.name)
            .join(", ")}`,
        );
      }
    } catch (err) {
      failParityGap(
        this,
        "multi-root.folders.present",
        "ISSUE-multi-root-folders",
        `Multi-root folders not opened: ${String(err)}`,
      );
    }
  });

  test("multi-root.pkg-a.vue.hover", async function () {
    onlyMultiRoot(this);
    try {
      // Resolve relative to whichever folder owns pkg-a.
      const folders = vscode.workspace.workspaceFolders ?? [];
      const a = folders.find((f) => /pkg-a|a$/i.test(f.name) || f.uri.fsPath.includes("pkg-a"));
      if (!a) throw new Error(`pkg-a folder not found among ${folders.map((f) => f.uri.fsPath)}`);
      const uri = vscode.Uri.joinPath(a.uri, "src", "App.vue");
      const doc = await vscode.workspace.openTextDocument(uri);
      await vscode.window.showTextDocument(doc);
      const text = doc.getText();
      const idx = text.indexOf("rootA");
      if (idx < 0) throw new Error("rootA missing");
      const pos = doc.positionAt(idx + 1);
      const hovers =
        (await vscode.commands.executeCommand<vscode.Hover[]>(
          "vscode.executeHoverProvider",
          doc.uri,
          pos,
        )) ?? [];
      if (hovers.length === 0) throw new Error("no hover in pkg-a App.vue");
    } catch (err) {
      failParityGap(
        this,
        "multi-root.pkg-a.vue.hover",
        "ISSUE-multi-root-vue-hover",
        `pkg-a Vue hover failed: ${String(err)}`,
      );
    }
  });

  test("multi-root.pkg-b.svelte.hover", async function () {
    onlyMultiRoot(this);
    try {
      const folders = vscode.workspace.workspaceFolders ?? [];
      const b = folders.find((f) => /pkg-b|b$/i.test(f.name) || f.uri.fsPath.includes("pkg-b"));
      if (!b) throw new Error(`pkg-b folder not found among ${folders.map((f) => f.uri.fsPath)}`);
      const uri = vscode.Uri.joinPath(b.uri, "src", "App.svelte");
      const doc = await vscode.workspace.openTextDocument(uri);
      await vscode.window.showTextDocument(doc);
      if (doc.languageId !== "svelte") {
        throw new Error(`expected svelte languageId, got ${doc.languageId}`);
      }
      const text = doc.getText();
      const idx = text.indexOf("rootB");
      if (idx < 0) throw new Error("rootB missing");
      const pos = doc.positionAt(idx + 1);
      const hovers =
        (await vscode.commands.executeCommand<vscode.Hover[]>(
          "vscode.executeHoverProvider",
          doc.uri,
          pos,
        )) ?? [];
      if (hovers.length === 0) throw new Error("no hover in pkg-b App.svelte");
    } catch (err) {
      failParityGap(
        this,
        "multi-root.pkg-b.svelte.hover",
        "ISSUE-multi-root-svelte-hover",
        `pkg-b Svelte hover failed: ${String(err)}`,
      );
    }
  });

  test("multi-root.no-cross-folder-poison", async function () {
    onlyMultiRoot(this);
    try {
      const folders = vscode.workspace.workspaceFolders ?? [];
      if (folders.length < 2) throw new Error("need two folders");
      // Opening both roots should not throw or leave LSP unready.
      const root = workspaceRoot();
      if (!root) throw new Error("no workspace root");
    } catch (err) {
      failParityGap(
        this,
        "multi-root.no-cross-folder-poison",
        "ISSUE-multi-root-isolation",
        `Multi-root isolation check failed: ${String(err)}`,
      );
    }
  });

  test("multi-root.isolation.rootA-not-in-pkg-b", async function () {
    onlyMultiRoot(this);
    try {
      const folders = vscode.workspace.workspaceFolders ?? [];
      const b = folders.find((f) => f.uri.fsPath.includes("pkg-b"));
      if (!b) throw new Error("pkg-b missing");
      const uri = vscode.Uri.joinPath(b.uri, "src", "App.svelte");
      const doc = await vscode.workspace.openTextDocument(uri);
      const text = doc.getText();
      // Isolation: package-b must not contain package-a symbols.
      if (text.includes("rootA") || text.includes("package-a")) {
        throw new Error("pkg-b source incorrectly contains pkg-a identifiers");
      }
      if (!text.includes("rootB")) throw new Error("pkg-b missing rootB");
    } catch (err) {
      failParityGap(
        this,
        "multi-root.isolation.rootA-not-in-pkg-b",
        "ISSUE-multi-root-isolation",
        `Cross-root source isolation failed: ${String(err)}`,
        "product-gap",
      );
    }
  });

  test("multi-root.both-roots-hover-typed", async function () {
    onlyMultiRoot(this);
    try {
      const folders = vscode.workspace.workspaceFolders ?? [];
      const a = folders.find((f) => f.uri.fsPath.includes("pkg-a"));
      const b = folders.find((f) => f.uri.fsPath.includes("pkg-b"));
      if (!a || !b) throw new Error("need pkg-a and pkg-b");
      for (const [folder, rel, token] of [
        [a, "src/App.vue", "rootA"],
        [b, "src/App.svelte", "rootB"],
      ] as const) {
        const uri = vscode.Uri.joinPath(folder.uri, ...rel.split("/"));
        const doc = await vscode.workspace.openTextDocument(uri);
        await vscode.window.showTextDocument(doc);
        const idx = doc.getText().indexOf(token);
        if (idx < 0) throw new Error(`missing ${token} in ${rel}`);
        const hovers =
          (await vscode.commands.executeCommand<vscode.Hover[]>(
            "vscode.executeHoverProvider",
            doc.uri,
            doc.positionAt(idx + 1),
          )) ?? [];
        if (hovers.length === 0) throw new Error(`no hover for ${token}`);
        const body = hovers
          .flatMap((h) => h.contents)
          .map((c) => (typeof c === "string" ? c : c.value))
          .join("\n");
        if (!body.includes(token) && !/string|const|let/.test(body)) {
          throw new Error(`hover for ${token} not meaningful: ${body.slice(0, 120)}`);
        }
      }
    } catch (err) {
      failParityGap(
        this,
        "multi-root.both-roots-hover-typed",
        "ISSUE-multi-root-dual-hover",
        `Multi-root dual hover failed: ${String(err)}`,
        "product-gap",
      );
    }
  });
});
