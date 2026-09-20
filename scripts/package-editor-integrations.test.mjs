import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { packageEditorIntegrations } from "./package-editor-integrations.mjs";

const lapceBinary = "extensions/lapce/target/wasm32-wasip1/release/verter_lapce.wasm";
const zedBinary = "extensions/zed/target/wasm32-wasip2/release/verter_zed.wasm";
const coreWasm = Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]);
const componentWasm = Buffer.from([0, 97, 115, 109, 13, 0, 1, 0]);

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "verter-editor-packages-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const write = (path, content) => {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), content);
  };
  write("packages/vue-vscode/package.json", JSON.stringify({ version: "0.0.4" }));
  write(
    "extensions/lapce/volt.toml",
    'name = "verter-volt"\nversion = "0.0.4"\nwasm = "bin/verter-lapce.wasm"\n',
  );
  write(
    "extensions/zed/extension.toml",
    'id = "verter"\nversion = "0.0.4"\n[language_servers.verter]\nlanguages = ["Vue.js", "Svelte"]\n',
  );
  write(
    "extensions/zed/Cargo.lock",
    'version = 4\n\n[[package]]\nname = "zed_extension_api"\nversion = "0.7.0"\n',
  );
  write(lapceBinary, coreWasm);
  write(zedBinary, componentWasm);
  for (const folder of ["extensions/lapce", "extensions/zed", "editors/nvim", "editors/helix"]) {
    write(`${folder}/README.md`, `Install ${folder}\n`);
    write(`${folder}/tests/private.txt`, "not a runtime file");
    write(`${folder}/.env.local`, "not a runtime file");
  }
  write("editors/nvim/lua/verter/init.lua", "return { setup = function() end }\n");
  write("editors/nvim/lua/verter/config.lua", "return {}\n");
  write("editors/nvim/plugin/verter.lua", "vim.g.loaded_verter = true\n");
  write("editors/helix/languages.toml", '[language-server.verter]\ncommand = "verter-lsp"\n');
  write("LICENSE", "MIT license fixture\n");
  return { root, outputDirectory: join(root, "out"), write };
}

test("packages four installable editor layouts with version, license and instructions", (t) => {
  const f = fixture(t);
  packageEditorIntegrations(f);
  assert.deepEqual(readdirSync(f.outputDirectory).sort(), [
    "verter-helix.tar.gz",
    "verter-lapce.tar.gz",
    "verter-nvim.tar.gz",
    "verter-zed.tar.gz",
  ]);
  const layouts = {
    lapce: ["verter-volt", ["volt.toml", "bin/verter-lapce.wasm"]],
    zed: ["verter", ["extension.toml", "extension.wasm"]],
    nvim: ["verter.nvim", ["lua/verter/init.lua", "lua/verter/config.lua", "plugin/verter.lua"]],
    helix: ["verter-helix", ["languages.toml"]],
  };
  for (const [editor, [folder, runtimeFiles]] of Object.entries(layouts)) {
    const archive = join(f.outputDirectory, `verter-${editor}.tar.gz`);
    const entries = execFileSync("tar", ["-tzf", archive], { encoding: "utf8" })
      .trim()
      .split(/\r?\n/)
      .filter((p) => !p.endsWith("/"));
    assert.deepEqual(
      entries.sort(),
      [...runtimeFiles, "README.md", "LICENSE", "IDE_VERSION"].map((p) => `${folder}/${p}`).sort(),
    );
    const extracted = join(f.root, `unpacked-${editor}`);
    mkdirSync(extracted);
    execFileSync("tar", ["-xzf", archive, "-C", extracted]);
    assert.equal(readFileSync(join(extracted, folder, "IDE_VERSION"), "utf8"), "0.0.4\n");
    assert.equal(readFileSync(join(extracted, folder, "LICENSE"), "utf8"), "MIT license fixture\n");
    if (editor === "lapce") {
      assert.deepEqual(readFileSync(join(extracted, folder, "bin/verter-lapce.wasm")), coreWasm);
    }
    if (editor === "zed") {
      assert.deepEqual(readFileSync(join(extracted, folder, "extension.wasm")), componentWasm);
      assert.match(
        readFileSync(join(extracted, folder, "extension.toml"), "utf8"),
        /\[lib\]\s+kind = "Rust"\s+version = "0\.7\.0"/,
      );
    }
  }
});

test("refuses mismatched editor versions before writing any archive", (t) => {
  const f = fixture(t);
  f.write(
    "extensions/lapce/volt.toml",
    'name = "verter-volt"\nversion = "0.0.3"\nwasm = "bin/verter-lapce.wasm"\n',
  );
  assert.throws(() => packageEditorIntegrations(f), /version.*0\.0\.3.*0\.0\.4/i);
  assert.equal(existsSync(f.outputDirectory), false);
});

test("refuses a missing compiled plugin", (t) => {
  const f = fixture(t);
  rmSync(join(f.root, zedBinary));
  assert.throws(() => packageEditorIntegrations(f), /verter_zed\.wasm/);
  assert.equal(existsSync(f.outputDirectory), false);
});

test("refuses a core WASM module where Zed requires a component", (t) => {
  const f = fixture(t);
  f.write(zedBinary, coreWasm);
  assert.throws(() => packageEditorIntegrations(f), /wasm32-wasip2/);
});

test("refuses a component where Lapce requires a core WASM module", (t) => {
  const f = fixture(t);
  f.write(lapceBinary, componentWasm);
  assert.throws(() => packageEditorIntegrations(f), /wasm32-wasip1/);
});

test("refuses a missing locked Zed API version", (t) => {
  const f = fixture(t);
  f.write("extensions/zed/Cargo.lock", "version = 4\n");
  assert.throws(() => packageEditorIntegrations(f), /locked Zed extension API version/);
});

test("refuses a prerelease IDE version", (t) => {
  const f = fixture(t);
  f.write("packages/vue-vscode/package.json", JSON.stringify({ version: "0.0.4-beta.1" }));
  assert.throws(() => packageEditorIntegrations(f), /Invalid IDE release version/);
});

test("refuses an unexpected Lapce binary path", (t) => {
  const f = fixture(t);
  f.write(
    "extensions/lapce/volt.toml",
    'name = "verter-volt"\nversion = "0.0.4"\nwasm = "missing.wasm"\n',
  );
  assert.throws(() => packageEditorIntegrations(f), /bin\/verter-lapce\.wasm/);
});

test("does not overwrite an existing archive", (t) => {
  const f = fixture(t);
  f.write("out/verter-zed.tar.gz", "existing");
  assert.throws(() => packageEditorIntegrations(f), /already exists/);
  assert.equal(readFileSync(join(f.outputDirectory, "verter-zed.tar.gz"), "utf8"), "existing");
});
