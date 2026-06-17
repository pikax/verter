/**
 * DX harness verter raw-LSP spine smoke.
 *
 * The verter half of the required raw-LSP hermetic DX job. The workflow builds
 * `verter-lsp` and `verter-dx-baseline`; this gate DRIVES the REAL `verter-lsp`
 * binary over the committed, vendored hermetic corpus to prove the raw-LSP SPINE
 * is healthy and NON-VACUOUS:
 *
 *   1. verter-lsp starts, reaches readiness (a matched `$/verter/ready` +
 *      `$/verter/typeProviderSyncComplete` generation), and the host quiesces —
 *      the `awaitRawLspStartup` gate. A server that never starts, or whose type
 *      provider never syncs, FAILS here.
 *   2. verter-lsp RESPONDS to hover, definition, AND completion over the corpus
 *      entry. A request that throws or times out — a hung or dead spine — FAILS.
 *   3. verter produced REAL output: at least one signal is contentful (hover text,
 *      a definition target, or a completion item). An all-empty spine "responded
 *      with nothing" and FAILS.
 *
 * What it deliberately does NOT do: enforce verter-vs-tsgo type PARITY. verter has
 * known, expected divergences (macro-binding hovers landing on `any`, …); a parity
 * gate here would make CI permanently red. This smoke RECORDS each response and
 * asserts only the spine + non-vacuity, never the type content. The full
 * record-and-diff sweep over the whole corpus is the scheduled `dx-extended.yml`
 * job, not this required PR gate.
 *
 * Hermetic: the provider is the repo-pinned `tsserver` (via `--tsdk`, the SAME pin
 * the strict baseline gate validates lives under the repo), the corpus is the
 * committed vendored `minimal-member-access` scenario — a self-contained
 * `<script setup>` SFC with NO Vue macros, so it carries no macro-binding
 * divergence surface — and the vendored Vue shims stand in for an install. No
 * network, no tsgo download.
 *
 * Inputs (env):
 *   - DX_LSP_BIN       (required) absolute path to the built `verter-lsp`.
 *   - DX_BASELINE_BIN  (required) absolute path to the built `verter-dx-baseline`
 *                      (materializes the corpus workspace).
 *   - DX_REPO_ROOT     (optional) repo root; defaults to this script's repo root.
 *   - DX_FIXTURE_DIR   (optional) corpus dir; defaults to the committed vendored
 *                      `minimal-member-access` hermetic scenario.
 *   - DX_SMOKE_OUT     (optional) dir for the response record; defaults to an OS
 *                      temp dir, so a default run leaves NO artifact in the repo.
 */

import { existsSync, mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HARNESS_DIST = new URL("../../packages/dx-harness/dist/index.js", import.meta.url);

const {
  canonicalizePath,
  resolveToolRoots,
  createMaterializedWorkspace,
  disposeMaterializedWorkspace,
  awaitRawLspStartup,
  extractQuiescenceCounters,
  pollUntilQuiesced,
  GET_STATISTICS_METHOD,
} = await import(HARNESS_DIST.href);

// `LspClient` lives in the sibling `@verter/lsp-test-client` package (a direct
// dependency of `@verter/dx-harness`); resolve it through the harness's own
// dependency tree so this script needs no hoisting assumptions.
const harnessRequire = createRequire(
  fileURLToPath(new URL("../../packages/dx-harness/package.json", import.meta.url)),
);
const { LspClient } = await import(
  pathToFileURL(harnessRequire.resolve("@verter/lsp-test-client")).href
);

const failures = [];
function check(ok, message) {
  if (!ok) failures.push(message);
  return ok;
}
function fatal(message) {
  console.error(`::error::DX raw-LSP smoke: ${message}`);
  process.exit(1);
}

const repoRoot = canonicalizePath(
  process.env.DX_REPO_ROOT ?? fileURLToPath(new URL("../../", import.meta.url)),
).replace(/\/+$/, "");
const lspBin = process.env.DX_LSP_BIN;
const baselineBin = process.env.DX_BASELINE_BIN;
if (!lspBin) fatal("DX_LSP_BIN is required (absolute path to the built verter-lsp binary)");
if (!existsSync(lspBin)) fatal(`DX_LSP_BIN does not exist: ${lspBin}`);
if (!baselineBin) {
  fatal("DX_BASELINE_BIN is required (absolute path to the built verter-dx-baseline binary)");
}
if (!existsSync(baselineBin)) fatal(`DX_BASELINE_BIN does not exist: ${baselineBin}`);

// The committed, vendored hermetic corpus the smoke drives verter-lsp over. The
// default is the self-contained `minimal-member-access` SFC: a typed `<script
// setup>` const with a member access — hover/definition/completion all resolve
// against the vendored Vue shims with no fixture install, and there are no Vue
// macros, so the known macro-binding divergences never enter this gate.
const fixtureDir =
  process.env.DX_FIXTURE_DIR ??
  path.join(repoRoot, "packages", "dx-harness", "fixtures", "hermetic", "minimal-member-access");
if (!existsSync(fixtureDir)) fatal(`hermetic corpus fixture missing: ${fixtureDir}`);

const REQUEST_TIMEOUT_MS = 30_000;

// ── LSP-response extractors (verter-side, source-space) ──────────────────────

function hoverText(result) {
  if (result === null || typeof result !== "object") return "";
  const contents = result.contents;
  if (contents === undefined || contents === null) return "";
  if (typeof contents === "string") return contents;
  if (Array.isArray(contents)) {
    return contents.map((c) => (typeof c === "string" ? c : String(c?.value ?? ""))).join("\n");
  }
  if (typeof contents === "object" && "value" in contents) return String(contents.value);
  return "";
}

function definitionTargets(result) {
  if (result === null || result === undefined) return [];
  const arr = Array.isArray(result) ? result : [result];
  const out = [];
  for (const raw of arr) {
    if (raw === null || typeof raw !== "object") continue;
    const uri = raw.targetUri ?? raw.uri;
    const range = raw.targetSelectionRange ?? raw.targetRange ?? raw.range;
    if (typeof uri === "string" && range?.start) {
      out.push({ uri, line: range.start.line ?? 0, character: range.start.character ?? 0 });
    }
  }
  return out;
}

function completionLabels(result) {
  if (result === null || result === undefined) return [];
  const items = Array.isArray(result) ? result : (result.items ?? []);
  return items
    .map((i) => (typeof i?.label === "string" ? i.label : ""))
    .filter((s) => s.length > 0);
}

// ── position helpers (the vendored corpus is all-ASCII: offset === character) ──

function offsetToPos(text, offset) {
  let line = 0;
  let lineStart = 0;
  for (let i = 0; i < offset && i < text.length; i++) {
    if (text[i] === "\n") {
      line += 1;
      lineStart = i + 1;
    }
  }
  return { line, character: offset - lineStart };
}

/** Position at `token` inside the first occurrence of `lineFragment`. */
function posInFragment(text, lineFragment, token) {
  const base = text.indexOf(lineFragment);
  if (base < 0) return null;
  const rel = lineFragment.indexOf(token);
  if (rel < 0) return null;
  return offsetToPos(text, base + rel);
}

/**
 * Resolve the source position at `token` inside `lineFragment`, REQUIRING the
 * anchor to exist in the vendored corpus. On fixture drift — the committed entry
 * no longer contains the fragment — this FAILS LOUDLY (naming the missing fragment
 * + fixture) instead of silently probing offset 0:0, which would let a hover or
 * definition probe land on `0:0` and pass vacuously.
 */
function requireAnchorPos(text, lineFragment, token, entryRel) {
  const pos = posInFragment(text, lineFragment, token);
  if (pos === null) {
    fatal(
      `anchor not found in hermetic fixture — expected fragment '${lineFragment}' ` +
        `(token '${token}') in entry '${entryRel}' under '${path.basename(fixtureDir)}'. ` +
        "Fixture drift: update the smoke anchors or the vendored corpus instead of probing 0:0.",
    );
  }
  return pos;
}

// ── verter driver bootstrap ──────────────────────────────────────────────────

async function startVerter(root, tsdk) {
  const rootUri = pathToFileURL(root).toString();
  // Pinned tsserver via `--tsdk` — the same repo-pinned TypeScript the strict
  // baseline gate enforces; fully hermetic, no tsgo download.
  const client = new LspClient("verter-lsp", lspBin, [
    root,
    "--type-provider=tsserver",
    `--tsdk=${tsdk}`,
  ]);
  await client.initialize(
    {
      processId: process.pid,
      capabilities: { workspace: { workspaceFolders: true } },
      rootUri,
      workspaceFolders: [{ uri: rootUri, name: "dx-smoke" }],
    },
    30_000,
  );
  const startup = awaitRawLspStartup(client, {
    readyTimeoutMs: 120_000,
    quiescence: { timeoutMs: 30_000 },
  });
  client.sendNotification("initialized", {});
  await startup;
  return client;
}

function quiescer(client) {
  // No warn-drain feed here (the drainer helper is not on the package barrel); the
  // settle rests purely on host-counter stability, which is enough for a smoke.
  const noWarns = () => [];
  return async () => {
    const result = await pollUntilQuiesced(
      async () =>
        extractQuiescenceCounters(await client.sendRequest(GET_STATISTICS_METHOD, {}, 10_000)),
      noWarns,
      { timeoutMs: 10_000 },
    );
    return result.quiesced;
  };
}

// ── the smoke ────────────────────────────────────────────────────────────────

async function main() {
  const toolRoots = resolveToolRoots(repoRoot);
  const tsdk = toolRoots.tsserverTsdk;
  check(!!tsdk && existsSync(tsdk), `pinned tsserver tsdk missing: ${tsdk}`);
  check(
    !!toolRoots.expectedTsserverJs && existsSync(toolRoots.expectedTsserverJs),
    `pinned tsserver.js missing: ${toolRoots.expectedTsserverJs}`,
  );
  if (failures.length > 0) {
    for (const f of failures) console.error(`::error::DX raw-LSP smoke: ${f}`);
    fatal(`${failures.length} precondition(s) failed`);
  }

  // Materialize the vendored corpus (real tsconfig + vendored Vue shims). Strict:
  // a compile error on the VALID hermetic corpus is fatal here, never tolerated.
  const ws = await createMaterializedWorkspace({
    fixtureDir,
    repoRoot,
    typeProvider: "tsserver",
    baselineBin,
    // Strict-by-default: vendored-Vue drift hard-fails materialization.
  });

  const signals = { hover: null, definition: null, completion: null };
  let startupOk = false;
  let client = null;
  try {
    check(
      ws.materializeReport.compileErrors.length === 0,
      `materialize reported compile errors on the valid hermetic corpus: ${JSON.stringify(
        ws.materializeReport.compileErrors,
      )}`,
    );
    const entryRel = ws.sourceFiles.find((f) => f.endsWith(".vue"));
    if (!check(!!entryRel, "vendored corpus has no .vue entry to drive")) {
      fatal("no .vue entry in the materialized corpus");
    }
    const entryPath = path.join(ws.root, entryRel);
    const entryUri = pathToFileURL(entryPath).toString();
    const entryText = readFileSync(entryPath, "utf-8");

    // 1. Spine health: verter-lsp starts, reaches readiness, host quiesces.
    client = await startVerter(ws.root, tsdk);
    startupOk = true;

    // Open the SOURCE .vue — verter-lsp compiles it internally (IDE codegen path)
    // and the pinned tsserver type-checks the generated TSX.
    client.sendNotification("textDocument/didOpen", {
      textDocument: { uri: entryUri, languageId: "vue", version: 1, text: entryText },
    });
    await new Promise((r) => setTimeout(r, 250));

    // 2a. Hover on the resolved `.label` member of `item.label`.
    const hoverPos = requireAnchorPos(entryText, "item.label", "label", entryRel);
    signals.hover = await driveSignal("hover", () =>
      client.sendRequest(
        "textDocument/hover",
        { textDocument: { uri: entryUri }, position: hoverPos },
        REQUEST_TIMEOUT_MS,
      ),
    );

    // 2b. Definition on the `item` receiver — resolves the `const item` decl.
    const defPos = requireAnchorPos(entryText, "item.label", "item", entryRel);
    signals.definition = await driveSignal("definition", () =>
      client.sendRequest(
        "textDocument/definition",
        { textDocument: { uri: entryUri }, position: defPos },
        REQUEST_TIMEOUT_MS,
      ),
    );

    // 2c. Completion: type `.` after a typed receiver and read the member set.
    signals.completion = await driveCompletion(client, entryUri, entryText);
  } finally {
    if (client) await client.kill().catch(() => {});
    disposeMaterializedWorkspace(ws);
  }

  // ── assertions ──────────────────────────────────────────────────────────────
  check(startupOk, "verter-lsp did not reach raw-LSP startup readiness");
  // The spine must RESPOND to every signal — a throw/timeout is a hung/dead spine.
  for (const name of ["hover", "definition", "completion"]) {
    const s = signals[name];
    check(
      !!s && s.responded,
      `verter-lsp did not respond to ${name}: ${s ? s.error : "no result"}`,
    );
  }
  // Non-vacuity: verter produced REAL output on at least one signal. An all-empty
  // spine "responded with nothing" and fails — this is NOT a type-parity check.
  const contentful = ["hover", "definition", "completion"].filter((n) => signals[n]?.contentful);
  check(
    contentful.length > 0,
    "verter-lsp responded with NOTHING on every signal (no hover content, no definition target, no completion item)",
  );

  // Record the run (never asserted for parity) outside the repo by default.
  const outDir = process.env.DX_SMOKE_OUT ?? mkdtempSync(path.join(tmpdir(), "dx-smoke-"));
  mkdirSync(outDir, { recursive: true });
  const record = {
    provider: "tsserver",
    fixture: path.basename(fixtureDir),
    startupOk,
    contentfulSignals: contentful,
    signals,
  };
  const outPath = path.join(outDir, "dx-lsp-smoke.json");
  writeFileSync(outPath, `${JSON.stringify(record, null, 2)}\n`, "utf-8");

  if (failures.length > 0) {
    for (const f of failures) console.error(`::error::DX raw-LSP smoke: ${f}`);
    fatal(`${failures.length} check(s) failed`);
  }

  console.log(`raw-LSP spine: started, ready, responded to hover/definition/completion`);
  console.log(`contentful signals: ${contentful.join(", ")}`);
  console.log(`response record: ${outPath}`);
  console.log("DX raw-LSP smoke: PASS");
}

/**
 * Drive one verter request, recording whether it RESPONDED (resolved) and whether
 * the response was CONTENTFUL. A throw/timeout is a spine failure (`responded:
 * false`); the type content is recorded but never asserted for parity.
 */
async function driveSignal(kind, send) {
  try {
    const result = await send();
    let contentful = false;
    let detail = "";
    if (kind === "hover") {
      const t = hoverText(result);
      contentful = t.length > 0;
      detail = t.slice(0, 200);
    } else if (kind === "definition") {
      const t = definitionTargets(result);
      contentful = t.length > 0;
      detail = t.map((x) => `${x.uri}:${x.line + 1}:${x.character + 1}`).join(", ");
    } else {
      const labels = completionLabels(result);
      contentful = labels.length > 0;
      detail = labels.slice(0, 20).join(", ");
    }
    return { responded: true, contentful, detail };
  } catch (err) {
    return { responded: false, contentful: false, error: String(err) };
  }
}

/**
 * Completion is the mutate-then-query signal: re-open the doc, insert a `.` after a
 * typed receiver, settle to host quiescence, then request member completion. Only
 * the RESPONSE is required (the spine must not hang); an empty member set on a
 * cold provider is allowed — non-vacuity is carried by hover/definition.
 */
async function driveCompletion(client, entryUri, entryText) {
  try {
    // Insert `.` after the `item` receiver on the `const sameItem = item` line.
    const fragment = "const sameItem = item";
    const base = entryText.indexOf(fragment);
    if (base < 0) {
      return { responded: false, contentful: false, error: "completion receiver not found" };
    }
    const insertOffset = base + fragment.length; // just after `item`
    const insertPos = offsetToPos(entryText, insertOffset);

    client.sendNotification("textDocument/didChange", {
      textDocument: { uri: entryUri, version: 2 },
      contentChanges: [{ range: { start: insertPos, end: insertPos }, text: "." }],
    });
    // Settle the recompile + provider re-sync before probing.
    await quiescer(client)().catch(() => false);
    await new Promise((r) => setTimeout(r, 250));

    const result = await client.sendRequest(
      "textDocument/completion",
      {
        textDocument: { uri: entryUri },
        position: { line: insertPos.line, character: insertPos.character + 1 },
        context: { triggerKind: 2, triggerCharacter: "." },
      },
      REQUEST_TIMEOUT_MS,
    );
    const labels = completionLabels(result);
    return {
      responded: true,
      contentful: labels.length > 0,
      detail: labels.slice(0, 20).join(", "),
    };
  } catch (err) {
    return { responded: false, contentful: false, error: String(err) };
  }
}

main().catch((err) => fatal(String(err?.stack ?? err)));
