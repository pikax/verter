import fs from "node:fs";
import path from "node:path";
import type { Manifest } from "@verter/language-shared";
import {
  CARRIER_STORE_FORMAT,
  CARRIER_STORE_HEAD_FILE,
  carrierJournalFile,
  carrierSnapshotFile,
  frameCarrierJournalRecord,
  parseCarrierStoreHead,
  type CarrierJournalRecord,
  type CarrierStoreHead,
} from "./carrierJournal";

/**
 * Test fixtures for the incremental carrier store, written in the exact on-disk
 * form the Rust publisher commits (see `./carrierJournal`).
 */

function currentHead(dir: string): CarrierStoreHead | undefined {
  try {
    return parseCarrierStoreHead(fs.readFileSync(path.join(dir, CARRIER_STORE_HEAD_FILE), "utf8"));
  } catch {
    return undefined;
  }
}

function writeAtomic(dir: string, name: string, bytes: string | Buffer): void {
  const tmp = path.join(dir, `.tmp-${process.pid}-${name}`);
  fs.writeFileSync(tmp, bytes);
  fs.renameSync(tmp, path.join(dir, name));
}

/**
 * Publish `manifest` as the base of the store's NEXT generation (generation 1 for
 * a new store) with an empty journal, then swap the head — exactly a compaction,
 * so a following reader reloads it. Returns the generation written.
 */
export function writeCarrierStoreFixture(dir: string, manifest: Manifest): number {
  fs.mkdirSync(dir, { recursive: true });
  const prior = currentHead(dir);
  const generation = (prior?.generation ?? 0) + 1;
  writeAtomic(dir, carrierSnapshotFile(generation), JSON.stringify(manifest));
  writeAtomic(dir, carrierJournalFile(generation), "");
  const head: CarrierStoreHead = {
    format: CARRIER_STORE_FORMAT,
    generation,
    instance: prior?.instance ?? "fixture",
    host_version: manifest.host_version,
  };
  writeAtomic(dir, CARRIER_STORE_HEAD_FILE, JSON.stringify(head));
  return generation;
}

/** Append one committed record (or raw bytes) to the current generation's journal. */
export function appendCarrierStoreRecord(dir: string, record: CarrierJournalRecord | Buffer): void {
  const head = currentHead(dir);
  if (head === undefined) throw new Error(`no carrier store head in ${dir}`);
  const bytes = Buffer.isBuffer(record) ? record : frameCarrierJournalRecord(record);
  fs.appendFileSync(path.join(dir, carrierJournalFile(head.generation)), bytes);
}

/** The current generation's journal path. */
export function carrierStoreJournalPath(dir: string): string {
  const head = currentHead(dir);
  if (head === undefined) throw new Error(`no carrier store head in ${dir}`);
  return path.join(dir, carrierJournalFile(head.generation));
}
