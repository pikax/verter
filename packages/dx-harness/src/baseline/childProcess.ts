/**
 * A minimal one-shot child-process runner for the baseline binary.
 *
 * Spawns a process, optionally pipes a stdin payload, collects stdout as text,
 * and buffers stderr through the shared {@link StderrBuffer} (the same buffered,
 * line-addressable capture the LSP test client uses). Resolves with the exit
 * code and the collected streams; a bounded `timeoutMs` hard-kills a stuck child.
 */

import { spawn } from "node:child_process";

import { StderrBuffer } from "@verter/lsp-test-client";

/** Options for {@link runOneShot}. */
export interface OneShotOptions {
  /** Process arguments. */
  args?: string[];
  /** Payload written to the child's stdin, then closed. */
  input?: string;
  /** Working directory. */
  cwd?: string;
  /** Environment (defaults to the parent's). */
  env?: NodeJS.ProcessEnv;
  /** Hard-kill the child after this many ms. */
  timeoutMs?: number;
}

/** The outcome of a one-shot run. */
export interface OneShotResult {
  code: number | null;
  signal: NodeJS.Signals | null;
  stdout: string;
  stderr: string;
}

/** Run a process to completion, returning its exit status and collected output. */
export function runOneShot(bin: string, opts: OneShotOptions = {}): Promise<OneShotResult> {
  return new Promise<OneShotResult>((resolve, reject) => {
    const child = spawn(bin, opts.args ?? [], {
      cwd: opts.cwd,
      env: opts.env,
      stdio: ["pipe", "pipe", "pipe"],
    });

    const stderr = new StderrBuffer();
    let stdout = "";
    let settled = false;

    const finish = (fn: () => void): void => {
      if (settled) return;
      settled = true;
      if (timer) clearTimeout(timer);
      fn();
    };

    const timer =
      opts.timeoutMs !== undefined
        ? setTimeout(() => {
            finish(() => {
              child.kill("SIGKILL");
              reject(new Error(`one-shot "${bin}" timed out after ${opts.timeoutMs}ms`));
            });
          }, opts.timeoutMs)
        : undefined;
    timer?.unref();

    child.stdout.setEncoding("utf-8");
    child.stdout.on("data", (c: string) => {
      stdout += c;
    });
    child.stderr.on("data", (c: Buffer) => stderr.append(c));

    child.on("error", (err) => finish(() => reject(err)));
    child.on("close", (code, signal) =>
      finish(() => resolve({ code, signal, stdout, stderr: stderr.text() })),
    );

    child.stdin.end(opts.input ?? "");
  });
}
