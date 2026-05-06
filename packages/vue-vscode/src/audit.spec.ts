/**
 * Tests for the audit "Show Recent Audit Records" command's data-transform
 * logic. The transform converts the JSON-shaped records returned by the LSP
 * `$/verter/audit/getRecent` method into `vscode.QuickPickItem`-shaped
 * objects.
 *
 * These tests exercise only the pure data-transformation layer — they
 * import from `audit.transforms.ts`, which has no dependency on the
 * `vscode` module (vitest cannot load `vscode`).
 */
import { describe, it, expect } from "vitest";
import {
  formatRecordTagFromKind,
  recordsToQuickPickItems,
  formatRecordAsJson,
  type AuditRecordSummary,
} from "./audit.transforms";

function rec(overrides: Partial<AuditRecordSummary> = {}): AuditRecordSummary {
  return {
    request_id: "1",
    canonical_id: "/some/file.vue",
    kind: "ComponentMeta",
    ...overrides,
  } as AuditRecordSummary;
}

describe("formatRecordTagFromKind", () => {
  it("returns the bare variant name for unit-like kinds", () => {
    expect(formatRecordTagFromKind("ComponentMeta")).toBe("ComponentMeta");
    expect(formatRecordTagFromKind("TypeResolution")).toBe("TypeResolution");
    expect(formatRecordTagFromKind("SemanticAnalysis")).toBe("SemanticAnalysis");
  });

  it("extracts the variant key from object-shaped kinds", () => {
    expect(formatRecordTagFromKind({ Compile: { target: "Vdom" } })).toBe("Compile");
    expect(formatRecordTagFromKind({ Workspace: { op: "Resolve" } })).toBe("Workspace");
    expect(formatRecordTagFromKind({ Lsp: { method: "Hover" } })).toBe("Lsp");
    expect(formatRecordTagFromKind({ Mcp: { tool: "search" } })).toBe("Mcp");
  });

  it("returns 'Unknown' for malformed kinds", () => {
    // Discriminating: a missing/empty kind must NOT silently render as "" —
    // QuickPick items with empty labels are invisible to the user.
    expect(formatRecordTagFromKind({})).toBe("Unknown");
    expect(formatRecordTagFromKind(undefined)).toBe("Unknown");
    expect(formatRecordTagFromKind(null)).toBe("Unknown");
  });
});

describe("recordsToQuickPickItems", () => {
  it("preserves source order (server already sorted descending by request id)", () => {
    const items = recordsToQuickPickItems([
      rec({ request_id: "30", canonical_id: "/c.vue", kind: "ComponentMeta" }),
      rec({ request_id: "20", canonical_id: "/b.vue", kind: "TypeResolution" }),
      rec({ request_id: "10", canonical_id: "/a.vue", kind: "SemanticAnalysis" }),
    ]);
    expect(items.map((i) => i.label)).toEqual([
      "30 ComponentMeta",
      "20 TypeResolution",
      "10 SemanticAnalysis",
    ]);
  });

  it("sets canonical_id as the detail line", () => {
    const items = recordsToQuickPickItems([rec({ canonical_id: "/foo.vue" })]);
    expect(items[0]!.detail).toBe("/foo.vue");
  });

  it("falls back to '(no canonical id)' when canonical_id is empty", () => {
    // Discriminating: some kinds (e.g. some MCP tool calls) leave
    // canonical_id empty. The UI must not show a blank detail line.
    const items = recordsToQuickPickItems([rec({ canonical_id: "" })]);
    expect(items[0]!.detail).toBe("(no canonical id)");
  });

  it("threads request_id through the picked-item payload", () => {
    // The handler reads back the request_id from the selected item to
    // call getRecord. Losing it would break the next step of the flow.
    const items = recordsToQuickPickItems([rec({ request_id: "42" })]);
    expect(items[0]!.requestId).toBe("42");
  });

  it("renders Compile target / Workspace op / Lsp method / Mcp tool in description", () => {
    const items = recordsToQuickPickItems([
      rec({ request_id: "1", kind: { Compile: { target: "Vdom" } } }),
      rec({ request_id: "2", kind: { Workspace: { op: "Resolve" } } }),
      rec({ request_id: "3", kind: { Lsp: { method: "Hover" } } }),
      rec({ request_id: "4", kind: { Mcp: { tool: "search" } } }),
      rec({ request_id: "5", kind: "ComponentMeta" }),
    ]);
    expect(items[0]!.description).toBe("target=Vdom");
    expect(items[1]!.description).toBe("op=Resolve");
    expect(items[2]!.description).toBe("method=Hover");
    expect(items[3]!.description).toBe("tool=search");
    expect(items[4]!.description).toBeUndefined();
  });

  it("returns an empty array for an empty record list", () => {
    expect(recordsToQuickPickItems([])).toEqual([]);
  });
});

describe("formatRecordAsJson", () => {
  it("pretty-prints with two-space indent", () => {
    const json = formatRecordAsJson({
      request_id: "1",
      canonical_id: "/foo.vue",
      kind: "ComponentMeta",
    });
    expect(json).toContain('"request_id": "1"');
    expect(json).toContain('"canonical_id": "/foo.vue"');
    // Discriminating: indentation must be present (two-space). A flat
    // single-line stringification would defeat the "open in JSON editor"
    // affordance.
    expect(json).toMatch(/\n {2}"request_id"/);
  });

  it("handles null records (record drained or unknown id) gracefully", () => {
    expect(formatRecordAsJson(null)).toBe("null");
  });
});
