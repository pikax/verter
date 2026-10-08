import type { OwnedSource, ReadyFile } from "@verter/language-shared";

/**
 * The incremental on-disk publication format the Rust `verter_lsp` carrier store
 * writes (`crates/verter_lsp/src/external_ts/carrier_publish_journal.rs` is the
 * authority; this module mirrors only what a reader needs).
 *
 * - `head.json` — `{ format, generation, instance, host_version }`, swapped
 *   atomically; names the authoritative generation.
 * - `snapshot-<generation>.json` — the compacted base, a full `Manifest`.
 * - `journal-<generation>.log` — append-only records, one per publication, each a
 *   line `<fnv1a32 hex8> <json>\n`. A record is committed exactly when its whole
 *   line verifies; a reader never applies past the first line that does not.
 *
 * Records name RESOLVED row operations (the writer owns the publication
 * semantics), so a reader is a plain applier of the vocabulary below.
 */

/** The `head.json` format this reader understands. */
export const CARRIER_STORE_FORMAT = 2;
export const CARRIER_STORE_HEAD_FILE = "head.json";

export function carrierSnapshotFile(generation: number): string {
  return `snapshot-${generation}.json`;
}

export function carrierJournalFile(generation: number): string {
  return `journal-${generation}.log`;
}

export interface CarrierStoreHead {
  format: number;
  generation: number;
  instance: string;
  host_version: string;
}

/** One primitive row operation; a record's ops apply in order. */
export type CarrierJournalOp =
  | { op: "project_put"; project: string }
  | { op: "owned_clear"; project: string }
  | { op: "owned_put"; project: string; source_uri: string; rows: OwnedSource[] }
  | { op: "owned_del"; project: string; source_uri: string }
  | { op: "ready_put"; project: string; provider_uri: string; file: ReadyFile }
  | { op: "ready_del"; project: string; provider_uri: string };

export interface CarrierJournalRecord {
  epoch: number;
  ops: CarrierJournalOp[];
}

/** FNV-1a 32 over `bytes[start, end)` — the per-line torn-write check. */
export function fnv1a32(bytes: Uint8Array, start = 0, end = bytes.length): number {
  let hash = 0x811c9dc5;
  for (let i = start; i < end; i++) {
    hash ^= bytes[i]!;
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/** Parse a head, or `undefined` when it is not a head this reader understands. */
export function parseCarrierStoreHead(raw: string): CarrierStoreHead | undefined {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return undefined;
  }
  if (parsed === null || typeof parsed !== "object") return undefined;
  const head = parsed as Partial<CarrierStoreHead>;
  if (
    head.format !== CARRIER_STORE_FORMAT ||
    typeof head.generation !== "number" ||
    typeof head.instance !== "string"
  ) {
    return undefined;
  }
  return head as CarrierStoreHead;
}

const OP_KINDS = new Set([
  "project_put",
  "owned_clear",
  "owned_put",
  "owned_del",
  "ready_put",
  "ready_del",
]);

/**
 * Decode one complete journal line `bytes[start, end)` (without its `\n`), or
 * `undefined` when its checksum, JSON or shape does not verify.
 */
export function decodeCarrierJournalLine(
  bytes: Uint8Array,
  start: number,
  end: number,
): CarrierJournalRecord | undefined {
  if (end - start < 9 || bytes[start + 8] !== 0x20) return undefined;
  const declared = Number.parseInt(
    Buffer.from(bytes.buffer, bytes.byteOffset + start, 8).toString("latin1"),
    16,
  );
  if (!Number.isFinite(declared) || fnv1a32(bytes, start + 9, end) !== declared) {
    return undefined;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(
      Buffer.from(bytes.buffer, bytes.byteOffset + start + 9, end - start - 9).toString("utf8"),
    );
  } catch {
    return undefined;
  }
  if (parsed === null || typeof parsed !== "object") return undefined;
  const record = parsed as Partial<CarrierJournalRecord>;
  if (typeof record.epoch !== "number" || !Array.isArray(record.ops)) return undefined;
  for (const op of record.ops) {
    if (
      op === null ||
      typeof op !== "object" ||
      !OP_KINDS.has((op as { op?: unknown }).op as string) ||
      typeof (op as { project?: unknown }).project !== "string"
    ) {
      return undefined;
    }
  }
  return record as CarrierJournalRecord;
}

/** Frame one record as a journal line (the writer's exact byte form). */
export function frameCarrierJournalRecord(record: CarrierJournalRecord): Buffer {
  const json = Buffer.from(JSON.stringify(record), "utf8");
  const checksum = fnv1a32(json).toString(16).padStart(8, "0");
  return Buffer.concat([Buffer.from(`${checksum} `, "latin1"), json, Buffer.from("\n", "latin1")]);
}
