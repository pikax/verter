import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";
import * as os from "os";
import * as assert from "assert";

// ── Environment ────────────────────────────────────────────────

export const FIXTURE_NAME = process.env.VERTER_E2E_FIXTURE || "single-project";
export const LOG_FILE = process.env.VERTER_E2E_LOG_FILE || path.join(os.tmpdir(), "verter-e2e.log");
export const TIMING_FILE = process.env.VERTER_E2E_TIMING_FILE || path.join(os.tmpdir(), "verter-e2e-timing.json");

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
export async function waitForExtensionReady(timeoutMs = 60_000): Promise<void> {
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
    await sleep(500);
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
 * Wait for diagnostics to appear on a document URI.
 * Optionally filter by source (e.g., "ts", "Verter").
 */
export async function waitForDiagnostics(
  uri: vscode.Uri,
  options: { source?: string; timeoutMs?: number; minCount?: number } = {},
): Promise<vscode.Diagnostic[]> {
  const { source, timeoutMs = 30_000, minCount = 0 } = options;
  const start = Date.now();

  while (Date.now() - start < timeoutMs) {
    let diags = vscode.languages.getDiagnostics(uri);
    if (source) {
      diags = diags.filter((d) => d.source === source);
    }
    if (diags.length > minCount) {
      return diags;
    }
    await sleep(500);
  }

  // Return whatever we have (possibly empty)
  let diags = vscode.languages.getDiagnostics(uri);
  if (source) {
    diags = diags.filter((d) => d.source === source);
  }
  return diags;
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
export function parseStartupTiming(): { activationMs?: number; readyMs?: number; deltaMs?: number } {
  const log = readTestLog();
  const activationMatch = log.match(/\[TIMING\] activation_start (\d+)/);
  const readyMatch = log.match(/\[TIMING\] ready (\d+)/);

  const activationMs = activationMatch ? parseInt(activationMatch[1], 10) : undefined;
  const readyMs = readyMatch ? parseInt(readyMatch[1], 10) : undefined;
  const deltaMs = activationMs && readyMs ? readyMs - activationMs : undefined;

  return { activationMs, readyMs, deltaMs };
}

// ── Utilities ──────────────────────────────────────────────────

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
