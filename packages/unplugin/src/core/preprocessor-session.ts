/**
 * `PreprocessorSession` - plugin-scoped lifecycle owner for style preprocessing.
 *
 * Delegates style preprocessor languages (scss, sass, less, styl, stylus) to an
 * isolated child process that runs Vite's `preprocessCSS()`. This ensures leaked
 * Sass/Stylus worker threads are killed when the child exits, preventing the
 * parent build process from hanging.
 *
 * Template, script, and custom block preprocessing remain in-process via the
 * existing handlers from `preprocessor.ts`.
 */

import { fork, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import type { HostPreprocessorRequest } from "@verter/native";
import type { BlockPreprocessor } from "./types";
import {
  preprocessCustom,
  preprocessScript,
  preprocessTemplate,
} from "./preprocessor";

interface PreprocessResult {
  code: string;
  sourceMap?: string;
}

interface PendingRequest {
  resolve: (value: PreprocessResult | null) => void;
  reject: (reason: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

interface CleanupHandler {
  event: "exit" | NodeJS.Signals;
  handler: () => void;
}

interface WorkerLaunchConfig {
  modulePath?: string;
  execArgv?: string[];
}

const STYLE_LANGS = new Set(["scss", "sass", "less", "styl", "stylus"]);
const REQUEST_TIMEOUT_MS = 30_000;
const CLOSE_TIMEOUT_MS = 2_000;
const require = createRequire(import.meta.url);

export function isStylePreprocessorRequest(
  req: Pick<HostPreprocessorRequest, "blockType" | "lang">,
): boolean {
  return req.blockType === "style" && STYLE_LANGS.has(req.lang.toLowerCase());
}

export class PreprocessorSession {
  private child: ChildProcess | null = null;
  private childReady = false;
  private readyPromise: Promise<void> | null = null;
  private pending = new Map<number, PendingRequest>();
  private nextId = 1;
  private dead = false;
  private closing = false;
  private expectedExit = false;
  private cleanupHandlers: CleanupHandler[] = [];

  constructor(
    private viteConfig: {
      configFile?: string;
      root?: string;
      cssOptions?: Record<string, unknown>;
    } | null,
  ) {}

  /**
   * Process a single preprocessor request. Style preprocessors go through the
   * child process; everything else stays in-process.
   */
  async process(
    req: HostPreprocessorRequest,
    filename: string,
    customBlockHandlers?: Record<string, BlockPreprocessor>,
  ): Promise<PreprocessResult | null> {
    if (req.blockType === "template") {
      return preprocessTemplate(req.lang, req.content, filename);
    }
    if (req.blockType === "script") {
      return preprocessScript(req.lang, req.content, filename);
    }
    if (req.blockType === "custom") {
      return preprocessCustom(req.lang, req.content, filename, customBlockHandlers);
    }

    if (!isStylePreprocessorRequest(req)) {
      return null;
    }

    if (!this.viteConfig) {
      console.warn(
        `[verter] Style preprocessing for lang="${req.lang}" requires Vite. ` +
          `Other bundlers are not yet supported for style preprocessing.`,
      );
      return null;
    }

    if (this.dead) {
      throw new Error("[verter] PreprocessorSession is dead - child process crashed.");
    }

    await this.ensureChild();
    return this.sendPreprocess(req.content, filename, req.lang);
  }

  /**
   * Starts the worker before the first style request needs it.
   * Callers should fire-and-forget this and handle failures at process time.
   */
  prewarm(): Promise<void> {
    if (!this.viteConfig || this.dead) {
      return Promise.resolve();
    }
    return this.ensureChild();
  }

  /**
   * Kill the child process, reject all pending requests, and clean up.
   * Idempotent - safe to call multiple times.
   */
  async close(): Promise<void> {
    const child = this.child;
    this.removeCleanupGuards();

    for (const [, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(new Error("[verter] PreprocessorSession closed."));
    }
    this.pending.clear();

    this.child = null;
    this.childReady = false;
    this.readyPromise = null;
    this.closing = true;
    this.expectedExit = true;

    if (!child) {
      this.closing = false;
      this.expectedExit = false;
      return;
    }

    if (child.connected) {
      try {
        child.send({ type: "close" });
      } catch {
        // Already disconnected.
      }
    }

    await new Promise<void>((resolve) => {
      if (child.exitCode !== null || child.signalCode !== null) {
        resolve();
        return;
      }

      const timer = setTimeout(() => {
        try {
          child.kill();
        } catch {
          // Already dead.
        }
        resolve();
      }, CLOSE_TIMEOUT_MS);

      child.once("exit", () => {
        clearTimeout(timer);
        resolve();
      });
    });

    this.closing = false;
    this.expectedExit = false;
    this.dead = false;
  }

  /** Whether the child process is still running. */
  isAlive(): boolean {
    return !this.dead && this.child !== null;
  }

  private async ensureChild(): Promise<void> {
    if (this.childReady) return;
    if (this.readyPromise) {
      await this.readyPromise;
      return;
    }

    this.readyPromise = this.spawnChild().finally(() => {
      this.readyPromise = null;
    });
    await this.readyPromise;
  }

  private async spawnChild(): Promise<void> {
    const worker = resolveWorkerLaunchConfig();

    return new Promise<void>((resolve, reject) => {
      const child = fork(worker.modulePath!, [], {
        stdio: ["ignore", "pipe", "pipe", "ipc"],
        serialization: "advanced",
        execArgv: worker.execArgv,
      });
      let settled = false;

      this.child = child;
      this.childReady = false;
      this.dead = false;
      this.closing = false;
      this.expectedExit = false;
      this.installCleanupGuards();

      // Unref the child so its IPC channel and stdio pipes do not prevent the
      // parent's event loop from exiting.  Messages still work as long as the
      // process is alive for other reasons (pending I/O, timers, etc.).
      // Without this, a leaked session (e.g., missing closeBundle) keeps the
      // parent process alive indefinitely.
      child.unref();
      child.stdout?.unref();
      child.stderr?.unref();

      child.stdout?.on("data", (chunk: Buffer) => {
        process.stdout.write(chunk);
      });
      child.stderr?.on("data", (chunk: Buffer) => {
        process.stderr.write(chunk);
      });

      child.on("message", (msg: any) => {
        if (msg.type === "ready") {
          settled = true;
          this.childReady = true;
          resolve();
          return;
        }

        if (msg.type !== "result" && msg.type !== "error") {
          return;
        }

        if (msg.type === "error" && msg.id === -1) {
          settled = true;
          this.cleanupFailedChild(child);
          reject(new Error(msg.message));
          return;
        }

        const pending = this.pending.get(msg.id);
        if (!pending) return;

        this.pending.delete(msg.id);
        clearTimeout(pending.timer);

        if (msg.type === "error") {
          pending.reject(new Error(msg.message));
          return;
        }

        pending.resolve({
          code: msg.code,
          sourceMap: msg.sourceMap,
        });
      });

      child.on("exit", (code, signal) => {
        const wasExpected = this.expectedExit || this.child !== child;
        if (this.child === child) {
          this.child = null;
        }
        this.childReady = false;

        if (wasExpected) {
          if (!this.closing) {
            this.dead = false;
          }
          return;
        }

        this.dead = true;
        this.removeCleanupGuards();

        const error = new Error(
          `[verter] Style preprocessor child exited unexpectedly (code=${code}, signal=${signal}).`,
        );
        for (const [, pending] of this.pending) {
          clearTimeout(pending.timer);
          pending.reject(error);
        }
        this.pending.clear();

        if (!settled) {
          settled = true;
          reject(error);
        }
      });

      child.on("error", (err) => {
        this.cleanupFailedChild(child);
        if (!settled) {
          settled = true;
          reject(err);
        }
      });

      // Only send serializable fields. The resolved Vite config often contains
      // non-cloneable objects (e.g., browserslist functions). The worker will
      // reload the full config from configFile if available. When configFile
      // is not set, extract only preprocessorOptions which is what the worker
      // actually uses for sass/less/stylus compilation.
      const cssOptions = this.viteConfig?.cssOptions;
      let serializableCss: Record<string, unknown> | undefined;
      if (cssOptions && !this.viteConfig?.configFile) {
        const pp = (cssOptions as any).preprocessorOptions;
        if (pp && typeof pp === "object") {
          serializableCss = { preprocessorOptions: pp };
        }
      }
      child.send({
        type: "init",
        configFile: this.viteConfig?.configFile,
        root: this.viteConfig?.root,
        cssOptions: serializableCss,
      });
    });
  }

  private sendPreprocess(
    content: string,
    filename: string,
    lang: string,
  ): Promise<PreprocessResult | null> {
    return new Promise<PreprocessResult | null>((resolve, reject) => {
      if (!this.child || !this.childReady) {
        reject(new Error("[verter] Child not ready for preprocessing."));
        return;
      }

      const id = this.nextId++;
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(
          new Error(
            `[verter] Style preprocess timed out after ${REQUEST_TIMEOUT_MS}ms for ${filename}`,
          ),
        );
      }, REQUEST_TIMEOUT_MS);

      this.pending.set(id, { resolve, reject, timer });

      this.child.send({
        type: "preprocess",
        id,
        content,
        filename,
        lang,
      });
    });
  }

  private installCleanupGuards(): void {
    this.removeCleanupGuards();

    const cleanup = () => {
      if (!this.child) return;
      try {
        this.child.kill();
      } catch {
        // Already dead.
      }
    };

    for (const event of ["exit", "SIGINT", "SIGTERM"] as const) {
      process.on(event, cleanup);
      this.cleanupHandlers.push({ event, handler: cleanup });
    }
  }

  private removeCleanupGuards(): void {
    for (const { event, handler } of this.cleanupHandlers) {
      process.removeListener(event, handler);
    }
    this.cleanupHandlers = [];
  }

  private cleanupFailedChild(child: ChildProcess): void {
    if (this.child === child) {
      this.child = null;
    }
    this.childReady = false;
    this.readyPromise = null;
    this.closing = false;
    this.expectedExit = false;
    this.dead = false;
    this.removeCleanupGuards();
    try {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill();
      }
    } catch {
      // Already dead.
    }
  }
}

function resolveWorkerLaunchConfig(): WorkerLaunchConfig {
  const thisDir = resolveThisDir();
  const mjsPath = path.resolve(thisDir, "style-preprocess-worker.mjs");
  const cjsPath = path.resolve(thisDir, "style-preprocess-worker.cjs");
  const tsPath = path.resolve(thisDir, "style-preprocess-worker.ts");

  if (existsSync(mjsPath)) {
    return { modulePath: mjsPath };
  }
  if (existsSync(cjsPath)) {
    return { modulePath: cjsPath };
  }
  if (existsSync(tsPath)) {
    return {
      modulePath: tsPath,
      execArgv: resolveTsWorkerExecArgv(),
    };
  }

  return { modulePath: mjsPath };
}

function resolveThisDir(): string {
  try {
    return path.dirname(fileURLToPath(import.meta.url));
  } catch {
    return __dirname;
  }
}

function resolveTsxImportSpecifier(): string {
  return pathToFileURL(require.resolve("tsx")).href;
}

function resolveTsWorkerExecArgv(): string[] {
  const major = Number.parseInt(process.versions.node.split(".")[0] ?? "0", 10);
  if (major >= 22) {
    return ["--experimental-strip-types"];
  }
  return ["--import", resolveTsxImportSpecifier()];
}
