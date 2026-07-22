import * as assert from "assert";
import * as fs from "fs";
import * as path from "path";

import * as vscode from "vscode";

import { readE2eEnv } from "../../../src/e2eEnv";
import {
  classifyDefinition,
  classifyHoverText,
  classifyMemberCompletion,
  classifyReferences,
  type AnswerVerdict,
  type CompletionItemFacts,
  type HoverProbeContract,
  type ResolutionVerdict,
} from "../tsAnswer";
import {
  isTypeScriptCarrier,
  redactIdentifier,
  redactPath,
  selectProbes,
  type SourceProbe,
} from "../probes";

/**
 * The VS Code acceptance lane.
 *
 * This suite runs inside a real extension host, against a real project supplied
 * at the shell, driving the real built extension and the real `verter-lsp`
 * binary. It answers the product owner's question directly: **does VS Code show
 * the results from the TypeScript server, and at what cost?**
 *
 * Three things make it an acceptance test rather than another harness:
 *
 * 1. Every result is classified by `tsAnswer.ts`, which refuses to credit a
 *    Verter-native hover to the TypeScript engine.
 * 2. The same operations run against a plain `.ts` file in the SAME editor
 *    session, so the reported overhead is a ratio taken under identical
 *    conditions rather than two absolute numbers.
 * 3. It fails closed HONESTLY. A workspace with no TypeScript project must
 *    report provider status `none` with a reason; reporting `none` while
 *    answering, or answering nothing while claiming a connected engine, is an
 *    assertion failure rather than a shrug.
 *
 * ## Who answers which side
 *
 * `--disable-extensions` does NOT unload VS Code's built-in
 * `vscode.typescript-language-features`; a control run with Verter's provider
 * disabled still answered `.ts` hovers with real quickinfo. So the two sides of
 * the ratio have different owners, and that is deliberate:
 *
 * - `.vue` carriers are answered ONLY by Verter — the built-in extension does
 *   not register for that language. This side measures the Verter path.
 * - `.ts` files are answered by the built-in TypeScript extension. This side is
 *   the NATIVE TypeScript yardstick, in the same editor session, on the same
 *   machine, under the same load.
 *
 * `carrierOverPlainTsP50Ratio` is therefore Verter-on-a-carrier over
 * native-TypeScript-on-plain-TypeScript: the overhead the product owner feels.
 * The receipt records the extension inventory so this can be re-checked rather
 * than assumed.
 *
 * Nothing identifying reaches disk: paths and identifiers are digested before
 * they enter the receipt (see `redactPath` / `redactIdentifier`).
 */

const LOG_FILE = readE2eEnv("LOG_FILE") ?? "";
const RECEIPT_FILE = readE2eEnv("ACCEPTANCE_RECEIPT") ?? "";
const CORPUS_LABEL = readE2eEnv("ACCEPTANCE_LABEL") ?? "unlabelled";
const REPEATS = Number(readE2eEnv("ACCEPTANCE_REPEATS") ?? "3");
const CARRIER_FILE_BUDGET = Number(readE2eEnv("ACCEPTANCE_FILES") ?? "3");
const FIRST_HOVER_TIMEOUT_MS = Number(readE2eEnv("ACCEPTANCE_FIRST_HOVER_TIMEOUT_MS") ?? "90000");
/**
 * Set by the provider-disabled control run. It turns "no TypeScript answers"
 * from a permissive outcome into a proof obligation: the run must also have
 * OBSERVED Verter-native hovers, otherwise a session where nothing answered
 * would satisfy the control vacuously.
 */
const EXPECT_NATIVE = readE2eEnv("ACCEPTANCE_EXPECT_NATIVE") === "1";
/** Wall-clock ceiling on the probe sweep so a slow workspace still yields a receipt. */
const SWEEP_BUDGET_MS = Number(readE2eEnv("ACCEPTANCE_SWEEP_BUDGET_MS") ?? "240000");
/** How long to wait for the server to publish a provider status. */
const PROVIDER_STATUS_TIMEOUT_MS = Number(
  readE2eEnv("ACCEPTANCE_PROVIDER_STATUS_TIMEOUT_MS") ?? "60000",
);

type OperationName = "hover" | "definition" | "completion" | "references";
type FileKind = "carrier" | "typescript";

interface Sample {
  readonly fileKind: FileKind;
  readonly operation: OperationName;
  readonly probeClass: string;
  /**
   * Hover yields an `AnswerVerdict` — it is the only operation whose payload
   * can be attributed to the engine. The other three yield a
   * `ResolutionVerdict`, which by construction cannot say `typescript`.
   */
  readonly verdict: AnswerVerdict | ResolutionVerdict;
  readonly reason: string;
  readonly marker?: string;
  readonly latencyMs: number;
  /**
   * 0 is the first touch of that exact position — cold for the engine. Later
   * indices are warm. Both are reported: cold is what the user feels on the
   * first interaction, warm is what they feel for the rest of the session, and
   * collapsing them into one percentile hides whichever one is the problem.
   */
  readonly repeatIndex: number;
}

interface ProviderFacts {
  readonly kind: string;
  readonly reason: string;
}

const samples: Sample[] = [];
const notes: string[] = [];

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function readLog(): string {
  if (!LOG_FILE || !fs.existsSync(LOG_FILE)) return "";
  return fs.readFileSync(LOG_FILE, "utf8");
}

/**
 * The provider status the SERVER published, as the extension logged it.
 *
 * `kind: "none"` with a reason is the honest fail-closed state; `kind: "none"`
 * with an empty reason is the silent failure this lane exists to catch.
 */
function readProviderFacts(): ProviderFacts {
  const matches = Array.from(
    readLog().matchAll(
      /Type provider status:\s+(tsgo|tsserver|editor-tsserver|none)(?: \((.+?)\))?/g,
    ),
  );
  const last = matches[matches.length - 1];
  if (!last) return { kind: "unreported", reason: "" };
  return { kind: last[1], reason: last[2] ?? "" };
}

function percentile(values: readonly number[], p: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[Math.max(0, index)];
}

// ── Workspace discovery ────────────────────────────────────────────────────

function workspaceRoot(): string {
  const folders = vscode.workspace.workspaceFolders;
  assert.ok(folders && folders.length > 0, "acceptance lane requires an open workspace folder");
  return folders[0].uri.fsPath;
}

function relPosix(fsPath: string): string {
  return path.relative(workspaceRoot(), fsPath).replace(/\\/g, "/");
}

interface ProbedFile {
  readonly uri: vscode.Uri;
  readonly text: string;
  readonly probes: readonly SourceProbe[];
}

/**
 * Find files that yield usable probes.
 *
 * The lane runs against arbitrary real projects, so it discovers its own
 * targets rather than shipping fixed positions. `findFiles` is used instead of
 * a manual walk so VS Code's own exclusion rules apply and huge dependency
 * trees are never traversed.
 */
const DISCOVERY_EXCLUDE = "**/{node_modules,dist,.output,.nuxt,.git,coverage}/**";

async function findCandidates(glob: string): Promise<vscode.Uri[]> {
  const found = await vscode.workspace.findFiles(glob, DISCOVERY_EXCLUDE, 400);
  return [...found].sort((a, b) => a.fsPath.localeCompare(b.fsPath));
}

async function discoverProbedFiles(
  glob: string,
  wantCarrier: boolean,
  budget: number,
): Promise<ProbedFile[]> {
  const sorted = await findCandidates(glob);
  const picked: ProbedFile[] = [];
  for (const uri of sorted) {
    if (picked.length >= budget) break;
    let text: string;
    try {
      text = fs.readFileSync(uri.fsPath, "utf8");
    } catch {
      continue;
    }
    if (wantCarrier && !isTypeScriptCarrier(text)) continue;
    if (!wantCarrier && /\.d\.ts$/i.test(uri.fsPath)) continue;
    const probes = selectProbes(text, 2);
    const classes = new Set(probes.map((p) => p.probeClass));
    // Require at least a member probe — the strongest rail — plus one more class.
    if (!classes.has("member") || classes.size < 2) continue;
    picked.push({ uri, text, probes });
  }
  return picked;
}

/**
 * Open SOME carrier even when none of them yields probes.
 *
 * Verter starts its language server on the first carrier the editor opens, so a
 * workspace whose SFCs are all plain JavaScript — no `lang="ts"` — would
 * otherwise never start the server, never publish a provider status, and be
 * recorded as "nothing happened". That is precisely the silent outcome this
 * lane exists to convert into an explicit, reasoned `none`.
 */
async function openAnyCarrierForStatus(): Promise<boolean> {
  const candidates = await findCandidates("**/*.{vue,svelte}");
  const target = candidates[0];
  if (!target) return false;
  const doc = await vscode.workspace.openTextDocument(target);
  await vscode.window.showTextDocument(doc);
  return true;
}

/**
 * Wait for the server to publish a provider status.
 *
 * Reading the status once at the END of the sweep loses it entirely on a
 * workspace slow enough that the status arrives late — which is exactly the
 * workspace whose status matters most.
 */
async function waitForProviderStatus(timeoutMs: number): Promise<ProviderFacts> {
  const deadline = Date.now() + timeoutMs;
  let facts = readProviderFacts();
  while (facts.kind === "unreported" && Date.now() < deadline) {
    await sleep(500);
    facts = readProviderFacts();
  }
  return facts;
}

// ── Operation drivers ──────────────────────────────────────────────────────

function toContract(probe: SourceProbe): HoverProbeContract {
  return {
    probeClass: probe.probeClass,
    identifier: probe.identifier,
    declarationHasNoAuthoredAnnotation: probe.declarationHasNoAuthoredAnnotation,
  };
}

function hoverMarkdown(hovers: readonly vscode.Hover[] | undefined): string {
  if (!hovers || hovers.length === 0) return "";
  return hovers
    .flatMap((hover) => hover.contents)
    .map((content) => (typeof content === "string" ? content : content.value))
    .join("\n");
}

async function timedHover(
  uri: vscode.Uri,
  position: vscode.Position,
): Promise<{ text: string; latencyMs: number }> {
  const start = Date.now();
  const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
    "vscode.executeHoverProvider",
    uri,
    position,
  );
  return { text: hoverMarkdown(hovers), latencyMs: Date.now() - start };
}

/**
 * Poll a probe until it produces a TypeScript answer.
 *
 * This is the number the product owner feels: how long after opening a file
 * does correct IntelliSense appear. Returns `undefined` when the deadline
 * passes without a TypeScript answer, and records the last verdict seen so the
 * receipt can say WHY it never arrived.
 */
async function timeToFirstTypeScriptHover(
  uri: vscode.Uri,
  position: vscode.Position,
  contract: HoverProbeContract,
  openedAtMs: number,
): Promise<{ elapsedMs?: number; lastVerdict: AnswerVerdict; lastReason: string }> {
  let lastVerdict: AnswerVerdict = "empty";
  let lastReason = "never probed";
  while (Date.now() - openedAtMs < FIRST_HOVER_TIMEOUT_MS) {
    const { text } = await timedHover(uri, position);
    const verdict = classifyHoverText(text, contract);
    lastVerdict = verdict.verdict;
    lastReason = verdict.reason;
    if (verdict.verdict === "typescript") {
      return { elapsedMs: Date.now() - openedAtMs, lastVerdict, lastReason };
    }
    await sleep(250);
  }
  return { lastVerdict, lastReason };
}

async function runHoverProbes(
  file: ProbedFile,
  fileKind: FileKind,
  doc: vscode.TextDocument,
): Promise<void> {
  for (const probe of file.probes) {
    const position = doc.positionAt(probe.offset);
    for (let i = 0; i < REPEATS; i++) {
      const { text, latencyMs } = await timedHover(doc.uri, position);
      const verdict = classifyHoverText(text, toContract(probe));
      samples.push({
        fileKind,
        operation: "hover",
        probeClass: probe.probeClass,
        verdict: verdict.verdict,
        reason: verdict.reason,
        marker: verdict.marker,
        latencyMs,
        repeatIndex: i,
      });
    }
  }
}

async function runDefinitionProbes(
  file: ProbedFile,
  fileKind: FileKind,
  doc: vscode.TextDocument,
): Promise<void> {
  const targets = file.probes.filter((p) => p.probeClass === "alias" || p.probeClass === "member");
  for (const probe of targets) {
    const position = doc.positionAt(probe.offset);
    for (let i = 0; i < REPEATS; i++) {
      const start = Date.now();
      const raw =
        (await vscode.commands.executeCommand<Array<vscode.Location | vscode.LocationLink>>(
          "vscode.executeDefinitionProvider",
          doc.uri,
          position,
        )) ?? [];
      const latencyMs = Date.now() - start;
      const targetPaths = raw.map((entry) =>
        "targetUri" in entry ? entry.targetUri.fsPath : entry.uri.fsPath,
      );
      const verdict = classifyDefinition({ targetPaths, sourcePath: doc.uri.fsPath });
      samples.push({
        fileKind,
        operation: "definition",
        probeClass: probe.probeClass,
        verdict: verdict.verdict,
        reason: verdict.reason,
        marker: verdict.marker,
        latencyMs,
        repeatIndex: i,
      });
    }
  }
}

async function runCompletionProbes(
  file: ProbedFile,
  fileKind: FileKind,
  doc: vscode.TextDocument,
): Promise<void> {
  const targets = file.probes.filter((p) => p.probeClass === "member");
  for (const probe of targets) {
    // The property identifier starts immediately after the `.`, so requesting
    // completions AT the identifier start is exactly the member-completion the
    // editor issues while the user types past the dot.
    const position = doc.positionAt(probe.offset);
    for (let i = 0; i < REPEATS; i++) {
      const start = Date.now();
      const list = await vscode.commands.executeCommand<vscode.CompletionList>(
        "vscode.executeCompletionItemProvider",
        doc.uri,
        position,
        ".",
      );
      const latencyMs = Date.now() - start;
      const items: CompletionItemFacts[] = (list?.items ?? []).map((item) => ({
        label: typeof item.label === "string" ? item.label : item.label.label,
        kind: item.kind,
        detail: item.detail,
      }));
      const verdict = classifyMemberCompletion(items, file.text);
      samples.push({
        fileKind,
        operation: "completion",
        probeClass: probe.probeClass,
        verdict: verdict.verdict,
        reason: verdict.reason,
        marker: verdict.marker,
        latencyMs,
        repeatIndex: i,
      });
    }
  }
}

async function runReferenceProbes(
  file: ProbedFile,
  fileKind: FileKind,
  doc: vscode.TextDocument,
): Promise<void> {
  const targets = file.probes.filter(
    (p) => p.probeClass === "alias" || p.probeClass === "inferred-local",
  );
  for (const probe of targets) {
    const position = doc.positionAt(probe.offset);
    for (let i = 0; i < REPEATS; i++) {
      const start = Date.now();
      const locations =
        (await vscode.commands.executeCommand<vscode.Location[]>(
          "vscode.executeReferenceProvider",
          doc.uri,
          position,
        )) ?? [];
      const latencyMs = Date.now() - start;
      const verdict = classifyReferences({
        locationPaths: locations.map((l) => l.uri.fsPath),
        sourcePath: doc.uri.fsPath,
      });
      samples.push({
        fileKind,
        operation: "references",
        probeClass: probe.probeClass,
        verdict: verdict.verdict,
        reason: verdict.reason,
        marker: verdict.marker,
        latencyMs,
        repeatIndex: i,
      });
    }
  }
}

// ── Receipt ────────────────────────────────────────────────────────────────

function summarise() {
  const operations: OperationName[] = ["hover", "definition", "completion", "references"];
  const fileKinds: FileKind[] = ["carrier", "typescript"];
  const table: Record<string, unknown> = {};
  for (const fileKind of fileKinds) {
    for (const operation of operations) {
      const subset = samples.filter((s) => s.fileKind === fileKind && s.operation === operation);
      if (subset.length === 0) continue;
      const all = subset.map((s) => s.latencyMs);
      const cold = subset.filter((s) => s.repeatIndex === 0).map((s) => s.latencyMs);
      const warm = subset.filter((s) => s.repeatIndex > 0).map((s) => s.latencyMs);
      const count = (v: string) => subset.filter((s) => s.verdict === v).length;
      table[`${fileKind}.${operation}`] = {
        samples: subset.length,
        // `typescript` is reachable only from hover; see the Sample docs.
        typescript: count("typescript"),
        verterNative: count("verter-native"),
        empty: count("empty"),
        indeterminate: count("indeterminate"),
        resolved: count("resolved"),
        unresolved: count("unresolved"),
        p50Ms: percentile(all, 50),
        p95Ms: percentile(all, 95),
        maxMs: Math.max(...all),
        coldP50Ms: percentile(cold, 50),
        coldMaxMs: cold.length > 0 ? Math.max(...cold) : 0,
        warmP50Ms: percentile(warm, 50),
        warmP95Ms: percentile(warm, 95),
      };
    }
  }
  const ratios: Record<string, { p50: number | null; warmP50: number | null }> = {};
  for (const operation of operations) {
    const carrier = table[`carrier.${operation}`] as
      | { p50Ms: number; warmP50Ms: number }
      | undefined;
    const ts = table[`typescript.${operation}`] as { p50Ms: number; warmP50Ms: number } | undefined;
    ratios[operation] = {
      p50: carrier && ts && ts.p50Ms > 0 ? Number((carrier.p50Ms / ts.p50Ms).toFixed(2)) : null,
      warmP50:
        carrier && ts && ts.warmP50Ms > 0
          ? Number((carrier.warmP50Ms / ts.warmP50Ms).toFixed(2))
          : null,
    };
  }
  return { table, ratios };
}

function verdictCount(fileKind: FileKind, verdict: AnswerVerdict | ResolutionVerdict): number {
  return samples.filter((s) => s.fileKind === fileKind && s.verdict === verdict).length;
}

function typescriptAnswers(fileKind: FileKind, operation?: OperationName): number {
  return samples.filter(
    (s) =>
      s.fileKind === fileKind &&
      s.verdict === "typescript" &&
      (operation === undefined || s.operation === operation),
  ).length;
}

// ── The suite ──────────────────────────────────────────────────────────────

suite("VS Code acceptance — TypeScript results in the editor", () => {
  let provider: ProviderFacts = { kind: "unreported", reason: "" };
  let firstHover: { elapsedMs?: number; lastVerdict: AnswerVerdict; lastReason: string } = {
    lastVerdict: "empty",
    lastReason: "not run",
  };
  let openToReadyMs = 0;
  let carrierFiles: ProbedFile[] = [];
  let typescriptFiles: ProbedFile[] = [];
  let tsCapableExtensions: string[] = [];

  suiteSetup(async function (this: Mocha.Context) {
    // Derived from the phases it actually runs, not a fixed number: a fixed
    // timeout smaller than first-hover polling plus the sweep budget aborts the
    // hook and discards everything measured so far.
    this.timeout(FIRST_HOVER_TIMEOUT_MS + PROVIDER_STATUS_TIMEOUT_MS + SWEEP_BUDGET_MS + 180_000);

    const ext = vscode.extensions.getExtension("pikax.verter-vscode");
    assert.ok(ext, "Verter extension must be installed in the acceptance host");
    if (!ext.isActive) await ext.activate();

    // WHO is answering matters as much as whether anything answers. `.ts`
    // probes are served by whichever TypeScript-capable extension the host
    // loaded, and a control run showed `.ts` hovers being answered with real
    // quickinfo while Verter's own provider was disabled. Recording the
    // inventory is what turns that from a puzzle into a fact — and these ids
    // are VS Code's own, never corpus data.
    tsCapableExtensions = vscode.extensions.all
      .map((e) => e.id)
      .filter((id) => /typescript|javascript|verter|vue|volar/i.test(id))
      .sort();
    notes.push(`ts-capable extensions present: ${tsCapableExtensions.join(", ") || "none"}`);

    carrierFiles = await discoverProbedFiles("**/*.vue", true, CARRIER_FILE_BUDGET);
    typescriptFiles = await discoverProbedFiles("**/*.ts", false, CARRIER_FILE_BUDGET);
    notes.push(
      `discovered ${carrierFiles.length} probeable carrier(s), ${typescriptFiles.length} probeable .ts file(s)`,
    );

    if (carrierFiles.length === 0) {
      notes.push(
        "no probeable TypeScript carrier found — this workspace cannot be MEASURED, but its " +
          "provider status is still an acceptance outcome and is collected below",
      );
      const opened = await openAnyCarrierForStatus();
      notes.push(opened ? "opened a carrier to start the server" : "workspace contains no carrier");
      provider = await waitForProviderStatus(PROVIDER_STATUS_TIMEOUT_MS);
      return;
    }

    // ── The owner-felt number: open an SFC, then time until correct hover ──
    const primary = carrierFiles[0];
    const openedAt = Date.now();
    const doc = await vscode.workspace.openTextDocument(primary.uri);
    await vscode.window.showTextDocument(doc);
    openToReadyMs = Date.now() - openedAt;

    const strongest =
      primary.probes.find((p) => p.probeClass === "member") ??
      primary.probes.find((p) => p.probeClass === "alias") ??
      primary.probes[0];
    firstHover = await timeToFirstTypeScriptHover(
      doc.uri,
      doc.positionAt(strongest.offset),
      toContract(strongest),
      openedAt,
    );

    // Capture the provider status BEFORE the probe sweep. Reading it only at the
    // end loses it on any workspace slow enough for the sweep to be cut short —
    // and a workspace that slow is exactly the one whose status matters. Losing
    // it there produced a false "status never published" failure that blamed the
    // product for a defect in this lane.
    provider = await waitForProviderStatus(PROVIDER_STATUS_TIMEOUT_MS);

    // ── Per-operation sampling ────────────────────────────────────────────
    // Carrier and plain-TypeScript files are INTERLEAVED rather than run in two
    // blocks. Running all carriers first and all `.ts` files afterwards would
    // hand the `.ts` side a warmer server and quietly inflate the overhead
    // ratio the lane exists to report.
    // A slow workspace must still produce a receipt. Without a budget the sweep
    // can outlive the suite timeout, and the run is then reported as a harness
    // failure instead of as the very-slow-editor result it actually is.
    const sweepDeadline = Date.now() + SWEEP_BUDGET_MS;
    const rounds = Math.max(carrierFiles.length, typescriptFiles.length);
    let truncated = false;
    for (let round = 0; round < rounds && !truncated; round++) {
      for (const [file, kind] of [
        [carrierFiles[round], "carrier"] as const,
        [typescriptFiles[round], "typescript"] as const,
      ]) {
        if (!file) continue;
        if (Date.now() > sweepDeadline) {
          truncated = true;
          break;
        }
        const opened = await vscode.workspace.openTextDocument(file.uri);
        await vscode.window.showTextDocument(opened);
        await runHoverProbes(file, kind, opened);
        await runDefinitionProbes(file, kind, opened);
        await runCompletionProbes(file, kind, opened);
        await runReferenceProbes(file, kind, opened);
      }
    }
    if (truncated) {
      notes.push(
        `sweep truncated after ${SWEEP_BUDGET_MS}ms — this workspace is slow enough that the ` +
          "full probe matrix does not complete; the samples below are a prefix, not the whole set",
      );
    }
    // Re-read: a late status change (a fallback, a restart) must be the one
    // reported, but an early capture is already banked if the sweep was cut short.
    const finalProvider = readProviderFacts();
    if (finalProvider.kind !== "unreported") provider = finalProvider;
  });

  suiteTeardown(() => {
    if (!RECEIPT_FILE) return;
    const { table, ratios } = summarise();
    const receipt = {
      corpus: CORPUS_LABEL,
      typeProviderRequested: readE2eEnv("TYPE_PROVIDER") ?? "auto",
      provider,
      tsCapableExtensions,
      openToShownMs: openToReadyMs,
      timeToFirstTypeScriptHoverMs: firstHover.elapsedMs ?? null,
      firstHoverLastVerdict: firstHover.lastVerdict,
      firstHoverLastReason: firstHover.lastReason,
      probedCarriers: carrierFiles.map((f) => ({
        file: redactPath(relPosix(f.uri.fsPath)),
        probes: f.probes.map((p) => ({
          probeClass: p.probeClass,
          identifier: redactIdentifier(p.identifier),
        })),
      })),
      probedTypeScriptFiles: typescriptFiles.map((f) => ({
        file: redactPath(relPosix(f.uri.fsPath)),
        probeCount: f.probes.length,
      })),
      operations: table,
      carrierOverPlainTsP50Ratio: ratios,
      attribution: {
        hover:
          "engine-exclusive — a `typescript` count here means a TS quickinfo kind prefix, or " +
          "an inferred type on a declaration proven to carry no authored annotation",
        definitionCompletionReferences:
          "resolution only — a provider-disabled control run still produced cross-file " +
          "definitions, foreign member completions and cross-file references, so these " +
          "cannot be attributed to the engine from their payload; compare a paired " +
          "VERTER_ACCEPTANCE_PROVIDER=off run to attribute them",
      },
      notes,
    };
    fs.mkdirSync(path.dirname(RECEIPT_FILE), { recursive: true });
    fs.writeFileSync(RECEIPT_FILE, `${JSON.stringify(receipt, null, 2)}\n`, "utf8");
  });

  test("the server reports a type-provider status, and never silently", () => {
    assert.notStrictEqual(
      provider.kind,
      "unreported",
      "no `Type provider status` was ever published — the editor cannot tell the user " +
        "whether TypeScript is available",
    );
    if (provider.kind === "none") {
      assert.ok(
        provider.reason.trim().length > 0,
        "provider status `none` was published with NO reason — a workspace that cannot get " +
          "an engine must say why, not fail silently",
      );
    }
  });

  test("a workspace with a connected engine answers hovers with real TypeScript", function () {
    if (provider.kind === "none") {
      // Fail-closed consistency: claiming no engine while producing engine
      // answers would mean the status is lying to the user.
      assert.strictEqual(
        typescriptAnswers("carrier", "hover"),
        0,
        "provider status is `none` yet carrier hovers produced TypeScript answers — " +
          "the reported status contradicts the observed behaviour",
      );
      // LIVE negative control. With no engine every hover the editor shows is
      // produced by Verter itself, so this run is the proof that the
      // discriminator rejects real Verter-native payloads in the real system
      // and not merely against fixtures. Requiring that native answers were
      // actually OBSERVED is what stops "zero TypeScript answers" from being
      // satisfied vacuously by a run where nothing answered at all.
      if (EXPECT_NATIVE) {
        assert.ok(
          verdictCount("carrier", "verter-native") > 0,
          "negative control saw no Verter-native hover at all, so `0 TypeScript answers` " +
            "proves nothing about the discriminator",
        );
      }
      this.skip();
      return;
    }
    assert.ok(
      carrierFiles.length > 0,
      `provider status is \`${provider.kind}\` but no probeable TypeScript carrier was found, ` +
        "so the editor experience on this workspace cannot be verified",
    );
    assert.ok(
      typescriptAnswers("carrier", "hover") > 0,
      `provider status is \`${provider.kind}\` but ZERO carrier hovers carried real TypeScript ` +
        `content (last verdict: ${firstHover.lastVerdict} — ${firstHover.lastReason})`,
    );
  });

  test("correct IntelliSense arrives within the first-hover budget", function () {
    if (provider.kind === "none") {
      this.skip();
      return;
    }
    assert.ok(
      firstHover.elapsedMs !== undefined,
      `no TypeScript hover ever arrived within ${FIRST_HOVER_TIMEOUT_MS}ms of opening the SFC ` +
        `(last verdict: ${firstHover.lastVerdict} — ${firstHover.lastReason})`,
    );
  });

  test("the native-TypeScript yardstick answered in the same session", function () {
    if (provider.kind === "none") {
      this.skip();
      return;
    }
    assert.ok(
      typescriptFiles.length > 0,
      "no probeable .ts file was discovered, so no overhead ratio can be computed",
    );
    assert.ok(
      typescriptAnswers("typescript") > 0,
      "the native-TypeScript yardstick produced no TypeScript answers, so any carrier " +
        "measurement has nothing to be compared against",
    );
  });
});
