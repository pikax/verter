/**
 * @ai-generated - Drives an exported editor launch plan through the shared
 * side-effect-free stdio LSP client. The caller must provision the server,
 * fixture dependencies, and plan file explicitly; missing inputs fail closed.
 */
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { LspClient } from "../../packages/lsp-test-client/dist/index.js";

function option(name) {
  const prefix = `--${name}=`;
  const value = process.argv
    .slice(2)
    .find((arg) => arg.startsWith(prefix))
    ?.slice(prefix.length);
  assert.ok(value, `missing required ${prefix}<value>`);
  return value;
}

function markerPosition(text, marker) {
  const offset = text.indexOf(marker);
  assert.notEqual(offset, -1, `marker ${marker} must exist`);
  const before = text.slice(0, offset);
  const lines = before.split(/\r?\n/);
  return { line: lines.length - 1, character: lines.at(-1).length };
}

function hoverText(hover) {
  const contents = hover?.contents;
  if (typeof contents === "string") return contents;
  if (typeof contents?.value === "string") return contents.value;
  if (Array.isArray(contents)) {
    return contents
      .map((entry) => (typeof entry === "string" ? entry : (entry?.value ?? "")))
      .join("\n");
  }
  return "";
}

function isFixtureUri(uri, file) {
  return (
    typeof uri === "string" &&
    decodeURIComponent(uri).replaceAll("\\", "/").toLowerCase().endsWith(`/${file.toLowerCase()}`)
  );
}

function diagnosticCode(diagnostic) {
  return Number(diagnostic?.code?.value ?? diagnostic?.code);
}

function isExpectedMutationDiagnostic(diagnostic, text, marker) {
  const start = markerPosition(text, marker);
  return (
    diagnosticCode(diagnostic) === 2322 &&
    diagnostic?.range?.start?.line === start.line &&
    diagnostic?.range?.start?.character === start.character &&
    diagnostic?.range?.end?.line === start.line &&
    diagnostic?.range?.end?.character === start.character + marker.length &&
    diagnostic?.message?.includes("Type 'number' is not assignable to type 'string'.")
  );
}

function waitForMatchedGeneration(observed, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const poll = () => {
      for (const generation of observed.ready) {
        if (observed.sync.has(generation)) {
          resolve(generation);
          return;
        }
      }
      if (Date.now() >= deadline) {
        reject(
          new Error("timed out waiting for matched $/verter/ready + typeProviderSyncComplete"),
        );
        return;
      }
      setTimeout(poll, 25).unref();
    };
    poll();
  });
}

function waitForProcessExit(client, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const poll = () => {
      if (!client.isAlive()) {
        resolve();
        return;
      }
      if (Date.now() >= deadline) {
        reject(new Error("language server did not exit after the LSP shutdown/exit sequence"));
        return;
      }
      setTimeout(poll, 20);
    };
    poll();
  });
}

const planPath = path.resolve(option("plan"));
const root = path.resolve(option("root"));
const plan = JSON.parse(await readFile(planPath, "utf8"));

assert.equal(typeof plan.editor, "string");
assert.equal(typeof plan.command, "string");
assert.ok(Array.isArray(plan.args));
assert.equal(
  plan.args.filter((arg) => arg === "--type-provider=tsgo").length,
  1,
  "shipping plan must select exactly one tsgo provider",
);
assert.equal(
  path.resolve(plan.args.at(-1)),
  root,
  "shipping plan must keep the workspace root last",
);
assert.deepEqual(
  plan.languages,
  ["vue", "svelte"],
  "shipping plan must select exactly Vue and Svelte",
);

const observed = { ready: new Set(), sync: new Set() };
const latestDiagnostics = new Map();
const client = new LspClient(`${plan.editor}-contract`, plan.command, plan.args, root, {
  defaultTimeout: 30_000,
  stderr: { maxBytes: Number.POSITIVE_INFINITY },
  onAnyNotification(method, params) {
    if (method === "textDocument/publishDiagnostics" && typeof params?.uri === "string") {
      latestDiagnostics.set(params.uri, params.diagnostics ?? []);
    }
    if (process.env.VERTER_EDITOR_CONTRACT_TRACE === "1") {
      console.error(`[${method}] ${JSON.stringify(params)}`);
    }
  },
});

function latestFixtureDiagnostics(file) {
  for (const [uri, diagnostics] of latestDiagnostics) {
    if (isFixtureUri(uri, file)) return diagnostics;
  }
  return [];
}
client.onNotification("$/verter/ready", (params) => observed.ready.add(params?.gen));
client.onNotification("$/verter/typeProviderSyncComplete", (params) =>
  observed.sync.add(params?.gen),
);

try {
  const rootUri = pathToFileURL(root).href;
  const initialized = await client.initialize({
    processId: process.pid,
    rootUri,
    workspaceFolders: [{ uri: rootUri, name: path.basename(root) }],
    capabilities: {
      textDocument: { publishDiagnostics: { relatedInformation: true } },
      workspace: { workspaceFolders: true },
    },
    initializationOptions: plan.initializationOptions,
  });
  assert.ok(
    initialized?.capabilities?.hoverProvider,
    "shipping plan must launch a hover-capable server",
  );
  assert.equal(client.positionEncoding, "utf-8");
  client.sendNotification("initialized", {});
  await waitForMatchedGeneration(observed, 60_000);

  const cases = [
    {
      file: "VueTs.vue",
      languageId: "vue",
      marker: "vueTsTitle",
      invalidLiteral: '"Vue TypeScript"',
    },
    {
      file: "SvelteTs.svelte",
      languageId: "svelte",
      marker: "svelteTsTitle",
      invalidLiteral: '"Svelte TypeScript"',
    },
  ];
  for (const testCase of cases) {
    const file = path.join(root, testCase.file);
    const text = await readFile(file, "utf8");
    const uri = pathToFileURL(file).href;
    client.sendNotification("textDocument/didOpen", {
      textDocument: { uri, languageId: testCase.languageId, version: 1, text },
    });

    const hover = await client.sendRequest("textDocument/hover", {
      textDocument: { uri },
      position: markerPosition(text, testCase.marker),
    });
    const rendered = hoverText(hover);
    assert.match(rendered, /string/);
    assert.doesNotMatch(rendered, /\b(?:any|unknown)\b|__Verter/);

    const invalidText = text.replace(testCase.invalidLiteral, "123");
    assert.notEqual(invalidText, text, `${testCase.file} diagnostic mutation must apply`);
    const invalidDiagnostics = client.waitForNotification(
      "textDocument/publishDiagnostics",
      30_000,
      (params) =>
        isFixtureUri(params?.uri, testCase.file) &&
        params.diagnostics?.some((diagnostic) =>
          isExpectedMutationDiagnostic(diagnostic, invalidText, testCase.marker),
        ),
    );
    client.sendNotification("textDocument/didChange", {
      textDocument: { uri, version: 2 },
      contentChanges: [{ text: invalidText }],
    });
    const invalidPublished = await invalidDiagnostics.catch((error) => {
      throw new Error(
        `${plan.editor}/${testCase.file} did not publish the authored mutation diagnostic; ` +
          `latest=${JSON.stringify(latestFixtureDiagnostics(testCase.file))}: ${error.message}`,
      );
    });
    assert.equal(
      invalidPublished.diagnostics.filter((diagnostic) =>
        isExpectedMutationDiagnostic(diagnostic, invalidText, testCase.marker),
      ).length,
      1,
      `${plan.editor}/${testCase.file} must publish exactly one authored TS2322 for the mutation`,
    );
    assert.equal(
      invalidPublished.diagnostics.some((diagnostic) => diagnosticCode(diagnostic) === 7026),
      false,
      `${plan.editor}/${testCase.file} diagnostic mutation must not surface TS7026`,
    );

    const restoredDiagnostics = client.waitForNotification(
      "textDocument/publishDiagnostics",
      30_000,
      (params) => isFixtureUri(params?.uri, testCase.file) && params.diagnostics?.length === 0,
    );
    client.sendNotification("textDocument/didChange", {
      textDocument: { uri, version: 3 },
      contentChanges: [{ text }],
    });
    const restored = await restoredDiagnostics.catch((error) => {
      throw new Error(
        `${plan.editor}/${testCase.file} did not restore to zero diagnostics; ` +
          `latest=${JSON.stringify(latestFixtureDiagnostics(testCase.file))}: ${error.message}`,
      );
    });
    assert.deepEqual(
      restored.diagnostics,
      [],
      `${plan.editor}/${testCase.file} valid source must restore to zero diagnostics`,
    );
    assert.equal(
      restored.diagnostics.some((diagnostic) => diagnosticCode(diagnostic) === 7026),
      false,
      `${plan.editor}/${testCase.file} must not publish TS7026`,
    );
    client.sendNotification("textDocument/didClose", { textDocument: { uri } });
  }

  await client.sendRequest("shutdown", null, 10_000);
  client.sendNotification("exit");
  await waitForProcessExit(client, 10_000);
  console.log(`${plan.editor}: real shipping-plan LSP smoke passed`);
} finally {
  if (client.isAlive()) await client.kill();
}
