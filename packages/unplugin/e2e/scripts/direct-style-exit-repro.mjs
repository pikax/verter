import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { loadConfigFromFile, resolveConfig } from "vite";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const e2eDir = path.resolve(__dirname, "..");
const unpluginDir = path.resolve(e2eDir, "..");
const fixtureDir = path.resolve(e2eDir, "fixtures", "vite-lib-exit");
const fixtureConfigFile = path.resolve(fixtureDir, "vite.config.ts");
const distEntryUrl = pathToFileURL(path.resolve(unpluginDir, "dist", "index.mjs")).href;

async function main() {
  const loaded = await loadConfigFromFile(
    { command: "build", mode: "production" },
    fixtureConfigFile,
  );

  if (!loaded?.config) {
    throw new Error(`Could not load fixture config: ${fixtureConfigFile}`);
  }

  const resolved = await resolveConfig(loaded.config, "build", "production");
  const { unpluginFactory } = await import(distEntryUrl);
  const plugin = unpluginFactory(undefined, {
    framework: "vite",
    versions: { unplugin: "0.0.0", vite: "7.0.0" },
  });

  plugin.vite?.configResolved?.(resolved);

  const file = path.resolve(fixtureDir, "src", "DirectStyleExitRegression.vue").replace(/\\/g, "/");
  const sfc = [
    "<template>",
    '  <button class="direct-style-regression">scoped scss</button>',
    "</template>",
    '<style scoped lang="scss">',
    "$border: #555;",
    "",
    ".direct-style-regression {",
    "  &:hover {",
    "    border-color: $border;",
    "  }",
    "}",
    "</style>",
  ].join("\n");

  const mainResult = await plugin.transform(sfc, file);
  if (!mainResult || typeof mainResult === "string") {
    throw new Error("Main SFC transform did not return a compiled module.");
  }

  const styleId = `${file}?vue&type=style&index=0&lang.scss`;
  const loadedStyle = await plugin.load(styleId);
  if (!loadedStyle || typeof loadedStyle === "string") {
    throw new Error("Style virtual module did not load.");
  }

  if (loadedStyle.code.includes("$border")) {
    throw new Error("Style virtual module still contains raw SCSS.");
  }

  const transformedStyle = await plugin.transform(loadedStyle.code, styleId);
  if (!transformedStyle || typeof transformedStyle === "string") {
    throw new Error("Style virtual transform did not return scoped CSS.");
  }

  if (!transformedStyle.code.includes("[data-v-")) {
    throw new Error("Style virtual transform did not scope the compiled CSS.");
  }

  if (!transformedStyle.code.includes("#555")) {
    throw new Error("Style virtual transform lost the compiled Sass output.");
  }

  console.log("[direct-style-exit-repro] scoped CSS generated successfully");
}

main().catch((error) => {
  console.error(
    `[direct-style-exit-repro] ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exit(1);
});
