// Lint-staged configuration.
//
// Overrides the inline `lint-staged` block in `package.json`. Lint-staged
// loads `.lintstagedrc.cjs` (or `.js`/`.ts`/etc.) in preference to the
// inline JSON config when both are present.
//
// The function form of the `*.rs` rule deliberately returns a command
// that does NOT depend on the staged file list. This avoids the Windows
// 8 KB argv limit that the string form `cargo fmt --check -- <files...>`
// hits when ~30+ Rust files are staged in a single commit (e.g. the
// R21 Commit 1 mechanical refactor that adds `declared_in_macro_type_arg`
// across the resolver chain). `cargo fmt --all --check` is the canonical
// workspace check and covers every staged Rust file by construction;
// the function form (lint-staged's documented escape hatch) instructs
// lint-staged to invoke the command verbatim without appending paths,
// so the argv stays a fixed short length regardless of how many `.rs`
// files are staged.
//
// The `*.{ts,js,mjs,cjs}` rule keeps the original string form because
// `oxfmt` accepts arbitrary file lists and lint-staged's auto-append
// is the correct ergonomics for that surface.
module.exports = {
  "*.rs": () => "cargo fmt --all --check",
  "*.{ts,js,mjs,cjs}": "oxfmt --check --no-error-on-unmatched-pattern",
};
