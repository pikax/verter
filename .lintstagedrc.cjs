// Lint-staged configuration.
//
// Overrides the inline `lint-staged` block in `package.json`. Lint-staged
// loads `.lintstagedrc.cjs` (or `.js`/`.ts`/etc.) in preference to the
// inline JSON config when both are present.
//
// `*.rs` rule: the function form returns a chained per-package `cargo fmt
// --check` invocation enumerated from `cargo metadata`. Why not
// `cargo fmt --all --check`? On Windows the workspace-wide invocation
// itself hits the 8 KB `CreateProcess` argv limit (`os error 206`) once
// the workspace grows past a threshold — cargo internally collects every
// crate's `.rs` files and passes them to `rustfmt` in a single argv,
// blowing past the OS cap. Per-package fmt chunks rustfmt invocation so
// the argv per child stays well under the limit regardless of total
// workspace file count. The function form also instructs lint-staged
// not to append staged-file paths, so the argv lint-staged constructs
// for this hook also stays a fixed short length.
//
// The `*.{ts,js,mjs,cjs}` rule keeps the original string form because
// `oxfmt` accepts arbitrary file lists and lint-staged's auto-append
// is the correct ergonomics for that surface.

const { execSync } = require("node:child_process");

function rustWorkspacePackages() {
  const meta = execSync("cargo metadata --format-version 1 --no-deps", {
    encoding: "utf8",
  });
  const parsed = JSON.parse(meta);
  return parsed.packages.map((p) => p.name).sort();
}

module.exports = {
  "*.rs": () => {
    const packages = rustWorkspacePackages();
    return packages.map((name) => `cargo fmt --package ${name} --check`).join(" && ");
  },
  "*.{ts,js,mjs,cjs}": "oxfmt --check --no-error-on-unmatched-pattern",
};
