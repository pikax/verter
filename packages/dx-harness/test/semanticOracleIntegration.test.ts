// End-to-end semantic-oracle runner against the REAL binaries.
//
// The oracle compares verter-on-`.vue` (DX_LSP_BIN) against tsgo/tsserver on the
// hand-authored `.ts` gold standard (DX_BASELINE_BIN). Each side is gated on its
// own binary so the default `pnpm test` stays hermetic; the full runner needs both.
//   cargo build -p verter_dx_baseline       # target/debug/verter-dx-baseline
//   cargo build -p verter_lsp                # target/debug/verter-lsp
//   DX_BASELINE_BIN=$PWD/target/debug/verter-dx-baseline \
//   DX_LSP_BIN=$PWD/target/debug/verter-lsp pnpm -C packages/dx-harness test
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { LspClient } from "@verter/lsp-test-client";
import { afterEach, describe, expect, it } from "vitest";

import { BridgeClient } from "../src/baseline/bridgeClient.js";
import { awaitRawLspStartup } from "../src/core/startupGate.js";
import { bridgeHoverFact, prepareOracleSource } from "../src/semantic-oracle/index.js";
import {
  runResolvedOracleQuery,
  type OracleVerterClient,
  type ResolvedOracleQuery,
} from "../src/semantic-oracle/runner.js";
import type { Probe } from "../src/scenario/index.js";

const BASELINE_BIN = process.env.DX_BASELINE_BIN;
const LSP_BIN = process.env.DX_LSP_BIN;
const PROVIDER = process.env.DX_LSP_PROVIDER ?? "tsgo";

const tmps: string[] = [];
const bridges: BridgeClient[] = [];
const clients: LspClient[] = [];
afterEach(async () => {
  for (const b of bridges.splice(0)) await b.dispose();
  for (const c of clients.splice(0)) await c.kill();
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

// A minimal oracle `.ts` resolvable with the provider's default lib (no DOM types).
const ORACLE_TS = "const count: number = 1\ncount // @dx-anchor count\n";

function hoverProbe(): Probe {
  return {
    id: "count.hover",
    method: "hover",
    anchor: "count",
    mappingPolicy: "none",
    confidence: "high",
    dimension: "vueSemanticValidity",
    requiresSourceMap: false,
    requiredDrivers: ["rawLsp", "tsgo"],
    capabilityRequirements: [],
  };
}

describe.skipIf(!BASELINE_BIN)("semantic oracle — real bridge (`.ts` gold standard)", () => {
  it("answers a hover on the oracle `.ts` anchor and folds it to a NormalizedHover", async () => {
    const root = mkdtempSync(join(tmpdir(), "dx-oracle-bridge-"));
    tmps.push(root);
    const oraclePath = join(root, "oracle.ts");
    writeFileSync(oraclePath, ORACLE_TS);
    const prepared = prepareOracleSource(ORACLE_TS);

    const bridge = new BridgeClient(BASELINE_BIN!);
    bridges.push(bridge);
    const hello = await bridge.hello({
      workspaceRoot: root,
      repoRoot: process.cwd(),
      provider: "tsgo",
      strictCi: false,
      toolRoot: {},
    });
    if (hello.type !== "hello") throw new Error("expected hello");
    if (hello.skipped) {
      // No provider available in this environment — record the reason, do not fail.
      expect(typeof hello.skipReason).toBe("string");
      return;
    }

    await bridge.open([{ path: oraclePath, content: prepared.stripped, role: "entry" }], 1);
    const offset = prepared.byteOffsets.get("count")!;
    const response = await bridge.query({
      method: "hover",
      uri: pathToFileURL(oraclePath).toString(),
      path: oraclePath,
      offset,
      version: 1,
    });
    const fact = bridgeHoverFact(response);
    expect(fact.ok).toBe(true);
    if (!fact.ok) throw new Error("expected ok");
    // tsgo's hover on `count` mentions the `number` type the oracle pins.
    expect(fact.output?.contents ?? "").toContain("number");
  });
});

describe.skipIf(!BASELINE_BIN || !LSP_BIN)(
  "semantic oracle — full runner (verter `.vue` vs `.ts` gold standard)",
  () => {
    it("drives both binaries through runResolvedOracleQuery into a vueSemanticValidity outcome", async () => {
      // verter workspace.
      const wsRoot = mkdtempSync(join(tmpdir(), "dx-oracle-ws-"));
      tmps.push(wsRoot);
      const vuePath = join(wsRoot, "Widget.vue");
      const vueText =
        '<script setup lang="ts">\nconst count = 1\ncount\n</script>\n<template>{{ count }}</template>\n';
      writeFileSync(vuePath, vueText);
      const vueUri = pathToFileURL(vuePath).toString();
      const rootUri = pathToFileURL(wsRoot).toString();

      const verter = new LspClient("verter-lsp", LSP_BIN!, [wsRoot, `--type-provider=${PROVIDER}`]);
      clients.push(verter);
      await verter.initialize(
        {
          processId: process.pid,
          capabilities: { workspace: { workspaceFolders: true } },
          rootUri,
          workspaceFolders: [{ uri: rootUri, name: "dx-oracle" }],
        },
        30_000,
      );
      const startup = awaitRawLspStartup(verter, {
        readyTimeoutMs: 120_000,
        quiescence: { timeoutMs: 30_000 },
      });
      verter.sendNotification("initialized", {});
      await startup;
      verter.sendNotification("textDocument/didOpen", {
        textDocument: { uri: vueUri, languageId: "vue", version: 1, text: vueText },
      });

      // oracle bridge.
      const oracleRoot = mkdtempSync(join(tmpdir(), "dx-oracle-ts-"));
      tmps.push(oracleRoot);
      const oraclePath = join(oracleRoot, "oracle.ts");
      const prepared = prepareOracleSource(ORACLE_TS);
      writeFileSync(oraclePath, prepared.stripped);

      const bridge = new BridgeClient(BASELINE_BIN!);
      bridges.push(bridge);
      const hello = await bridge.hello({
        workspaceRoot: oracleRoot,
        repoRoot: process.cwd(),
        provider: "tsgo",
        strictCi: false,
        toolRoot: {},
      });
      if (hello.type !== "hello") throw new Error("expected hello");
      if (hello.skipped) {
        expect(typeof hello.skipReason).toBe("string");
        return;
      }
      await bridge.open([{ path: oraclePath, content: prepared.stripped, role: "entry" }], 1);

      const resolved: ResolvedOracleQuery = {
        probe: hoverProbe(),
        binding: { probeId: "count.hover", oracleAnchor: "count", requiredSnippets: ["number"] },
        // verter answers `count` on the `.vue` script line (0-based line 2).
        vue: { uri: vueUri, position: { line: 2, character: 0 } },
        oracle: {
          uri: pathToFileURL(oraclePath).toString(),
          path: oraclePath,
          version: 1,
          offset: prepared.byteOffsets.get("count")!,
        },
      };

      const outcomes = await runResolvedOracleQuery(resolved, {
        verter: verter as unknown as OracleVerterClient,
        tsgo: bridge,
      });

      expect(outcomes.length).toBeGreaterThan(0);
      // Whatever the verdict, it is a well-formed vueSemanticValidity outcome the
      // report layer can consume — never a thrown failure or a missing dimension.
      for (const outcome of outcomes) {
        expect(outcome.probe.dimension).toBe("vueSemanticValidity");
      }
    });
  },
);
