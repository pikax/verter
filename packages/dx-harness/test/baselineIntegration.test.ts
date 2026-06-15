// End-to-end integration against the REAL `verter-dx-baseline` binary.
//
// Gated on DX_BASELINE_BIN (an absolute path to the built binary) so the default
// `pnpm test` stays hermetic and needs no cargo build. Build it with
//   cargo build -p verter_dx_baseline
// then run, e.g.:
//   DX_BASELINE_BIN=$PWD/target/debug/verter-dx-baseline pnpm -C packages/dx-harness test
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { BridgeClient } from "../src/baseline/bridgeClient.js";
import { runMaterialize, buildMaterializeRequest } from "../src/baseline/materializeClient.js";
import {
  createMaterializedWorkspace,
  disposeMaterializedWorkspace,
} from "../src/materializedWorkspace.js";

const BIN = process.env.DX_BASELINE_BIN;
const tmps: string[] = [];
const clients: BridgeClient[] = [];
afterEach(async () => {
  for (const c of clients.splice(0)) await c.dispose();
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

function fixtureDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "dx-int-fixture-"));
  tmps.push(dir);
  writeFileSync(
    join(dir, "Widget.vue"),
    '<script setup lang="ts">\n// @dx-anchor n\nconst count = 1\n</script>\n<template>{{ count }}</template>\n',
  );
  return dir;
}

describe.skipIf(!BIN)("verter-dx-baseline materialize (real binary)", () => {
  it("materializes a workspace and emits an IDE artifact with an authoritative map", async () => {
    const ws = await createMaterializedWorkspace({
      fixtureDir: fixtureDir(),
      repoRoot: process.cwd(),
      materialize: (req) => runMaterialize(BIN!, req),
    });
    tmps.push(ws.root);

    expect(ws.materializeReport.compileErrors).toEqual([]);
    const ide = ws.materializeReport.ideArtifacts.find((a) =>
      a.generatedPath.endsWith("Widget.vue.tsx"),
    );
    expect(ide, "an IDE artifact for Widget.vue.tsx").toBeDefined();
    // C produced a source map and B carries it verbatim.
    expect(ide!.sourceMapPresent).toBe(true);
    expect(typeof ide!.sourceMap).toBe("string");
    // The anchor survived materialization into the merged map.
    expect(ws.anchorMap.has("n")).toBe(true);

    disposeMaterializedWorkspace(ws);
  });

  it("buildMaterializeRequest + runMaterialize produce a report from the real one-shot", async () => {
    const dir = fixtureDir();
    const root = mkdtempSync(join(tmpdir(), "dx-int-root-"));
    tmps.push(root);
    // Place the stripped source directly (the one-shot over-materializes the root).
    writeFileSync(join(root, "Widget.vue"), "<template><div/></template>\n");
    const report = await runMaterialize(
      BIN!,
      buildMaterializeRequest({ workspaceRoot: root, entries: [join(root, "Widget.vue")] }),
    );
    expect(report.ideArtifacts.length).toBeGreaterThan(0);
    void dir;
  });
});

describe.skipIf(!BIN)("verter-dx-baseline bridge (real binary)", () => {
  it("handshakes (ready or gracefully skipped) and reports a probe count on shutdown", async () => {
    const client = new BridgeClient(BIN!);
    clients.push(client);
    const hello = await client.hello({
      workspaceRoot: process.cwd(),
      repoRoot: process.cwd(),
      provider: "tsgo",
      strictCi: false,
      toolRoot: {},
    });
    expect(hello.type).toBe("hello");
    if (hello.type !== "hello") throw new Error("expected hello");
    // Non-strict: either a real provider spawned, or it skipped with a reason.
    expect(hello.ok).toBe(true);
    if (hello.skipped) {
      expect(typeof hello.skipReason).toBe("string");
    } else {
      expect(hello.capabilities?.positionEncoding).toBe("utf-8");
    }

    const bye = await client.shutdown();
    expect(bye.type).toBe("shutdown");
    if (bye.type !== "shutdown") throw new Error("expected shutdown");
    expect(typeof bye.baselineRan).toBe("number");
  });
});
