/**
 * CSS validation diagnostics publication path.
 *
 * FAIL CLOSED on availability (TE-C-11): a `null` validation result means the
 * structure was stale/unavailable/closed or the transport failed — keep the
 * last-known diagnostics and publish nothing. An empty array is a genuinely
 * clean validation the publisher may publish (clearing prior diagnostics).
 *
 * Publication RE-ADMISSION (B-29): the validation awaited, so the document may
 * have moved or closed while it ran. A stale invocation's return — computed
 * against an older revision's text — must never be published against the live
 * document, so the publisher re-checks version/closed AFTER the await and
 * drops the result on any mismatch. The newer revision owes (and triggers)
 * its own validation.
 */

import * as vscode from "vscode";
import type { Diagnostic as CSSDiagnostic } from "vscode-css-languageservice";
import { isFrameworkCarrierLanguageId } from "../frameworkWiring";

/** The one `CssService` capability the publisher consumes. */
export interface CssValidationProvider {
  doValidation(
    uri: string,
    source: string,
    version: number,
  ): Promise<Array<{ blockToken: string; diagnostics: CSSDiagnostic[] }> | null>;
}

/**
 * Create the CSS diagnostics updater used by the extension's document
 * change/open listeners (debounced per URI by the caller).
 */
export function createCssDiagnosticsUpdater(
  getCssService: () => CssValidationProvider,
  cssDiagnostics: Pick<vscode.DiagnosticCollection, "set">,
): (document: vscode.TextDocument) => Promise<void> {
  return async (document) => {
    if (!isFrameworkCarrierLanguageId(document.languageId)) return;
    try {
      const uri = document.uri.toString();
      const source = document.getText();
      const version = document.version;
      const results = await getCssService().doValidation(uri, source, version);
      if (results === null) {
        // Structure stale/unavailable (or transport failed): NOT a successful
        // validation — keep the last-known diagnostics, publish nothing
        // (TE-C-11 fail-closed).
        return;
      }
      // Publication re-admission (B-29): drop a stale invocation's return.
      if (document.isClosed || document.version !== version) {
        return;
      }
      const allDiags: vscode.Diagnostic[] = [];
      for (const { diagnostics } of results) {
        for (const d of diagnostics) {
          allDiags.push(
            new vscode.Diagnostic(
              new vscode.Range(
                new vscode.Position(d.range.start.line, d.range.start.character),
                new vscode.Position(d.range.end.line, d.range.end.character),
              ),
              d.message,
              d.severity === 1
                ? vscode.DiagnosticSeverity.Error
                : d.severity === 2
                  ? vscode.DiagnosticSeverity.Warning
                  : vscode.DiagnosticSeverity.Information,
            ),
          );
        }
      }
      cssDiagnostics.set(document.uri, allDiags);
    } catch {
      // Silently fail — CSS diagnostics are best-effort
    }
  };
}
