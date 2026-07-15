import { describe, expect, it } from "vitest";

import {
  EDITOR_TSSERVER_ATTESTATION_CONFIG_KEY,
  editorTsserverAttestationFileName,
  parseEditorTsserverAttestationReceipt,
  parseEditorTsserverAttestationRequest,
} from "./editorTsserverAttestation";

const NONCE = "0123456789abcdef0123456789abcdef";

describe("editor tsserver attestation schema", () => {
  it("accepts a scoped request and a project-bound process receipt", () => {
    expect(
      parseEditorTsserverAttestationRequest({
        [EDITOR_TSSERVER_ATTESTATION_CONFIG_KEY]: { directory: "/tmp/receipt", nonce: NONCE },
      }),
    ).toEqual({ directory: "/tmp/receipt", nonce: NONCE });
    expect(
      parseEditorTsserverAttestationReceipt(
        { version: 1, nonce: NONCE, pid: 42, projects: ["/ws/b.json", "/ws/a.json"] },
        NONCE,
      ),
    ).toEqual({
      version: 1,
      nonce: NONCE,
      pid: 42,
      projects: ["/ws/a.json", "/ws/b.json"],
    });
    expect(editorTsserverAttestationFileName(NONCE)).toBe(`verter-editor-tsserver-${NONCE}.json`);
  });

  it("rejects an unbound, wrong-session, or malformed receipt", () => {
    expect(
      parseEditorTsserverAttestationReceipt(
        { version: 1, nonce: NONCE, pid: 42, projects: [] },
        NONCE,
      ),
    ).toBeUndefined();
    expect(
      parseEditorTsserverAttestationReceipt(
        { version: 1, nonce: "f".repeat(32), pid: 42, projects: ["/ws/tsconfig.json"] },
        NONCE,
      ),
    ).toBeUndefined();
    expect(
      parseEditorTsserverAttestationReceipt(
        { version: 1, nonce: NONCE, pid: 0, projects: ["/ws/tsconfig.json"] },
        NONCE,
      ),
    ).toBeUndefined();
  });
});
