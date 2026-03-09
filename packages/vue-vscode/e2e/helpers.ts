import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";
import * as os from "os";
import * as assert from "assert";

// ── Environment ────────────────────────────────────────────────

export const FIXTURE_NAME = process.env.VERTER_E2E_FIXTURE || "single-project";
export const TYPE_PROVIDER = process.env.VERTER_E2E_TYPE_PROVIDER;
export const LOG_FILE = process.env.VERTER_E2E_LOG_FILE || path.join(os.tmpdir(), "verter-e2e.log");
export const TIMING_FILE = process.env.VERTER_E2E_TIMING_FILE || path.join(os.tmpdir(), "verter-e2e-timing.json");

export interface StartupTiming {
  activationStartMs?: number;
  typeProviderStartedMs?: number;
  lspReadyMs?: number;
  firstTypedCompletionMs?: number;
  firstDiagnosticMs?: number;
  providerKind?: "tsgo" | "tsserver" | "verter-only";
  activationToReadyMs?: number;
  activationToFirstTypedCompletionMs?: number;
}

/**
 * Check whether the LSP reached the "ready" state by looking for the
 * ready notification in the log file.
 */
export function isLspReady(): boolean {
  const log = readTestLog();
  return log.includes("Verter ready");
}

// ── Extension helpers ──────────────────────────────────────────

/**
 * Wait for the Verter extension to activate and the LSP to become ready.
 * Opens a .vue file to trigger LSP initialization (Verter only starts the
 * LSP when a Vue file is first opened), then polls the log file for
 * "Verter ready".
 */
export async function waitForExtensionReady(timeoutMs = 45_000): Promise<void> {
  const ext = vscode.extensions.getExtension("pikax.verter-vscode");
  assert.ok(ext, "Verter extension should be installed");

  if (!ext.isActive) {
    await ext.activate();
  }

  // Open a .vue file to trigger LSP startup — Verter only initializes
  // the language server when a Vue document is first opened.
  try {
    const appVuePath = getAppVuePath();
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (workspaceFolders && workspaceFolders.length > 0) {
      const fileUri = vscode.Uri.file(
        path.join(workspaceFolders[0].uri.fsPath, appVuePath),
      );
      if (fs.existsSync(fileUri.fsPath)) {
        const doc = await vscode.workspace.openTextDocument(fileUri);
        await vscode.window.showTextDocument(doc);
      }
    }
  } catch {
    // Best-effort — don't fail if file doesn't exist (e.g. bad fixture)
  }

  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    if (fs.existsSync(LOG_FILE)) {
      const content = fs.readFileSync(LOG_FILE, "utf-8");
      if (content.includes("Verter ready")) {
        return;
      }
    }
    await sleep(200);
  }

  // If we timed out, check if the extension at least activated
  assert.ok(ext.isActive, `Extension should have activated within ${timeoutMs}ms timeout`);
}

/**
 * Open a file from the current workspace folder.
 * Tries common locations: `src/App.vue`, `App.vue`, etc.
 */
export async function openVueFile(relativePath: string): Promise<vscode.TextDocument> {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  assert.ok(workspaceFolders && workspaceFolders.length > 0, "Workspace should have folders");

  const fileUri = vscode.Uri.file(
    path.join(workspaceFolders[0].uri.fsPath, relativePath),
  );
  const doc = await vscode.workspace.openTextDocument(fileUri);
  await vscode.window.showTextDocument(doc);
  return doc;
}

/**
 * Wait until the type provider has processed a file by probing completions.
 * When the type provider hasn't synced yet, completions return Text-kind or
 * empty results. Once synced, identifiers get proper kinds (Variable, Function).
 *
 * Auto-detects a probe position from `{{ identifier }}` patterns in the doc.
 * Falls back to hover probing on `defineProps`/`defineEmits` if no mustache found.
 */
export async function waitForFileReady(
  doc: vscode.TextDocument,
  options: {
    probePosition?: vscode.Position;
    expectedLabel?: string;
    timeoutMs?: number;
    intervalMs?: number;
  } = {},
): Promise<void> {
  const { timeoutMs = 20_000, intervalMs = 150 } = options;
  let { probePosition, expectedLabel } = options;

  // Auto-detect from mustache expressions if not provided
  if (!probePosition || !expectedLabel) {
    const text = doc.getText();
    const mustacheMatch = text.match(/\{\{\s*(\w+)\s*\}\}/);
    if (mustacheMatch) {
      const idx = text.indexOf(mustacheMatch[0]);
      // Position inside the identifier (after "{{ ")
      const identStart = idx + mustacheMatch[0].indexOf(mustacheMatch[1]);
      probePosition = probePosition || doc.positionAt(identStart);
      expectedLabel = expectedLabel || mustacheMatch[1];
    }
  }

  // Fallback: look for defineProps/defineEmits and use hover
  if (!probePosition || !expectedLabel) {
    const text = doc.getText();
    const macroMatch = text.match(/\b(defineProps|defineEmits)\b/);
    if (macroMatch) {
      const idx = text.indexOf(macroMatch[0]);
      const pos = doc.positionAt(idx);
      // Hover-based probe: wait until hover returns non-empty
      const start = Date.now();
      while (Date.now() - start < timeoutMs) {
        const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
          "vscode.executeHoverProvider",
          doc.uri,
          pos,
        );
        if (hovers && hovers.length > 0 && hovers[0].contents.length > 0) {
          return;
        }
        await sleep(intervalMs);
      }
      console.log(`    waitForFileReady: timed out (hover probe, ${timeoutMs}ms)`);
      return;
    }
    // No probe target found — just return
    console.log("    waitForFileReady: no probe target found, skipping");
    return;
  }

  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const completions = await vscode.commands.executeCommand<vscode.CompletionList>(
      "vscode.executeCompletionItemProvider",
      doc.uri,
      probePosition,
    );

    if (completions?.items) {
      const match = completions.items.find((item) => item.label === expectedLabel);
      if (match && match.kind !== undefined && match.kind !== vscode.CompletionItemKind.Text) {
        return;
      }
    }
    await sleep(intervalMs);
  }

  console.log(
    `    waitForFileReady: timed out waiting for "${expectedLabel}" ` +
    `to get typed completions (${timeoutMs}ms)`,
  );
}

/**
 * Resolve the main App.vue path for the current fixture.
 * Different fixtures place App.vue in different locations.
 */
export function getAppVuePath(): string {
  switch (FIXTURE_NAME) {
    case "monorepo":
      return "packages/app/src/App.vue";
    case "no-config":
    case "single-file":
      return "App.vue";
    default:
      return "src/App.vue";
  }
}

/**
 * Resolve the component file path for the current fixture.
 */
export function getCompVuePath(): string | undefined {
  switch (FIXTURE_NAME) {
    case "single-project":
    case "tsconfig-extends":
    case "tsconfig-references":
      return "src/MyComp.vue";
    case "path-aliases":
      return "src/components/MyComp.vue";
    case "composite-paths":
      return "src/components/HelloWorld.vue";
    case "barrel-exports":
      return "src/components/Overlay.vue";
    case "monorepo":
      return "packages/shared/src/SharedComp.vue";
    case "no-config":
      return "MyComp.vue";
    case "single-file":
      return undefined; // No component file
    default:
      return undefined;
  }
}

/**
 * Wait for diagnostics to appear on a document URI using event-based detection.
 * Subscribes to `onDidChangeDiagnostics` and resolves within milliseconds
 * of diagnostic arrival instead of polling every 500ms.
 */
export async function waitForDiagnostics(
  uri: vscode.Uri,
  options: { source?: string; timeoutMs?: number; minCount?: number; predicate?: (d: vscode.Diagnostic) => boolean } = {},
): Promise<vscode.Diagnostic[]> {
  const { source, timeoutMs = 30_000, minCount = 0, predicate } = options;

  const getFiltered = () => {
    let diags = vscode.languages.getDiagnostics(uri);
    if (source) {
      diags = diags.filter((d) => d.source === source);
    }
    return diags;
  };

  const isSatisfied = (diags: vscode.Diagnostic[]) => {
    if (predicate) return diags.some(predicate);
    return diags.length > minCount;
  };

  // Check if already satisfied
  const existing = getFiltered();
  if (isSatisfied(existing)) {
    return existing;
  }

  // Subscribe to diagnostic change events
  return new Promise<vscode.Diagnostic[]>((resolve) => {
    const timer = setTimeout(() => {
      sub.dispose();
      resolve(getFiltered());
    }, timeoutMs);

    const sub = vscode.languages.onDidChangeDiagnostics((e) => {
      const matched = e.uris.some((u) => u.toString() === uri.toString());
      if (!matched) return;

      const diags = getFiltered();
      if (isSatisfied(diags)) {
        clearTimeout(timer);
        sub.dispose();
        resolve(diags);
      }
    });
  });
}


/**
 * Execute the decoration state command (only available in E2E test mode).
 */
export async function getDecorationState(): Promise<DecorationState | undefined> {
  try {
    return await vscode.commands.executeCommand<DecorationState>("verter._getDecorationState");
  } catch {
    return undefined;
  }
}

export interface DecorationRange {
  startLine: number;
  startChar: number;
  endLine: number;
  endChar: number;
}

export interface DecorationState {
  bindingColors: Record<string, DecorationRange[]>;
  vueApiCalls: Record<string, DecorationRange[]>;
  propConstness: Record<string, DecorationRange[]>;
}

/**
 * Measure hover latency at a specific position.
 * Returns the hover result and the time it took in milliseconds.
 */
export async function measureHover(
  uri: vscode.Uri,
  position: vscode.Position,
): Promise<{ hovers: vscode.Hover[]; latencyMs: number }> {
  const start = Date.now();
  const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
    "vscode.executeHoverProvider",
    uri,
    position,
  );
  const latencyMs = Date.now() - start;
  return { hovers: hovers || [], latencyMs };
}

/**
 * Trigger a decoration refresh by making a no-op edit to the active document.
 * The decoration providers listen for `onDidChangeTextDocument` and re-apply
 * decorations when the active file changes. This inserts and immediately
 * removes a character to force the event to fire.
 */
export async function triggerDecorationRefresh(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor) return;

  const doc = editor.document;
  const lastLine = doc.lineAt(doc.lineCount - 1);
  const endPos = lastLine.range.end;

  // Insert a space at the end of the file, then immediately undo
  await editor.edit((editBuilder) => {
    editBuilder.insert(endPos, " ");
  });
  // Undo the edit to restore original content
  await vscode.commands.executeCommand("undo");
}

// ── Log file helpers ───────────────────────────────────────────

export function readTestLog(): string {
  if (!fs.existsSync(LOG_FILE)) return "";
  return fs.readFileSync(LOG_FILE, "utf-8");
}

export function assertLogContains(needle: string, message?: string): void {
  const log = readTestLog();
  assert.ok(
    log.includes(needle),
    message || `Expected log to contain "${needle}" but it did not. Log length: ${log.length} chars`,
  );
}

export function assertLogNotContains(needle: string, message?: string): void {
  const log = readTestLog();
  assert.ok(
    !log.includes(needle),
    message || `Expected log NOT to contain "${needle}" but it did`,
  );
}

// ── Timing helpers ─────────────────────────────────────────────

/**
 * Parse startup timing from the log file.
 * Looks for [TIMING] markers written by the extension in test mode.
 */
export function parseStartupTiming(): StartupTiming {
  const log = readTestLog();
  const activationMatch = log.match(/\[TIMING\] activation_start (\d+)/);
  const typeProviderMatch = log.match(
    /\[TIMING\] type_provider_started (\d+) (tsgo|tsserver)/,
  );
  const readyMatch = log.match(/\[TIMING\] ready (\d+)/);
  const typedCompletionMatch = log.match(
    /\[TIMING\] first_typed_completion (\d+) [^\s]+ [^\s]+/,
  );
  const firstDiagnosticMatch = log.match(/\[TIMING\] first_diagnostic (\d+)/);

  const activationStartMs = activationMatch
    ? parseInt(activationMatch[1], 10)
    : undefined;
  const typeProviderStartedMs = typeProviderMatch
    ? parseInt(typeProviderMatch[1], 10)
    : undefined;
  const lspReadyMs = readyMatch ? parseInt(readyMatch[1], 10) : undefined;
  const firstTypedCompletionMs = typedCompletionMatch
    ? parseInt(typedCompletionMatch[1], 10)
    : undefined;
  const firstDiagnosticMs = firstDiagnosticMatch
    ? parseInt(firstDiagnosticMatch[1], 10)
    : undefined;
  const providerKind = typeProviderMatch
    ? (typeProviderMatch[2] as StartupTiming["providerKind"])
    : log.includes("verter-only mode")
      ? "verter-only"
      : undefined;

  return {
    activationStartMs,
    typeProviderStartedMs,
    lspReadyMs,
    firstTypedCompletionMs,
    firstDiagnosticMs,
    providerKind,
    activationToReadyMs:
      activationStartMs !== undefined && lspReadyMs !== undefined
        ? lspReadyMs - activationStartMs
        : undefined,
    activationToFirstTypedCompletionMs:
      activationStartMs !== undefined && firstTypedCompletionMs !== undefined
        ? firstTypedCompletionMs - activationStartMs
        : undefined,
  };
}

// ── IDE Feature Helpers ────────────────────────────────────────

/**
 * Get completions at a position.
 */
export async function getCompletions(
  uri: vscode.Uri,
  position: vscode.Position,
): Promise<vscode.CompletionList | undefined> {
  return vscode.commands.executeCommand<vscode.CompletionList>(
    "vscode.executeCompletionItemProvider",
    uri,
    position,
  );
}

/**
 * Get go-to-definition results at a position.
 */
export async function getDefinitions(
  uri: vscode.Uri,
  pos: vscode.Position,
): Promise<vscode.Location[]> {
  const locations = await vscode.commands.executeCommand<vscode.Location[]>(
    "vscode.executeDefinitionProvider",
    uri,
    pos,
  );
  return locations || [];
}

/**
 * Prepare rename at a position (check if rename is allowed).
 */
export async function getPrepareRename(
  uri: vscode.Uri,
  position: vscode.Position,
): Promise<vscode.Range | { range: vscode.Range; placeholder: string } | undefined> {
  try {
    return await vscode.commands.executeCommand(
      "vscode.prepareRename",
      uri,
      position,
    );
  } catch {
    return undefined;
  }
}

/**
 * Execute rename at a position with a new name.
 */
export async function getRenameEdits(
  uri: vscode.Uri,
  position: vscode.Position,
  newName: string,
): Promise<vscode.WorkspaceEdit | undefined> {
  try {
    return await vscode.commands.executeCommand<vscode.WorkspaceEdit>(
      "vscode.executeRenameProvider",
      uri,
      position,
      newName,
    );
  } catch {
    return undefined;
  }
}

/**
 * Get all references at a position.
 */
export async function getReferences(
  uri: vscode.Uri,
  position: vscode.Position,
): Promise<vscode.Location[]> {
  const locations = await vscode.commands.executeCommand<vscode.Location[]>(
    "vscode.executeReferenceProvider",
    uri,
    position,
  );
  return locations || [];
}

/**
 * Get document symbols for a file.
 */
export async function getDocumentSymbols(
  uri: vscode.Uri,
): Promise<vscode.DocumentSymbol[] | vscode.SymbolInformation[]> {
  const symbols = await vscode.commands.executeCommand<
    vscode.DocumentSymbol[] | vscode.SymbolInformation[]
  >("vscode.executeDocumentSymbolProvider", uri);
  return symbols || [];
}

/**
 * Find position of `needle` in document text, offset by `charOffset` into the match.
 */
export function findPosition(
  doc: vscode.TextDocument,
  needle: string,
  charOffset = 0,
): vscode.Position | undefined {
  const idx = doc.getText().indexOf(needle);
  if (idx === -1) return undefined;
  return doc.positionAt(idx + charOffset);
}

/**
 * Find the Nth occurrence of `needle` (0-indexed) and return position at charOffset into it.
 */
export function findNthPosition(
  doc: vscode.TextDocument,
  needle: string,
  n: number,
  charOffset = 0,
): vscode.Position | undefined {
  const text = doc.getText();
  let idx = -1;
  for (let i = 0; i <= n; i++) {
    idx = text.indexOf(needle, idx + 1);
    if (idx === -1) return undefined;
  }
  return doc.positionAt(idx + charOffset);
}

// ── Utilities ──────────────────────────────────────────────────

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
