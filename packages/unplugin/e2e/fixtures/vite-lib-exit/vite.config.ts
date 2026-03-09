import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import vue from "../../../dist/vite.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function fixVueReExports(): Plugin {
  return {
    name: "fixture-fix-export-vue",
    enforce: "pre",
    transform(code, id) {
      if (!id.endsWith(path.join("src", "index.ts").replace(/\\/g, "/"))) {
        return code;
      }

      const matches = [...code.matchAll(/export\s+\{\s*default\s+as\s+(?<name>\w+)\s*\}\s+from\s+"(?<importPath>\.\/[^"]+\.vue)";?/g)];
      if (matches.length === 0) {
        return code;
      }

      let importBlock = "";
      let exportBlock = "";

      for (const [index, match] of matches.entries()) {
        const name = match.groups?.name;
        const importPath = match.groups?.importPath;
        if (!name || !importPath) {
          continue;
        }

        const localName = index === 0 ? "__fixture_vue_default" : `__fixture_vue_default_${index}`;
        importBlock += `import { default as ${localName} } from "${importPath}"\n`;
        exportBlock += `export const ${name} = ${localName}\n`;
        code = code.replace(match[0], "");
      }

      return `${importBlock}${code.trim()}\n${exportBlock}`.trim();
    },
  };
}

function emitStyleFiles(): Plugin {
  const emittedCss = new Map<string, string>();

  return {
    name: "fixture-vue-style-file",
    apply: "build",
    transform(code, id) {
      if (!id.includes("?vue&type=style")) {
        return code;
      }

      const sourcePath = id.slice(0, id.indexOf("?"));
      const baseName = path.basename(sourcePath, ".vue");
      emittedCss.set(`styles/${baseName}.css`, code);
      return code;
    },
    buildEnd() {
      for (const [fileName, source] of emittedCss) {
        this.emitFile({
          type: "asset",
          fileName,
          source,
        });
      }
    },
  };
}

export default defineConfig(async () => {
  const { default: tailwindcss } = await import("@tailwindcss/vite");

  return {
    root: __dirname,
    build: {
      cssCodeSplit: false,
      emptyOutDir: true,
      minify: false,
      outDir: path.resolve(__dirname, "dist"),
      lib: {
        entry: path.resolve(__dirname, "src/index.ts"),
        name: "ExitRegressionFixture",
        formats: ["es", "cjs"],
        fileName: (format, entryName) =>
          `${entryName}.${format === "cjs" ? "cjs" : "mjs"}`,
      },
      rollupOptions: {
        external: ["vue"],
        output: {
          preserveModules: true,
          preserveModulesRoot: path.resolve(__dirname, "src"),
        },
      },
    },
    css: {
      preprocessorOptions: {
        scss: {
          api: "modern-compiler",
        },
      },
    },
    plugins: [
      tailwindcss(),
      vue(),
      fixVueReExports(),
      emitStyleFiles(),
    ],
  };
});
