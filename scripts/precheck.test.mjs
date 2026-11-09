import assert from "node:assert/strict";
import { test } from "node:test";

import { chunkFiles, commandsFor, globToRegExp, matchRules } from "./precheck.mjs";

test("globs match like lint-staged: base name without a slash, whole path with one", () => {
  const config = {
    "*.rs": "rs-check",
    "*.{ts,js,mjs,cjs}": "js-check --flag",
    "docs/**/*.md": "md-check",
  };
  const files = [
    "crates/a/src/lib.rs",
    "scripts/precheck.mjs",
    "packages/x/index.d.ts",
    "packages/x/package.json",
    "docs/guide/intro.md",
    "README.md",
  ];
  assert.deepEqual(
    matchRules(config, files).map(({ pattern, files }) => [pattern, files]),
    [
      ["*.rs", ["crates/a/src/lib.rs"]],
      ["*.{ts,js,mjs,cjs}", ["scripts/precheck.mjs", "packages/x/index.d.ts"]],
      ["docs/**/*.md", ["docs/guide/intro.md"]],
    ],
  );
  assert.equal(globToRegExp("*.rs").test("a.rsx"), false);
  assert.equal(globToRegExp("**/*.md").test("top.md"), true);
});

test("a string rule gets the files appended; a function rule chooses its own commands", () => {
  assert.deepEqual(commandsFor("oxfmt --check", ["/r/a.ts", "/r/b.ts"]), [
    ["oxfmt", "--check", "/r/a.ts", "/r/b.ts"],
  ]);
  const seen = [];
  const rule = (files) => {
    seen.push(files);
    return ["cargo fmt --package a --check", "cargo fmt --package b --check"];
  };
  assert.deepEqual(commandsFor(rule, ["/r/x.rs"]), [
    ["cargo", "fmt", "--package", "a", "--check"],
    ["cargo", "fmt", "--package", "b", "--check"],
  ]);
  assert.deepEqual(seen, [["/r/x.rs"]]);
});

test("file arguments are split into chunks under the length limit", () => {
  assert.deepEqual(chunkFiles(["aaaa", "bbbb", "cccc"], 10), [["aaaa", "bbbb"], ["cccc"]]);
  assert.deepEqual(chunkFiles(["a-very-long-name"], 4), [["a-very-long-name"]]);
});
