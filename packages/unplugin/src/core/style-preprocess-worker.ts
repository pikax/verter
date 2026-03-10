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

let resolvedConfig: ResolvedConfig | null = null;

async function handleInit(msg: InitMessage): Promise<void> {
  const { resolveConfig, loadConfigFromFile } = await import("vite");

  if (msg.configFile) {
    const loaded = await loadConfigFromFile(
      { command: "build", mode: "production" },
      msg.configFile,
    );
    if (loaded?.config) {
      resolvedConfig = await resolveConfig(loaded.config, "build", "production");
    } else {
      resolvedConfig = await resolveConfig(
        { root: msg.root, css: msg.cssOptions as any },
        "build",
        "production",
      );
    }
  } else {
    resolvedConfig = await resolveConfig(
      { root: msg.root, css: msg.cssOptions as any },
      "build",
      "production",
    );
  }

  process.send!({ type: "ready" });
}

async function handlePreprocess(msg: PreprocessMessage): Promise<void> {
  if (!resolvedConfig) {
    process.send!({ type: "error", id: msg.id, message: "Worker not initialized" });
    return;
  }

  try {
    const { preprocessCSS } = await import("vite");
    const result = await preprocessCSS(
      msg.content,
      `${msg.filename}.${msg.lang}`,
      resolvedConfig,
    );
    process.send!({
      type: "result",
      id: msg.id,
      code: result.code,
      sourceMap: result.map ? JSON.stringify(result.map) : undefined,
    });
  } catch (e: unknown) {
    process.send!({
      type: "error",
      id: msg.id,
      message: e instanceof Error ? e.message : String(e),
    });
  }
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
      await handlePreprocess(msg);
      break;
    case "close":
      process.exit(0);
      break;
  }
});
