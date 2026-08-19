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
  /** Legacy canonical-id projection retained for older audit records. */
  canonical_id: string;
  /** Additive tagged target identity; absent only on older audit records. */
  target_identity?:
    | { kind: "RegisteredCanonical"; value: string }
    | { kind: "UnregisteredUri"; value: string }
    | { kind: "NotApplicable" };
  /**
   * Either a bare variant name (`"ComponentMeta"`, `"TypeResolution"`,
   * `"SemanticAnalysis"`) or a single-key object whose key is the
   * variant name (`{ Compile: { products: {...}, backend: "Vdom" } }`).
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
 * Comma-joined list of the truthy product-set fields (`runtime_client`,
 * `ide_companion`, …) — a `CompileRequest` may carry more than one product
 * at once, so this must not collapse to a single "primary" value the way
 * the old `CompileTargetTag` mirror did. `"none"` when the field is
 * absent/malformed or every product is false.
 */
function formatCompileProducts(products: unknown): string {
  if (!products || typeof products !== "object") {
    return "none";
  }
  const set = Object.entries(products as Record<string, unknown>)
    .filter(([, v]) => v === true)
    .map(([k]) => k);
  return set.length > 0 ? set.join(",") : "none";
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
    return `products=${formatCompileProducts(obj.Compile.products)} backend=${String(
      obj.Compile.backend ?? "?",
    )}`;
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

/** Render the tagged target identity, with a legacy fallback for old records. */
function formatTargetDetail(record: AuditRecordSummary): string {
  switch (record.target_identity?.kind) {
    case "RegisteredCanonical":
    case "UnregisteredUri":
      return record.target_identity.value;
    case "NotApplicable":
      return "(no single target)";
    default:
      return record.canonical_id ? record.canonical_id : "(no canonical id)";
  }
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
    const detail = formatTargetDetail(rec);
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
