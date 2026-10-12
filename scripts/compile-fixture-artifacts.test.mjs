import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { fixtureBuildArgs, stageFixtureArtifacts } from "./compile-fixture-artifacts.mjs";

test("archive-only options never reach the build/list phase", () => {
  for (const archiveArgs of [
    ["--archive-file", "bundle.tar.zst", "--archive-format", "tar-zst", "--zstd-level", "3"],
    ["--archive-file=bundle.tar.zst", "--archive-format=tar-zst", "--zstd-level=3"],
  ]) {
    assert.deepEqual(
      fixtureBuildArgs(["-p", "verter_semantic", ...archiveArgs, "--test", "main", "--timings"]),
      ["-p", "verter_semantic", "--test", "main", "--timings"],
    );
  }
});

for (const format of ["rmeta", "rlib", "both"]) {
  test(`fixture ${format} uses Cargo's current artifacts and relocates with the test`, () => {
    const root = mkdtempSync(join(tmpdir(), "fixture-artifacts-"));
    try {
      const deps = join(root, "debug", "deps");
      mkdirSync(deps, { recursive: true });
      const src = join(root, "src");
      mkdirSync(join(src, "type_info"), { recursive: true });
      writeFileSync(join(src, "type_info", "core.rs"), "sealed gateway source");
      writeFileSync(join(src, "convert.rs"), "allocator source");
      writeFileSync(join(deps, "libverter_semantic-stale.rmeta"), "stale");
      const names = [
        "verter_semantic",
        "verter_session_query",
        "verter_macro_dto",
        "oxc_allocator",
      ];
      const extension = format === "rlib" ? "rlib" : "rmeta";
      const artifacts = names.map((name) => {
        const metadata = join(deps, `lib${name}-current.${extension}`);
        writeFileSync(metadata, name);
        const filenames = [metadata];
        if (format === "both") {
          const library = join(deps, `lib${name}-current.rlib`);
          writeFileSync(library, "metadata plus object code");
          filenames.unshift(library);
        }
        return {
          reason: "compiler-artifact",
          target: { name, kind: ["lib"], src_path: join(src, "lib.rs") },
          filenames,
          executable: null,
        };
      });
      // A sibling left by an earlier build is not part of this Cargo stream.
      if (format === "rlib") {
        writeFileSync(join(deps, "libverter_semantic-current.rmeta"), "stale sibling");
      }
      const macro = join(deps, "libfixture_macro-current.dll");
      writeFileSync(macro, "proc macro library");
      artifacts.push({
        reason: "compiler-artifact",
        target: { name: "fixture_macro", kind: ["proc-macro"], src_path: join(src, "lib.rs") },
        filenames: [macro],
        executable: null,
      });
      const binary = join(deps, "main-current");
      writeFileSync(binary, "test before rebuild");
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
      assert.equal(
        manifest.test_digests[manifest.tests[0]],
        createHash("sha256").update("test before rebuild").digest("hex"),
      );
      assert.equal(manifest.externs.verter_semantic, `libverter_semantic-current.${extension}`);
      assert.deepEqual(manifest.artifacts[manifest.externs.verter_semantic], {
        sha256: createHash("sha256").update("verter_semantic").digest("hex"),
        build_path: `debug/deps/libverter_semantic-current.${extension}`,
      });
      assert.equal(
        readFileSync(join(bundle, manifest.externs.verter_semantic), "utf8"),
        "verter_semantic",
      );
      assert.throws(() => readFileSync(join(bundle, "libverter_semantic-stale.rmeta")), /ENOENT/);
      if (format === "rlib") {
        assert.throws(
          () => readFileSync(join(bundle, "libverter_semantic-current.rmeta")),
          /ENOENT/,
        );
      }
      assert.equal(
        readFileSync(join(bundle, `liboxc_allocator-current.${extension}`), "utf8"),
        "oxc_allocator",
      );
      assert.equal(
        readFileSync(join(bundle, "libfixture_macro-current.dll"), "utf8"),
        "proc macro library",
      );
      assert.equal(
        readFileSync(join(bundle, manifest.sources["src/type_info/core.rs"]), "utf8"),
        "sealed gateway source",
      );
      writeFileSync(binary, "test after rebuild");
      stageFixtureArtifacts(artifacts);
      const rebuilt = JSON.parse(readFileSync(join(bundle, "manifest.json"), "utf8"));
      assert.deepEqual(rebuilt.tests, manifest.tests);
      assert.notEqual(
        rebuilt.test_digests[rebuilt.tests[0]],
        manifest.test_digests[manifest.tests[0]],
      );
      assert.throws(() => stageFixtureArtifacts(artifacts.slice(1)), /verter_semantic/);
      rmSync(join(deps, `liboxc_allocator-current.${extension}`));
      assert.throws(
        () => stageFixtureArtifacts(artifacts),
        /missing compiler artifact:.*liboxc_allocator/,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

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
