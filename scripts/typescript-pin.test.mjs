import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, test } from "node:test";
import { gzipSync } from "node:zlib";

import {
  AUTHORITY,
  checkGuard,
  checkPins,
  extractTarball,
  guardCount,
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

test("the guard flags TypeScript 7.0 references but not third-party 7.0.x versions", () => {
  const flagged = [
    "Measured on TypeScript 7.0.2: `x` is string",
    "npm install -D typescript@7.0.2",
    "the pinned tsgo 7.0.2 answers",
    '"typescript": "7.0.2",',
    "node_modules/.pnpm/@typescript+typescript-win32-x64@7.0.2",
    "Version 7.0.2",
    "7.0.0-dev.20260526.1",
    "stable >=7.0.2, <7.1.0",
    "verter-signature-corpus-v0@typescript-7.0.2",
  ];
  for (const line of flagged) assert.equal(guardCount("docs/x.md", line), 1, line);
  const unrelated = [
    'imbl = "7.0.1"',
    '"webpack-cli": "^7.0.2",',
    "hosted-git-info@7.0.2",
    "http://127.0.0.1:6772/mcp",
    "vite: 7.0.0",
    "TypeScript 7.1.0-beta",
  ];
  for (const line of unrelated) assert.equal(guardCount("docs/x.md", line), 0, line);
  // A bare version counts only on the engine-facing paths, never in a crate manifest.
  assert.equal(guardCount("crates/verter_tsgo_api/src/x.rs", '"7.0.3"'), 1);
  assert.equal(guardCount("crates/verter_tsc/Cargo.toml", 'oxc_sourcemap = "7.0.0"'), 0);
  assert.equal(guardCount("crates/verter_semantic/src/x.rs", '"7.0.3"'), 0);
});

test("the guard baseline can only shrink and every entry names a known reason", () => {
  const allowed = { "a.md": { count: 2, reason: "evidence-record" } };
  assert.deepEqual(checkGuard({ "a.md": 2 }, allowed), []);
  assert.match(checkGuard({ "a.md": 3 }, allowed).join(), /1 new TypeScript 7\.0 reference/);
  assert.match(checkGuard({ "a.md": 2, "b.md": 1 }, allowed).join(), /b\.md: 1 new/);
  assert.match(checkGuard({ "a.md": 1 }, allowed).join(), /shrink it/);
  assert.match(checkGuard({}, allowed).join(), /shrink it/);
  assert.match(
    checkGuard({ "a.md": 2 }, { "a.md": { count: 2, reason: "because" } }).join(),
    /unknown reason/,
  );
});

test("no TypeScript 7.0 reference outside the guard baseline", async () => {
  const { checkGuard: check, readGuardBaseline, scanGuard } = await import("./typescript-pin.mjs");
  assert.deepEqual(
    check(scanGuard(ROOT), readGuardBaseline(ROOT)),
    [],
    "run `node scripts/typescript-pin.mjs --guard` for the details",
  );
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
