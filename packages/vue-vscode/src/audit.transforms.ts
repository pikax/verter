/**
 * Pure data-transform helpers for the audit "Show Recent Audit Records"
 * command. Lives in its own module so the unit tests in
 * `audit.spec.ts` can import these helpers without pulling in the
 * `vscode` module (vitest cannot load `vscode`).
 *
 * The VS Code-facing command registration is in `audit.ts`.
 */

/**
 * Minimal shape of a `RequestAuditRecord` as projected over JSON-RPC.
 * The full record schema is owned by the Rust crate `verter_audit` (see
 * `packages/types/audit.generated.ts`); the extension only depends on
 * the fields the quickpick + viewer flow needs, so we keep a narrow
 * structural type here rather than a transitive dep on `@verter/types`.
 */
export interface AuditRecordSummary {
  /** Decimal-stringified `u64` request id. */
  request_id: string;
  /** Canonical file id; may be empty for kinds without a single file. */
  canonical_id: string;
  /**
   * Either a bare variant name (`"ComponentMeta"`, `"TypeResolution"`,
   * `"SemanticAnalysis"`) or a single-key object whose key is the
   * variant name (`{ Compile: { target: "Vdom" } }`).
   */
  kind: unknown;
  /** Other fields are passed through verbatim to the JSON viewer. */
  [extra: string]: unknown;
}

/**
 * QuickPick item structurally compatible with `vscode.QuickPickItem`,
 * but defined locally so this module stays independent of the `vscode`
 * runtime dependency.
 */
export interface AuditQuickPickItem {
  label: string;
  description?: string;
  detail?: string;
  /** Decimal-stringified `u64` request id — looked up via `getRecord`. */
  requestId: string;
}

/**
 * Extract a human-readable variant tag from a `RequestKind` JSON value.
 *
 * Returns the bare variant name for unit-like kinds (`ComponentMeta`,
 * `TypeResolution`, `SemanticAnalysis`) and the single object-key for
 * data-carrying kinds (`Compile`, `Workspace`, `Lsp`, `Mcp`,
 * `BundlerBatch`, `Custom`). Returns `"Unknown"` for malformed input so
 * QuickPick labels never collapse to an empty string.
 */
export function formatRecordTagFromKind(kind: unknown): string {
  if (typeof kind === "string") {
    return kind;
  }
  if (kind && typeof kind === "object") {
    const keys = Object.keys(kind as Record<string, unknown>);
    if (keys.length === 1) {
      return keys[0]!;
    }
  }
  return "Unknown";
}

/**
 * Pick a short data tag for the `description` field — the secondary
 * text shown to the right of the QuickPick label. Matches the
 * `RequestKind::matches_filter` parameter set so the user can correlate
 * what they see with the kind filter from the CLI.
 */
function formatRecordDescription(kind: unknown): string | undefined {
  if (!kind || typeof kind !== "object") {
    return undefined;
  }
  const obj = kind as Record<string, Record<string, unknown> | undefined>;
  if (obj.Compile && typeof obj.Compile === "object") {
    return `target=${String(obj.Compile.target ?? "?")}`;
  }
  if (obj.Workspace && typeof obj.Workspace === "object") {
    return `op=${String(obj.Workspace.op ?? "?")}`;
  }
  if (obj.Lsp && typeof obj.Lsp === "object") {
    return `method=${String(obj.Lsp.method ?? "?")}`;
  }
  if (obj.Mcp && typeof obj.Mcp === "object") {
    return `tool=${String(obj.Mcp.tool ?? "?")}`;
  }
  if (obj.BundlerBatch && typeof obj.BundlerBatch === "object") {
    return `kind=${String(obj.BundlerBatch.kind ?? "?")}`;
  }
  if (obj.Custom && typeof obj.Custom === "object") {
    return `name=${String(obj.Custom.name ?? "?")}`;
  }
  return undefined;
}

/**
 * Map an array of audit records (as returned by `$/verter/audit/getRecent`)
 * into QuickPick-shaped entries. Source order is preserved (the LSP
 * handler already sorts descending by request id — re-sorting
 * client-side would just hide that contract).
 *
 * Each item carries `requestId` so the selection handler can call
 * `getRecord` without re-reading the original record.
 */
export function recordsToQuickPickItems(
  records: ReadonlyArray<AuditRecordSummary>,
): AuditQuickPickItem[] {
  return records.map((rec) => {
    const tag = formatRecordTagFromKind(rec.kind);
    const detail = rec.canonical_id ? rec.canonical_id : "(no canonical id)";
    const description = formatRecordDescription(rec.kind);
    return {
      label: `${rec.request_id} ${tag}`,
      description,
      detail,
      requestId: rec.request_id,
    };
  });
}

/** Two-space indented JSON for the editor viewer. `null` is rendered as the literal string `"null"`. */
export function formatRecordAsJson(record: unknown): string {
  return JSON.stringify(record, null, 2);
}
