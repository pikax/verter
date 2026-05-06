/**
 * Audit-inspection commands.
 *
 * Implements `verter.showRecentAuditRecords` — a VS Code QuickPick that
 * lists recent records returned by the LSP `$/verter/audit/getRecent`
 * method. Selecting a record fetches the full payload via
 * `$/verter/audit/getRecord` and opens it in a JSON editor pane.
 *
 * Architectural rule: NO webview, NO dashboard. The CLI
 * (`verter-audit-inspect`) is the heavy interface — VS Code is
 * read-only quickpick only.
 *
 * Pure data-transform helpers (`formatRecordTagFromKind`,
 * `recordsToQuickPickItems`, `formatRecordAsJson`) and their unit
 * tests live in `audit.transforms.ts` / `audit.spec.ts`.
 */
import { commands, window, workspace, type ExtensionContext } from "vscode";
import type { LogOutputChannel, QuickPickItem } from "vscode";
import {
  formatRecordAsJson,
  recordsToQuickPickItems,
  type AuditQuickPickItem,
  type AuditRecordSummary,
} from "./audit.transforms";

export {
  formatRecordTagFromKind,
  recordsToQuickPickItems,
  formatRecordAsJson,
  type AuditQuickPickItem,
  type AuditRecordSummary,
} from "./audit.transforms";

/**
 * Register the `verter.showRecentAuditRecords` command on `context`.
 *
 * `getClient` is a thunk-style accessor matching the rest of the
 * extension's command registrations — we resolve the language client
 * lazily so the command can be invoked before the LSP server has
 * finished starting (the command awaits `ensureLanguageServerStarted`
 * first).
 */
export function addShowRecentAuditRecordsCommand(
  context: ExtensionContext,
  log: LogOutputChannel,
  ensureLanguageServerStarted: () => Promise<unknown>,
  getClient: () => unknown,
) {
  context.subscriptions.push(
    commands.registerCommand("verter.showRecentAuditRecords", async () => {
      try {
        await ensureLanguageServerStarted();
        // The extension's `PatchClient` narrows `sendRequest` to the
        // `RequestType` enum. The audit methods are not enum members
        // (they are intentionally extension-only and read-only), so we
        // call into the underlying `LanguageClient.sendRequest` via a
        // structural cast.
        const client = getClient() as {
          sendRequest: <T>(method: string, params?: unknown) => Promise<T>;
        };
        const records = await client.sendRequest<AuditRecordSummary[] | null>(
          "$/verter/audit/getRecent",
          { limit: 50 },
        );

        if (!records || records.length === 0) {
          window.showInformationMessage(
            "No Verter audit records available. Enable audit capture and trigger a request first.",
          );
          return;
        }

        const items: (AuditQuickPickItem & QuickPickItem)[] = recordsToQuickPickItems(records);
        const picked = await window.showQuickPick(items, {
          placeHolder: "Select an audit record to view (most recent first)",
          matchOnDescription: true,
          matchOnDetail: true,
        });
        if (!picked) {
          return;
        }

        const record = await client.sendRequest<unknown>("$/verter/audit/getRecord", {
          request_id: picked.requestId,
        });
        if (record === null || record === undefined) {
          window.showWarningMessage(
            `Audit record ${picked.requestId} is no longer available (capture disabled or already drained).`,
          );
          return;
        }

        const document = await workspace.openTextDocument({
          content: formatRecordAsJson(record),
          language: "json",
        });
        await window.showTextDocument(document, { preview: true });
      } catch (err) {
        log.error("Failed to fetch audit records", err as Error);
        const message = err instanceof Error ? err.message : String(err);
        window.showErrorMessage(`Failed to fetch Verter audit records: ${message}`);
      }
    }),
  );
}
