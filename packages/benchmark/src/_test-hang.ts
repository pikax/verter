import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { loadVerterCompatModule } from "./verter-compat.js";

const componentPath = process.argv[2];
if (!componentPath) {
  console.error("Usage: tsx src/_test-hang.ts <relative-path-to-vue-file>");
  process.exit(1);
}

const uiRoot = resolve("../../.integration-tests/repos/nuxt-ui");
const file = resolve(uiRoot, componentPath).replace(/\\/g, "/");

const compat = await loadVerterCompatModule();
const checker = await compat.createCheckerByJson(
  uiRoot.replace(/\\/g, "/"),
  { compilerOptions: { strict: true, jsx: "preserve" } },
  { forceUseTs: true, runtimeMode: "dedicated" },
);

checker.updateFile(file, readFileSync(file.replace(/\//g, "\\"), "utf-8"));
const t0 = performance.now();
const meta = await checker.getComponentMeta(file);
console.log(Math.round(performance.now() - t0) + "ms", meta?.props?.length + " props");
checker.close();
process.exit(0);
