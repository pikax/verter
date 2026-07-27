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
 * Two workspace facts the drive depends on, both enforced as preconditions rather
 * than left to produce a false spine verdict: `@verter/typescript-plugin` must be
 * BUILT (it is what makes the `.vue` carrier a member of its configured tsserver
 * project), and the BASELINE's on-disk generated layer must be PRUNED from the
 * materialized workspace before verter opens it (verter owns the carrier companion
 * namespace and fails closed when a real file occupies it). Either one missing
 * makes every signal come back empty for a reason that has nothing to do with the
 * spine.
 *
 * Inputs (env):
 *   - DX_LSP_BIN       (required) absolute path to the built `verter-lsp`.
 *   - DX_BASELINE_BIN  (required) absolute path to the built `verter-dx-baseline`
 *                      (materializes the corpus workspace).
 *   - DX_REPO_ROOT     (optional) repo root; defaults to this script's repo root.
 *   - DX_PLUGIN_PATH   (optional) tsserver plugin PROBE LOCATION — a directory
 *                      whose `node_modules` holds `@verter/typescript-plugin`;
 *                      defaults to `packages/vue-vscode`.
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
  pruneBaselineGeneratedArtifacts,
  definitionTargets,
  partitionDefinitionTargets,
  requireAnchor,
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

// `@verter/typescript-plugin` is what makes a framework carrier a member of its
// configured tsserver project (`getExternalFiles` + `extraFileExtensions`), so
// without it verter-lsp reaches readiness and answers every carrier request with
// NOTHING — indistinguishable from a broken spine.
//
// The value travels to tsserver as `--pluginProbeLocations <dir>` alongside
// `--globalPlugins @verter/typescript-plugin`, and tsserver resolves the package
// NAME by requiring it out of `<dir>/node_modules`. So the probe location is a
// directory CONTAINING `node_modules/@verter/typescript-plugin` — NOT the
// package's own `dist` output directory, and not the package root.
// `packages/vue-vscode` is that directory: pnpm links the workspace package into
// its `node_modules`, so the DIRECT candidate `<probe>/node_modules/@verter/…`
// exists and tsserver loads it on the first try.
//
// It is deliberately NOT the form the shipped extension uses. The extension passes
// `<extensionPath>/node_modules`, whose direct candidate is
// `node_modules/node_modules/@verter/typescript-plugin` — which does not exist, so
// production resolves the plugin only through Node's ANCESTOR walk. That is a
// latent product dependency on package layout, tracked separately; this gate
// deliberately does not reproduce it, because a gate that relies on the accident
// cannot detect the accident going away.
//
// The check below mirrors what tsserver consumes rather than what this script
// hands it. It deliberately does NOT use a bare `require.resolve`: Node's
// resolver walks ANCESTOR `node_modules`, so a wrong probe location still
// resolves — through pnpm's private `.pnpm/node_modules` hoist dir, an
// unguaranteed layout detail — and the preflight would pass while the probe
// location contributed nothing. Only a DIRECT hit under this probe proves
// tsserver can load the plugin from where it was pointed.
const pluginPath = process.env.DX_PLUGIN_PATH ?? path.join(repoRoot, "packages", "vue-vscode");
const pluginPackageDir = path.join(pluginPath, "node_modules", "@verter", "typescript-plugin");
const pluginEntry = path.join(pluginPackageDir, "dist", "index.js");
if (!existsSync(pluginPackageDir)) {
  fatal(
    `plugin probe location holds no @verter/typescript-plugin: ${pluginPackageDir} does not ` +
      `exist, so tsserver cannot resolve the plugin from ${pluginPath}. Run: pnpm install`,
  );
}
if (!existsSync(pluginEntry)) {
  fatal(
    `@verter/typescript-plugin build is missing its entry: ${pluginEntry}. ` +
      "Without the plugin, tsserver never owns the .vue carrier and every provider signal is " +
      "empty. Produce it with: pnpm --filter @verter/language-shared --filter " +
      "@verter/typescript-plugin build",
  );
}

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

function completionLabels(result) {
  if (result === null || result === undefined) return [];
  const items = Array.isArray(result) ? result : (result.items ?? []);
  return items
    .map((i) => (typeof i?.label === "string" ? i.label : ""))
    .filter((s) => s.length > 0);
}

// ── position helpers (the vendored corpus is all-ASCII: offset === character) ──

/**
 * Resolve the probe position for `token` on the line the named `@dx-anchor`
 * marks, REQUIRING both to exist.
 *
 * The fixture's authored `@dx-anchor` comments are the designed probe mechanism:
 * the materializer strips them and records each one's post-strip `{ line,
 * character }` on `ws.anchorMap`. A code-trailing anchor (`const x = a.b; //
 * @dx-anchor id`) records the position where the comment WAS — end of the code —
 * so this resolves the anchored LINE and then locates `token` within it.
 *
 * Searching the whole document for a token instead would silently match the
 * FIRST textual occurrence, which in a documented fixture is routinely a mention
 * inside a preceding comment: a hover probing a comment resolves nothing and the
 * "spine responded with no content" verdict becomes an artifact of the probe, not
 * of the server. Both faults FAIL LOUDLY here.
 *
 * `token` is matched as a WHOLE IDENTIFIER and `occurrence` selects which one on
 * the line. A plain `indexOf` is the same defect one level down: on
 * `const itemLabel = item.label;` it resolves `"item"` at index 6 — inside
 * `itemLabel` — so a probe documented as "the `item` receiver" silently becomes
 * "the `itemLabel` binding", and its definition lands on that binding's own
 * declaration while looking perfectly contentful. Identifier boundaries exclude
 * the substring match; the explicit occurrence makes "which one" a stated
 * choice rather than whichever comes first.
 */
function requireAnchoredTokenPos(ws, entryText, anchorName, token, entryRel, occurrence = 0) {
  const anchors = new Map(ws.anchorMap);
  let anchor;
  try {
    anchor = requireAnchor(anchors, anchorName);
  } catch (err) {
    return fatal(
      `anchor '${anchorName}' is not in the hermetic fixture '${path.basename(fixtureDir)}': ` +
        `${String(err)}. Fixture drift: re-anchor the corpus or update the smoke.`,
    );
  }
  if (anchor.file !== entryRel) {
    return fatal(
      `anchor '${anchorName}' resolves to '${anchor.file}', not the driven entry '${entryRel}'.`,
    );
  }
  const lines = entryText.split("\n");
  const lineText = lines[anchor.line];
  if (lineText === undefined) {
    return fatal(
      `anchor '${anchorName}' names line ${anchor.line}, past the end of '${entryRel}'.`,
    );
  }
  // Whole-identifier matches only: `\w` on either side means the hit is part of a
  // longer name (`item` inside `itemLabel`) and is not the token asked for.
  const isIdentChar = (ch) => ch !== undefined && /[\w$]/.test(ch);
  const columns = [];
  for (let at = lineText.indexOf(token); at >= 0; at = lineText.indexOf(token, at + 1)) {
    if (isIdentChar(lineText[at - 1]) || isIdentChar(lineText[at + token.length])) continue;
    columns.push(at);
  }
  if (columns.length === 0) {
    return fatal(
      `token '${token}' does not occur as a whole identifier on the line anchored by ` +
        `'${anchorName}' (${entryRel}:${anchor.line}: ${JSON.stringify(lineText)}). Fixture ` +
        "drift: update the smoke anchors or the vendored corpus instead of probing an " +
        "unrelated position.",
    );
  }
  if (occurrence >= columns.length) {
    return fatal(
      `token '${token}' occurs ${columns.length} time(s) as a whole identifier on the line ` +
        `anchored by '${anchorName}', but occurrence ${occurrence} was requested ` +
        `(${entryRel}:${anchor.line}: ${JSON.stringify(lineText)}).`,
    );
  }
  return { line: anchor.line, character: columns[occurrence] };
}

// ── verter driver bootstrap ──────────────────────────────────────────────────

async function startVerter(root, tsdk) {
  const rootUri = pathToFileURL(root).toString();
  // Pinned tsserver via `--tsdk` — the same repo-pinned TypeScript the strict
  // baseline gate enforces; fully hermetic, no tsgo download. `--plugin-path`
  // loads `@verter/typescript-plugin` as a tsserver global language-service
  // plugin, which is what makes the `.vue` carrier a member of its configured
  // project; without it every carrier signal resolves empty.
  const client = new LspClient("verter-lsp", lspBin, [
    root,
    "--type-provider=tsserver",
    `--tsdk=${tsdk}`,
    `--plugin-path=${pluginPath}`,
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
  let prunedArtifacts = [];
  let declAnchorLine = null;
  let resolvedDefinitionLines = [];
  let otherDocumentTargets = [];
  try {
    check(
      ws.materializeReport.compileErrors.length === 0,
      `materialize reported compile errors on the valid hermetic corpus: ${JSON.stringify(
        ws.materializeReport.compileErrors,
      )}`,
    );

    // The materializer writes the BASELINE's generated layer as real files beside
    // the carrier (`App.vue.tsx` entry + `App.vue.ts` twin). verter-lsp owns that
    // companion namespace: a real file at a carrier's companion path is a
    // resolution conflict, so the server fails closed
    // (`CarrierPathOccupiedByRealFile`) and answers every carrier signal empty.
    // Prune the baseline's layer before pointing verter at the workspace. The
    // compile-error check above already consumed what the baseline produced.
    prunedArtifacts = pruneBaselineGeneratedArtifacts(ws);
    check(
      prunedArtifacts.length > 0,
      "materialize emitted no generated artifacts to prune — either the baseline " +
        "stopped writing its companion layer or the prune silently matched nothing; " +
        "verter-lsp would be driven over an unverified workspace either way",
    );
    const entryRel = ws.sourceFiles.find((f) => f.endsWith(".vue"));
    if (failures.length > 0 || !check(!!entryRel, "vendored corpus has no .vue entry to drive")) {
      // A corpus that never satisfied its own preconditions cannot produce a
      // meaningful spine verdict — report the real cause instead of driving verter
      // over it and surfacing a misleading "responded with NOTHING".
      disposeMaterializedWorkspace(ws);
      for (const f of failures) console.error(`::error::DX raw-LSP smoke: ${f}`);
      fatal(`${failures.length} corpus precondition(s) failed`);
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
    // `label` as a whole identifier: the `.label` MEMBER, never the `label` inside
    // the binding name `itemLabel` on the same line.
    const hoverPos = requireAnchoredTokenPos(ws, entryText, "mma.member", "label", entryRel, 0);
    signals.hover = await driveSignal("hover", () =>
      client.sendRequest(
        "textDocument/hover",
        { textDocument: { uri: entryUri }, position: hoverPos },
        REQUEST_TIMEOUT_MS,
      ),
    );

    // 2b. Definition on the `item` receiver — resolves the `const item` decl.
    // `item` as a whole identifier: the RHS RECEIVER of `item.label`. A substring
    // match would land on `itemLabel` and resolve that binding to its own
    // declaration on this very line, proving nothing about cross-statement
    // resolution.
    const defPos = requireAnchoredTokenPos(ws, entryText, "mma.member", "item", entryRel, 0);
    let resolvedTargets = [];
    signals.definition = await driveSignal("definition", async () => {
      const result = await client.sendRequest(
        "textDocument/definition",
        { textDocument: { uri: entryUri }, position: defPos },
        REQUEST_TIMEOUT_MS,
      );
      resolvedTargets = resolvedTargets.concat(definitionTargets(result));
      return result;
    });
    // The definition must reach the `const item` DECLARATION, on its own anchored
    // line. Non-vacuity alone cannot check this: ANY target counts as contentful,
    // so a probe that silently resolved `itemLabel` — the substring match this
    // helper exists to prevent — returns its own declaration on the SAME line as
    // the probe and looks equally healthy. Naming the expected line is what makes
    // the signal prove cross-statement resolution.
    // Partitioned by DOCUMENT before any line comparison: a line number alone would
    // be satisfied by line 12 of some unrelated file, which is not what "resolves
    // the declaration" means. The split is shared harness logic so it can be tested
    // against that near-miss directly — the live server always answers in the right
    // document, so a URI filter cannot be discriminated end-to-end here.
    declAnchorLine = requireAnchoredTokenPos(ws, entryText, "mma.decl", "item", entryRel, 0).line;
    const partitioned = partitionDefinitionTargets(resolvedTargets, entryUri);
    resolvedDefinitionLines = partitioned.inDocument.map((t) => t.line);
    otherDocumentTargets = partitioned.elsewhere.map((t) => `${t.uri}:${t.line + 1}`);

    // 2c. Completion: type `.` after a typed receiver and read the member set.
    signals.completion = await driveCompletion(client, ws, entryUri, entryText, entryRel);
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

  // Definition must reach the RIGHT declaration, not merely some declaration.
  // Non-vacuity accepts ANY target, so a probe that silently resolved the wrong
  // token still looks healthy: `item` matched as a substring lands on `itemLabel`,
  // whose declaration is on the probe's OWN line, and the signal is contentful and
  // wrong. Naming the anchored declaration line is what makes this signal prove
  // cross-statement resolution rather than "something answered".
  check(
    declAnchorLine !== null && resolvedDefinitionLines.includes(declAnchorLine),
    `verter-lsp definition did not resolve the anchored declaration: expected a target in the ` +
      `driven entry at line ` +
      `${declAnchorLine === null ? "<unresolved anchor>" : declAnchorLine + 1} (the 'mma.decl' ` +
      `anchored 'const item' declaration), got ${
        resolvedDefinitionLines.length === 0
          ? "no targets in that document"
          : `line(s) ${resolvedDefinitionLines.map((l) => l + 1).join(", ")}`
      }${
        otherDocumentTargets.length === 0
          ? ""
          : ` (targets in other documents, not accepted: ${otherDocumentTargets.join(", ")})`
      }`,
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
async function driveCompletion(client, ws, entryUri, entryText, entryRel) {
  try {
    // Insert `.` just after the `item` receiver on the anchored incomplete-expression
    // line. Anchor-scoped for the same reason hover/definition are: a whole-document
    // token search would match a mention in a preceding comment.
    const receiverPos = requireAnchoredTokenPos(
      ws,
      entryText,
      "mma.incomplete",
      // The bare `item` reference, not the `item` inside `sameItem`.
      "item",
      entryRel,
      0,
    );
    const insertPos = { line: receiverPos.line, character: receiverPos.character + "item".length };

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
