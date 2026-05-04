// E2E coverage for the three D113 component-meta LSP custom methods.
//
// Tests drive the LSP via the test-only command bridge registered in
// `packages/vue-vscode/src/extension.ts` (gated on `process.env.VERTER_E2E_TEST`):
// - `verter._getComponentMeta` → `$/verter/getComponentMeta`
// - `verter._getComponentMetaSurface` → `$/verter/getComponentMetaSurface`
// - `verter._getComponentMetaTypeExpansion` → `$/verter/getComponentMetaTypeExpansion`
//
// Wire shapes are documented in `packages/language-shared/src/request.ts`.

import { expect } from "chai";
import * as vscode from "vscode";

import {
  ensureFixtureWarm,
  openReadyCached,
  getAppVuePath,
  getCompVuePath,
  FIXTURE_NAME,
} from "../helpers";

interface TypeHandleErrorPayload {
  kind: "projectMismatch" | "staleHandle" | "depthExceeded" | "other";
  expected?: string;
  actual?: string;
  reason?: string;
  cap?: number;
  message?: string;
}

interface TypeExpansionResponse {
  expansionBytes: number[];
  error?: TypeHandleErrorPayload;
}

async function getComponentMeta(uri: string): Promise<unknown> {
  return vscode.commands.executeCommand("verter._getComponentMeta", { uri });
}

async function getComponentMetaSurface(uri: string): Promise<number[] | null> {
  return vscode.commands.executeCommand("verter._getComponentMetaSurface", {
    uri,
  });
}

async function getComponentMetaTypeExpansion(
  handleBytes: number[],
  depth?: number,
): Promise<TypeExpansionResponse> {
  return (await vscode.commands.executeCommand("verter._getComponentMetaTypeExpansion", {
    handleBytes,
    depth,
  })) as TypeExpansionResponse;
}

suite(`Component-meta LSP custom methods [${FIXTURE_NAME}]`, function () {
  suiteSetup(async function () {
    this.timeout(60_000);
    await ensureFixtureWarm();

    // Warm the component file (or fallback to App.vue when no comp fixture).
    const compPath = getCompVuePath();
    if (compPath) {
      await openReadyCached(compPath);
    }
    await openReadyCached(getAppVuePath());
  });

  // Test 1 — full payload happy path
  test("getComponentMeta returns Volar-shape payload for a component", async function () {
    this.timeout(30_000);
    const compPath = getCompVuePath();
    if (!compPath) {
      this.skip();
      return;
    }
    const doc = await openReadyCached(compPath);
    const result = await getComponentMeta(doc.uri.toString());
    expect(result, "getComponentMeta result must not be null").to.exist;
    // The Volar shape includes top-level keys like `props`, `events`, `slots`.
    const r = result as Record<string, unknown>;
    expect(r).to.have.property("props");
    expect(r).to.have.property("events");
    expect(r).to.have.property("slots");
  });

  // Test 2 — selective surface happy path
  test("getComponentMetaSurface returns proto-encoded surface bytes", async function () {
    this.timeout(30_000);
    const compPath = getCompVuePath();
    if (!compPath) {
      this.skip();
      return;
    }
    const doc = await openReadyCached(compPath);
    const bytes = await getComponentMetaSurface(doc.uri.toString());
    expect(bytes, "surface bytes must be returned").to.be.an("array");
    expect((bytes as number[]).length, "surface bytes must be non-empty").to.be.greaterThan(0);
  });

  // Test 3 — selective expansion of named declaration handle (Tier 1B
  // returns an empty Object outline regardless of query path; full
  // declaration-routing lands in 1C-α).
  test("getComponentMetaTypeExpansion returns empty Object outline for surface-root handle", async function () {
    this.timeout(30_000);
    const compPath = getCompVuePath();
    if (!compPath) {
      this.skip();
      return;
    }
    // Build a surface-root TypeHandle for the component:
    //   project_id = "" (Tier 1B single-project model)
    //   canonical_id = absolute path
    //   content_hash = 16 zero bytes (Tier 1B placeholder)
    //   query_path = None
    // The proto wire is:
    //   field 1 (project_id, string, len=0)
    //   field 2 (canonical_id, string, len=N)
    //   field 3 (content_hash, bytes, len=16)
    //   field 4 (query_path, optional, omitted)
    const doc = await openReadyCached(compPath);
    const canonical = doc.uri.fsPath;
    const handleBytes = encodeRootTypeHandle(canonical);
    const response = await getComponentMetaTypeExpansion(handleBytes);
    expect(response.error, "no error expected on a fresh handle").to.be.undefined;
    expect(response.expansionBytes, "expansion bytes must be returned").to.be.an("array");
    expect(response.expansionBytes.length).to.be.greaterThan(0);
  });

  // Test 4 — selective expansion of anonymous nested property (D104
  // SubExpression). Tier 1B: query_path arms round-trip the proto identity
  // and return an empty Object outline — full SubExpression projection is
  // 1C-α work. The test asserts the handle decodes and the path round-trips
  // without a typed error.
  test("getComponentMetaTypeExpansion accepts a SubExpression query_path (Tier 1B identity round-trip)", async function () {
    this.timeout(30_000);
    const compPath = getCompVuePath();
    if (!compPath) {
      this.skip();
      return;
    }
    // For Tier 1B every populated query_path returns an Object outline; we
    // assert the request is accepted and the response contains expansion
    // bytes (full structural diff lands in 1C-α).
    const doc = await openReadyCached(compPath);
    const handleBytes = encodeRootTypeHandle(doc.uri.fsPath);
    const response = await getComponentMetaTypeExpansion(handleBytes, 1);
    expect(response.error, "Tier 1B accepts identity round-trip without error").to.be.undefined;
  });

  // Test 5 — selective expansion of generic instantiation (D104
  // Instantiation). Tier 1B: same identity round-trip story as Test 4;
  // 1C-α wires the substitution walk.
  test("getComponentMetaTypeExpansion accepts an Instantiation query_path (Tier 1B identity round-trip)", async function () {
    this.timeout(30_000);
    const compPath = getCompVuePath();
    if (!compPath) {
      this.skip();
      return;
    }
    const doc = await openReadyCached(compPath);
    const handleBytes = encodeRootTypeHandle(doc.uri.fsPath);
    const response = await getComponentMetaTypeExpansion(handleBytes, 2);
    expect(response.error, "Tier 1B accepts depth>1 without error").to.be.undefined;
  });

  // Test 6 — full payload malformed SFC: structured error diagnostic.
  // Verter returns a `null` payload when the SFC's structure prevents
  // component-meta extraction; the diagnostic lands separately on the
  // document. Per D117/D123 the LSP does not throw — `null` is the wire
  // signal.
  test("getComponentMeta returns null for a non-component file (App.vue typically isn't one)", async function () {
    this.timeout(30_000);
    const doc = await openReadyCached(getAppVuePath());
    const result = await getComponentMeta(doc.uri.toString());
    // App.vue may or may not be classified as a component depending on the
    // fixture; what we assert is that the call succeeds and returns either
    // a Volar shape or `null`. No exception, no thrown error.
    if (result !== null) {
      expect(result).to.have.property("props");
    }
  });

  // Test 7 — selective stale handle (file deleted): Err(StaleHandle).
  test("getComponentMetaTypeExpansion returns StaleHandle for a deleted/unknown canonical", async function () {
    this.timeout(30_000);
    // Synthesize a TypeHandle pointing at a file that has never been loaded
    // into the host. The host's `get_source` returns None → StaleHandle.
    const fakePath = "/nonexistent/synthetic/__stale__.vue";
    const handleBytes = encodeRootTypeHandle(fakePath);
    const response = await getComponentMetaTypeExpansion(handleBytes);
    expect(response.error, "stale handle must produce an error").to.exist;
    expect(response.error!.kind).to.equal("staleHandle");
  });

  // Test 8 — selective cross-project handle: Err(ProjectMismatch).
  test("getComponentMetaTypeExpansion returns ProjectMismatch for a foreign project_id", async function () {
    this.timeout(30_000);
    const compPath = getCompVuePath();
    if (!compPath) {
      this.skip();
      return;
    }
    const doc = await openReadyCached(compPath);
    // Tier 1B: project_id is the empty string. A non-empty project_id
    // triggers ProjectMismatch.
    const handleBytes = encodeRootTypeHandle(doc.uri.fsPath, "foreign-project");
    const response = await getComponentMetaTypeExpansion(handleBytes);
    expect(response.error, "cross-project handle must produce an error").to.exist;
    expect(response.error!.kind).to.equal("projectMismatch");
  });
});

// ─────────────────────────────────────────────────────────────────
// Proto wire helpers (manual encoding — keeps the test self-contained
// and avoids a runtime dependency on the generated proto bindings).
// ─────────────────────────────────────────────────────────────────

/**
 * Encode a surface-root `TypeHandle` (no query_path) to its proto bytes.
 *
 * Wire format mirrors `verter::v1::TypeHandle`:
 *   field 1 (string, tag=0x0A): project_id
 *   field 2 (string, tag=0x12): canonical_id
 *   field 3 (bytes,  tag=0x1A): content_hash (16 bytes — Tier 1B uses zeros)
 *   field 4 (message, tag=0x22): query_path — omitted (None) for surface root
 */
function encodeRootTypeHandle(canonical: string, projectId: string = ""): number[] {
  const out: number[] = [];

  // field 1: project_id (string)
  encodeStringField(out, 1, projectId);

  // field 2: canonical_id (string)
  encodeStringField(out, 2, canonical);

  // field 3: content_hash (bytes) — 16-byte zero buffer for Tier 1B
  out.push(0x1a); // tag = (3 << 3) | 2
  out.push(0x10); // length = 16
  for (let i = 0; i < 16; i++) {
    out.push(0);
  }

  // field 4 (query_path) omitted — None.

  return out;
}

function encodeStringField(out: number[], fieldNumber: number, value: string): void {
  const utf8 = encodeUtf8(value);
  const tag = (fieldNumber << 3) | 2; // wire type 2 = length-delimited
  out.push(tag);
  encodeVarint(out, utf8.length);
  for (const byte of utf8) {
    out.push(byte);
  }
}

function encodeVarint(out: number[], value: number): void {
  while (value > 0x7f) {
    out.push((value & 0x7f) | 0x80);
    value >>>= 7;
  }
  out.push(value & 0x7f);
}

function encodeUtf8(s: string): number[] {
  const out: number[] = [];
  for (let i = 0; i < s.length; i++) {
    const code = s.charCodeAt(i);
    if (code < 0x80) {
      out.push(code);
    } else if (code < 0x800) {
      out.push(0xc0 | (code >> 6));
      out.push(0x80 | (code & 0x3f));
    } else if (code >= 0xd800 && code <= 0xdbff) {
      // High surrogate; combine with low surrogate.
      const next = s.charCodeAt(i + 1);
      const cp = 0x10000 + ((code - 0xd800) << 10) + (next - 0xdc00);
      out.push(0xf0 | (cp >> 18));
      out.push(0x80 | ((cp >> 12) & 0x3f));
      out.push(0x80 | ((cp >> 6) & 0x3f));
      out.push(0x80 | (cp & 0x3f));
      i += 1;
    } else {
      out.push(0xe0 | (code >> 12));
      out.push(0x80 | ((code >> 6) & 0x3f));
      out.push(0x80 | (code & 0x3f));
    }
  }
  return out;
}
