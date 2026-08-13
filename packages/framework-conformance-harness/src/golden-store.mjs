// Immutable, ATOMICALLY-PUBLISHED golden storage (conformance-goldens.md
// "Golden provenance"; BF2 required exit "atomic result accounting").
//
// LAYOUT: each golden record is an immutable CONTENT-ADDRESSED file at
// `goldens/records/<sha256-of-record-text>.json`, and ONE manifest file
// (`goldens/manifest.json`) names which records constitute the current
// valid set (logical name -> record digest). The manifest is replaced via
// write-to-temp-then-rename — atomic on a same-filesystem rename — so the
// ENTIRE golden set has exactly ONE reader-visible commit point:
//
//  - a generation run that fails after writing any number of record files
//    has not touched the manifest, so every reader still sees the complete
//    PREVIOUS set — never a mixed or partial set, not even transiently
//    (orphan record files are invisible: readers resolve only through the
//    manifest);
//  - a record file is write-once: publishing a record whose digest already
//    exists verifies byte-identity instead of rewriting (a digest collision
//    with different bytes is a hard error, never a silent overwrite).
//
// READER-SCHEDULE SAFETY (generation-grace retention): the atomic-commit
// PRIMITIVE above only makes the manifest swap itself un-torn; a reader
// that loaded the OLD manifest and is about to read the records it lists
// is a separate hazard — an immediate post-commit sweep of every record
// the NEW manifest no longer references would delete those records out
// from under it. So each manifest carries a monotonic `generation` plus a
// `graceEntries` copy of the entries of the manifest it replaced, and
// post-commit GC retains the union of THREE generations of records: the
// NEW manifest's, the IMMEDIATELY-REPLACED manifest's, and the
// manifest-before-that's (TWO full generations of grace — generations N,
// N-1, and N-2 relative to the newest). Two generations, not one, because
// a reader may observe manifest N and take a bounded-but-nonzero time
// before dereferencing individual records — long enough for TWO further
// publishes (N+1 and N+2) to complete in between; one generation of grace
// provably lost that reader's records, and two bounds the schedule without
// unbounded retention. An in-flight reader of manifest generation N can
// therefore read every record N lists until a THIRD publish completes — a
// bounded, testable window, not a probabilistic one. Records unreferenced
// for three consecutive generations are collected.
//
// WRITE PATH: exactly one function publishes a golden set
// (`publishGoldenSet`), called ONLY from `bin/generate-goldens.mjs` — never
// from the comparator, never from a candidate-producing module. The
// comparator (`src/compare.mjs`) never imports `node:fs` write functions at
// all, so "candidate output cannot update its own expectation" holds
// structurally. Readers return deep-frozen objects so an accidental
// in-memory mutation cannot silently succeed either.

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

export function sha256(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

function deepFreeze(value) {
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) return value;
  Object.freeze(value);
  for (const key of Object.keys(value)) deepFreeze(value[key]);
  return value;
}

export function goldenManifestPath(goldensRoot) {
  return path.join(goldensRoot, "manifest.json");
}

function recordsDir(goldensRoot) {
  return path.join(goldensRoot, "records");
}

function recordPath(goldensRoot, digest) {
  return path.join(recordsDir(goldensRoot), `${digest}.json`);
}

export function serializeGoldenRecord(record) {
  return `${JSON.stringify(record, null, 2)}\n`;
}

/** Writes one content-addressed record file (write-once). @returns digest */
function writeRecordFile(goldensRoot, record) {
  const text = serializeGoldenRecord(record);
  const digest = sha256(text);
  const target = recordPath(goldensRoot, digest);
  if (existsSync(target)) {
    const existing = readFileSync(target, "utf8");
    if (existing !== text)
      throw new Error(`golden record digest collision with different bytes: ${digest}`);
    return digest;
  }
  mkdirSync(recordsDir(goldensRoot), { recursive: true });
  const tmp = `${target}.tmp-${process.pid}`;
  writeFileSync(tmp, text, "utf8");
  renameSync(tmp, target);
  return digest;
}

/**
 * Publishes a complete golden set through ONE atomic commit point.
 *
 * @param {string} goldensRoot
 * @param {Array<{ name: string, record: object }>} entries logical name ->
 *   full-provenance record (see generate-goldens.mjs's `buildProvenance`)
 * @param {object} [meta] informational manifest metadata (NOT part of any
 *   record's identity)
 * @returns {{ published: number, manifest: object }}
 */
export function publishGoldenSet(goldensRoot, entries, meta = {}) {
  const names = new Set();
  for (const { name } of entries) {
    if (names.has(name)) throw new Error(`duplicate golden name in set: ${name}`);
    names.add(name);
  }
  // Records first — the manifest is untouched until every record landed, so
  // a failure at the Nth record (serialization or write) leaves the entire
  // previous set fully observable and no partial set ever reader-visible.
  const manifestEntries = {};
  for (const { name, record } of entries) {
    manifestEntries[name] = writeRecordFile(goldensRoot, record);
  }
  // The manifest being REPLACED defines the first grace set, and ITS
  // recorded `graceEntries` (the entries of the manifest it replaced in
  // turn) define the second: both stay on disk through this publish so a
  // reader that loaded either of the two prior manifests can still read
  // everything its manifest lists (see the module header's reader-schedule
  // contract — two full generations of grace).
  const manifestTarget = goldenManifestPath(goldensRoot);
  let previousManifest = null;
  try {
    previousManifest = JSON.parse(readFileSync(manifestTarget, "utf8"));
  } catch {
    /* first publish: no previous manifest, no grace set */
  }
  const manifest = {
    schemaVersion: 2,
    generation: (previousManifest?.generation ?? 0) + 1,
    ...meta,
    entries: manifestEntries,
    // The replaced manifest's entries, carried so the NEXT publish can
    // retain them as its second grace generation.
    graceEntries: previousManifest?.entries ?? {},
  };
  mkdirSync(goldensRoot, { recursive: true });
  const manifestTmp = `${manifestTarget}.tmp-${process.pid}`;
  writeFileSync(manifestTmp, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
  renameSync(manifestTmp, manifestTarget); // THE single reader-visible commit point
  // Post-commit GC (best-effort; readers only ever resolve through a
  // manifest, so a leftover orphan is inert). Retained: the NEW manifest's
  // records ∪ the REPLACED manifest's records ∪ the records of the
  // manifest REPLACED BEFORE THAT (two full generations of grace — see the
  // module header).
  const retained = new Set(Object.values(manifestEntries).map((digest) => `${digest}.json`));
  for (const digest of Object.values(previousManifest?.entries ?? {})) {
    retained.add(`${digest}.json`);
  }
  for (const digest of Object.values(previousManifest?.graceEntries ?? {})) {
    retained.add(`${digest}.json`);
  }
  try {
    for (const file of readdirSync(recordsDir(goldensRoot))) {
      if (!retained.has(file)) rmSync(path.join(recordsDir(goldensRoot), file), { force: true });
    }
  } catch {
    /* best-effort */
  }
  return { published: entries.length, manifest };
}

export function readGoldenManifest(goldensRoot) {
  return deepFreeze(JSON.parse(readFileSync(goldenManifestPath(goldensRoot), "utf8")));
}

/** @returns {Map<string, object>} logical name -> deep-frozen record */
export function readGoldenSet(goldensRoot) {
  const manifest = readGoldenManifest(goldensRoot);
  const set = new Map();
  for (const [name, digest] of Object.entries(manifest.entries)) {
    const text = readFileSync(recordPath(goldensRoot, digest), "utf8");
    if (sha256(text) !== digest)
      throw new Error(`golden record ${name} bytes do not match manifest digest ${digest}`);
    set.set(name, deepFreeze(JSON.parse(text)));
  }
  return set;
}

export function readGoldenByName(goldensRoot, name) {
  const manifest = readGoldenManifest(goldensRoot);
  const digest = manifest.entries[name];
  if (digest === undefined) throw new Error(`golden ${name} not in manifest`);
  const text = readFileSync(recordPath(goldensRoot, digest), "utf8");
  if (sha256(text) !== digest)
    throw new Error(`golden record ${name} bytes do not match manifest digest ${digest}`);
  return deepFreeze(JSON.parse(text));
}
