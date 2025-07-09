const { build } = require("esbuild");
const alias = require("esbuild-plugin-alias");
const { aliasPath } = require("esbuild-plugin-alias-path");
const { captureRejectionSymbol } = require("events");
const path = require("path");
const fs = require("node:fs");
const { execSync } = require("child_process");

const outFolder = "extensions/vscode/dist";

function run(cmd, opts = {}) {
  console.log(`\n> ${cmd}`);
  execSync(cmd, { stdio: "inherit", ...opts });
}

(async () => {
  await build({
    entryPoints: ["packages/core/src/index.ts"],
    bundle: true,
    platform: "node",
    target: "node16",
    outfile: `${outFolder}/core.js`,
    external: ["vscode", "@vue/*", "vue", "oxc-parser"], // never bundle the vscode module
  }).catch(() => process.exit(1));

  await build({
    entryPoints: ["packages/language-server/src/server.ts"],
    bundle: true,
    platform: "node",
    target: "node16",
    outfile: `${outFolder}/server.js`,
    external: [
      "vscode",
      "vscode-languageserver",
      "@vue/*",
      "vue",
      "oxc-parser",
      "ts-node",
    ],
  }).catch(() => process.exit(1));

  await build({
    entryPoints: ["packages/typescript-plugin/src/index.ts"],
    bundle: true,
    platform: "node",
    target: "node16",
    outfile: `${outFolder}/plugin.js`,
    external: [
      "vscode",
      "vscode-languageserver",
      "@vue/*",
      "vue",
      "oxc-parser",
      "ts-node",
    ],
  }).catch(() => process.exit(1));

  await build({
    entryPoints: ["packages/vue-vscode/src/extension.ts"],
    bundle: true,
    platform: "node",
    target: "node20",
    outfile: `${outFolder}/extension.js`,
    external: [
      "vscode",
      "vscode-languageserver",
      // "vscode-languageclient",

      "vscode-languageclient",
      "vscode-jsonrpc",
      "vscode-languageserver-protocol",
      "vscode-languageserver-types",

      "@vue/*",
      "vue",
      "oxc-parser",
      "ts-node",

      "@verter/typescript-plugin",
      "@verter/language-server/dist/server.js",
    ],

    // alias: {
    //   "vscode-languageclient/node": path.resolve(
    //     __dirname,
    //     "../node_modules/vscode-languageclient/lib/node.js"
    //   ),
    // },
    // alias: {
    //   "@verter/typescript-plugin": "./plugin.js",
    //   "@verter/language-server/dist/server.js": "./server.js",
    // },
    plugins: [
      {
        name: "patch-verter",
        setup(build) {
          build.onDispose(async () => {
            console.log("ff");
            try {
              const fp = path.resolve(
                __dirname,
                `../${outFolder}/extension.js`
              );
              let text = (await fs.promises.readFile(fp)).toString();

              text = text.replace("@verter/typescript-plugin", "./plugin.js");
              text = text.replace(
                "@verter/language-server/dist/server.js",
                "./server.js"
              );

              await fs.promises.writeFile(fp, text);
            } catch (e) {
              console.error("failed", e);
            }
          });
        },
      },
    ],
  }).catch(() => process.exit(1));

  process.chdir(path.resolve(outFolder, ".."));
  console.log("Running npm install...");
  run("npm install --omit=dev");

  console.log("packaging VSIX...");
  run("npx vsce package");
  console.log("done");
})();
