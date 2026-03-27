import { extractSsrRenderBody } from "./normalize.mjs";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

const require = createRequire(import.meta.url);
const ROOT = path.resolve(import.meta.dirname, "../..");
const { parse, compileScript, compileTemplate } = require(
  require.resolve("@vue/compiler-sfc", { paths: [path.join(ROOT, "node_modules/.pnpm")] }),
);

const { VerterHost } = require(path.join(ROOT, "packages/native/index.js"));
const host = new VerterHost({ devMode: false, analysisLevel: "none" });

function compileVue(source, filename) {
  const { descriptor } = parse(source, { filename });
  if (!descriptor.template) return null;
  let bm = {};
  if (descriptor.script || descriptor.scriptSetup) {
    try {
      const sr = compileScript(descriptor, { id: filename, inlineTemplate: false });
      bm = sr.bindings || {};
    } catch (e) {
      console.log("Vue compileScript FAILED:", e.message?.slice(0, 200));
    }
  }
  console.log("Vue bindings:", JSON.stringify(bm).slice(0, 300));
  const result = compileTemplate({
    source: descriptor.template.content,
    filename,
    id: filename,
    ssr: true,
    compilerOptions: { mode: "module", bindingMetadata: bm },
  });
  if (result.errors?.length) {
    console.log(
      "Vue errors:",
      result.errors.map((e) => e.message),
    );
    return null;
  }
  return result.code;
}

function compileVerter(source, filePath) {
  const upsertResult = host.upsert({ inputId: filePath, source, fileKind: "vue" });
  const result = host.getVirtualFile({
    canonicalId: upsertResult.canonicalId,
    nodeKind: { kind: "main" },
    compileProfile: {
      filename: path.basename(filePath),
      ssr: true,
      forceJs: true,
      sourceMap: false,
    },
  });
  return result?.code;
}

const VERTER_TEST_REPOS = process.env.VERTER_TEST_REPOS;
if (!VERTER_TEST_REPOS) {
  console.error("Set VERTER_TEST_REPOS env var");
  process.exit(1);
}
const file = path.join(VERTER_TEST_REPOS, "element-plus/packages/components/alert/src/alert.vue");
const source = fs.readFileSync(file, "utf8");

const vueCode = compileVue(source, "alert.vue");
const verterCode = compileVerter(source, file);

console.log("\n=== Vue SSR ===");
console.log(extractSsrRenderBody(vueCode));

console.log("\n=== Verter SSR ===");
console.log(extractSsrRenderBody(verterCode));
