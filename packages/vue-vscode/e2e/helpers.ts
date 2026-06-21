import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";
import * as os from "os";
import * as assert from "assert";
import { readE2eEnv } from "../src/e2eEnv";
import { computeStartupSegments } from "../src/startupOptimizations";

// ── Environment ────────────────────────────────────────────────

export const FIXTURE_NAME = readE2eEnv("FIXTURE") || "single-project";
export const TYPE_PROVIDER = readE2eEnv("TYPE_PROVIDER");
export const LOG_FILE = readE2eEnv("LOG_FILE") || path.join(os.tmpdir(), "verter-e2e.log");
export const TIMING_FILE =
  readE2eEnv("TIMING_FILE") || path.join(os.tmpdir(), "verter-e2e-timing.json");

export interface StartupTiming {
  activationStartMs?: number;
  typeProviderStartedMs?: number;
  lspReadyMs?: number;
  firstTypedCompletionMs?: number;
  firstTypedCompletionLabel?: string;
  firstTypedCompletionKind?: string;
  firstDiagnosticMs?: number;
  providerKind?: "tsgo" | "tsserver" | "verter-only";
  typeProviderReason?: string;
  activationToReadyMs?: number;
  activationToFirstTypedCompletionMs?: number;
  readyToFirstTypedCompletionMs?: number;
  activationToTypeProviderStartedMs?: number;
  typeProviderStartedToFirstTypedCompletionMs?: number;
  typeProviderStartedToReadyMs?: number;
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
      const fileUri = vscode.Uri.file(path.join(workspaceFolders[0].uri.fsPath, appVuePath));
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

  const fileUri = vscode.Uri.file(path.join(workspaceFolders[0].uri.fsPath, relativePath));
  const doc = await vscode.workspace.openTextDocument(fileUri);
  await vscode.window.showTextDocument(doc);
  return doc;
}

export async function openAndReady(
  relativePath: string,
  options: Parameters<typeof waitForFileReady>[1] = {},
): Promise<vscode.TextDocument> {
  const doc = await openVueFile(relativePath);
  await waitForFileReady(doc, options);
  return doc;
}

export async function revealDefinition(
  uri: vscode.Uri,
  position: vscode.Position,
): Promise<{
  uri: vscode.Uri;
  selection: vscode.Selection;
}> {
  const config = vscode.workspace.getConfiguration("editor");
  await config.update(
    "gotoLocation.multipleDefinitions",
    "goto",
    vscode.ConfigurationTarget.Workspace,
  );

  const doc = await vscode.workspace.openTextDocument(uri);
  const editor = await vscode.window.showTextDocument(doc);
  editor.selection = new vscode.Selection(position, position);
  editor.revealRange(new vscode.Range(position, position), vscode.TextEditorRevealType.InCenter);

  const sourceUri = uri.toString();
  await vscode.commands.executeCommand("editor.action.revealDefinition");

  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    const active = vscode.window.activeTextEditor;
    if (active && active.document.uri.toString() !== sourceUri) {
      return {
        uri: active.document.uri,
        selection: active.selection,
      };
    }
    await sleep(100);
  }

  const active = vscode.window.activeTextEditor;
  assert.ok(active, "Expected an active editor after revealDefinition");
  return {
    uri: active.document.uri,
    selection: active.selection,
  };
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
    expectedKinds?: vscode.CompletionItemKind[];
    triggerCharacter?: string;
    timeoutMs?: number;
    intervalMs?: number;
  } = {},
): Promise<void> {
  const { timeoutMs = 20_000, intervalMs = 150, triggerCharacter } = options;
  let { probePosition, expectedLabel, expectedKinds } = options;

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
      triggerCharacter,
    );

    if (completions?.items) {
      const match = completions.items.find((item) => item.label === expectedLabel);
      if (
        match &&
        match.kind !== undefined &&
        matchesExpectedCompletionKind(match.kind, expectedKinds)
      ) {
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
 * Run the on-type formatter as if the user typed the `>` immediately AFTER
 * `marker` in `doc`, returning the concatenated inserted text (empty when the
 * provider returns no edit).
 *
 * `marker` must end with the `>` whose typing triggers the formatter; the
 * cursor is placed at the position right after it, and `>` is passed as the
 * trigger character — exactly what VS Code dispatches when `editor.formatOnType`
 * fires on a `>` keystroke.
 */
export async function runFormatOnTypeAfter(
  doc: vscode.TextDocument,
  marker: string,
): Promise<string> {
  const idx = doc.getText().indexOf(marker);
  if (idx < 0) {
    throw new Error(`runFormatOnTypeAfter: marker "${marker}" not found in document`);
  }
  const position = doc.positionAt(idx + marker.length);
  const edits =
    (await vscode.commands.executeCommand<vscode.TextEdit[]>(
      "vscode.executeFormatOnTypeProvider",
      doc.uri,
      position,
      ">",
      { tabSize: 2, insertSpaces: true },
    )) || [];
  return edits.map((e) => e.newText).join("");
}

/**
 * Deterministically wait until the on-type auto-close provider is live for
 * `doc` by repeatedly driving it against a KNOWN-positive control tag until it
 * inserts that tag's closing tag.
 *
 * This is the readiness probe the auto-close suite needs: the auto-close
 * scratch carriers carry no `{{ }}` interpolation and no `defineProps` macro, so
 * `waitForFileReady` returns immediately WITHOUT proving the document reached
 * the LSP. A positive on-type round-trip proves BOTH that the provider is wired
 * AND that the LSP has processed this document — so a subsequent assertion that
 * a void / generic / script `>` yields NO edit distinguishes "ready + correctly
 * no edit" from "provider not ready yet".
 *
 * `controlMarker` must be a markup open tag (ending in `>`) that the server
 * MUST close (e.g. `<section>` in the template region) and whose
 * `expectedClose` (e.g. `</section>`) is not already present after it. Returns
 * when the round-trip succeeds; throws on timeout so the suite fails loudly
 * rather than silently testing an un-ready provider.
 */
export async function waitForOnTypeReady(
  doc: vscode.TextDocument,
  controlMarker: string,
  expectedClose: string,
  options: { timeoutMs?: number; intervalMs?: number } = {},
): Promise<void> {
  const { timeoutMs = 20_000, intervalMs = 150 } = options;
  const start = Date.now();
  let last = "";
  while (Date.now() - start < timeoutMs) {
    last = await runFormatOnTypeAfter(doc, controlMarker);
    if (last === expectedClose) {
      return;
    }
    await sleep(intervalMs);
  }
  throw new Error(
    `waitForOnTypeReady: on-type auto-close did not produce "${expectedClose}" for ` +
      `control "${controlMarker}" within ${timeoutMs}ms (last="${last}") — the LSP/provider ` +
      `never became ready, so negative auto-close assertions cannot be trusted`,
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
  options: {
    source?: string;
    timeoutMs?: number;
    minCount?: number;
    predicate?: (d: vscode.Diagnostic) => boolean;
  } = {},
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
 * Wait until no diagnostics matching the predicate remain on a document.
 * This is useful for flaky resolution scenarios where diagnostics briefly
 * appear before the type provider finishes reconciling imports.
 */
export async function waitForNoDiagnosticsMatching(
  uri: vscode.Uri,
  options: {
    source?: string;
    timeoutMs?: number;
    intervalMs?: number;
    stableMs?: number;
    predicate: (d: vscode.Diagnostic) => boolean;
  },
): Promise<vscode.Diagnostic[]> {
  const { source, timeoutMs = 30_000, intervalMs = 150, stableMs = 400, predicate } = options;

  const getFiltered = () => {
    let diags = vscode.languages.getDiagnostics(uri);
    if (source) {
      diags = diags.filter((d) => d.source === source);
    }
    return diags;
  };

  const start = Date.now();
  let clearSince: number | undefined;

  while (Date.now() - start < timeoutMs) {
    const diags = getFiltered();
    const matching = diags.filter(predicate);

    if (matching.length === 0) {
      clearSince ??= Date.now();
      if (Date.now() - clearSince >= stableMs) {
        return diags;
      }
    } else {
      clearSince = undefined;
    }

    await sleep(intervalMs);
  }

  return getFiltered();
}

/**
 * Wait until diagnostics are stable (no changes for stableMs).
 * Returns whatever diagnostics exist at that point (may be empty).
 * Use for "I want to see what diagnostics look like after processing"
 * without requiring any specific predicate.
 */
export async function waitForDiagnosticsSettled(
  uri: vscode.Uri,
  options?: {
    source?: string;
    timeoutMs?: number;
    stableMs?: number;
  },
): Promise<vscode.Diagnostic[]> {
  const { source, timeoutMs = 5_000, stableMs = 500 } = options ?? {};

  const getFiltered = () => {
    let diags = vscode.languages.getDiagnostics(uri);
    if (source) {
      diags = diags.filter((d) => d.source === source);
    }
    return diags;
  };

  return new Promise<vscode.Diagnostic[]>((resolve) => {
    let stableTimer: ReturnType<typeof setTimeout> | undefined;

    const finish = () => {
      if (stableTimer) clearTimeout(stableTimer);
      clearTimeout(deadlineTimer);
      sub.dispose();
      resolve(getFiltered());
    };

    // Start the quiescence timer — if no events fire for stableMs, resolve
    const resetStableTimer = () => {
      if (stableTimer) clearTimeout(stableTimer);
      stableTimer = setTimeout(finish, stableMs);
    };

    // Absolute deadline
    const deadlineTimer = setTimeout(finish, timeoutMs);

    const sub = vscode.languages.onDidChangeDiagnostics((e) => {
      const matched = e.uris.some((u) => u.toString() === uri.toString());
      if (!matched) return;
      // Reset the quiescence timer on each relevant change
      resetStableTimer();
    });

    // Start the initial quiescence timer
    resetStableTimer();
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
  const timeoutMs = 20_000;
  const intervalMs = 150;
  const deadline = Date.now() + timeoutMs;

  let latest: vscode.Hover[] = [];
  let latestLatencyMs = 0;

  while (Date.now() < deadline) {
    const start = Date.now();
    latest =
      (await vscode.commands.executeCommand<vscode.Hover[]>(
        "vscode.executeHoverProvider",
        uri,
        position,
      )) || [];
    latestLatencyMs = Date.now() - start;

    if (latest.length > 0) {
      return { hovers: latest, latencyMs: latestLatencyMs };
    }

    await sleep(intervalMs);
  }

  return { hovers: latest, latencyMs: latestLatencyMs };
}

export async function waitForHoverMatching(
  uri: vscode.Uri,
  position: vscode.Position,
  options: {
    timeoutMs?: number;
    intervalMs?: number;
    stableMs?: number;
    predicate: (hovers: readonly vscode.Hover[]) => boolean;
  },
): Promise<vscode.Hover[]> {
  const { timeoutMs = 20_000, intervalMs = 150, stableMs = 400, predicate } = options;

  const start = Date.now();
  let matchedSince: number | undefined;
  let latest: vscode.Hover[] = [];

  while (Date.now() - start < timeoutMs) {
    latest =
      (await vscode.commands.executeCommand<vscode.Hover[]>(
        "vscode.executeHoverProvider",
        uri,
        position,
      )) || [];

    if (predicate(latest)) {
      matchedSince ??= Date.now();
      if (Date.now() - matchedSince >= stableMs) {
        return latest;
      }
    } else {
      matchedSince = undefined;
    }

    await sleep(intervalMs);
  }

  return latest;
}

export function hoverText(hover: vscode.Hover): string {
  return hover.contents
    .map((content) => (typeof content === "string" ? content : content.value))
    .join("\n");
}

export async function getHoverText(uri: vscode.Uri, position: vscode.Position): Promise<string> {
  const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
    "vscode.executeHoverProvider",
    uri,
    position,
  );

  assert.ok(
    hovers && hovers.length > 0,
    `Expected hover results at ${uri.toString()} ${position.line}:${position.character}`,
  );
  const text = hoverText(hovers[0]);
  assert.ok(
    text.trim().length > 0,
    `Expected non-empty hover text at ${uri.toString()} ${position.line}:${position.character}`,
  );
  return text;
}

export async function expectHoverContains(
  uri: vscode.Uri,
  position: vscode.Position,
  expected: string | string[],
): Promise<string> {
  const text = await getHoverText(uri, position);
  const needles = Array.isArray(expected) ? expected : [expected];
  for (const needle of needles) {
    assert.ok(
      text.includes(needle),
      `Expected hover text to contain "${needle}" but got:\n${text}`,
    );
  }
  return text;
}

export async function expectHoverNotContains(
  uri: vscode.Uri,
  position: vscode.Position,
  unexpected: string | string[],
): Promise<string> {
  const text = await getHoverText(uri, position);
  const needles = Array.isArray(unexpected) ? unexpected : [unexpected];
  for (const needle of needles) {
    assert.ok(
      !text.includes(needle),
      `Expected hover text to NOT contain "${needle}" but got:\n${text}`,
    );
  }
  return text;
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

/**
 * Wait for the workspace scanner to finish syncing all files to the type provider.
 *
 * Reads the last "Verter ready (init generation N)" from the log to get the
 * current generation, then polls for "TypeProviderSyncComplete (init generation M)"
 * where M >= N. This prevents stale scanner signals from satisfying the wait.
 */
export async function waitForTypeProviderSync(timeoutMs = 30_000): Promise<void> {
  const start = Date.now();

  // Find current init generation from the ready notification
  let currentGen: number | undefined;
  while (Date.now() - start < timeoutMs) {
    const log = readTestLog();
    const readyMatches = log.matchAll(/Verter ready \(init generation (\d+)\)/g);
    for (const m of readyMatches) {
      const gen = parseInt(m[1], 10);
      if (currentGen === undefined || gen > currentGen) {
        currentGen = gen;
      }
    }
    if (currentGen !== undefined) break;
    await sleep(200);
  }

  assert.ok(
    currentGen !== undefined,
    `waitForTypeProviderSync: no "Verter ready" notification found in log within ${timeoutMs}ms`,
  );

  // Poll for TypeProviderSyncComplete with gen >= currentGen
  while (Date.now() - start < timeoutMs) {
    const log = readTestLog();
    const syncMatches = log.matchAll(/TypeProviderSyncComplete \(init generation (\d+)\)/g);
    for (const m of syncMatches) {
      const gen = parseInt(m[1], 10);
      if (gen >= currentGen) {
        return;
      }
    }
    await sleep(200);
  }

  assert.fail(
    `waitForTypeProviderSync: timed out waiting for TypeProviderSyncComplete ` +
      `(gen >= ${currentGen}, ${timeoutMs}ms)`,
  );
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
    message ||
      `Expected log to contain "${needle}" but it did not. Log length: ${log.length} chars`,
  );
}

export function assertLogNotContains(needle: string, message?: string): void {
  const log = readTestLog();
  assert.ok(!log.includes(needle), message || `Expected log NOT to contain "${needle}" but it did`);
}

// ── Timing helpers ─────────────────────────────────────────────

/**
 * Parse startup timing from the log file.
 * Looks for [TIMING] markers written by the extension in test mode.
 */
export function parseStartupTiming(): StartupTiming {
  const log = readTestLog();
  const activationMatch = log.match(/\[TIMING\] activation_start (\d+)/);
  const typeProviderMatch = log.match(/\[TIMING\] type_provider_started (\d+) (tsgo|tsserver)/);
  const readyMatch = log.match(/\[TIMING\] ready (\d+)/);
  const typedCompletionMatch = log.match(
    /\[TIMING\] first_typed_completion (\d+) ([^\s]+) ([^\s]+)/,
  );
  const firstDiagnosticMatch = log.match(/\[TIMING\] first_diagnostic (\d+)/);
  const typeProviderStatusMatches = Array.from(
    log.matchAll(/Type provider status:\s+(tsgo|tsserver|none)(?: \((.+?)\))?/g),
  );
  const lastTypeProviderStatus = typeProviderStatusMatches[typeProviderStatusMatches.length - 1];

  const activationStartMs = activationMatch ? parseInt(activationMatch[1], 10) : undefined;
  const typeProviderStartedMs = typeProviderMatch ? parseInt(typeProviderMatch[1], 10) : undefined;
  const lspReadyMs = readyMatch ? parseInt(readyMatch[1], 10) : undefined;
  const firstTypedCompletionMs = typedCompletionMatch
    ? parseInt(typedCompletionMatch[1], 10)
    : undefined;
  const firstTypedCompletionLabel = typedCompletionMatch?.[2];
  const firstTypedCompletionKind = typedCompletionMatch?.[3];
  const firstDiagnosticMs = firstDiagnosticMatch
    ? parseInt(firstDiagnosticMatch[1], 10)
    : undefined;
  const providerKind = typeProviderMatch
    ? (typeProviderMatch[2] as StartupTiming["providerKind"])
    : lastTypeProviderStatus?.[1] === "none"
      ? "verter-only"
      : ((lastTypeProviderStatus?.[1] as Exclude<StartupTiming["providerKind"], "verter-only">) ??
        (log.includes("verter-only mode") ? "verter-only" : undefined));
  const typeProviderReason =
    lastTypeProviderStatus?.[1] === "none" ? lastTypeProviderStatus[2] : undefined;

  const segments = computeStartupSegments({
    activationStartMs,
    typeProviderStartedMs,
    lspReadyMs,
    firstTypedCompletionMs,
  });

  return {
    activationStartMs,
    typeProviderStartedMs,
    lspReadyMs,
    firstTypedCompletionMs,
    firstTypedCompletionLabel,
    firstTypedCompletionKind,
    firstDiagnosticMs,
    providerKind,
    typeProviderReason,
    ...segments,
  };
}

// ── IDE Feature Helpers ────────────────────────────────────────

/**
 * Get completions at a position.
 */
export async function getCompletions(
  uri: vscode.Uri,
  position: vscode.Position,
  triggerCharacter?: string,
): Promise<vscode.CompletionList | undefined> {
  return vscode.commands.executeCommand<vscode.CompletionList>(
    "vscode.executeCompletionItemProvider",
    uri,
    position,
    triggerCharacter,
  );
}

export async function measureCompletion(
  uri: vscode.Uri,
  position: vscode.Position,
  triggerCharacter?: string,
): Promise<{ completions: vscode.CompletionList | undefined; latencyMs: number }> {
  const start = Date.now();
  const completions = await getCompletions(uri, position, triggerCharacter);
  return {
    completions,
    latencyMs: Date.now() - start,
  };
}

export async function waitForCompletionsMatching(
  uri: vscode.Uri,
  position: vscode.Position,
  options: {
    timeoutMs?: number;
    intervalMs?: number;
    stableMs?: number;
    triggerCharacter?: string;
    predicate: (list: vscode.CompletionList | undefined) => boolean;
  },
): Promise<vscode.CompletionList | undefined> {
  const {
    timeoutMs = 20_000,
    intervalMs = 150,
    stableMs = 400,
    triggerCharacter,
    predicate,
  } = options;

  const start = Date.now();
  let matchedSince: number | undefined;
  let lastList: vscode.CompletionList | undefined;

  while (Date.now() - start < timeoutMs) {
    lastList = await getCompletions(uri, position, triggerCharacter);

    if (predicate(lastList)) {
      matchedSince ??= Date.now();
      if (Date.now() - matchedSince >= stableMs) {
        return lastList;
      }
    } else {
      matchedSince = undefined;
    }

    await sleep(intervalMs);
  }

  return lastList;
}

export async function measureTimeToCompletionsMatching(
  uri: vscode.Uri,
  position: vscode.Position,
  options: Parameters<typeof waitForCompletionsMatching>[2],
): Promise<{ completions: vscode.CompletionList | undefined; latencyMs: number }> {
  const start = Date.now();
  const completions = await waitForCompletionsMatching(uri, position, options);
  return {
    completions,
    latencyMs: Date.now() - start,
  };
}

export function getCompletionLabel(item: vscode.CompletionItem): string {
  return typeof item.label === "string" ? item.label : item.label.label;
}

export function getCompletionItem(
  completions: vscode.CompletionList,
  label: string,
): vscode.CompletionItem | undefined {
  return completions.items.find((item) => getCompletionLabel(item) === label);
}

export function expectCompletionHas(
  completions: vscode.CompletionList,
  label: string,
  options: { allowText?: boolean } = {},
): vscode.CompletionItem {
  const item = getCompletionItem(completions, label);
  assert.ok(item, `Expected completion "${label}" to be present`);
  assert.notStrictEqual(item!.kind, undefined, `Expected completion "${label}" to have a kind`);
  if (!options.allowText) {
    assert.notStrictEqual(
      item!.kind,
      vscode.CompletionItemKind.Text,
      `Expected completion "${label}" to not be Text`,
    );
  }
  return item!;
}

export function expectCompletionMissing(completions: vscode.CompletionList, label: string): void {
  const item = getCompletionItem(completions, label);
  assert.ok(!item, `Expected completion "${label}" to be absent`);
}

export function expectCompletionKind(
  completions: vscode.CompletionList,
  label: string,
  kinds: vscode.CompletionItemKind | vscode.CompletionItemKind[],
  options: { allowText?: boolean } = {},
): vscode.CompletionItem {
  const item = expectCompletionHas(completions, label, options);
  const allowedKinds = Array.isArray(kinds) ? kinds : [kinds];
  assert.ok(
    allowedKinds.includes(item.kind!),
    `Expected completion "${label}" kind ${item.kind} to be one of [${allowedKinds.join(", ")}]`,
  );
  return item;
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
    return await vscode.commands.executeCommand("vscode.prepareRename", uri, position);
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

// ── Warm Session API ───────────────────────────────────────────
// Singleton state — one warm context per fixture@provider run.
// The root `beforeAll` in suite/index.ts calls these once; individual
// suites skip redundant polling by checking the cached flags.

let _fixtureWarm = false;
let _typeProviderSynced = false;
const _fileReadyCache = new Map<string, boolean>();

/** Run once per fixture@provider. Idempotent. */
export async function ensureFixtureWarm(): Promise<void> {
  if (_fixtureWarm) return;
  await waitForExtensionReady();
  _fixtureWarm = true;
}

/** Run once per fixture@provider after ensureFixtureWarm. Idempotent. */
export async function ensureTypeProviderSynced(): Promise<void> {
  if (_typeProviderSynced) return;
  await ensureFixtureWarm();
  await waitForTypeProviderSync();
  _typeProviderSynced = true;
}

/** Opens file, waits for readiness, caches result. */
export async function openReadyCached(
  relativePath: string,
  options?: Parameters<typeof waitForFileReady>[1],
): Promise<vscode.TextDocument> {
  const doc = await openVueFile(relativePath);
  const cacheKey = `${relativePath}:${JSON.stringify(options ?? {})}`;
  if (!_fileReadyCache.has(cacheKey)) {
    await waitForFileReady(doc, options);
    _fileReadyCache.set(cacheKey, true);
  }
  return doc;
}

/** Invalidate a cached file (after mutation tests). */
export function invalidateFileCache(relativePath: string): void {
  for (const key of _fileReadyCache.keys()) {
    if (key.startsWith(relativePath + ":")) {
      _fileReadyCache.delete(key);
    }
  }
}

// ── Utilities ──────────────────────────────────────────────────

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Get code actions for a range, optionally filtered by kind.
 */
export async function getCodeActions(
  uri: vscode.Uri,
  range: vscode.Range,
  kind?: vscode.CodeActionKind,
): Promise<(vscode.CodeAction | vscode.Command)[]> {
  const result = await vscode.commands.executeCommand<(vscode.CodeAction | vscode.Command)[]>(
    "vscode.executeCodeActionProvider",
    uri,
    range,
    kind?.value,
  );
  return result || [];
}

/**
 * Poll code actions until the expected action set is present and stable.
 * This avoids one-shot assertions while diagnostics and provider state are still settling.
 */
export async function waitForCodeActionsMatching(
  uri: vscode.Uri,
  range: vscode.Range,
  options: {
    kind?: vscode.CodeActionKind;
    timeoutMs?: number;
    intervalMs?: number;
    stableMs?: number;
    predicate: (items: readonly (vscode.CodeAction | vscode.Command)[]) => boolean;
  },
): Promise<(vscode.CodeAction | vscode.Command)[]> {
  const { kind, timeoutMs = 20_000, intervalMs = 150, stableMs = 400, predicate } = options;

  const start = Date.now();
  let matchedSince: number | undefined;
  let lastItems: (vscode.CodeAction | vscode.Command)[] = [];

  while (Date.now() - start < timeoutMs) {
    lastItems = await getCodeActions(uri, range, kind);

    if (predicate(lastItems)) {
      matchedSince ??= Date.now();
      if (Date.now() - matchedSince >= stableMs) {
        return lastItems;
      }
    } else {
      matchedSince = undefined;
    }

    await sleep(intervalMs);
  }

  return lastItems;
}

/**
 * Get inlay hints for a range.
 */
export async function getInlayHints(
  uri: vscode.Uri,
  range: vscode.Range,
): Promise<vscode.InlayHint[]> {
  const hints = await vscode.commands.executeCommand<vscode.InlayHint[]>(
    "vscode.executeInlayHintProvider",
    uri,
    range,
  );
  return hints || [];
}

function matchesExpectedCompletionKind(
  actualKind: vscode.CompletionItemKind,
  expectedKinds?: readonly vscode.CompletionItemKind[],
): boolean {
  if (expectedKinds && expectedKinds.length > 0) {
    return expectedKinds.includes(actualKind);
  }
  return actualKind !== vscode.CompletionItemKind.Text;
}
