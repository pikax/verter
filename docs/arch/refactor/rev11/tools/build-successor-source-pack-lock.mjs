#!/usr/bin/env node
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { PACKAGE_ROOT, readToml } from "./lib.mjs";

export const SUCCESSOR_SOURCE_PACK_RELATIVE = "sources/successor-dag-charter-pack";
export const SUCCESSOR_SOURCE_PACK_LOCK_RELATIVE = "provenance/successor-source-pack-lock.toml";
export const SUCCESSOR_SOURCE_PACK_FILE_COUNT = 92;

const sha256 = (value) => crypto.createHash("sha256").update(value).digest("hex");

function packRows(packageRoot) {
  const packRoot = path.join(packageRoot, SUCCESSOR_SOURCE_PACK_RELATIVE);
  const rows = [];
  const walk = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      if (entry.isSymbolicLink())
        throw new Error(
          `successor source pack contains forbidden symlink ${path.relative(packRoot, absolute)}`,
        );
      if (entry.isDirectory()) walk(absolute);
      else if (entry.isFile()) {
        const bytes = fs.readFileSync(absolute);
        rows.push({
          path: path.relative(packRoot, absolute).split(path.sep).join("/"),
          bytes: bytes.length,
          sha256: sha256(bytes),
        });
      } else
        throw new Error(
          `successor source pack contains unsupported entry ${path.relative(packRoot, absolute)}`,
        );
    }
  };
  walk(packRoot);
  return rows.sort((left, right) => left.path.localeCompare(right.path));
}

function inventoryDigest(rows) {
  return sha256(`${JSON.stringify(rows)}\n`);
}

export function renderSuccessorSourcePackLock(packageRoot = PACKAGE_ROOT) {
  const rows = packRows(packageRoot);
  return [
    "schema = 1",
    'catalog = "successor-source-pack-lock"',
    `source_root = ${JSON.stringify(SUCCESSOR_SOURCE_PACK_RELATIVE)}`,
    `file_count = ${rows.length}`,
    `inventory_sha256 = ${JSON.stringify(inventoryDigest(rows))}`,
    "",
    ...rows.flatMap((row) => [
      "[[file]]",
      `path = ${JSON.stringify(row.path)}`,
      `bytes = ${row.bytes}`,
      `sha256 = ${JSON.stringify(row.sha256)}`,
      "",
    ]),
  ].join("\n");
}

export function validateSuccessorSourcePack(packageRoot = PACKAGE_ROOT) {
  const errors = [];
  let actual;
  try {
    actual = packRows(packageRoot);
  } catch (error) {
    return [error.message];
  }
  if (actual.length !== SUCCESSOR_SOURCE_PACK_FILE_COUNT)
    errors.push(
      `successor source pack must contain exactly ${SUCCESSOR_SOURCE_PACK_FILE_COUNT} files, found ${actual.length}`,
    );

  let model;
  try {
    model = readToml(path.join(packageRoot, SUCCESSOR_SOURCE_PACK_LOCK_RELATIVE));
  } catch (error) {
    return [...errors, `unable to read successor source pack lock: ${error.message}`];
  }
  const rows = model.file || [];
  if (
    model.schema !== 1 ||
    model.catalog !== "successor-source-pack-lock" ||
    model.source_root !== SUCCESSOR_SOURCE_PACK_RELATIVE
  )
    errors.push("successor source pack lock header mismatch");
  if (
    model.file_count !== SUCCESSOR_SOURCE_PACK_FILE_COUNT ||
    rows.length !== SUCCESSOR_SOURCE_PACK_FILE_COUNT
  )
    errors.push("successor source pack lock file count mismatch");
  if (new Set(rows.map((row) => row.path)).size !== rows.length)
    errors.push("successor source pack lock contains duplicate paths");
  const normalized = rows.map((row) => ({ path: row.path, bytes: row.bytes, sha256: row.sha256 }));
  if (JSON.stringify(normalized) !== JSON.stringify(actual))
    errors.push("successor source pack path/byte/digest inventory mismatch");
  if (model.inventory_sha256 !== inventoryDigest(normalized))
    errors.push("successor source pack inventory digest mismatch");
  return errors;
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const lockFile = path.join(PACKAGE_ROOT, SUCCESSOR_SOURCE_PACK_LOCK_RELATIVE);
  if (process.argv.includes("--check")) {
    const errors = validateSuccessorSourcePack(PACKAGE_ROOT);
    if (errors.length) {
      for (const error of errors) console.error(`ERROR: ${error}`);
      process.exit(1);
    }
    if (fs.readFileSync(lockFile, "utf8") !== renderSuccessorSourcePackLock(PACKAGE_ROOT)) {
      console.error("ERROR: successor source pack lock rendering is stale");
      process.exit(1);
    }
    console.log(
      `build-successor-source-pack-lock: PASS (${SUCCESSOR_SOURCE_PACK_FILE_COUNT} exact files)`,
    );
  } else {
    fs.writeFileSync(lockFile, renderSuccessorSourcePackLock(PACKAGE_ROOT));
    console.log(
      `build-successor-source-pack-lock: wrote ${SUCCESSOR_SOURCE_PACK_FILE_COUNT} exact files`,
    );
  }
}
