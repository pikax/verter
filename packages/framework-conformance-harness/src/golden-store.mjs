// Immutable, atomically published golden storage (conformance-goldens.md).
//
// Each record is a content-addressed file at
// `goldens/records/<sha256-of-record-text>.json`. One manifest
// (`goldens/manifest.json`) names the current valid set (logical name →
// record digest). The manifest is replaced via write-to-temp-then-rename,
// so the entire set has one reader-visible commit point:
//
//  - a generation run that fails after writing records has not touched the
//    manifest; readers still see the previous complete set (orphan records
//    are invisible — readers resolve only through the manifest);
//  - a record file is write-once: a digest that already exists verifies
//    byte-identity instead of rewriting (collision with different bytes is
//    a hard error).
//
// Reader-schedule safety: an immediate post-commit sweep of records the
// new manifest no longer references would delete them under a reader that
// loaded the old manifest. Each manifest carries a monotonic `generation`
// plus `graceEntries` of the manifest it replaced. Post-commit GC retains
// three generations (N, N-1, N-2). Two generations of grace, not one: a
// reader of N may take long enough that N+1 and N+2 complete before it
// dereferences records; one generation of grace lost those records. An
// in-flight reader of N can read every record N lists until a third
// publish completes. Records unreferenced for three consecutive
// generations are collected.
//
// Write path: only `publishGoldenSet`, called only from
// `bin/generate-goldens.mjs`. The comparator (`src/compare.mjs`) never
// imports `node:fs` write functions, so candidate output cannot update its
// own expectation. Readers return deep-frozen objects.

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
