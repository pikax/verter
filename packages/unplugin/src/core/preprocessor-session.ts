/**
 * `PreprocessorSession` — plugin-scoped lifecycle owner for style preprocessing.
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
import { fileURLToPath } from "node:url";
import path from "node:path";
import type { HostPreprocessorRequest } from "@verter/native";
import type { BlockPreprocessor } from "./types";
import { preprocessTemplate, preprocessScript, preprocessCustom } from "./preprocessor";

interface PreprocessResult {
  code: string;
  sourceMap?: string;
}

interface PendingRequest {
  resolve: (value: PreprocessResult | null) => void;
  reject: (reason: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const STYLE_LANGS = new Set(["scss", "sass", "less", "styl", "stylus"]);
const REQUEST_TIMEOUT_MS = 30_000;

export class PreprocessorSession {
  private child: ChildProcess | null = null;
  private childReady = false;
  private readyPromise: Promise<void> | null = null;
  private pending = new Map<number, PendingRequest>();
  private nextId = 1;
  private dead = false;
  private cleanupHandlers: (() => void)[] = [];

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
    // Route non-style blocks to in-process handlers
    if (req.blockType === "template") {
      return preprocessTemplate(req.lang, req.content, filename);
    }
    if (req.blockType === "script") {
      return preprocessScript(req.lang, req.content, filename);
    }
    if (req.blockType === "custom") {
      return preprocessCustom(req.lang, req.content, filename, customBlockHandlers);
    }

    // Style blocks: check if this lang needs preprocessing
    if (req.blockType !== "style" || !STYLE_LANGS.has(req.lang.toLowerCase())) {
      return null;
    }

    // No viteConfig means non-Vite bundlers — warn and return null (same as before)
    if (!this.viteConfig) {
      console.warn(
        `[verter] Style preprocessing for lang="${req.lang}" requires Vite. ` +
        `Other bundlers are not yet supported for style preprocessing.`,
      );
      return null;
    }

    if (this.dead) {
      throw new Error("[verter] PreprocessorSession is dead — child process crashed.");
    }

    await this.ensureChild();
    return this.sendPreprocess(req.content, filename, req.lang);
  }

  /**
   * Kill the child process, reject all pending requests, and clean up.
   * Idempotent — safe to call multiple times.
   */
  async close(): Promise<void> {
    if (!this.child) return;

    // Remove cleanup guards
    for (const handler of this.cleanupHandlers) {
      process.removeListener("beforeExit", handler);
      process.removeListener("SIGINT", handler);
      process.removeListener("SIGTERM", handler);
    }
    this.cleanupHandlers = [];

    // Reject all pending requests
    for (const [, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(new Error("[verter] PreprocessorSession closed."));
    }
    this.pending.clear();

    // Try graceful close, then force kill
    const child = this.child;
    this.child = null;
    this.childReady = false;
    this.readyPromise = null;

    if (child.connected) {
      try {
        child.send({ type: "close" });
      } catch {
        // Already disconnected
      }
    }

    // Wait briefly for graceful exit, then force kill
    await new Promise<void>((resolve) => {
      const timer = setTimeout(() => {
        try {
          child.kill("SIGKILL");
        } catch {
          // Already dead
        }
        resolve();
      }, 2_000);

      child.once("exit", () => {
        clearTimeout(timer);
        resolve();
      });
    });
  }

  /** Whether the child process is still running. */
  isAlive(): boolean {
    return !this.dead && this.child !== null;
  }

  // ── Internal ──────────────────────────────────────────────────────

  private async ensureChild(): Promise<void> {
    if (this.childReady) return;
    if (this.readyPromise) {
      await this.readyPromise;
      return;
    }

    this.readyPromise = this.spawnChild();
    await this.readyPromise;
  }

  private async spawnChild(): Promise<void> {
    // Resolve worker path relative to this module
    const workerPath = resolveWorkerPath();

    return new Promise<void>((resolve, reject) => {
      const child = fork(workerPath, [], {
        stdio: ["ignore", "pipe", "pipe", "ipc"],
        serialization: "advanced",
      });

      this.child = child;

      // Pipe child stdout/stderr to parent (for debugging)
      child.stdout?.on("data", (chunk: Buffer) => {
        process.stdout.write(chunk);
      });
      child.stderr?.on("data", (chunk: Buffer) => {
        process.stderr.write(chunk);
      });

      child.on("message", (msg: any) => {
        if (msg.type === "ready") {
          this.childReady = true;
          resolve();
          return;
        }
        if (msg.type === "result" || msg.type === "error") {
          const pending = this.pending.get(msg.id);
          if (!pending) return;
          this.pending.delete(msg.id);
          clearTimeout(pending.timer);

          if (msg.type === "error") {
            if (msg.id === -1) {
              // Init failure
              reject(new Error(msg.message));
              return;
            }
            pending.reject(new Error(msg.message));
          } else {
            pending.resolve({
              code: msg.code,
              sourceMap: msg.sourceMap,
            });
          }
        }
      });

      child.on("exit", (code, signal) => {
        this.dead = true;
        this.childReady = false;
        this.child = null;

        // Reject all pending requests
        for (const [, p] of this.pending) {
          clearTimeout(p.timer);
          p.reject(
            new Error(
              `[verter] Style preprocessor child exited unexpectedly (code=${code}, signal=${signal}).`,
            ),
          );
        }
        this.pending.clear();
      });

      child.on("error", (err) => {
        reject(err);
      });

      // Send init message
      child.send({
        type: "init",
        configFile: this.viteConfig?.configFile,
        root: this.viteConfig?.root,
        cssOptions: this.viteConfig?.cssOptions,
      });

      // Register cleanup guards
      const cleanup = () => {
        this.close().catch(() => {});
      };
      process.on("beforeExit", cleanup);
      process.on("SIGINT", cleanup);
      process.on("SIGTERM", cleanup);
      this.cleanupHandlers.push(cleanup);
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
        reject(new Error(`[verter] Style preprocess timed out after ${REQUEST_TIMEOUT_MS}ms for ${filename}`));
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
}

function resolveWorkerPath(): string {
  // In ESM context, resolve relative to this file
  try {
    const thisDir = path.dirname(fileURLToPath(import.meta.url));
    // In development (ts-node, vitest), use the TS source directly via tsx
    const tsPath = path.resolve(thisDir, "style-preprocess-worker.ts");
    // In production (built dist), use the compiled JS
    const mjsPath = path.resolve(thisDir, "style-preprocess-worker.mjs");
    const cjsPath = path.resolve(thisDir, "style-preprocess-worker.js");

    const fs = require("fs");
    if (fs.existsSync(mjsPath)) return mjsPath;
    if (fs.existsSync(cjsPath)) return cjsPath;
    if (fs.existsSync(tsPath)) return tsPath;
    return mjsPath; // Fallback — will error at fork time
  } catch {
    // Fallback for CJS context
    const thisDir = __dirname;
    const mjsPath = path.resolve(thisDir, "style-preprocess-worker.mjs");
    const cjsPath = path.resolve(thisDir, "style-preprocess-worker.js");
    const fs = require("fs");
    if (fs.existsSync(mjsPath)) return mjsPath;
    if (fs.existsSync(cjsPath)) return cjsPath;
    return mjsPath;
  }
}
