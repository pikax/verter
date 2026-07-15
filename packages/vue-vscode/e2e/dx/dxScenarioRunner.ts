/**
 * In-host wiring for the extension-host DX driver.
 *
 * This is the only DX module that touches the real `vscode` runtime API. It binds
 * the unit-tested pure cores (`dxReadiness`, `dxTyping`, `dxAcceptCompletion`,
 * `dxLogCanary`) to live VS Code I/O and the existing `helpers.ts` substrate. It
 * holds NO comparison/normalization/collector/report logic — that lives in
 * `@verter/dx-harness` and is driven Node-side.
 *
 * Startup readiness runs the ONE shared startup-gate engine: this module loads the
 * harness's `parseExtensionStartupLog` from the dependency-light, CJS-compatible
 * `@verter/dx-harness/startup-gate` subpath via a runtime `import()` (the in-host
 * suite compiles CommonJS; `importEsm` keeps the genuine dynamic import) and injects
 * it into the wait loop. There is no in-host re-implementation of the fold.
 */
import * as vscode from "vscode";

import { readTestLog, sleep } from "../helpers";
import {
  acceptCompletion,
  type AcceptCompletionDeps,
  type AcceptOutcome,
} from "./dxAcceptCompletion";
import { diagnoseCanary, summarizeCanaryLog, type CanaryVerdict } from "./dxLogCanary";
import {
  waitForDxReadiness,
  type DxReadinessOptions,
  type EvaluateStartupLog,
} from "./dxReadiness";
import { importEsm } from "./esmImport";

/** Default time to let the suggestion widget populate between accept commands. */
const DEFAULT_SETTLE_MS = 300;

/** The shape consumed from the harness `@verter/dx-harness/startup-gate` subpath. */
interface HarnessStartupGate {
  parseExtensionStartupLog: EvaluateStartupLog;
}

const HARNESS_STARTUP_GATE_SPECIFIER = "@verter/dx-harness/startup-gate";
let startupGatePromise: Promise<HarnessStartupGate> | undefined;

/** Load (once) the harness startup-gate parser/fold — the single shared engine. */
function loadHarnessStartupGate(): Promise<HarnessStartupGate> {
  return (startupGatePromise ??= importEsm<HarnessStartupGate>(HARNESS_STARTUP_GATE_SPECIFIER));
}

/** Build the readiness gate I/O for a document from live VS Code + the log file. */
export function dxReadinessDeps(uri: vscode.Uri): Omit<DxReadinessOptions, "evaluateLog"> {
  return {
    readLog: () => readTestLog(),
    sampleQuiescence: () => ({
      diagnosticsCount: vscode.languages.getDiagnostics(uri).length,
      logLength: readTestLog().length,
    }),
    sleep,
    now: () => Date.now(),
  };
}

/**
 * Wait for the matching-generation newest startup gate AND diagnostics/log
 * quiescence for `uri` before the first edit, using the shared harness fold.
 */
export async function waitForDxReady(uri: vscode.Uri): Promise<{ matchedGeneration: number }> {
  const { parseExtensionStartupLog } = await loadHarnessStartupGate();
  return waitForDxReadiness({ ...dxReadinessDeps(uri), evaluateLog: parseExtensionStartupLog });
}

/** The text of the `<script>` block (where an auto-import lands), or the whole doc. */
export function scriptBlockText(doc: vscode.TextDocument): string {
  const text = doc.getText();
  const start = text.indexOf("<script");
  const end = text.indexOf("</script>");
  return start >= 0 && end > start ? text.slice(start, end) : text;
}

/**
 * Wire the real accept-path dependencies for an editor: commands run through
 * `vscode.commands.executeCommand`, the document text is the whole document, and
 * the import text is the `<script>` block, so a real auto-import accept registers as
 * BOTH a document and an import change.
 */
export function acceptCompletionDeps(
  editor: vscode.TextEditor,
  settleMs: number = DEFAULT_SETTLE_MS,
): AcceptCompletionDeps {
  return {
    runCommand: (command) => vscode.commands.executeCommand(command),
    readDocText: () => editor.document.getText(),
    readImportText: () => scriptBlockText(editor.document),
    settle: () => sleep(settleMs),
  };
}

/** Drive the real accept path on `editor` and assert the auto-import completed. */
export function acceptCompletionInEditor(
  editor: vscode.TextEditor,
  settleMs?: number,
): Promise<AcceptOutcome> {
  return acceptCompletion(acceptCompletionDeps(editor, settleMs));
}

/** Read the current extension log and produce the log-canary verdict. */
export function runLogCanary(): CanaryVerdict {
  return diagnoseCanary(summarizeCanaryLog(readTestLog()));
}
