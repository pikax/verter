import { describe, expect, it } from "vitest";

import {
  type CanonicalDiagnostic,
  type DiagnosticsResponse,
  type NormalizedDiagnostic,
  type ProviderCapabilities,
  normalizeDiagnostics,
} from "../src/index.js";

// The raw-LSP response-input unions (CompletionResponse / HoverResponse /
// DefinitionResponse / DiagnosticsResponse) are the normalizers' INTERNAL input
// shapes, not public harness surface — the public normalize surface is the
// Canonical* outputs plus the normalize* functions. The root barrel must NOT
// re-export the raw-LSP response unions by name, so the only `DiagnosticsResponse`
// reachable from the root is the bridge's object envelope (asserted below).
// Each raw-LSP union is pinned on its OWN `@ts-expect-error` single-name import, so
// a partial re-leak of exactly one union is caught independently (a combined import
// would stay satisfied as long as ANY one name remained unresolvable).
// @ts-expect-error — CompletionResponse (raw-LSP input union) is not part of the public root surface.
import type { CompletionResponse } from "../src/index.js";
// @ts-expect-error — DefinitionResponse (raw-LSP input union) is not part of the public root surface.
import type { DefinitionResponse } from "../src/index.js";
// @ts-expect-error — HoverResponse (raw-LSP input union) is not part of the public root surface.
import type { HoverResponse } from "../src/index.js";

describe("public barrel — DiagnosticsResponse resolves to the bridge envelope", () => {
  it("types the root `DiagnosticsResponse` as the bridge object envelope, not a raw-LSP array", () => {
    const capabilities = {} as ProviderCapabilities;
    const envelope: DiagnosticsResponse = {
      type: "diagnostics",
      uri: "file:///App.vue",
      version: 1,
      diagnostics: [] as NormalizedDiagnostic[],
      capabilities,
    };
    expect(envelope.type).toBe("diagnostics");
    expect(envelope.diagnostics).toEqual([]);

    // @ts-expect-error — the bridge envelope is an object, so a bare raw-LSP
    // diagnostics array is NOT assignable to the root `DiagnosticsResponse`.
    const rawArray: DiagnosticsResponse = [];
    void rawArray;
  });

  it("exposes the normalize surface (functions + Canonical outputs) from the root", () => {
    expect(typeof normalizeDiagnostics).toBe("function");
    const canonical: readonly CanonicalDiagnostic[] = normalizeDiagnostics(null);
    expect(canonical).toEqual([]);
  });
});
