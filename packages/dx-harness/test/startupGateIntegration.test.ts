// End-to-end startup gate against the REAL `verter-lsp` binary.
//
// Gated on DX_LSP_BIN (an absolute path to the built binary) so the default
// `pnpm test` stays hermetic and needs no Rust build. Build it with
//   cargo build -p verter_lsp        # produces target/debug/verter-lsp[.exe]
// then run, e.g.:
//   DX_LSP_BIN=$PWD/target/debug/verter-lsp pnpm -C packages/dx-harness test
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { LspClient } from "@verter/lsp-test-client";
import { afterEach, describe, expect, it } from "vitest";

import { awaitRawLspStartup } from "../src/core/startupGate.js";

const BIN = process.env.DX_LSP_BIN;
const PROVIDER = process.env.DX_LSP_PROVIDER ?? "tsgo";

const tmps: string[] = [];
const clients: LspClient[] = [];
afterEach(async () => {
  for (const c of clients.splice(0)) await c.kill();
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

function workspaceDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "dx-startup-ws-"));
  tmps.push(dir);
  writeFileSync(
    join(dir, "Widget.vue"),
    '<script setup lang="ts">\nconst count = 1\n</script>\n<template>{{ count }}</template>\n',
  );
  return dir;
}

describe.skipIf(!BIN)("verter-lsp startup gate (real binary)", () => {
  it("waits for a matched ready+sync generation and reaches quiescence", async () => {
    const root = workspaceDir();
    const rootUri = pathToFileURL(root).toString();
    const client = new LspClient("verter-lsp", BIN!, [root, `--type-provider=${PROVIDER}`]);
    clients.push(client);

    await client.initialize(
      {
        processId: process.pid,
        capabilities: { workspace: { workspaceFolders: true } },
        rootUri,
        workspaceFolders: [{ uri: rootUri, name: "dx-startup" }],
      },
      30_000,
    );
    // Register the readiness handlers BEFORE `initialized`, so a `ready`/`sync`
    // notification that races the post-init background scan is observed and never
    // missed (the handlers must be live before the server starts publishing).
    const startup = awaitRawLspStartup(client, {
      readyTimeoutMs: 120_000,
      quiescence: { timeoutMs: 30_000 },
    });
    client.sendNotification("initialized", {});

    const result = await startup;

    expect(typeof result.matchedGeneration).toBe("number");
    expect(result.matchedGeneration).toBeGreaterThanOrEqual(0);
    // Both channels reached the SAME generation (the whole point of the gate).
    expect(result.generation.maxReadyGeneration).toBe(result.matchedGeneration);
    expect(result.generation.maxSyncGeneration).toBe(result.matchedGeneration);
    expect(result.quiescence.quiesced).toBe(true);
  });
});
