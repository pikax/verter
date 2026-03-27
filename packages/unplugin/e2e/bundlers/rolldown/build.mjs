import { rolldown } from "rolldown";
import path from "path";
import { fileURLToPath } from "url";
import vue from "@verter/unplugin/rolldown";
import fs from "fs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const appDir = path.resolve(__dirname, "../../app");
const outDir = path.resolve(__dirname, "dist");

const bundle = await rolldown({
  input: path.resolve(appDir, "src/main.ts"),
  plugins: [vue()],
  resolve: {
    extensions: [".ts", ".js", ".vue", ".json"],
  },
  define: {
    __VUE_OPTIONS_API__: "true",
    __VUE_PROD_DEVTOOLS__: "false",
    __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: "false",
    "process.env.NODE_ENV": JSON.stringify("production"),
  },
});

await bundle.write({
  dir: outDir,
  format: "es",
  entryFileNames: "bundle.js",
});

// Copy index.html
const html = fs
  .readFileSync(path.resolve(appDir, "index.html"), "utf-8")
  .replace("./src/main.ts", "./bundle.js");
fs.writeFileSync(path.resolve(outDir, "index.html"), html);

console.log("rolldown: Build complete");
