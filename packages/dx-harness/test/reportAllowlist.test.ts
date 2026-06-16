import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  BENIGN_DIVERGENCES_V1_FILENAME,
  FindingsError,
  loadBenignAllowlist,
  validateBenignAllowlist,
} from "../src/report/index.js";

const COMMITTED_V1 = fileURLToPath(
  new URL(`../allowlists/${BENIGN_DIVERGENCES_V1_FILENAME}`, import.meta.url),
);

describe("report — benign-divergence allowlist schema", () => {
  it("loads the committed v1 file as a valid, intentionally-empty allowlist", () => {
    const allowlist = loadBenignAllowlist(COMMITTED_V1);
    expect(allowlist.version).toBe(1);
    expect(allowlist.entries).toEqual([]);
    expect(typeof allowlist.description).toBe("string");
  });

  it("accepts an entry that matches by fingerprint and/or dedupe key", () => {
    const allowlist = validateBenignAllowlist({
      version: 1,
      entries: [
        { id: "a", reason: "fp match", fingerprint: "abc" },
        { id: "b", reason: "key match", match: { scenario: "s", signal: "hover_parity" } },
      ],
    });
    expect(allowlist.entries).toHaveLength(2);
  });

  it("rejects a wrong version, a non-array entries, and an entry with no matcher", () => {
    expect(() => validateBenignAllowlist({ version: 2, entries: [] })).toThrow(FindingsError);
    expect(() => validateBenignAllowlist({ version: 1, entries: {} })).toThrow(FindingsError);
    expect(() =>
      validateBenignAllowlist({ version: 1, entries: [{ id: "x", reason: "no matcher" }] }),
    ).toThrow(/matcher|fingerprint|match/);
  });
});
