import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import {
  EDITOR_TSSERVER_ATTESTATION_CONFIG_KEY,
  parseEditorTsserverAttestationReceipt,
} from "@verter/language-shared";
import { writeEditorTsserverAttestation } from "./editorAttestation";

const NONCE = "0123456789abcdef0123456789abcdef";
const dirs: string[] = [];

afterEach(() => {
  for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe("writeEditorTsserverAttestation", () => {
  it("atomically writes the tsserver pid and bound project identities", () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), "verter-editor-tsserver-test-"));
    dirs.push(directory);
    const receiptPath = writeEditorTsserverAttestation(
      {
        [EDITOR_TSSERVER_ATTESTATION_CONFIG_KEY]: { directory, nonce: NONCE },
      },
      ["/ws/b/tsconfig.json", "/ws/a/tsconfig.json", "/ws/a/tsconfig.json"],
      4242,
    );
    expect(receiptPath).toBeDefined();
    const receipt = parseEditorTsserverAttestationReceipt(
      JSON.parse(fs.readFileSync(receiptPath!, "utf8")),
      NONCE,
    );
    expect(receipt).toEqual({
      version: 1,
      nonce: NONCE,
      pid: 4242,
      projects: ["/ws/a/tsconfig.json", "/ws/b/tsconfig.json"],
    });
    expect(fs.readdirSync(directory)).toEqual([path.basename(receiptPath!)]);
  });

  it("does not attest a module that has no bound project", () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), "verter-editor-tsserver-test-"));
    dirs.push(directory);
    expect(
      writeEditorTsserverAttestation(
        { [EDITOR_TSSERVER_ATTESTATION_CONFIG_KEY]: { directory, nonce: NONCE } },
        [],
        4242,
      ),
    ).toBeUndefined();
    expect(fs.readdirSync(directory)).toEqual([]);
  });
});
