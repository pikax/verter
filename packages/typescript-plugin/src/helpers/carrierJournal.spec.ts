import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { decodeCarrierJournalLine, fnv1a32, frameCarrierJournalRecord } from "./carrierJournal";

/** Journal lines the Rust writer frames byte-for-byte (see the Rust journal tests). */
const corpus = readFileSync(join(__dirname, "carrierJournalCorpus.txt"));

function lines(bytes: Buffer): Buffer[] {
  return bytes
    .toString("latin1")
    .split("\n")
    .filter((l) => l.length > 0)
    .map((l) => Buffer.from(l, "latin1"));
}

describe("carrier journal wire format", () => {
  it("matches the published FNV-1a 32 vectors", () => {
    // http://www.isthe.com/chongo/tech/comp/fnv/
    expect(fnv1a32(Buffer.from(""))).toBe(0x811c9dc5);
    expect(fnv1a32(Buffer.from("a"))).toBe(0xe40c292c);
    expect(fnv1a32(Buffer.from("foobar"))).toBe(0xbf9cf968);
  });

  it("decodes Rust-framed lines and re-frames them to the identical bytes", () => {
    const framed = lines(corpus);
    expect(framed).toHaveLength(2);
    for (const line of framed) {
      const record = decodeCarrierJournalLine(line, 0, line.length);
      expect(record).toBeDefined();
      expect(Buffer.concat([frameCarrierJournalRecord(record!)])).toEqual(
        Buffer.concat([line, Buffer.from("\n")]),
      );
    }
  });

  it("rejects a checksum-valid record whose operation payload is incomplete", () => {
    const bad = [
      { op: "owned_put", project: "p" },
      { op: "owned_put", project: "p", source_uri: "s", rows: [{ source_uri: "s" }] },
      { op: "owned_del", project: "p" },
      { op: "ready_put", project: "p", provider_uri: "u" },
      { op: "ready_del", project: "p" },
      { op: "project_put" },
    ];
    for (const op of bad) {
      const line = frameCarrierJournalRecord({ epoch: 8, ops: [op as never] });
      expect(decodeCarrierJournalLine(line, 0, line.length - 1)).toBeUndefined();
    }
  });
});
