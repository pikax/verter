/**
 * Shared E2E harness for IDE parity suites (Vue + Svelte).
 *
 * Design goals:
 * - Hard asserts on authored-source results (no virtual carrier leaks).
 * - Stable token anchors via occurrence-aware search.
 * - Product gaps remain failed tests and cite an ISSUES.md row.
 */
import { strict as assert } from "node:assert";
import { pollBudget } from "./timeouts";
import * as path from "node:path";
import * as vscode from "vscode";

import {
  FIXTURE_NAME,
  ensureTypeProviderSynced,
  invalidateTypeProviderSyncCache,
  sleep,
  waitForDiagnostics,
  waitForDiagnosticsSettled,
} from "../helpers";
import { VIRTUAL_CARRIER_PATTERN } from "./virtualCarrier";

export type ParityFramework = "vue" | "svelte";

/**
 * Register a framework-owned parity test only for its applicable fixture.
 * Inapplicable behavior is absent from the run inventory instead of being
 * represented by a vacuous pass or an artificial product failure.
 */
export function registerFrameworkTest(
  framework: ParityFramework,
  title: string,
  body: (this: Mocha.Context) => void | Promise<void>,
): void {
  if (FIXTURE_NAME === `${framework}-parity`) {
    test(title, body as Mocha.AsyncFunc);
  }
}

export const VIRTUAL_CARRIER = VIRTUAL_CARRIER_PATTERN;

export interface TokenAnchor {
  readonly file: string;
  readonly token: string;
  /** 0-based occurrence of `token` in file text. */
  readonly occurrence?: number;
  /** Extra chars into the token for caret placement (default min(1, token.length)). */
  readonly caretOffset?: number;
}

export type GapReason = "architecture" | "product-gap" | "provider-gap" | "fixture-limit";

/** Product/test gap classifications recorded during a run. */
export const parityGapLog: Array<{
  id: string;
  reason: GapReason;
  issue: string;
  detail: string;
}> = [];

export function workspaceRoot(): string {
  const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  assert.ok(root, "parity suite requires one workspace root");
  return root;
}

export function absoluteFile(relative: string): string {
  return path.normalize(path.join(workspaceRoot(), relative));
}

export async function openRelative(relative: string): Promise<vscode.TextDocument> {
  const doc = await vscode.workspace.openTextDocument(vscode.Uri.file(absoluteFile(relative)));
  await vscode.window.showTextDocument(doc);
  return doc;
}

export async function ensureParityReady(entry: string): Promise<vscode.TextDocument> {
  await ensureTypeProviderSynced();
  return openRelative(entry);
}

/** Start a clean provider epoch for a state-sensitive parity suite. */
export async function restartParityReady(entry: string): Promise<vscode.TextDocument> {
  await vscode.commands.executeCommand("verter.restartLanguageServer");
  invalidateTypeProviderSyncCache();
  return ensureParityReady(entry);
}

export function tokenOffset(doc: vscode.TextDocument, anchor: TokenAnchor): number {
  assert.equal(
    path.normalize(doc.uri.fsPath),
    absoluteFile(anchor.file),
    `token opened wrong file (want ${anchor.file})`,
  );
  const occurrence = anchor.occurrence ?? 0;
  let offset = -1;
  for (let i = 0; i <= occurrence; i++) {
    offset = doc.getText().indexOf(anchor.token, offset + 1);
    assert.notEqual(offset, -1, `missing token ${anchor.file}#${anchor.token}[${occurrence}]`);
  }
  return offset;
}

export function tokenPosition(doc: vscode.TextDocument, anchor: TokenAnchor): vscode.Position {
  const offset = tokenOffset(doc, anchor);
  const into = anchor.caretOffset ?? Math.min(1, anchor.token.length);
  return doc.positionAt(offset + into);
}

export async function pollUntil<T>(
  label: string,
  request: () => Promise<T>,
  ready: (value: T) => boolean,
  timeoutMs = pollBudget("parityHarnessSettle"),
): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  let latest = await request();
  while (Date.now() < deadline) {
    if (ready(latest)) return latest;
    await sleep(150);
    latest = await request();
  }
  throw new Error(`${label} not ready within ${timeoutMs}ms`);
}

export function toLocations(
  values: readonly (vscode.Location | vscode.LocationLink)[] | undefined,
): vscode.Location[] {
  return (values ?? []).map((value) =>
    "targetUri" in value
      ? new vscode.Location(value.targetUri, value.targetSelectionRange ?? value.targetRange)
      : value,
  );
}

export function assertNoVirtualLocations(
  locations: readonly vscode.Location[],
  feature: string,
): void {
  const leaked = locations.filter((location) => VIRTUAL_CARRIER.test(location.uri.fsPath));
  assert.deepEqual(
    leaked.map((location) => location.uri.fsPath),
    [],
    `${feature} exposed generated carrier paths`,
  );
}

async function pollDefinitions(
  anchor: TokenAnchor,
  ready: (locations: readonly vscode.Location[]) => boolean,
): Promise<vscode.Location[]> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  const locations = await pollUntil(
    `definition ${anchor.file}#${anchor.token}`,
    async () =>
      toLocations(
        await vscode.commands.executeCommand<Array<vscode.Location | vscode.LocationLink>>(
          "vscode.executeDefinitionProvider",
          doc.uri,
          position,
        ),
      ),
    ready,
  );
  assertNoVirtualLocations(locations, "definition");
  return locations;
}

export async function definitionsAt(anchor: TokenAnchor): Promise<vscode.Location[]> {
  return pollDefinitions(anchor, (locations) => locations.length > 0);
}

export async function assertDefinitionTargetsFile(
  source: TokenAnchor,
  targetFile: string,
): Promise<void> {
  const want = absoluteFile(targetFile);
  const locations = await pollDefinitions(source, (candidate) =>
    candidate.some((location) => path.normalize(location.uri.fsPath) === want),
  );
  assert.ok(
    locations.some((location) => path.normalize(location.uri.fsPath) === want),
    `definition from ${source.file}#${source.token} did not reach ${targetFile}; got ${locations
      .map((location) => location.uri.fsPath)
      .join(", ")}`,
  );
}

export async function assertDefinitionTargetsToken(
  source: TokenAnchor,
  target: TokenAnchor,
): Promise<void> {
  const targetDoc = await vscode.workspace.openTextDocument(
    vscode.Uri.file(absoluteFile(target.file)),
  );
  const expected = tokenOffset(targetDoc, target);
  const hitsTarget = (location: vscode.Location): boolean => {
    if (path.normalize(location.uri.fsPath) !== absoluteFile(target.file)) return false;
    const start = targetDoc.offsetAt(location.range.start);
    const end = targetDoc.offsetAt(location.range.end);
    return start === end ? expected === start : start <= expected && expected < end;
  };
  // Capture-only navigation may first expose a native same-file fallback while
  // the provider finishes its nonblocking project warm-up. Wait for the exact
  // authored target, not merely the first non-empty partial location set.
  const locations = await pollDefinitions(source, (candidate) => candidate.some(hitsTarget));
  assert.ok(
    locations.some(hitsTarget),
    `definition from ${source.file}#${source.token} did not hit ${target.file}#${target.token}`,
  );
}

export async function hoverTextAt(anchor: TokenAnchor): Promise<string> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  const hovers = await pollUntil(
    `hover ${anchor.file}#${anchor.token}`,
    async () =>
      (await vscode.commands.executeCommand<vscode.Hover[]>(
        "vscode.executeHoverProvider",
        doc.uri,
        position,
      )) ?? [],
    (result) => result.length > 0,
  );
  return hovers
    .flatMap((hover) => hover.contents)
    .map((content) => (typeof content === "string" ? content : content.value))
    .join("\n");
}

export async function assertHoverNeedles(
  anchor: TokenAnchor,
  needles: readonly string[],
  options?: { forbidAny?: boolean; forbidUnknown?: boolean; forbidGenerated?: boolean },
): Promise<string> {
  const text = await hoverTextAt(anchor);
  for (const needle of needles) {
    assert.ok(text.includes(needle), `hover missing ${needle}: ${text}`);
  }
  if (options?.forbidAny !== false) {
    assert.ok(!/:\s*any\b/.test(text), `hover degraded to any: ${text}`);
  }
  if (options?.forbidUnknown) {
    assert.ok(!/\bunknown\b/.test(text), `hover lost concrete type: ${text}`);
  }
  if (options?.forbidGenerated !== false) {
    assert.ok(!/__Verter\w*/.test(text), `hover leaked generated symbols: ${text}`);
  }
  return text;
}

export async function completionLabelsAt(
  anchor: TokenAnchor,
  triggerCharacter?: string,
): Promise<string[]> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  const list = await pollUntil(
    `completion ${anchor.file}#${anchor.token}`,
    async () =>
      (await vscode.commands.executeCommand<vscode.CompletionList>(
        "vscode.executeCompletionItemProvider",
        doc.uri,
        position,
        triggerCharacter,
      )) ?? { items: [], isIncomplete: false },
    (result) => (result.items?.length ?? 0) > 0,
  );
  return (list.items ?? []).map((item) =>
    typeof item.label === "string" ? item.label : item.label.label,
  );
}

export async function assertCompletionsInclude(
  anchor: TokenAnchor,
  required: readonly string[],
  triggerCharacter?: string,
): Promise<string[]> {
  const labels = await completionLabelsAt(anchor, triggerCharacter);
  for (const label of required) {
    assert.ok(
      labels.some((entry) => entry === label || entry.startsWith(label)),
      `completion missing ${label}; sample=${labels.slice(0, 40).join(", ")}`,
    );
  }
  return labels;
}

export async function assertCompletionsExclude(
  anchor: TokenAnchor,
  forbidden: readonly string[],
  triggerCharacter?: string,
): Promise<void> {
  const labels = await completionLabelsAt(anchor, triggerCharacter);
  for (const label of forbidden) {
    assert.ok(
      !labels.some((entry) => entry === label),
      `completion unexpectedly includes ${label}`,
    );
  }
}

export async function referencesAt(anchor: TokenAnchor): Promise<vscode.Location[]> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  const refs = await pollUntil(
    `references ${anchor.file}#${anchor.token}`,
    async () =>
      (await vscode.commands.executeCommand<vscode.Location[]>(
        "vscode.executeReferenceProvider",
        doc.uri,
        position,
      )) ?? [],
    (result) => result.length > 0,
  );
  assertNoVirtualLocations(refs, "references");
  return refs;
}

export async function assertReferenceCountAtLeast(
  anchor: TokenAnchor,
  min: number,
): Promise<vscode.Location[]> {
  const refs = await referencesAt(anchor);
  assert.ok(
    refs.length >= min,
    `expected >= ${min} references for ${anchor.file}#${anchor.token}, got ${refs.length}`,
  );
  return refs;
}

export async function settledDiagnostics(relative: string): Promise<vscode.Diagnostic[]> {
  const doc = await openRelative(relative);
  return waitForDiagnosticsSettled(doc.uri, { timeoutMs: 12_000, stableMs: 600 });
}

export async function errorDiagnostics(relative: string): Promise<vscode.Diagnostic[]> {
  const diagnostics = await settledDiagnostics(relative);
  return diagnostics.filter(
    (diagnostic) => diagnostic.severity === vscode.DiagnosticSeverity.Error,
  );
}

export async function assertCleanErrors(relative: string): Promise<void> {
  const errors = await errorDiagnostics(relative);
  assert.deepEqual(
    errors.map(
      (diagnostic) =>
        `${diagnostic.source ?? "unknown"}:${String(diagnostic.code)}:${diagnostic.message}`,
    ),
    [],
    `${relative} must be error-clean`,
  );
}

export async function assertHasErrorMatching(
  relative: string,
  matcher: RegExp | string,
): Promise<vscode.Diagnostic[]> {
  const doc = await openRelative(relative);
  const matches = (diagnostic: vscode.Diagnostic) => {
    if (diagnostic.severity !== vscode.DiagnosticSeverity.Error) return false;
    const hay = `${String(diagnostic.code)}:${diagnostic.message}`;
    if (typeof matcher === "string") return hay.includes(matcher);
    matcher.lastIndex = 0;
    return matcher.test(hay);
  };
  const diagnostics = await waitForDiagnostics(doc.uri, {
    timeoutMs: 12_000,
    predicate: matches,
  });
  const errors = diagnostics.filter(
    (diagnostic) => diagnostic.severity === vscode.DiagnosticSeverity.Error,
  );
  const hit = errors.filter((diagnostic) => {
    return matches(diagnostic);
  });
  assert.ok(
    hit.length > 0,
    `${relative} expected error matching ${matcher}; got ${errors
      .map((diagnostic) => `${String(diagnostic.code)}:${diagnostic.message}`)
      .join(" | ")}`,
  );
  return hit;
}

/**
 * File must contain at least `minCount` `@ts-expect-error` directives and be
 * error-clean. That means every directive suppressed a real error (TS2578 =
 * unused @ts-expect-error would fail this). Used for intentional negatives.
 */
export async function assertTsExpectErrorFileHolds(relative: string, minCount = 1): Promise<void> {
  const doc = await openRelative(relative);
  const text = doc.getText();
  const markers = text.match(/@ts-expect-error\b/g) ?? [];
  assert.ok(
    markers.length >= minCount,
    `TEST_DEFECT ${relative}: need >=${minCount} @ts-expect-error, found ${markers.length}`,
  );
  // Unused @ts-expect-error is TS2578; other errors also fail the negative suite.
  await assertCleanErrors(relative);
}

/** Require at least `minErrors` error diagnostics (any code). */
export async function assertErrorCountAtLeast(
  relative: string,
  minErrors: number,
): Promise<vscode.Diagnostic[]> {
  const errors = await errorDiagnostics(relative);
  assert.ok(
    errors.length >= minErrors,
    `${relative} expected >=${minErrors} errors; got ${errors.length}: ${errors
      .map((d) => `${String(d.code)}:${d.message}`)
      .join(" | ")}`,
  );
  return errors;
}

export function verterUnknownPropDiags(uri: vscode.Uri): vscode.Diagnostic[] {
  return vscode.languages.getDiagnostics(uri).filter((diagnostic) => {
    const isVerter = diagnostic.source === "Verter" || diagnostic.source === "verter";
    if (!isVerter) return false;
    const code =
      typeof diagnostic.code === "object" && diagnostic.code && "value" in diagnostic.code
        ? String((diagnostic.code as { value: string | number }).value)
        : String(diagnostic.code ?? "");
    return code === "verter/unknown-prop" || diagnostic.message.toLowerCase().includes("unknown");
  });
}

/**
 * HARD FAIL (never mocha-skip).
 *
 * Dynamic catch-to-skip behavior would misclassify broken fixtures as product
 * gaps. Do not restore context.skip() here.
 *
 * - `TEST_DEFECT …` → broken test/fixture (anchors, needles)
 * - `PRODUCT_GAP ISSUE-…` → likely product missing behavior (still red)
 */
export function failParityGap(
  _context: Mocha.Context,
  id: string,
  issue: string,
  detail: string,
  reason: GapReason = "architecture",
): never {
  if (!/^ISSUE-[\w-]+$/.test(issue)) {
    throw new Error(
      `TEST_DEFECT ${id}: failParityGap requires ISSUE-* id, got ${JSON.stringify(issue)}`,
    );
  }
  const defect =
    /TEST_DEFECT|missing (token|needle|anchor)|missing completion offset|opened wrong file|fixture must/i.test(
      detail,
    );
  parityGapLog.push({ id, reason, issue, detail });
  const kind = defect ? "TEST_DEFECT" : "PRODUCT_GAP";
  throw new Error(`${kind} ${issue} ${id}: ${detail}`);
}

/** Explicit product-gap hard fail (same as failParityGap outcome; clearer at call sites). */
export function failProduct(id: string, issue: string, detail: string): never {
  if (!/^ISSUE-[\w-]+$/.test(issue)) {
    throw new Error(
      `TEST_DEFECT ${id}: failProduct requires ISSUE-* id, got ${JSON.stringify(issue)}`,
    );
  }
  throw new Error(`PRODUCT_GAP ${issue} ${id}: ${detail}`);
}

export async function signatureHelpAt(
  anchor: TokenAnchor,
): Promise<vscode.SignatureHelp | undefined> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  return vscode.commands.executeCommand<vscode.SignatureHelp>(
    "vscode.executeSignatureHelpProvider",
    doc.uri,
    position,
  );
}

export async function documentHighlightsAt(
  anchor: TokenAnchor,
): Promise<vscode.DocumentHighlight[]> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  return (
    (await vscode.commands.executeCommand<vscode.DocumentHighlight[]>(
      "vscode.executeDocumentHighlights",
      doc.uri,
      position,
    )) ?? []
  );
}

export async function typeDefinitionsAt(anchor: TokenAnchor): Promise<vscode.Location[]> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  const locations = toLocations(
    await vscode.commands.executeCommand<Array<vscode.Location | vscode.LocationLink>>(
      "vscode.executeTypeDefinitionProvider",
      doc.uri,
      position,
    ),
  );
  assertNoVirtualLocations(locations, "typeDefinition");
  return locations;
}

/** A decoded semantic token resolved against the provider's own legend. */
export interface ResolvedSemanticToken {
  /** Legend NAME of the token type (e.g. `interface`, `variable`). */
  readonly tokenType: string;
  /** Legend NAMES of every set modifier bit (e.g. `declaration`, `readonly`). */
  readonly modifiers: readonly string[];
}

/**
 * Resolve the semantic token covering `anchor` to legend NAMES.
 *
 * `tokens.data.length > 0` is not evidence of correctness — the historical
 * defect class here is tokens that EXIST with the wrong type/modifier indices
 * (provider legend forwarded unmapped), which an existence probe can never
 * see. Callers assert the decoded NAME, which is exactly what themes key on.
 *
 * Returns `undefined` when no token covers the anchor within the poll budget
 * (feature absent, anchor unmapped, or the token was dropped).
 */
export async function semanticTokenAt(
  anchor: TokenAnchor,
): Promise<ResolvedSemanticToken | undefined> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);

  const resolve = async (): Promise<ResolvedSemanticToken | undefined> => {
    const legend = await vscode.commands.executeCommand<vscode.SemanticTokensLegend>(
      "vscode.provideDocumentSemanticTokensLegend",
      doc.uri,
    );
    const tokens = await vscode.commands.executeCommand<vscode.SemanticTokens>(
      "vscode.provideDocumentSemanticTokens",
      doc.uri,
    );
    if (!legend || !tokens) return undefined;
    // Decode the LSP delta stream: [deltaLine, deltaStartChar, length, type, modifiers].
    let line = 0;
    let char = 0;
    for (let i = 0; i + 4 < tokens.data.length; i += 5) {
      const deltaLine = tokens.data[i];
      const deltaChar = tokens.data[i + 1];
      const length = tokens.data[i + 2];
      const typeIndex = tokens.data[i + 3];
      const modifierBits = tokens.data[i + 4];
      if (deltaLine > 0) {
        line += deltaLine;
        char = deltaChar;
      } else {
        char += deltaChar;
      }
      if (
        line === position.line &&
        position.character >= char &&
        position.character < char + length
      ) {
        const tokenType = legend.tokenTypes[typeIndex];
        // An index outside the advertised legend is itself a defect worth
        // surfacing loudly rather than reading as "no token here".
        assert.ok(
          tokenType !== undefined,
          `token at ${anchor.file}#${anchor.token} carries type index ${typeIndex} ` +
            `outside the advertised ${legend.tokenTypes.length}-entry legend`,
        );
        const modifiers = legend.tokenModifiers.filter(
          (_, bit) => (modifierBits & (1 << bit)) !== 0,
        );
        return { tokenType, modifiers };
      }
    }
    return undefined;
  };

  try {
    return await pollUntil("semanticTokenAt", resolve, (value) => value !== undefined);
  } catch {
    return undefined;
  }
}

export async function prepareRenameAt(
  anchor: TokenAnchor,
): Promise<vscode.Range | { range: vscode.Range; placeholder: string } | undefined> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  try {
    return await vscode.commands.executeCommand("vscode.prepareRename", doc.uri, position);
  } catch {
    return undefined;
  }
}

export async function renameEditsAt(
  anchor: TokenAnchor,
  newName: string,
): Promise<vscode.WorkspaceEdit | undefined> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  try {
    return await vscode.commands.executeCommand<vscode.WorkspaceEdit>(
      "vscode.executeDocumentRenameProvider",
      doc.uri,
      position,
      newName,
    );
  } catch {
    return undefined;
  }
}

/**
 * Apply a rename, re-check definition markup→decl, then restore exact source bytes.
 */
export async function assertRenameCoversAndRestores(
  origin: TokenAnchor,
  newName: string,
  options?: { minEdits?: number; definitionFrom?: TokenAnchor; definitionTo?: TokenAnchor },
): Promise<void> {
  const edit = await renameEditsAt(origin, newName);
  assert.ok(edit, `rename from ${origin.file}#${origin.token} returned no edit`);
  const entries = edit.entries();
  const total = entries.reduce((sum, [, edits]) => sum + edits.length, 0);
  assert.ok(
    total >= (options?.minEdits ?? 2),
    `rename expected >= ${options?.minEdits ?? 2} edits, got ${total}`,
  );
  const locations = entries.flatMap(([uri, edits]) =>
    edits.map((textEdit) => new vscode.Location(uri, textEdit.range)),
  );
  assertNoVirtualLocations(locations, "rename");

  const originals = new Map<string, string>();
  for (const [uri] of entries) {
    if (!originals.has(uri.toString())) {
      originals.set(uri.toString(), (await vscode.workspace.openTextDocument(uri)).getText());
    }
  }
  try {
    assert.equal(await vscode.workspace.applyEdit(edit), true, "rename apply failed");
    if (options?.definitionFrom && options.definitionTo) {
      const from: TokenAnchor = { ...options.definitionFrom, token: newName };
      const to: TokenAnchor = { ...options.definitionTo, token: newName };
      await assertDefinitionTargetsToken(from, to);
    }
  } finally {
    const restore = new vscode.WorkspaceEdit();
    for (const [uriText, original] of originals) {
      const uri = vscode.Uri.parse(uriText);
      const doc = await vscode.workspace.openTextDocument(uri);
      restore.replace(
        uri,
        new vscode.Range(doc.positionAt(0), doc.positionAt(doc.getText().length)),
        original,
      );
    }
    assert.equal(await vscode.workspace.applyEdit(restore), true, "rename restore failed");
  }
}

export async function documentSymbolsAt(
  relative: string,
): Promise<Array<vscode.DocumentSymbol | vscode.SymbolInformation>> {
  const doc = await openRelative(relative);
  return (
    (await vscode.commands.executeCommand<Array<vscode.DocumentSymbol | vscode.SymbolInformation>>(
      "vscode.executeDocumentSymbolProvider",
      doc.uri,
    )) ?? []
  );
}

export async function workspaceSymbolsMatching(query: string): Promise<vscode.SymbolInformation[]> {
  return (
    (await vscode.commands.executeCommand<vscode.SymbolInformation[]>(
      "vscode.executeWorkspaceSymbolProvider",
      query,
    )) ?? []
  );
}

export async function inlayHintsForFile(relative: string): Promise<vscode.InlayHint[]> {
  const doc = await openRelative(relative);
  const range = new vscode.Range(doc.positionAt(0), doc.positionAt(doc.getText().length));
  return (
    (await vscode.commands.executeCommand<vscode.InlayHint[]>(
      "vscode.executeInlayHintProvider",
      doc.uri,
      range,
    )) ?? []
  );
}

export async function codeActionsForFile(
  relative: string,
  kind?: vscode.CodeActionKind,
): Promise<(vscode.CodeAction | vscode.Command)[]> {
  const doc = await openRelative(relative);
  const range = new vscode.Range(doc.positionAt(0), doc.positionAt(doc.getText().length));
  return (
    (await vscode.commands.executeCommand<(vscode.CodeAction | vscode.Command)[]>(
      "vscode.executeCodeActionProvider",
      doc.uri,
      range,
      kind?.value,
    )) ?? []
  );
}

export async function hoversAt(anchor: TokenAnchor): Promise<vscode.Hover[]> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  return pollUntil(
    `hover list ${anchor.file}#${anchor.token}`,
    async () =>
      (await vscode.commands.executeCommand<vscode.Hover[]>(
        "vscode.executeHoverProvider",
        doc.uri,
        position,
      )) ?? [],
    (result) => result.length > 0,
  );
}

/**
 * Assert hover range covers the authored token (mapping fidelity for highlight background).
 * Tolerates 0-width ranges only when the caret is inside the token.
 */
export async function assertHoverRangeCoversToken(anchor: TokenAnchor): Promise<vscode.Hover[]> {
  const doc = await openRelative(anchor.file);
  const tokenStart = tokenOffset(doc, anchor);
  const tokenEnd = tokenStart + anchor.token.length;
  const hovers = await hoversAt(anchor);
  const withRange = hovers.filter((h) => h.range);
  assert.ok(
    withRange.length > 0,
    `hover at ${anchor.file}#${anchor.token} returned no range (mapping may be missing)`,
  );
  const hit = withRange.some((hover) => {
    const range = hover.range!;
    const start = doc.offsetAt(range.start);
    const end = doc.offsetAt(range.end);
    // Exact cover preferred; partial overlap with token is still a mapping signal.
    const coversToken = start <= tokenStart && end >= tokenEnd;
    const overlapsToken = start < tokenEnd && end > tokenStart;
    const caretInside = start <= tokenStart + 1 && end >= tokenStart + 1;
    return (
      coversToken ||
      (overlapsToken && Math.abs(end - start) >= Math.min(2, anchor.token.length)) ||
      caretInside
    );
  });
  assert.ok(
    hit,
    `hover range does not cover token ${anchor.token} @ ${tokenStart}-${tokenEnd}; ranges=${withRange
      .map((h) => {
        const r = h.range!;
        return `${doc.offsetAt(r.start)}-${doc.offsetAt(r.end)}`;
      })
      .join(", ")}`,
  );
  // Negative: range must not be wildly larger than the token (off-by-chunk mapping).
  for (const hover of withRange) {
    const start = doc.offsetAt(hover.range!.start);
    const end = doc.offsetAt(hover.range!.end);
    const width = Math.max(0, end - start);
    assert.ok(
      width <= Math.max(anchor.token.length * 4, 32),
      `hover range suspiciously wide (${width} chars) for token ${anchor.token} — possible map skew`,
    );
  }
  return hovers;
}

export async function foldingRangesFor(relative: string): Promise<vscode.FoldingRange[]> {
  const doc = await openRelative(relative);
  return (
    (await vscode.commands.executeCommand<vscode.FoldingRange[]>(
      "vscode.executeFoldingRangeProvider",
      doc.uri,
    )) ?? []
  );
}

export async function selectionRangesAt(
  anchor: TokenAnchor,
): Promise<vscode.SelectionRange[] | undefined> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  return vscode.commands.executeCommand<vscode.SelectionRange[]>(
    "vscode.executeSelectionRangeProvider",
    doc.uri,
    [position],
  );
}

export async function documentLinksFor(relative: string): Promise<vscode.DocumentLink[]> {
  const doc = await openRelative(relative);
  return (
    (await vscode.commands.executeCommand<vscode.DocumentLink[]>(
      "vscode.executeLinkProvider",
      doc.uri,
    )) ?? []
  );
}

export async function prepareCallHierarchyAt(
  anchor: TokenAnchor,
): Promise<vscode.CallHierarchyItem[] | undefined> {
  const doc = await openRelative(anchor.file);
  const position = tokenPosition(doc, anchor);
  try {
    return await vscode.commands.executeCommand<vscode.CallHierarchyItem[]>(
      "vscode.prepareCallHierarchy",
      doc.uri,
      position,
    );
  } catch {
    return undefined;
  }
}

export async function incomingCalls(
  item: vscode.CallHierarchyItem,
): Promise<vscode.CallHierarchyIncomingCall[] | undefined> {
  try {
    return await vscode.commands.executeCommand<vscode.CallHierarchyIncomingCall[]>(
      "vscode.provideIncomingCalls",
      item,
    );
  } catch {
    return undefined;
  }
}

export async function outgoingCalls(
  item: vscode.CallHierarchyItem,
): Promise<vscode.CallHierarchyOutgoingCall[] | undefined> {
  try {
    return await vscode.commands.executeCommand<vscode.CallHierarchyOutgoingCall[]>(
      "vscode.provideOutgoingCalls",
      item,
    );
  } catch {
    return undefined;
  }
}

/** Completions at an absolute offset in a relative file. */
export async function completionsAtOffset(
  relative: string,
  offset: number,
  triggerCharacter?: string,
): Promise<string[]> {
  return completionsAtOffsetUntil(
    relative,
    offset,
    (labels) => labels.length > 0,
    triggerCharacter,
  );
}

export async function completionsAtOffsetUntil(
  relative: string,
  offset: number,
  ready: (labels: readonly string[]) => boolean,
  triggerCharacter?: string,
): Promise<string[]> {
  const doc = await openRelative(relative);
  const position = doc.positionAt(offset);
  const list = await pollUntil(
    `completion@${relative}:${offset}`,
    async () =>
      (await vscode.commands.executeCommand<vscode.CompletionList>(
        "vscode.executeCompletionItemProvider",
        doc.uri,
        position,
        triggerCharacter,
      )) ?? { items: [], isIncomplete: false },
    (result) =>
      ready(
        (result.items ?? []).map((item) =>
          typeof item.label === "string" ? item.label : item.label.label,
        ),
      ),
    15_000,
  );
  return (list.items ?? []).map((item) =>
    typeof item.label === "string" ? item.label : item.label.label,
  );
}

export function findOffset(doc: vscode.TextDocument, needle: string, occurrence = 0): number {
  let offset = -1;
  for (let i = 0; i <= occurrence; i++) {
    offset = doc.getText().indexOf(needle, offset + 1);
    assert.notEqual(offset, -1, `missing needle ${needle}[${occurrence}]`);
  }
  return offset;
}

/**
 * Soft assert: hover text indicates a non-null / narrowed type (no bare `| null` only).
 */
export function assertHoverLooksNarrowed(text: string, requiredNeedles: readonly string[]): void {
  for (const needle of requiredNeedles) {
    assert.ok(text.includes(needle), `narrowed hover missing ${needle}: ${text}`);
  }
  assert.ok(!/:\s*any\b/.test(text), `narrowed hover is any: ${text}`);
}
