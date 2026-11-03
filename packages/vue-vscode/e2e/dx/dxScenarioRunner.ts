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

import { pollBudget, readTestLog, sleep } from "../helpers";
import {
  acceptCompletion,
  vscodeAcceptCompletionDeps,
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

/**
 * Wire the real accept-path dependencies for an editor through the live `vscode`
 * API (see {@link vscodeAcceptCompletionDeps}). The hang guard takes the registered
 * `acceptCompletion` budget, checked against the awaiting test's deadline; it bounds
 * a stuck interaction and never decides a pass.
 */
export function acceptCompletionDeps(
  editor: vscode.TextEditor,
  expectedSuggestion: string,
): AcceptCompletionDeps {
  const budgetMs = pollBudget("acceptCompletion");
  return vscodeAcceptCompletionDeps(vscode, editor, {
    expectedSuggestion,
    armDeadline: (onExpired) => {
      const timer = setTimeout(onExpired, budgetMs);
      return { dispose: () => clearTimeout(timer) };
    },
  });
}

/**
 * Drive the real accept path on `editor`: one trigger, one accept of the focused
 * `expectedSuggestion`, and assert the identifier and its static import both landed.
 */
export function acceptCompletionInEditor(
  editor: vscode.TextEditor,
  expectedSuggestion: string,
): Promise<AcceptOutcome> {
  return acceptCompletion(acceptCompletionDeps(editor, expectedSuggestion));
}

/** Read the current extension log and produce the log-canary verdict. */
export function runLogCanary(): CanaryVerdict {
  return diagnoseCanary(summarizeCanaryLog(readTestLog()));
}
