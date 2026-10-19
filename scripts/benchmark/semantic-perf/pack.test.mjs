// Self-tests of the run-directory packer.
//
//   node --test scripts/benchmark/semantic-perf/pack.test.mjs

import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { inflateRawSync } from "node:zlib";

import { crc32, packSmallFiles, PACK_ARCHIVE, readZipDirectory, writeZip } from "./pack.mjs";

// Walks the archive's local headers front to back: a reading path independent
// of the central directory the packer itself verifies against.
function localEntries(buf) {
  const out = new Map();
  let p = 0;
  while (buf.readUInt32LE(p) === 0x04034b50) {
    const method = buf.readUInt16LE(p + 8);
    const csize = buf.readUInt32LE(p + 18);
    const nameLen = buf.readUInt16LE(p + 26);
    const extraLen = buf.readUInt16LE(p + 28);
    const name = buf.toString("utf8", p + 30, p + 30 + nameLen);
    const start = p + 30 + nameLen + extraLen;
    const body = buf.subarray(start, start + csize);
    out.set(name, {
      data: method === 8 ? inflateRawSync(body) : Buffer.from(body),
      crc: buf.readUInt32LE(p + 14),
    });
    p = start + csize;
  }
  return out;
}

test("crc32 matches the standard check value", () => {
  assert.equal(crc32(Buffer.from("123456789")), 0xcbf43926);
});

test("small files move into the archive byte for byte; large files and the reports stay", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pack-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  mkdirSync(join(dir, "sessions", "s1", "project"), { recursive: true });
  mkdirSync(join(dir, "bin"));
  const small = {
    "sessions/s1/project/a.ts": "export const a = 1;\n".repeat(50),
    "sessions/s1/job.json": "{}",
    "sessions/s1/empty.log": "",
    "sessions/s1/ü-name.txt": "unicode",
  };
  for (const [name, text] of Object.entries(small)) writeFileSync(join(dir, name), text);
  writeFileSync(join(dir, "bin", "probe"), Buffer.alloc(2048, 7));
  writeFileSync(join(dir, "results.json"), "{}");
  writeFileSync(join(dir, "results.md"), "# r");

  const r = packSmallFiles(dir, { maxBytes: 1024 });
  assert.equal(r.files, 4);
  const entries = localEntries(readFileSync(join(dir, PACK_ARCHIVE)));
  assert.deepEqual([...entries.keys()].sort(), Object.keys(small).sort());
  for (const [name, text] of Object.entries(small)) {
    assert.equal(entries.get(name).data.toString("utf8"), text, name);
    assert.equal(entries.get(name).crc, crc32(Buffer.from(text)), name);
  }
  assert.ok(!existsSync(join(dir, "sessions")), "emptied directories are removed");
  for (const kept of ["bin/probe", "results.json", "results.md"])
    assert.ok(existsSync(join(dir, kept)), kept);
});

test("more than 65,534 entries are written as Zip64 and read back in full", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pack64-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const data = Buffer.from("x");
  const count = 0x10000 + 3;
  const files = Array.from({ length: count }, (_, i) => ({
    data,
    name: `f${i}`,
    size: 1,
    mtime: new Date(0),
  }));
  const archive = join(dir, "a.zip");
  writeZip(archive, files);
  const buf = readFileSync(archive);
  assert.equal(buf.readUInt16LE(buf.length - 22 + 10), 0xffff, "the classic record saturates");
  const directory = readZipDirectory(archive);
  assert.equal(directory.length, count);
  assert.equal(directory.at(-1).name, `f${count - 1}`);
});

test("a run with nothing small to pack is left without an archive", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pack0-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "results.json"), "{}");
  assert.equal(packSmallFiles(dir).files, 0);
  assert.ok(!existsSync(join(dir, PACK_ARCHIVE)));
});
