import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, test } from "node:test";
import { gzipSync } from "node:zlib";

import {
  AUTHORITY,
  checkPins,
  extractTarball,
  jsonStringValues,
  parseVersion,
  PIN_SITES,
  readPin,
  ROOT,
  setPin,
} from "./typescript-pin.mjs";

const temps = [];
after(() => {
  for (const dir of temps) rmSync(dir, { recursive: true, force: true });
});

/** A copy of every pin-site file (and the authority) in a fresh root. */
function copyPinTree() {
  const root = mkdtempSync(join(tmpdir(), "typescript-pin-test-"));
  temps.push(root);
  const paths = new Set([AUTHORITY.path, ...PIN_SITES.map((site) => site.path)]);
  for (const path of paths) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    cpSync(join(ROOT, path), join(root, path));
  }
  return root;
}

test("the authority is the workspace typescript devDependency and is strict SemVer", () => {
  const pin = readPin(ROOT);
  const root = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
  assert.equal(pin, root.devDependencies.typescript);
  assert.doesNotThrow(() => parseVersion(pin));
});

test("every pin site in the tree agrees with the authority", () => {
  const { pin, problems } = checkPins(ROOT);
  assert.deepEqual(
    problems,
    [],
    `pin sites disagree with ${pin} — run \`node scripts/typescript-pin.mjs --check\``,
  );
});

test("--set moves every writable site, leaves derived records to their regenerator, and --check reports them", () => {
  const root = copyPinTree();
  const before = readPin(root);
  const next = (() => {
    const v = parseVersion(before);
    return `${v.major}.${v.minor}.${v.patch + 7}`;
  })();
  const { changed, followUp, lineChange } = setPin(root, next);
  assert.equal(lineChange, false);
  assert.equal(readPin(root), next);
  assert.ok(changed.includes("package.json"));
  assert.ok(changed.includes("crates/verter_tsgo_api/src/toolchain/policy.rs"));
  assert.match(
    readFileSync(join(root, "crates/verter_tsgo_api/src/toolchain/policy.rs"), "utf8"),
    new RegExp(`"${next.replace(/\./g, "\\.")}"`),
  );
  assert.ok(followUp.includes("pnpm install"));
  assert.ok(followUp.includes("pnpm gen:oracle-snapshots"));
  // Only the derived records (digests, resolutions) still disagree, each naming
  // the command that regenerates it.
  const { problems } = checkPins(root);
  assert.ok(problems.length > 0);
  for (const problem of problems) {
    assert.ok(problem.regenerate, `${problem.site} must be writable by --set: ${problem.problem}`);
  }
});

test("--set refuses another release line unless asked, and then moves the line macros", () => {
  const root = copyPinTree();
  const v = parseVersion(readPin(root));
  const nextLine = `${v.major}.${v.minor + 1}.0`;
  assert.throws(() => setPin(root, nextLine), /release line/);
  const { lineChange } = setPin(root, nextLine, { allowLineChange: true });
  assert.equal(lineChange, true);
  const policy = readFileSync(join(root, "crates/verter_tsgo_api/src/toolchain/policy.rs"), "utf8");
  assert.match(
    policy,
    new RegExp(`typescript_line \\{\\s*\\(\\) => \\{\\s*"${v.major}\\.${v.minor + 1}"`),
  );
  assert.match(
    policy,
    new RegExp(`typescript_line_ceiling \\{\\s*\\(\\) => \\{\\s*"${v.major}\\.${v.minor + 2}\\.0"`),
  );
});

test("--set rejects a version that is not strict SemVer", () => {
  const root = copyPinTree();
  for (const bad of ["7.1", "v7.1.0", "7.1.0+build", "latest", "^7.1.0"]) {
    assert.throws(() => setPin(root, bad), /SemVer/, bad);
  }
});

test("JSON values are located by key path without re-serialising the file", () => {
  const text = '{ "a": { "b": "x\\"y" }, "c": [ { "id": "n", "v": "1" } ] }';
  const values = jsonStringValues(text);
  const b = values.find((v) => v.path.join(".") === "a.b");
  assert.equal(b.value, 'x"y');
  assert.equal(text.slice(b.start, b.end), '"x\\"y"');
  assert.ok(values.some((v) => v.path.join(".") === "c.0.v" && v.value === "1"));
});

test("an npm tarball unpacks without its package/ prefix and refuses path escapes", () => {
  const entry = (name, body) => {
    const header = Buffer.alloc(512);
    header.write(name, 0, "latin1");
    header.write(body.length.toString(8).padStart(11, "0"), 124, "latin1");
    header.write("0", 156, "latin1");
    const padded = Buffer.alloc(Math.ceil(body.length / 512) * 512);
    Buffer.from(body).copy(padded);
    return Buffer.concat([header, padded]);
  };
  const tgz = gzipSync(Buffer.concat([entry("package/lib/a.d.ts", "x"), Buffer.alloc(1024)]));
  const dest = mkdtempSync(join(tmpdir(), "typescript-pin-tar-"));
  temps.push(dest);
  extractTarball(tgz, dest);
  assert.equal(readFileSync(join(dest, "lib", "a.d.ts"), "utf8"), "x");
  const evil = gzipSync(Buffer.concat([entry("package/../escape", "x"), Buffer.alloc(1024)]));
  assert.throws(() => extractTarball(evil, dest), /unsafe/);
});
