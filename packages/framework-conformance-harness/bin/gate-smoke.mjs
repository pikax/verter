#!/usr/bin/env node
// Canonical, harness-owned preflight entry point for the Rust gate. Each mode
// calls the same exported mechanism the conformance tests use; this file owns
// only the tiny assertion and receipt boundary around that real work.

import assert from "node:assert/strict";

import { ensureVaporRuntimePreloaded } from "../src/execute-vue-vapor.mjs";
import { observeTypeScript } from "../src/typescript-observe.mjs";

const RECEIPT_SCHEMA = "verter-harness-smoke/v1";

async function smokeVapor() {
  await ensureVaporRuntimePreloaded();
}

function smokeTypeScript() {
  const observation = observeTypeScript(
    [
      {
        fileName: "/shared.ts",
        code: 'export interface SmokeValue { value: "ready" }\n',
      },
      {
        fileName: "/entry.ts",
        code:
          'import type { SmokeValue } from "./shared.js";\n' +
          'export const smokeValue: SmokeValue = { value: "ready" };\n',
      },
    ],
    { frameworkDomain: "workspace" },
  );
  assert.deepEqual(observation.diagnostics, []);
  assert.equal(observation.observationDomain.framework, "workspace");
  const exported = observation.modules["/entry.ts"]?.exports?.smokeValue;
  assert.ok(exported, "workspace observation did not expose smokeValue");
  assert.equal(exported.type.members?.value?.display, '"ready"');
}

const mode = process.argv[2];
if (process.argv.length !== 3 || (mode !== "vapor" && mode !== "typescript")) {
  process.stderr.write(`unknown harness smoke mode: ${JSON.stringify(mode)}\n`);
  process.exit(2);
}

try {
  if (mode === "vapor") await smokeVapor();
  else smokeTypeScript();
  process.stdout.write(JSON.stringify({ schema: RECEIPT_SCHEMA, mode, ok: true }));
} catch (error) {
  process.stderr.write(`harness smoke ${mode} failed: ${String(error?.stack ?? error)}\n`);
  process.exit(3);
}
