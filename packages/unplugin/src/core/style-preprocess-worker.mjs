// Keep this source-side worker in sync with style-preprocess-worker.ts.
import { existsSync } from "node:fs";
import { pathToFileURL } from "node:url";

let resolvedConfig = null;
let initState = null;

async function handleInit(msg) {
  if (msg.configFile && !existsSync(msg.configFile)) {
    throw new Error(`Could not resolve "${msg.configFile}"`);
  }
  initState = msg;
  resolvedConfig = null;
  process.send({ type: "ready" });
}

async function getResolvedConfig() {
  if (resolvedConfig) {
    return resolvedConfig;
  }
  if (!initState) {
    throw new Error("Worker not initialized");
  }

  const { resolveConfig, loadConfigFromFile } = await import("vite");

  if (initState.configFile) {
    const loaded = await loadConfigFromFile(
      { command: "build", mode: "production" },
      initState.configFile,
    );
    if (loaded?.config) {
      resolvedConfig = await resolveConfig(loaded.config, "build", "production");
    } else {
      resolvedConfig = await resolveConfig(
        { root: initState.root, css: initState.cssOptions },
        "build",
        "production",
      );
    }
  } else {
    resolvedConfig = await resolveConfig(
      { root: initState.root, css: initState.cssOptions },
      "build",
      "production",
    );
  }

  return resolvedConfig;
}

function getStyleOptions(lang) {
  const cssOptions = initState?.cssOptions ?? {};
  const preprocessorOptions = cssOptions.preprocessorOptions ?? {};
  if (lang === "sass") {
    return preprocessorOptions.sass ?? preprocessorOptions.scss ?? {};
  }
  return preprocessorOptions.scss ?? preprocessorOptions.sass ?? {};
}

function normalizeLoadPaths(value) {
  if (!value) return undefined;
  if (Array.isArray(value)) {
    return value.filter((entry) => typeof entry === "string");
  }
  if (typeof value === "string") {
    return [value];
  }
  return undefined;
}

async function applyAdditionalData(content, additionalData, filename) {
  if (typeof additionalData === "string") {
    return `${additionalData}\n${content}`;
  }
  if (typeof additionalData === "function") {
    const result = await additionalData(content, filename);
    if (typeof result === "string") {
      return result;
    }
    if (result && typeof result === "object" && "content" in result) {
      const nextContent = result.content;
      if (typeof nextContent === "string") {
        return nextContent;
      }
    }
  }
  return content;
}

async function tryCompileSass(msg) {
  const lang = msg.lang.toLowerCase();
  if (lang !== "scss" && lang !== "sass") {
    return null;
  }

  let sass;
  try {
    sass = await import("sass");
  } catch {
    return null;
  }

  try {
    const options = getStyleOptions(lang);
    const content = await applyAdditionalData(
      msg.content,
      options.additionalData,
      msg.filename,
    );
    const result = await sass.compileStringAsync(content, {
      syntax: lang === "sass" ? "indented" : "scss",
      url: pathToFileURL(`${msg.filename}.${lang}`),
      loadPaths: normalizeLoadPaths(options.loadPaths ?? options.includePaths),
      sourceMap: true,
      style: options.outputStyle === "compressed" ? "compressed" : "expanded",
      quietDeps: typeof options.quietDeps === "boolean" ? options.quietDeps : true,
    });

    return {
      code: result.css,
      sourceMap: result.sourceMap ? JSON.stringify(result.sourceMap) : undefined,
    };
  } catch {
    return null;
  }
}

async function preprocessWithVite(msg) {
  const { preprocessCSS } = await import("vite");
  const result = await preprocessCSS(
    msg.content,
    `${msg.filename}.${msg.lang}`,
    await getResolvedConfig(),
  );

  return {
    code: result.code,
    sourceMap: result.map ? JSON.stringify(result.map) : undefined,
  };
}

async function handlePreprocess(msg) {
  try {
    const result = await tryCompileSass(msg) ?? await preprocessWithVite(msg);
    process.send({
      type: "result",
      id: msg.id,
      code: result.code,
      sourceMap: result.sourceMap,
    });
  } catch (error) {
    process.send({
      type: "error",
      id: msg.id,
      message: error instanceof Error ? error.message : String(error),
    });
  }
}

process.on("message", async (msg) => {
  switch (msg.type) {
    case "init":
      try {
        await handleInit(msg);
      } catch (error) {
        process.send({
          type: "error",
          id: -1,
          message: `Init failed: ${error instanceof Error ? error.message : String(error)}`,
        });
      }
      break;
    case "preprocess":
      await handlePreprocess(msg);
      break;
    case "close":
      process.exit(0);
      break;
  }
});
