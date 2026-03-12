import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const e2eDir = path.resolve(__dirname, "..");
const unpluginDir = path.resolve(e2eDir, "..");
const fixtureDir = path.resolve(e2eDir, "fixtures", "vite-lib-exit");
const fixtureConfigFile = path.resolve(fixtureDir, "vite.config.ts");
const distEntryUrl = pathToFileURL(path.resolve(unpluginDir, "dist", "index.mjs")).href;

async function main() {
  const { unpluginFactory } = await import(distEntryUrl);
  const plugin = unpluginFactory(undefined, {
    framework: "vite",
    versions: { unplugin: "0.0.0", vite: "7.0.0" },
  });

  plugin.vite?.configResolved?.({
    configFile: fixtureConfigFile,
    root: fixtureDir,
    css: {
      preprocessorOptions: {
        scss: {
          api: "modern-compiler",
        },
      },
    },
    command: "build",
    build: {
      cssCodeSplit: false,
      ssr: false,
    },
  });

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

  // In Vite-owned preprocessing mode, load() returns raw SCSS source.
  // Vite's CSS pipeline preprocesses between load() and transform().
  const styleId = `${file}?vue&type=style&index=0&lang.scss`;
  const loadedStyle = await plugin.load(styleId);
  if (!loadedStyle || typeof loadedStyle === "string") {
    throw new Error("Style virtual module did not load.");
  }

  // Raw source should still contain SCSS variables (Vite hasn't preprocessed yet)
  if (!loadedStyle.code.includes("$border")) {
    throw new Error("Style load() should return raw SCSS source, but SCSS variables are missing.");
  }

  // Simulate what Vite does: preprocess SCSS to CSS, then pass to our transform.
  // In this repro we feed compiled CSS directly to verify scoping works.
  const compiledCss = `.direct-style-regression:hover {\n  border-color: #555;\n}\n`;

  const transformedStyle = await plugin.transform(compiledCss, styleId);
  if (!transformedStyle || typeof transformedStyle === "string") {
    throw new Error("Style virtual transform did not return scoped CSS.");
  }

  if (!transformedStyle.code.includes("[data-v-")) {
    throw new Error("Style virtual transform did not scope the compiled CSS.");
  }

  if (!transformedStyle.code.includes("#555")) {
    throw new Error("Style virtual transform lost the compiled CSS output.");
  }

  // Tear down the plugin — no child process to kill, cleanup is trivial.
  await plugin.closeBundle?.();

  console.log("[direct-style-exit-repro] scoped CSS generated successfully (no child process)");
}

main().catch((error) => {
  console.error(
    `[direct-style-exit-repro] ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exit(1);
});
