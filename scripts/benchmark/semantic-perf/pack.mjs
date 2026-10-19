// Packs a run directory's small files into one zip archive, so that copying a
// run moves a few files instead of tens of thousands. Files at or above the
// size limit, and the top-level results.json and results.md, stay as they are.
// Every entry is read back from the archive and compared with its file before
// any file is removed, so a failed pack leaves the run untouched.
//
//   node scripts/benchmark/semantic-perf/pack.mjs <run dir> [--max-bytes <n>]
//
// Any zip tool opens the archive (Zip64 when it holds more than 65,534 entries
// or grows past 4 GiB). No dependency and no external program is needed.

import {
  closeSync,
  existsSync,
  openSync,
  readdirSync,
  readFileSync,
  readSync,
  rmdirSync,
  statSync,
  unlinkSync,
  writeSync,
} from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateRawSync, inflateRawSync } from "node:zlib";

export const PACK_ARCHIVE = "small-files.zip";
export const PACK_MAX_BYTES = 1024 * 1024;
const KEEP_LOOSE = new Set(["results.json", "results.md"]);

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();
export function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function dosTime(date) {
  const y = Math.max(1980, date.getFullYear());
  return {
    time: (date.getHours() << 11) | (date.getMinutes() << 5) | (date.getSeconds() >> 1),
    date: ((y - 1980) << 9) | ((date.getMonth() + 1) << 5) | date.getDate(),
  };
}

// The files to pack, in a stable order: every regular file below `dir` under
// the size limit, except the archive and the top-level reports.
export function smallFiles(dir, maxBytes = PACK_MAX_BYTES) {
  const out = [];
  const walk = (d) => {
    for (const entry of readdirSync(d, { withFileTypes: true }).sort((a, b) =>
      a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
    )) {
      const path = join(d, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.isFile()) {
        const name = relative(dir, path).split(sep).join("/");
        if (name === PACK_ARCHIVE || KEEP_LOOSE.has(name)) continue;
        const st = statSync(path);
        if (st.size < maxBytes) out.push({ path, name, size: st.size, mtime: st.mtime });
      }
    }
  };
  walk(dir);
  return out;
}

const MAX32 = 0xffffffff;
const MAX16 = 0xffff;

// Writes `files` (from smallFiles; a record may carry its bytes as `data`) to
// `archive`; returns the entries written.
export function writeZip(archive, files) {
  const fd = openSync(archive, "w");
  let offset = 0;
  // Writes are batched: a run holds tens of thousands of tiny entries.
  let pending = [];
  let pendingBytes = 0;
  const flush = () => {
    if (pendingBytes) writeSync(fd, Buffer.concat(pending, pendingBytes));
    pending = [];
    pendingBytes = 0;
  };
  const put = (buf) => {
    pending.push(buf);
    pendingBytes += buf.length;
    offset += buf.length;
    if (pendingBytes >= 1 << 20) flush();
  };
  const entries = [];
  try {
    for (const file of files) {
      const data = file.data ?? readFileSync(file.path);
      // A few dozen bytes never shrink enough to pay for a compressor.
      const deflated = data.length >= 64 ? deflateRawSync(data, { level: 6 }) : data;
      const method = deflated.length < data.length ? 8 : 0;
      const body = method === 8 ? deflated : data;
      const name = Buffer.from(file.name, "utf8");
      const { time, date } = dosTime(file.mtime);
      const crc = crc32(data);
      const local = Buffer.alloc(30);
      local.writeUInt32LE(0x04034b50, 0);
      local.writeUInt16LE(20, 4);
      local.writeUInt16LE(0x0800, 6);
      local.writeUInt16LE(method, 8);
      local.writeUInt16LE(time, 10);
      local.writeUInt16LE(date, 12);
      local.writeUInt32LE(crc, 14);
      local.writeUInt32LE(body.length, 18);
      local.writeUInt32LE(data.length, 22);
      local.writeUInt16LE(name.length, 26);
      local.writeUInt16LE(0, 28);
      entries.push({
        name,
        method,
        time,
        date,
        crc,
        csize: body.length,
        usize: data.length,
        offset,
      });
      put(local);
      put(name);
      put(body);
    }
    const cdStart = offset;
    for (const e of entries) {
      const big = e.offset >= MAX32;
      const extra = big ? Buffer.alloc(12) : Buffer.alloc(0);
      if (big) {
        extra.writeUInt16LE(0x0001, 0);
        extra.writeUInt16LE(8, 2);
        extra.writeBigUInt64LE(BigInt(e.offset), 4);
      }
      const h = Buffer.alloc(46);
      h.writeUInt32LE(0x02014b50, 0);
      h.writeUInt16LE(45, 4);
      h.writeUInt16LE(big ? 45 : 20, 6);
      h.writeUInt16LE(0x0800, 8);
      h.writeUInt16LE(e.method, 10);
      h.writeUInt16LE(e.time, 12);
      h.writeUInt16LE(e.date, 14);
      h.writeUInt32LE(e.crc, 16);
      h.writeUInt32LE(e.csize, 20);
      h.writeUInt32LE(e.usize, 24);
      h.writeUInt16LE(e.name.length, 28);
      h.writeUInt16LE(extra.length, 30);
      h.writeUInt32LE(big ? MAX32 : e.offset, 42);
      put(h);
      put(e.name);
      put(extra);
    }
    const cdSize = offset - cdStart;
    const zip64 = entries.length >= MAX16 || cdStart >= MAX32 || cdSize >= MAX32;
    if (zip64) {
      const eocd64At = offset;
      const r = Buffer.alloc(56);
      r.writeUInt32LE(0x06064b50, 0);
      r.writeBigUInt64LE(44n, 4);
      r.writeUInt16LE(45, 12);
      r.writeUInt16LE(45, 14);
      r.writeBigUInt64LE(BigInt(entries.length), 24);
      r.writeBigUInt64LE(BigInt(entries.length), 32);
      r.writeBigUInt64LE(BigInt(cdSize), 40);
      r.writeBigUInt64LE(BigInt(cdStart), 48);
      put(r);
      const loc = Buffer.alloc(20);
      loc.writeUInt32LE(0x07064b50, 0);
      loc.writeBigUInt64LE(BigInt(eocd64At), 8);
      loc.writeUInt32LE(1, 16);
      put(loc);
    }
    const end = Buffer.alloc(22);
    end.writeUInt32LE(0x06054b50, 0);
    end.writeUInt16LE(Math.min(entries.length, MAX16), 8);
    end.writeUInt16LE(Math.min(entries.length, MAX16), 10);
    end.writeUInt32LE(Math.min(cdSize, MAX32), 12);
    end.writeUInt32LE(Math.min(cdStart, MAX32), 16);
    put(end);
    flush();
  } finally {
    closeSync(fd);
  }
  return entries;
}

// Reads an archive's central directory: [{ name, method, crc, csize, usize, offset }].
export function readZipDirectory(archive) {
  const size = statSync(archive).size;
  const fd = openSync(archive, "r");
  const read = (pos, len) => {
    const b = Buffer.alloc(len);
    readSync(fd, b, 0, len, pos);
    return b;
  };
  try {
    const tailLen = Math.min(size, 22 + MAX16);
    const tail = read(size - tailLen, tailLen);
    let at = -1;
    for (let i = tailLen - 22; i >= 0; i--)
      if (tail.readUInt32LE(i) === 0x06054b50) {
        at = i;
        break;
      }
    if (at < 0) throw new Error(`${archive}: no end of central directory`);
    let count = tail.readUInt16LE(at + 10);
    let cdSize = tail.readUInt32LE(at + 12);
    let cdStart = tail.readUInt32LE(at + 16);
    if (at >= 20 && tail.readUInt32LE(at - 20) === 0x07064b50) {
      const r = read(Number(tail.readBigUInt64LE(at - 12)), 56);
      if (r.readUInt32LE(0) !== 0x06064b50) throw new Error(`${archive}: bad Zip64 record`);
      count = Number(r.readBigUInt64LE(32));
      cdSize = Number(r.readBigUInt64LE(40));
      cdStart = Number(r.readBigUInt64LE(48));
    }
    const cd = read(cdStart, cdSize);
    const out = [];
    let p = 0;
    for (let i = 0; i < count; i++) {
      if (cd.readUInt32LE(p) !== 0x02014b50) throw new Error(`${archive}: bad central header ${i}`);
      const nameLen = cd.readUInt16LE(p + 28);
      const extraLen = cd.readUInt16LE(p + 30);
      const commentLen = cd.readUInt16LE(p + 32);
      let offset = cd.readUInt32LE(p + 42);
      const extra = cd.subarray(p + 46 + nameLen, p + 46 + nameLen + extraLen);
      if (offset === MAX32)
        for (let q = 0; q + 4 <= extra.length; q += 4 + extra.readUInt16LE(q + 2))
          if (extra.readUInt16LE(q) === 0x0001) offset = Number(extra.readBigUInt64LE(q + 4));
      out.push({
        name: cd.toString("utf8", p + 46, p + 46 + nameLen),
        method: cd.readUInt16LE(p + 10),
        crc: cd.readUInt32LE(p + 16),
        csize: cd.readUInt32LE(p + 20),
        usize: cd.readUInt32LE(p + 24),
        offset,
      });
      p += 46 + nameLen + extraLen + commentLen;
    }
    return out;
  } finally {
    closeSync(fd);
  }
}

// One entry's bytes, read through its local header.
export function readZipEntry(archive, entry) {
  const fd = openSync(archive, "r");
  try {
    const h = Buffer.alloc(30);
    readSync(fd, h, 0, 30, entry.offset);
    if (h.readUInt32LE(0) !== 0x04034b50) throw new Error(`${entry.name}: bad local header`);
    const body = Buffer.alloc(entry.csize);
    readSync(fd, body, 0, entry.csize, entry.offset + 30 + h.readUInt16LE(26) + h.readUInt16LE(28));
    const data = entry.method === 8 ? inflateRawSync(body) : body;
    if (data.length !== entry.usize || crc32(data) !== entry.crc)
      throw new Error(`${entry.name}: size or checksum differs from its directory record`);
    return data;
  } finally {
    closeSync(fd);
  }
}

// Reads files at the paths the run recorded (absolute, on the machine that
// ran it): the plain disk.
export const DISK_FILES = {
  read: (path) => readFileSync(path, "utf8"),
  exists: (path) => existsSync(path),
};

// Reads a run's own files from `dir`, wherever the run was made: a recorded
// path under `recordedOut` (the run's output directory as recorded) resolves
// to the same relative path under `dir`, loose or in its small-files.zip; any
// other path is read from disk. A file in neither place throws ENOENT.
export function runFileReader(dir, recordedOut) {
  const root = resolve(dir);
  const archive = join(root, PACK_ARCHIVE);
  const entries = existsSync(archive)
    ? new Map(readZipDirectory(archive).map((e) => [e.name, e]))
    : new Map();
  const slashed = (p) => String(p).replace(/\\/g, "/");
  const out = recordedOut ? slashed(recordedOut).replace(/\/+$/, "") : null;
  const relativeName = (path) => {
    const p = slashed(path);
    return out && p.startsWith(`${out}/`) ? p.slice(out.length + 1) : null;
  };
  const missing = (path) =>
    Object.assign(new Error(`ENOENT: no such run file, ${path}`), { code: "ENOENT" });
  return {
    read(path) {
      const name = relativeName(path);
      if (name === null) return readFileSync(path, "utf8");
      const loose = join(root, ...name.split("/"));
      if (existsSync(loose)) return readFileSync(loose, "utf8");
      const entry = entries.get(name);
      if (!entry) throw missing(path);
      return readZipEntry(archive, entry).toString("utf8");
    },
    exists(path) {
      const name = relativeName(path);
      if (name === null) return existsSync(path);
      return existsSync(join(root, ...name.split("/"))) || entries.has(name);
    },
  };
}

function removeEmptyDirs(dir, root) {
  for (const entry of readdirSync(dir, { withFileTypes: true }))
    if (entry.isDirectory()) removeEmptyDirs(join(dir, entry.name), root);
  if (dir !== root && readdirSync(dir).length === 0) rmdirSync(dir);
}

// Packs `dir`'s small files into `dir/small-files.zip` and removes them.
// Returns { archive, files, bytes } (files 0: nothing to pack, no archive).
export function packSmallFiles(dir, { maxBytes = PACK_MAX_BYTES } = {}) {
  const root = resolve(dir);
  const archive = join(root, PACK_ARCHIVE);
  const files = smallFiles(root, maxBytes);
  if (files.length === 0) return { archive: null, files: 0, bytes: 0 };
  writeZip(archive, files);
  const directory = readZipDirectory(archive);
  if (directory.length !== files.length)
    throw new Error(`${archive}: ${directory.length} entries for ${files.length} files`);
  directory.forEach((entry, i) => {
    if (entry.name !== files[i].name)
      throw new Error(`${archive}: entry ${i} is ${entry.name}, expected ${files[i].name}`);
    if (!readZipEntry(archive, entry).equals(readFileSync(files[i].path)))
      throw new Error(`${archive}: ${entry.name} differs from its file`);
  });
  for (const f of files) unlinkSync(f.path);
  removeEmptyDirs(root, root);
  return { archive, files: files.length, bytes: files.reduce((n, f) => n + f.size, 0) };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const dir = args.find((a, i) => !a.startsWith("--") && args[i - 1] !== "--max-bytes");
  const mi = args.indexOf("--max-bytes");
  if (!dir) {
    console.error(
      "usage: node scripts/benchmark/semantic-perf/pack.mjs <run dir> [--max-bytes <n>]",
    );
    process.exit(2);
  }
  const maxBytes = mi >= 0 ? Number(args[mi + 1]) : PACK_MAX_BYTES;
  if (!(maxBytes > 0)) throw new Error("--max-bytes must be a positive number");
  const r = packSmallFiles(dir, { maxBytes });
  console.log(
    r.files
      ? `packed ${r.files} files (${(r.bytes / 1048576).toFixed(1)} MiB) into ${r.archive}`
      : "nothing to pack",
  );
}
