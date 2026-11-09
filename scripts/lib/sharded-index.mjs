#!/usr/bin/env node

/**
 * sharded-index.mjs — the shared loader for sharded registration indexes.
 *
 * A sharded index is a header JSON file plus a fragment directory where each
 * product node registers rows by adding its own fragment file, so no node
 * edits a shared row array. This loader is the single authority that merges
 * fragments back into the index view: rows are returned in id order and the
 * same id in two fragments (or twice in one fragment) is a typed error, never
 * a silently shadowed row.
 *
 * Fragment layouts (options.entry):
 *   - flat (default): `<dir>/<id>.json`
 *   - nested:         `<dir>/<id>/<entry>` (e.g. `examples/<id>/example.json`)
 *
 * A fragment holds either one row object or an array of row objects; every
 * row carries a string `id` (the pre-sharding row schemas are unchanged).
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

/** Typed loader failure: `code` names the class, extras carry the fragment. */
export class ShardedIndexError extends Error {
  constructor(code, message, extra = {}) {
    super(message);
    this.name = "ShardedIndexError";
    this.code = code;
    Object.assign(this, extra);
  }
}

/** Locale-independent ordering shared by resolved indexes and their receipts. */
export const compareIds = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

const defaultIo = {
  /**
   * Fragment file paths directly under `dir` for the given layout. A missing
   * directory is an empty collection (an index section no node has produced
   * yet), not an error.
   */
  list(dir, entry) {
    let names;
    try {
      names = readdirSync(dir).sort();
    } catch (cause) {
      if (cause.code === "ENOENT") return [];
      throw cause;
    }
    const fragments = [];
    for (const name of names) {
      if (entry == null && !name.endsWith(".json")) continue;
      const path = entry == null ? join(dir, name) : join(dir, name, entry);
      try {
        if (statSync(path).isFile()) fragments.push(path);
      } catch (cause) {
        // A nested entry is optional; regular files in the parent are not
        // example directories. Permission and I/O failures remain errors.
        if (cause.code === "ENOENT" || (entry != null && cause.code === "ENOTDIR")) continue;
        throw cause;
      }
    }
    return fragments;
  },
  read(path) {
    return readFileSync(path, "utf8");
  },
};

/**
 * Loads one sharded index: the header JSON at `headerPath` plus every
 * fragment in `dir`. Returns `{ header, rows }` with `rows` sorted by id
 * (code-unit order, locale-independent). Throws ShardedIndexError with code
 * `header-invalid`, `directory-unreadable`, `fragment-invalid` or `duplicate-id`
 * on a corrupt or unreadable index.
 *
 * `options.io` (`{ list(dir, entry), read(path) }`) overrides filesystem
 * access so overlay-driven callers resolve the same merged view.
 */
export function loadShardedIndex(dir, headerPath, options = {}) {
  const io = options.io ?? defaultIo;
  let header;
  try {
    header = JSON.parse(io.read(headerPath));
  } catch (cause) {
    throw new ShardedIndexError("header-invalid", `index header is not valid JSON: ${headerPath}`, {
      headerPath,
      cause,
    });
  }
  const entry = options.entry ?? null;
  const rows = [];
  const fragmentById = new Map();
  const addRow = (row, fragment) => {
    if (
      row == null ||
      typeof row !== "object" ||
      Array.isArray(row) ||
      typeof row.id !== "string" ||
      row.id.length === 0
    ) {
      throw new ShardedIndexError(
        "fragment-invalid",
        `fragment row needs a string id: ${fragment}`,
        {
          fragment,
        },
      );
    }
    const prior = fragmentById.get(row.id);
    if (prior != null) {
      throw new ShardedIndexError(
        "duplicate-id",
        `duplicate row id "${row.id}" in ${dir}: ${prior} and ${fragment}`,
        { id: row.id, fragments: [prior, fragment] },
      );
    }
    fragmentById.set(row.id, fragment);
    rows.push(row);
  };
  // One listing per load: the fragment set is decided here and each fragment
  // file is read exactly once.
  let fragments;
  try {
    fragments = io.list(dir, entry);
  } catch (cause) {
    throw new ShardedIndexError("directory-unreadable", `cannot list index directory: ${dir}`, {
      dir,
      cause,
    });
  }
  for (const fragment of fragments) {
    let parsed;
    try {
      parsed = JSON.parse(io.read(fragment));
    } catch (cause) {
      throw new ShardedIndexError("fragment-invalid", `fragment is not valid JSON: ${fragment}`, {
        fragment,
        cause,
      });
    }
    if (Array.isArray(parsed)) {
      for (const row of parsed) addRow(row, fragment);
    } else {
      addRow(parsed, fragment);
    }
  }
  rows.sort((a, b) => compareIds(a.id, b.id));
  return { header, rows };
}
