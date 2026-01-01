const fs = require("fs");

const pkg = require("./../package.json");
const pkgVscode = require("./../../../packages/vue-vscode/package.json");

const scripts = pkg.scripts;
const dependencies = pkg.dependencies;

Object.assign(pkg, pkgVscode, {
  scripts,
  dependencies,
  devDependencies: undefined,
});
const pkgSource = require.resolve("./../package.json");
// write back to package.json
fs.writeFileSync(pkgSource, JSON.stringify(pkg, null, 2) + "\n", "utf-8");
