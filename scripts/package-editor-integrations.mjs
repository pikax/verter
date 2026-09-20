#!/usr/bin/env node
/** Package built editor clients without copying development files or local configuration. */
import { execFileSync } from "node:child_process";
import {
  constants,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const repositoryRoot = fileURLToPath(new URL("..", import.meta.url));
const editors = [
  {
    name: "lapce",
    source: "extensions/lapce",
    folder: "verter-volt",
    files: ["volt.toml"],
    binary: [
      "wasm32-wasip1",
      "verter_lapce.wasm",
      "bin/verter-lapce.wasm",
      [0, 97, 115, 109, 1, 0, 0, 0],
    ],
  },
  {
    name: "zed",
    source: "extensions/zed",
    folder: "verter",
    files: ["extension.toml"],
    binary: ["wasm32-wasip2", "verter_zed.wasm", "extension.wasm", [0, 97, 115, 109, 13, 0, 1, 0]],
  },
  {
    name: "nvim",
    source: "editors/nvim",
    folder: "verter.nvim",
    files: ["lua/verter/init.lua", "lua/verter/config.lua", "plugin/verter.lua"],
  },
  { name: "helix", source: "editors/helix", folder: "verter-helix", files: ["languages.toml"] },
];

// These manifests use plain quoted root keys; stop at the first TOML table so a
// dependency/config version cannot accidentally stand in for the editor version.
function rootString(text, key) {
  return text.split(/^\s*\[/m)[0].match(new RegExp(`^${key}\\s*=\\s*"([^"]+)"`, "m"))?.[1];
}

export function packageEditorIntegrations({ root = repositoryRoot, outputDirectory } = {}) {
  if (!outputDirectory) throw new Error("An output directory is required");
  const read = (path) => readFileSync(join(root, path), "utf8");
  const version = JSON.parse(read("packages/vue-vscode/package.json")).version;
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
    throw new Error(`Invalid IDE release version: ${version}`);
  }
  const lapceManifest = read("extensions/lapce/volt.toml");
  const zedManifest = read("extensions/zed/extension.toml");
  for (const [name, manifest] of [
    ["Lapce", lapceManifest],
    ["Zed", zedManifest],
  ]) {
    const actual = rootString(manifest, "version");
    if (actual !== version)
      throw new Error(`${name} version ${actual} does not match IDE version ${version}`);
  }
  if (
    rootString(lapceManifest, "name") !== "verter-volt" ||
    rootString(lapceManifest, "wasm") !== "bin/verter-lapce.wasm"
  ) {
    throw new Error("Lapce package requires name verter-volt and wasm path bin/verter-lapce.wasm");
  }
  if (rootString(zedManifest, "id") !== "verter" || /^\s*\[lib\]/m.test(zedManifest)) {
    throw new Error("Zed package requires id verter and generates its own [lib] metadata");
  }
  const apiPackages = read("extensions/zed/Cargo.lock")
    .split(/^\[\[package\]\]/m)
    .filter((block) => rootString(block.trimStart(), "name") === "zed_extension_api");
  const apiVersion = apiPackages.length === 1 && rootString(apiPackages[0].trimStart(), "version");
  if (!apiVersion || !/^\d+\.\d+\.\d+$/.test(apiVersion))
    throw new Error("Cannot resolve locked Zed extension API version");

  // Preflight every input/output before making archives, including the different
  // binary encodings: Lapce loads a core WASM module, Zed a WASI component.
  for (const editor of editors) {
    const archive = join(outputDirectory, `verter-${editor.name}.tar.gz`);
    if (existsSync(archive)) throw new Error(`Archive already exists: ${archive}`);
    for (const file of [...editor.files, "README.md"]) read(`${editor.source}/${file}`);
    if (editor.binary) {
      const [target, filename, , header] = editor.binary;
      const binaryPath = join(root, editor.source, "target", target, "release", filename);
      const binary = readFileSync(binaryPath);
      if (!binary.subarray(0, 8).equals(Buffer.from(header))) {
        throw new Error(`Expected ${target} binary at ${binaryPath}`);
      }
    }
  }
  read("LICENSE");

  const staging = mkdtempSync(join(tmpdir(), "verter-editor-packages-"));
  try {
    const copy = (from, to) => {
      mkdirSync(dirname(to), { recursive: true });
      copyFileSync(from, to, constants.COPYFILE_EXCL);
    };
    for (const editor of editors) {
      const directory = join(staging, editor.folder);
      for (const file of [...editor.files, "README.md"])
        copy(join(root, editor.source, file), join(directory, file));
      copy(join(root, "LICENSE"), join(directory, "LICENSE"));
      writeFileSync(join(directory, "IDE_VERSION"), `${version}\n`);
      if (editor.binary) {
        const [target, filename, destination] = editor.binary;
        copy(
          join(root, editor.source, "target", target, "release", filename),
          join(directory, destination),
        );
      }
      if (editor.name === "zed") {
        writeFileSync(
          join(directory, "extension.toml"),
          `${zedManifest.trimEnd()}\n\n[lib]\nkind = "Rust"\nversion = "${apiVersion}"\n`,
        );
      }
      execFileSync(
        "tar",
        ["-czf", join(staging, `verter-${editor.name}.tar.gz`), "-C", staging, editor.folder],
        { stdio: "pipe" },
      );
    }
    mkdirSync(outputDirectory, { recursive: true });
    for (const editor of editors)
      copy(
        join(staging, `verter-${editor.name}.tar.gz`),
        join(outputDirectory, `verter-${editor.name}.tar.gz`),
      );
    return {
      version,
      archives: editors.map((editor) => join(outputDirectory, `verter-${editor.name}.tar.gz`)),
    };
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const { values } = parseArgs({ options: { output: { type: "string" } } });
    const result = packageEditorIntegrations({ outputDirectory: values.output });
    console.log(
      `Packaged editor integrations for IDE ${result.version}:\n${result.archives.join("\n")}`,
    );
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
