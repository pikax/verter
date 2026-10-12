import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { stageFixtureArtifacts } from "./compile-fixture-artifacts.mjs";

test("fixture metadata uses Cargo's current artifacts and relocates with the test", () => {
  const root = mkdtempSync(join(tmpdir(), "fixture-artifacts-"));
  try {
    const deps = join(root, "debug", "deps");
    mkdirSync(deps, { recursive: true });
    const src = join(root, "src");
    mkdirSync(join(src, "type_info"), { recursive: true });
    writeFileSync(join(src, "type_info", "core.rs"), "sealed gateway source");
    writeFileSync(join(src, "convert.rs"), "allocator source");
    writeFileSync(join(deps, "libverter_semantic-stale.rmeta"), "stale");
    const names = ["verter_semantic", "verter_session_query", "verter_macro_dto", "oxc_allocator"];
    const artifacts = names.map((name) => {
      const metadata = join(deps, `lib${name}-current.rmeta`);
      writeFileSync(metadata, name);
      return {
        reason: "compiler-artifact",
        target: { name, kind: ["lib"], src_path: join(src, "lib.rs") },
        filenames: [metadata],
        executable: null,
      };
    });
    const binary = join(deps, "main-current");
    artifacts.push({
      reason: "compiler-artifact",
      target: {
        name: "main",
        kind: ["test"],
        src_path: "/repo/crates/verter_semantic/tests/main.rs",
      },
      filenames: [binary],
      executable: binary,
    });
    artifacts.push({ "rust-build-meta": { "target-directory": root } });
    const bundle = stageFixtureArtifacts(artifacts);
    assert.equal(bundle, join(root, "compile-fixtures"));
    const manifest = JSON.parse(readFileSync(join(bundle, "manifest.json"), "utf8"));
    assert.deepEqual(manifest.tests, ["debug/deps/main-current"]);
    assert.equal(manifest.externs.verter_semantic, "libverter_semantic-current.rmeta");
    assert.equal(
      readFileSync(join(bundle, manifest.externs.verter_semantic), "utf8"),
      "verter_semantic",
    );
    assert.throws(() => readFileSync(join(bundle, "libverter_semantic-stale.rmeta")), /ENOENT/);
    assert.equal(
      readFileSync(join(bundle, manifest.sources["src/type_info/core.rs"]), "utf8"),
      "sealed gateway source",
    );
    assert.throws(() => stageFixtureArtifacts(artifacts.slice(1)), /verter_semantic/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a missing Cargo executable remains a setup failure", () => {
  const root = mkdtempSync(join(tmpdir(), "fixture-missing-cargo-"));
  try {
    const result = spawnSync(
      process.execPath,
      [fileURLToPath(new URL("./compile-fixture-artifacts.mjs", import.meta.url)), "prepare"],
      {
        cwd: root,
        env: { ...process.env, PATH: root },
        encoding: "utf8",
        timeout: 10000,
      },
    );
    assert.equal(result.error, undefined);
    assert.equal(result.status, 127, result.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
