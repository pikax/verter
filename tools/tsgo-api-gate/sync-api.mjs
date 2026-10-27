// Shared TypeScript 7 sync-API plumbing for the tsgo --api gate scripts.
//
// - The client is imported through the PUBLIC package exports `<pkg>/sync` and
//   `<pkg>/fs` (resolved with `require.resolve`, honouring the `exports` map),
//   so the gate exercises exactly the surface a real consumer uses.
// - The API, every Snapshot and every Program are Disposable. Scripts declare
//   them with `using`; a snapshot is advanced only through `advanceSnapshot`,
//   which updates the CURRENT active snapshot, keeps the snapshot the update
//   returns, and disposes the superseded one. Nothing touches a snapshot after
//   it was updated away or disposed.
// - The off-disk overlay answers every filesystem callback the client
//   registers: a function for the off-disk files it owns, the `serverFS.useOS`
//   sentinel for everything else (the server then reads the real filesystem).

import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import path from "node:path";
import fs from "node:fs";

const require = createRequire(import.meta.url);

/** Load `<pkg>/sync` + `<pkg>/fs` from the TS>=7 source package. */
export async function loadSyncApi() {
  const searchPaths = (process.env.NM_BASE || "").split(path.delimiter).filter(Boolean);
  const opts = { paths: searchPaths.length ? searchPaths : undefined };
  const sourcePkgs = [process.env.TS7_SOURCE, "typescript"].filter(Boolean);
  for (const pkg of sourcePkgs) {
    let syncPath;
    let fsPath;
    try {
      syncPath = require.resolve(`${pkg}/sync`, opts);
      fsPath = require.resolve(`${pkg}/fs`, opts);
    } catch {
      continue;
    }
    const { API } = await import(pathToFileURL(syncPath).href);
    const { serverFS } = await import(pathToFileURL(fsPath).href);
    return { API, serverFS, specifier: `${pkg}/sync`, syncPath };
  }
  throw new Error(
    `could not resolve a TS>=7 sync API (<pkg>/sync) from ${JSON.stringify(sourcePkgs)}`,
  );
}

const key = (p) => {
  const n = p.replace(/\\/g, "/");
  return process.platform === "win32" ? n.toLowerCase() : n;
};

/**
 * An off-disk overlay over `files` (absolute path -> content string or a
 * `() => string`). Off-disk files exist, read their content, stat as fake
 * files, and are injected into their directory's listing so a tsconfig
 * `include` glob discovers them; every other query falls through to the
 * server's own filesystem. `onRead(path)` observes each off-disk read.
 */
export function overlayFileSystem(serverFS, files, { onRead } = {}) {
  const contents = new Map();
  const listings = new Map();
  for (const [file, content] of Object.entries(files)) {
    contents.set(key(file), typeof content === "function" ? content : () => content);
    const dir = key(path.dirname(file));
    if (!listings.has(dir)) listings.set(dir, { dir: path.dirname(file), names: [] });
    listings.get(dir).names.push(path.basename(file));
  }
  return {
    readFile(fileName) {
      const read = contents.get(key(fileName));
      if (!read) return serverFS.useOS;
      onRead?.(fileName);
      return read();
    },
    fileExists(fileName) {
      return contents.has(key(fileName)) ? true : serverFS.useOS;
    },
    stat(fileName) {
      return contents.has(key(fileName)) ? serverFS.fakeStat : serverFS.useOS;
    },
    getAccessibleEntries(dirName) {
      const listing = listings.get(key(dirName));
      if (!listing) return serverFS.useOS;
      let real;
      try {
        const entries = fs.readdirSync(dirName, { withFileTypes: true });
        real = {
          files: entries.filter((e) => e.isFile()).map((e) => e.name),
          directories: entries.filter((e) => e.isDirectory()).map((e) => e.name),
        };
      } catch {
        real = { files: [], directories: [] };
      }
      for (const name of listing.names) if (!real.files.includes(name)) real.files.push(name);
      return real;
    },
    directoryExists: serverFS.useOS,
    realpath: serverFS.useOS,
    writeFile: serverFS.useOS,
    removeFile: serverFS.useOS,
  };
}

/**
 * Update the CURRENT active `snapshot` with `params` (programs re-ensured by
 * default) and return the snapshot to use from now on. When the server
 * produced a new snapshot the superseded one is disposed; the caller must
 * replace its reference with the returned value and never touch the old one.
 */
export function advanceSnapshot(snapshot, params) {
  const next = snapshot.update({ ensurePrograms: true, ...params });
  if (next !== snapshot) snapshot.dispose();
  return next;
}

/**
 * A snapshot lineage: `current` is the one active snapshot, `advance(params)`
 * moves it forward (disposing the superseded snapshot), and disposing the
 * lineage disposes whatever snapshot is current. Declare it with `using`.
 */
export function openSnapshotLineage(api, params) {
  let current = api.createSnapshot(params);
  return {
    get current() {
      return current;
    },
    advance(changes) {
      current = advanceSnapshot(current, changes);
      return current;
    },
    [Symbol.dispose]() {
      if (!current.isDisposed()) current.dispose();
    },
  };
}
