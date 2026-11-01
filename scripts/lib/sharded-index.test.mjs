#!/usr/bin/env node

// Tests for sharded-index.mjs. Run: node --test scripts/lib/sharded-index.test.mjs
//
// The loader is the merge authority for every sharded registration index
// (exposure-ownership map, reference-examples manifest), so these tests pin
// its contract directly: id-ordered merge, duplicate rejection, both fragment
// layouts, one directory listing per load, and a missing directory meaning an
// absent collection. The reader tests (PG0/VSW0/DOC1/DOC5/DOC7) prove the
// readers consume this merged view; a fragment added here is visible to every
// reader because no reader parses anything else.

import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { ShardedIndexError, loadShardedIndex } from "./sharded-index.mjs";

// Each case gets its own root: the header sits beside the fragment directory
// (the real indexes never keep a header inside its own fragment directory).
const roots = [];
const makeCase = () => {
  const root = mkdtempSync(join(tmpdir(), "sharded-index-"));
  roots.push(root);
  const fragments = join(root, "fragments");
  mkdirSync(fragments);
  return { root, fragments };
};
test.after(() => {
  for (const dir of roots) rmSync(dir, { recursive: true, force: true });
});

const writeHeader = (root, header) => {
  const path = join(root, "header.json");
  writeFileSync(path, `${JSON.stringify(header, null, 2)}\n`);
  return path;
};

test("flat fragments merge in id order with the header passed through", () => {
  const { root, fragments } = makeCase();
  writeFileSync(join(fragments, "Zeta.json"), JSON.stringify({ id: "Zeta", n: 1 }));
  writeFileSync(join(fragments, "alpha.json"), JSON.stringify({ id: "alpha", n: 2 }));
  writeFileSync(join(fragments, "mid.json"), JSON.stringify({ id: "mid", n: 3 }));
  const { header, rows } = loadShardedIndex(
    fragments,
    writeHeader(root, { id: "map.v1", schema: 1 }),
  );
  assert.deepEqual(header, { id: "map.v1", schema: 1 });
  assert.deepEqual(
    rows.map((row) => row.id),
    ["Zeta", "alpha", "mid"],
  );
  assert.deepEqual(
    rows.map((row) => row.n),
    [1, 2, 3],
  );
});

test("nested entry fragments (<id>/example.json) merge like flat ones", () => {
  const { root, fragments } = makeCase();
  mkdirSync(join(fragments, "unplugin-vite"));
  mkdirSync(join(fragments, "types-helpers"));
  writeFileSync(
    join(fragments, "types-helpers", "example.json"),
    JSON.stringify({ id: "types-helpers" }),
  );
  writeFileSync(
    join(fragments, "unplugin-vite", "example.json"),
    JSON.stringify({ id: "unplugin-vite" }),
  );
  // Non-fragment directories without the entry file are not collections.
  mkdirSync(join(fragments, "guides"));
  writeFileSync(join(fragments, "guides", "README.md"), "not a fragment\n");
  const { rows } = loadShardedIndex(fragments, writeHeader(root, { home: "x" }), {
    entry: "example.json",
  });
  assert.deepEqual(
    rows.map((row) => row.id),
    ["types-helpers", "unplugin-vite"],
  );
});

test("array fragments (owner-grouped rows) concatenate before the id sort", () => {
  const { root, fragments } = makeCase();
  writeFileSync(
    join(fragments, "DX0.json"),
    JSON.stringify([
      { id: "rails-policy.v1", owner: "DX0" },
      { id: "cross-surface-inventory.v1", owner: "DX0" },
    ]),
  );
  writeFileSync(
    join(fragments, "VSC1.json"),
    JSON.stringify([{ id: "desktop-inventory.v1", owner: "VSC1" }]),
  );
  const { rows } = loadShardedIndex(fragments, writeHeader(root, {}));
  assert.deepEqual(
    rows.map((row) => row.id),
    ["cross-surface-inventory.v1", "desktop-inventory.v1", "rails-policy.v1"],
  );
});

test("the same id in two fragments fails with a typed duplicate-id error", () => {
  const { root, fragments } = makeCase();
  writeFileSync(join(fragments, "a.json"), JSON.stringify({ id: "bounty", n: 1 }));
  writeFileSync(join(fragments, "b.json"), JSON.stringify({ id: "bounty", n: 2 }));
  assert.throws(
    () => loadShardedIndex(fragments, writeHeader(root, {})),
    (error) =>
      error instanceof ShardedIndexError &&
      error.code === "duplicate-id" &&
      error.id === "bounty" &&
      error.fragments.length === 2 &&
      /bounty/.test(error.message),
  );
});

test("the same id twice inside one array fragment fails the same way", () => {
  const { root, fragments } = makeCase();
  writeFileSync(
    join(fragments, "owner.json"),
    JSON.stringify([
      { id: "clash", n: 1 },
      { id: "clash", n: 2 },
    ]),
  );
  assert.throws(
    () => loadShardedIndex(fragments, writeHeader(root, {})),
    (error) => error instanceof ShardedIndexError && error.code === "duplicate-id",
  );
});

test("a fragment row without a string id fails with a typed error", () => {
  const { root, fragments } = makeCase();
  writeFileSync(join(fragments, "broken.json"), JSON.stringify({ owner: "DX0" }));
  assert.throws(
    () => loadShardedIndex(fragments, writeHeader(root, {})),
    (error) =>
      error instanceof ShardedIndexError &&
      error.code === "fragment-invalid" &&
      error.fragment === join(fragments, "broken.json"),
  );
});

test("a fragment that is not valid JSON fails naming the fragment", () => {
  const { root, fragments } = makeCase();
  writeFileSync(join(fragments, "broken.json"), "{ not json");
  assert.throws(
    () => loadShardedIndex(fragments, writeHeader(root, {})),
    (error) =>
      error instanceof ShardedIndexError &&
      error.code === "fragment-invalid" &&
      /broken\.json/.test(error.message),
  );
});

test("a header that is not valid JSON fails with a typed error", () => {
  const { root, fragments } = makeCase();
  const header = join(root, "header.json");
  writeFileSync(header, "{ not json");
  assert.throws(
    () => loadShardedIndex(fragments, header),
    (error) => error instanceof ShardedIndexError && error.code === "header-invalid",
  );
});

test("adding one fragment (no other edit) makes its row visible on the next load", () => {
  const { root, fragments } = makeCase();
  const header = writeHeader(root, { id: "map.v1" });
  writeFileSync(join(fragments, "alpha.json"), JSON.stringify({ id: "alpha" }));
  const before = loadShardedIndex(fragments, header);
  assert.deepEqual(
    before.rows.map((row) => row.id),
    ["alpha"],
  );
  writeFileSync(join(fragments, "new-node.json"), JSON.stringify({ id: "beta", owner: "NEW0" }));
  const after = loadShardedIndex(fragments, header);
  assert.deepEqual(
    after.rows.map((row) => row.id),
    ["alpha", "beta"],
  );
});

test("a missing fragment directory is an absent collection, not an error", () => {
  const { root } = makeCase();
  const { rows } = loadShardedIndex(join(root, "journeys"), writeHeader(root, {}));
  assert.deepEqual(rows, []);
});

test("directory listing errors fail explicitly instead of erasing rows", () => {
  const { root } = makeCase();
  const header = writeHeader(root, {});
  const notDirectory = join(root, "not-a-directory");
  writeFileSync(notDirectory, "file");
  const denied = Object.assign(new Error("access denied"), { code: "EACCES" });
  for (const [dir, options, code] of [
    [notDirectory, {}, "ENOTDIR"],
    [
      root,
      {
        io: {
          read: () => "{}",
          list: () => {
            throw denied;
          },
        },
      },
      "EACCES",
    ],
  ]) {
    assert.throws(
      () => loadShardedIndex(dir, header, options),
      (error) =>
        error instanceof ShardedIndexError &&
        error.code === "directory-unreadable" &&
        error.dir === dir &&
        error.cause.code === code,
    );
  }
});

test("each load lists its directory exactly once", () => {
  const { root, fragments } = makeCase();
  const header = writeHeader(root, {});
  const seen = { flat: 0, nested: 0 };
  const reads = [];
  const io = {
    list(dir, entry) {
      seen[entry == null ? "flat" : "nested"] += 1;
      return entry == null
        ? ["a", "b"].map((name) => join(dir, `${name}.json`))
        : ["only"].map((name) => join(dir, name, entry));
    },
    read(path) {
      reads.push(path);
      if (path === header) return "{}";
      const id = path.endsWith("example.json")
        ? "nested"
        : path.split("/").pop().replace(".json", "");
      return JSON.stringify({ id });
    },
  };
  loadShardedIndex(fragments, header, { io });
  assert.equal(seen.flat, 1);
  loadShardedIndex(fragments, header, { io, entry: "example.json" });
  assert.equal(seen.nested, 1);
  // Fragment files are read once each; only the shared header repeats.
  assert.equal(reads.filter((path) => path !== header).length, 3);
});
