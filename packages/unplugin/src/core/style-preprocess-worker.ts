/**
 * Child process entry point for style preprocessing via Vite's `preprocessCSS()`.
 *
 * Spawned by `PreprocessorSession` via `child_process.fork()`. Runs Vite's CSS
 * preprocessor in an isolated process so that leaked Sass/Stylus worker threads
 * are killed on `process.exit(0)` rather than holding the parent alive.
 *
 * IPC messages:
 *   parent → child:  init, preprocess, close
 *   child → parent:  ready, result, error
 */

import { existsSync } from "node:fs";
import { pathToFileURL } from "node:url";
import type { ResolvedConfig } from "vite";

interface InitMessage {
  type: "init";
  configFile?: string;
  root?: string;
  cssOptions?: Record<string, unknown>;
}

interface PreprocessMessage {
  type: "preprocess";
  id: number;
  content: string;
  filename: string;
  lang: string;
}

interface CloseMessage {
  type: "close";
}

type ParentMessage = InitMessage | PreprocessMessage | CloseMessage;

type WorkerPreprocessResult = {
  code: string;
  sourceMap?: string;
};

let resolvedConfig: ResolvedConfig | null = null;
let initState: InitMessage | null = null;

async function handleInit(msg: InitMessage): Promise<void> {
  if (msg.configFile && !existsSync(msg.configFile)) {
    throw new Error(`Could not resolve "${msg.configFile}"`);
  }
  initState = msg;
  resolvedConfig = null;
  process.send!({ type: "ready" });
}

async function getResolvedConfig(): Promise<ResolvedConfig> {
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
        { root: initState.root, css: initState.cssOptions as any },
        "build",
        "production",
      );
    }
  } else {
    resolvedConfig = await resolveConfig(
      { root: initState.root, css: initState.cssOptions as any },
      "build",
      "production",
    );
  }

  return resolvedConfig;
}

function getStyleOptions(lang: string): Record<string, unknown> {
  const cssOptions = (initState?.cssOptions as Record<string, any> | undefined) ?? {};
  const preprocessorOptions = cssOptions.preprocessorOptions ?? {};
  if (lang === "sass") {
    return preprocessorOptions.sass ?? preprocessorOptions.scss ?? {};
  }
  return preprocessorOptions.scss ?? preprocessorOptions.sass ?? {};
}

function normalizeLoadPaths(value: unknown): string[] | undefined {
  if (!value) return undefined;
  if (Array.isArray(value)) {
    return value.filter((entry): entry is string => typeof entry === "string");
  }
  if (typeof value === "string") {
    return [value];
  }
  return undefined;
}

async function applyAdditionalData(
  content: string,
  additionalData: unknown,
  filename: string,
): Promise<string> {
  if (typeof additionalData === "string") {
    return `${additionalData}\n${content}`;
  }
  if (typeof additionalData === "function") {
    const result = await additionalData(content, filename);
    if (typeof result === "string") {
      return result;
    }
    if (result && typeof result === "object" && "content" in result) {
      const nextContent = (result as { content?: unknown }).content;
      if (typeof nextContent === "string") {
        return nextContent;
      }
    }
  }
  return content;
}

async function tryCompileSass(msg: PreprocessMessage): Promise<WorkerPreprocessResult | null> {
  const lang = msg.lang.toLowerCase();
  if (lang !== "scss" && lang !== "sass") {
    return null;
  }

  let sass: typeof import("sass");
  try {
    sass = await import("sass");
  } catch {
    return null;
  }

  try {
    const options = getStyleOptions(lang);
    const content = await applyAdditionalData(msg.content, options.additionalData, msg.filename);
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

async function preprocessWithVite(msg: PreprocessMessage): Promise<WorkerPreprocessResult> {
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

async function handlePreprocess(msg: PreprocessMessage): Promise<void> {
  try {
    const result = (await tryCompileSass(msg)) ?? (await preprocessWithVite(msg));
    process.send!({
      type: "result",
      id: msg.id,
      code: result.code,
      sourceMap: result.sourceMap,
    });
  } catch (e: unknown) {
    process.send!({
      type: "error",
      id: msg.id,
      message: e instanceof Error ? e.message : String(e),
    });
  }
}

// Concurrency limiter: avoid overwhelming the system when 100+ files
// queue up for preprocessing simultaneously (e.g., 123 Less files in
// zyronon-douyin).  Without this, all preprocessCSS() calls run in
// parallel, exhausting memory/CPU and causing timeouts.
const MAX_CONCURRENT = 8;
let activeCount = 0;
const queue: PreprocessMessage[] = [];

function drainQueue(): void {
  while (queue.length > 0 && activeCount < MAX_CONCURRENT) {
    const next = queue.shift()!;
    activeCount++;
    handlePreprocess(next).finally(() => {
      activeCount--;
      drainQueue();
    });
  }
}

function enqueuePreprocess(msg: PreprocessMessage): void {
  queue.push(msg);
  drainQueue();
}

process.on("message", async (msg: ParentMessage) => {
  switch (msg.type) {
    case "init":
      try {
        await handleInit(msg);
      } catch (e: unknown) {
        process.send!({
          type: "error",
          id: -1,
          message: `Init failed: ${e instanceof Error ? e.message : String(e)}`,
        });
      }
      break;
    case "preprocess":
      enqueuePreprocess(msg);
      break;
    case "close":
      process.exit(0);
      break;
  }
});
