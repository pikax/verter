// rollup.config.js
import path from "path";
import fs from "fs";
import { nodeResolve } from "@rollup/plugin-node-resolve";
import commonjs from "@rollup/plugin-commonjs";
import esbuild from "rollup-plugin-esbuild";

function patchVerter() {
  return {
    name: "patch-verter",
    writeBundle() {
      const outPath = path.resolve(
        import.meta.dirname,
        "extensions/vscode/dist/extension.js"
      );
      console.log("out to ", outPath);
      let code = fs.readFileSync(outPath, "utf8");
      code = code.replace(/@verter\/typescript-plugin/g, "./plugin.js");
      code = code.replace(
        /@verter\/language-server\/dist\/server\.js/g,
        "./server.js"
      );
      fs.writeFileSync(outPath, code);
      this.warn("✔ patched verter imports in extension.js");
    },
  };
}

const commonPlugins = (target) => [
  nodeResolve({
    preferBuiltins: true,
    extensions: [".js", ".ts"],
  }),
  commonjs(),
  esbuild({
    target,
    platform: "node",
    loaders: { ".ts": "ts" },
  }),
];

export default [
  // core.js
  {
    input: "packages/core/src/index.ts",
    output: {
      file: "extensions/vscode/dist/core.js",
      format: "cjs",
      exports: "auto",
    },
    external: ["vscode", /^@vue\//, "vue", "oxc-parser"],
    plugins: commonPlugins("node16"),
  },

  // server.js
  {
    input: "packages/language-server/src/server.ts",
    output: {
      file: "extensions/vscode/dist/server.js",
      format: "cjs",
      exports: "auto",
    },
    external: [
      "vscode",
      /^@vue\//,
      "vue",
      "oxc-parser",
      "ts-node",
      "typescript",
    ],
    plugins: commonPlugins("node16"),
  },

  // plugin.js
  {
    input: "packages/typescript-plugin/src/index.ts",
    output: {
      file: "extensions/vscode/dist/plugin.js",
      format: "cjs",
      exports: "auto",
    },
    external: ["vscode", /^@vue\//, "vue", "oxc-parser", "ts-node"],
    plugins: commonPlugins("node16"),
  },

  // extension.js (with post-write patch)
  {
    input: "packages/vue-vscode/src/extension.ts",
    output: {
      file: "extensions/vscode/dist/extension.js",
      format: "cjs",
      exports: "auto",
    },
    external: [
      "vscode",
      /^@vue\//,
      "vue",
      "oxc-parser",
      "ts-node",
      "@verter/typescript-plugin",
      "@verter/language-server/dist/server.js",
    ],
    plugins: [...commonPlugins("node20"), patchVerter()],
  },
];
