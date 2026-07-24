import { expect } from "chai";
import * as vscode from "vscode";
import * as fs from "fs";
import * as path from "path";
import {
  ensureFixtureWarm,
  isLspReady,
  runFormatOnTypeAfter,
  waitForOnTypeReady,
  FIXTURE_NAME,
  getAppVuePath,
} from "../helpers";

// Behavior B of proactive tag auto-close (ISSUE-4): the server's on-type
// formatting handler inserts a matching `</tag>` when you type the `>` that
// closes an open tag. The VS Code-facing wiring is:
//   - the LSP advertises `documentOnTypeFormattingProvider` with trigger `>`
//     (see capabilities.rs — the `/` more-trigger was dropped as a no-op), and
//   - the extension defaults `editor.formatOnType: true` for `[vue]`/`[svelte]`
//     via `contributes.configurationDefaults`.
//
// The handler GATES auto-close to the carrier's TEMPLATE/MARKUP region (Vue:
// inside `<template>`; Svelte: at the SFC root) and never fires inside
// `<script>` / `<style>`, inside a quoted attribute value, on a `{{ }}`
// interpolation expression, or in a non-carrier (`.ts` / `.js`) document. These
// tests drive `vscode.executeFormatOnTypeProvider` directly (the command VS
// Code dispatches when `formatOnType` fires). The provider runs against the
// document's CURRENT content, so each fixture is written with an open tag whose
// `>` sits immediately before the cursor and NO existing closing tag — exactly
// the state right after the user typed `>`.
//
// READINESS: the auto-close scratch carriers carry no `{{ }}` / `defineProps`,
// so the generic `waitForFileReady` probe would return immediately WITHOUT
// proving the document reached the LSP. Each test instead establishes readiness
// via `waitForOnTypeReady` against a KNOWN-positive control tag in the SAME
// document — a positive round-trip proves the provider is wired AND the LSP
// processed this document. Only then does a negative assertion ("void / generic
// / script `>` inserts nothing") distinguish "ready + correctly no edit" from
// "provider not ready yet".
//
// NOTE: this suite cannot run headless in the agent harness — it requires the
// VS Code Extension Host (`pnpm --filter verter-vscode test:e2e`). It is wired
// into the e2e matrix and is expected to be exercised there.

/**
 * Resolve the workspace-relative directory the scratch carrier should live in.
 *
 * The fixture matrix is not uniform: `single-project`/`path-aliases`/etc. keep
 * components under a root `src/`, but `monorepo` nests them in
 * `packages/app/src/` and `no-config`/`single-file` place them at the fixture
 * root with NO `src/` at all. Writing to a hardcoded `src/` therefore throws
 * for the latter three. Anchor scratch files next to the fixture's own
 * App.vue instead — that directory is guaranteed to exist for every fixture
 * AND sits inside the LSP's project scope, so the carrier is recognized as a
 * Vue/Svelte document and the on-type handler fires.
 */
function scratchDir(): string {
  return path.dirname(getAppVuePath());
}

/**
 * Write a throwaway carrier file into the workspace and open it.
 *
 * `name` is a bare basename (e.g. `__autoclose_scratch.vue`); it is anchored to
 * `scratchDir()` so the path resolves under an existing, project-scoped
 * directory for every fixture in the matrix. `mkdirSync(..., recursive)` is a
 * no-op safety net (the App.vue directory already exists) so the write never
 * fails on a fixture with an unusual layout.
 *
 * Readiness is NOT awaited here — callers establish it deterministically via
 * `waitForOnTypeReady` against a positive control tag in the written content.
 */
async function openScratch(
  name: string,
  content: string,
): Promise<{ doc: vscode.TextDocument; fsPath: string }> {
  const folders = vscode.workspace.workspaceFolders;
  expect(folders && folders.length > 0, "workspace must have a folder").to.be.true;
  const dir = path.join(folders![0].uri.fsPath, scratchDir());
  fs.mkdirSync(dir, { recursive: true });
  const fsPath = path.join(dir, name);
  fs.writeFileSync(fsPath, content, "utf8");
  const doc = await vscode.workspace.openTextDocument(vscode.Uri.file(fsPath));
  await vscode.window.showTextDocument(doc);
  return { doc, fsPath };
}

suite(`Auto-close tag (on-type formatting) [${FIXTURE_NAME}]`, function () {
  const scratchFiles: string[] = [];

  suiteSetup(async function () {
    expect(isLspReady(), "LSP must be ready").to.be.true;
    await ensureFixtureWarm();
  });

  suiteTeardown(function () {
    for (const f of scratchFiles) {
      try {
        fs.unlinkSync(f);
      } catch {
        // best-effort cleanup
      }
    }
  });

  test("inserts </section> when the open <section> tag is closed in a .vue file", async function () {
    const { doc, fsPath } = await openScratch(
      "__autoclose_scratch.vue",
      "<template><section>\n</template>\n",
    );
    scratchFiles.push(fsPath);

    // Positive case IS the readiness control: a round-trip on <section> proves
    // the provider is live for this document.
    await waitForOnTypeReady(doc, "<section>", "</section>");

    const inserted = await runFormatOnTypeAfter(doc, "<section>");
    expect(inserted, "typing the `>` of <section> must insert its closing tag").to.equal(
      "</section>",
    );
  });

  test("does NOT close a void <br> element in a .vue file (ready + no edit)", async function () {
    // Two tags in the template: a positive control <article> and the void <br>
    // under test. Readiness is proven by the control, so the empty result for
    // <br> is provably "ready + correctly no edit", not "provider not ready".
    const { doc, fsPath } = await openScratch(
      "__autoclose_void.vue",
      "<template><article><br>\n</template>\n",
    );
    scratchFiles.push(fsPath);

    await waitForOnTypeReady(doc, "<article>", "</article>");

    const inserted = await runFormatOnTypeAfter(doc, "<br>");
    expect(inserted, "void elements must not be auto-closed (provider was proven ready)").to.equal(
      "",
    );
  });

  test("does NOT close a TS generic `>` inside <script lang=ts> of a .vue file", async function () {
    // BLOCKER: with `editor.formatOnType` on, typing the `>` of `Box<Foo>` in
    // the script block must NOT insert `</Foo>`. The template control proves the
    // provider is ready, so the empty script result is the gate working, not a
    // cold provider.
    const { doc, fsPath } = await openScratch(
      "__autoclose_script_generic.vue",
      '<template><article>\n</template>\n<script lang="ts">\nconst x: Box<Foo> = mk();\n</script>\n',
    );
    scratchFiles.push(fsPath);

    await waitForOnTypeReady(doc, "<article>", "</article>");

    const inserted = await runFormatOnTypeAfter(doc, "Box<Foo>");
    expect(inserted, "a TS-generic `>` inside <script lang=ts> must never be auto-closed").to.equal(
      "",
    );
  });

  test("inserts </section> for a .svelte carrier (parity with vue)", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    svelte fixtures only in single-project — N/A");
      return;
    }
    const { doc, fsPath } = await openScratch(
      "__autoclose_scratch.svelte",
      "<section>\n<p>hi</p>\n",
    );
    scratchFiles.push(fsPath);

    await waitForOnTypeReady(doc, "<section>", "</section>");

    const inserted = await runFormatOnTypeAfter(doc, "<section>");
    expect(inserted, "a .svelte carrier must auto-close tags identically to .vue").to.equal(
      "</section>",
    );
  });

  test("does NOT close a TS generic `>` inside <script> of a .svelte carrier", async function () {
    if (FIXTURE_NAME !== "single-project") {
      console.log("    svelte fixtures only in single-project — N/A");
      return;
    }
    const { doc, fsPath } = await openScratch(
      "__autoclose_svelte_script.svelte",
      '<script lang="ts">\nconst x: Box<Foo> = mk();\n</script>\n<section>\n<p>hi</p>\n',
    );
    scratchFiles.push(fsPath);

    // Root markup control proves readiness for the svelte carrier.
    await waitForOnTypeReady(doc, "<section>", "</section>");

    const inserted = await runFormatOnTypeAfter(doc, "Box<Foo>");
    expect(
      inserted,
      "a TS-generic `>` inside a Svelte <script> must never be auto-closed",
    ).to.equal("");
  });
});
