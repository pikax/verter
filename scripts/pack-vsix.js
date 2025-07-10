const fs = require("fs");
const path = require("path");
const os = require("os");
const { execSync } = require("child_process");

const REPO_ROOT = path.resolve(__dirname, "..");
const TMP_ROOT = path.join(os.tmpdir(), "verter-npm-workspace");
const PKG_DEST = path.join(TMP_ROOT, "packages");
const EXT_IN_TMP = path.join(PKG_DEST, "vue-vscode");

function run(cmd, opts = {}) {
  console.log(`\n> ${cmd}`);
  execSync(cmd, { stdio: "inherit", ...opts });
}

// 1) Clean + recreate temp
fs.rmSync(TMP_ROOT, { recursive: true, force: true });
fs.mkdirSync(PKG_DEST, { recursive: true });

// 2) Copy root package.json (workspace config) & package-lock.json if you have one
console.log("📋 Copying root package.json & lockfile…");
fs.copyFileSync(
  path.join(REPO_ROOT, "package.json"),
  path.join(TMP_ROOT, "package.json")
);
if (fs.existsSync(path.join(REPO_ROOT, "package-lock.json"))) {
  fs.copyFileSync(
    path.join(REPO_ROOT, "package-lock.json"),
    path.join(TMP_ROOT, "package-lock.json")
  );
}

// 3) Copy packages/ folder (skip existing node_modules inside)
console.log("📂 Copying packages/ into temp…");
fs.cpSync(path.join(REPO_ROOT, "packages"), PKG_DEST, {
  recursive: true,
  filter: (src) => !/node_modules/.test(src),
});

const packages = fs.readdirSync(path.join(REPO_ROOT, "packages"));

const pkg = JSON.parse(fs.readFileSync(path.join(REPO_ROOT, "package.json")));

pkg.workspaces = packages.map((x) => `packages/${x}`);

fs.writeFileSync(path.join(TMP_ROOT, "package.json"), JSON.stringify(pkg));

// console.log("foudn paca", packages, packages);
// return;

// 4 ) update workspace to *


// 5) NPM install only production deps across the workspace
process.chdir(TMP_ROOT);
console.log("\n📦 Running npm install (production + workspaces) …");
run("npm install --omit=dev --workspaces");

// 6) Ensure extension is built
console.log("\n🛠 Building vue-vscode…");
run(`npm --workspace packages/vue-vscode run build`);

// 7) Package with vsce
console.log("\n🎁 Packaging VSIX…");
process.chdir(EXT_IN_TMP);
run("npx vsce package");

console.log("\n✅ VSIX ready in:", EXT_IN_TMP);
