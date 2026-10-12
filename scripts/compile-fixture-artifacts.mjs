import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, isAbsolute, join, relative } from "node:path";
import { pathToFileURL } from "node:url";

const EXTERNS = ["verter_semantic", "verter_session_query", "verter_macro_dto"];

// Select from Cargo's fresh artifact stream, never from a target-directory glob:
// shared caches can hold several incompatible revisions and feature sets.
export function stageFixtureArtifacts(messages) {
  const artifacts = messages.filter((message) => message.reason === "compiler-artifact");
  const tests = artifacts.filter(
    (artifact) =>
      artifact.executable &&
      artifact.target.kind.includes("test") &&
      artifact.target.src_path
        .replaceAll("\\", "/")
        .endsWith("/crates/verter_semantic/tests/main.rs"),
  );
  if (tests.length === 0) return null;
  const targetDir = messages.find((message) => message["rust-build-meta"])?.["rust-build-meta"][
    "target-directory"
  ];
  if (!targetDir) throw new Error("missing nextest target-directory metadata");
  const deps = dirname(tests[0].executable);
  const relativeDeps = relative(targetDir, deps);
  if (relativeDeps.startsWith("..") || isAbsolute(relativeDeps)) {
    throw new Error("compile fixture dependencies must be inside the Cargo target directory");
  }
  if (basename(deps) !== "deps" || tests.some((test) => dirname(test.executable) !== deps)) {
    throw new Error("compile fixtures require one Cargo dependency directory");
  }
  const files = new Map();
  const externs = {};
  const sources = {};
  for (const artifact of artifacts) {
    for (const filename of artifact.filenames) {
      const metadata = filename.endsWith(".rlib")
        ? filename.replace(/\.rlib$/, ".rmeta")
        : filename;
      const procMacro =
        artifact.target.kind.includes("proc-macro") && /\.(dll|so|dylib)$/.test(metadata);
      if (!metadata.endsWith(".rmeta") && !procMacro) continue;
      if (!existsSync(metadata)) throw new Error(`missing compiler artifact: ${metadata}`);
      const name = basename(metadata);
      if (files.has(name) && files.get(name) !== metadata) {
        throw new Error(`ambiguous compiler artifact: ${name}`);
      }
      files.set(name, metadata);
      if (EXTERNS.includes(artifact.target.name) && dirname(metadata) === deps) {
        if (externs[artifact.target.name] && externs[artifact.target.name] !== name) {
          throw new Error(`ambiguous fixture dependency: ${artifact.target.name}`);
        }
        externs[artifact.target.name] = name;
      }
    }
  }
  for (const name of EXTERNS) {
    if (!externs[name]) throw new Error(`missing fixture dependency: ${name}`);
  }
  for (const [crate, relative, diagnosticPath, bundledName] of [
    ["verter_semantic", "type_info/core.rs", "src/type_info/core.rs", "semantic-core.rs"],
    [
      "oxc_allocator",
      "convert.rs",
      "$CARGO/oxc_allocator-$VERSION/src/convert.rs",
      "allocator-convert.rs",
    ],
  ]) {
    const artifact = artifacts.find(
      (artifact) => artifact.target.name === crate && artifact.target.kind.includes("lib"),
    );
    if (!artifact) throw new Error(`missing fixture source owner: ${crate}`);
    const source = join(dirname(artifact.target.src_path), relative);
    if (!existsSync(source)) throw new Error(`missing fixture diagnostic source: ${source}`);
    files.set(bundledName, source);
    sources[diagnosticPath] = bundledName;
  }
  const bundle = join(targetDir, "compile-fixtures");
  const staging = `${bundle}.staging-${process.pid}`;
  // Both directories are fixed children of the target directory nextest named.
  rmSync(staging, { recursive: true, force: true });
  mkdirSync(staging, { recursive: true });
  try {
    for (const [name, source] of files) copyFileSync(source, join(staging, name));
    writeFileSync(
      join(staging, "manifest.json"),
      JSON.stringify({
        tests: tests.map((test) => relative(targetDir, test.executable).replaceAll("\\", "/")),
        externs,
        sources,
      }),
    );
    rmSync(bundle, { recursive: true, force: true });
    renameSync(staging, bundle);
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
  return bundle;
}

function runCargo(args, capture = false) {
  const result = spawnSync("cargo", args, {
    stdio: capture ? ["inherit", "pipe", "inherit"] : "inherit",
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
  });
  if (result.error) throw Object.assign(result.error, { exitCode: 127 });
  if (result.signal || result.status !== 0) {
    throw Object.assign(new Error(`cargo ${args[0]} failed: ${result.signal ?? result.status}`), {
      exitCode: result.status ?? 1,
    });
  }
  return result.stdout;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const [mode, ...args] = process.argv.slice(2);
    if (mode !== "archive" && mode !== "prepare") {
      throw new Error(
        "usage: compile-fixture-artifacts.mjs archive|prepare [nextest build options]",
      );
    }
    const buildArgs = [];
    for (let index = 0; index < args.length; index++) {
      if (["--archive-file", "--archive-format", "--zstd-level"].includes(args[index])) index++;
      else buildArgs.push(args[index]);
    }
    const output = runCargo(
      [
        "nextest",
        "list",
        "--list-type",
        "binaries-only",
        "--cargo-message-format",
        "json",
        "--message-format",
        "json",
        ...buildArgs,
      ],
      true,
    );
    let messages;
    try {
      messages = [JSON.parse(output)];
    } catch {
      messages = output
        .trim()
        .split(/\r?\n/)
        .map((line) => JSON.parse(line));
    }
    const bundle = stageFixtureArtifacts(messages);
    if (bundle) process.stderr.write(`Compile fixture metadata: ${bundle}\n`);
    // The list built the test universe. This second Cargo invocation checks
    // freshness and archives it without compiling another dependency graph.
    if (mode === "archive")
      runCargo(["nextest", "archive", ...args.filter((arg) => arg !== "--timings")]);
  } catch (error) {
    process.stderr.write(`${error.stack}\n`);
    process.exitCode = error.exitCode ?? 1;
  }
}
