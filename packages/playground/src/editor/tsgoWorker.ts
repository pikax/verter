/**
 * Web worker that runs tsgo (TypeScript-Go) as WASM with LSP over stdio.
 *
 * Architecture:
 * - Loads Go WASM runtime (wasm_exec.js) served locally
 * - Fetches tsgo.wasm served locally
 * - Sets up globalThis.fs with stdin/stdout simulation + virtual filesystem
 * - Uses SharedArrayBuffer for blocking stdin reads
 * - Runs tsgo --lsp --stdio mode
 * - Main thread sends LSP JSON-RPC requests via postMessage → SharedArrayBuffer
 * - Worker captures stdout and sends LSP responses back via postMessage
 */

/// <reference lib="webworker" />

// ── Constants ──

const TSGO_WASM_URL = new URL("/tsgo.wasm", self.location.origin).href;
const WASM_EXEC_URL = new URL("/wasm_exec.js", self.location.origin).href;

// SharedArrayBuffer layout for stdin:
// [0] = byte count ready to read (Atomics.notify/wait)
// [1..N] = stdin data bytes
const STDIN_BUFFER_SIZE = 64 * 1024; // 64KB

// ── Virtual filesystem for tsgo ──
// tsgo may try to read tsconfig.json and lib files from disk at startup.
// We provide a minimal virtual filesystem so it doesn't panic.

const virtualFiles = new Map<string, string>([
  [
    "/tsconfig.json",
    JSON.stringify({
      compilerOptions: {
        target: "esnext",
        module: "esnext",
        moduleResolution: "bundler",
        jsx: "preserve",
        jsxImportSource: "vue",
        strict: true,
        esModuleInterop: true,
        skipLibCheck: true,
        noEmit: true,
      },
    }),
  ],
]);

// ── State ──

let lspInitialized = false;
let requestIdCounter = 0;
const pendingRequests = new Map<number, { resolve: (result: unknown) => void }>();

// Accumulated stdout buffer for LSP response parsing
let stdoutBuffer = "";

// ── LSP JSON-RPC message framing ──

function parseLspMessages(buffer: string): { messages: unknown[]; remaining: string } {
  const messages: unknown[] = [];
  let remaining = buffer;

  while (remaining.length > 0) {
    // Find Content-Length header
    const headerEnd = remaining.indexOf("\r\n\r\n");
    if (headerEnd === -1) break;

    const header = remaining.slice(0, headerEnd);
    const match = header.match(/Content-Length:\s*(\d+)/i);
    if (!match) {
      // Invalid header, skip past it
      remaining = remaining.slice(headerEnd + 4);
      continue;
    }

    const contentLength = parseInt(match[1], 10);
    const bodyStart = headerEnd + 4;
    const bodyEnd = bodyStart + contentLength;

    if (remaining.length < bodyEnd) break; // Not enough data yet

    const body = remaining.slice(bodyStart, bodyEnd);
    remaining = remaining.slice(bodyEnd);

    try {
      messages.push(JSON.parse(body));
    } catch {
      // Skip invalid JSON
    }
  }

  return { messages, remaining };
}

function encodeLspMessage(obj: unknown): string {
  const body = JSON.stringify(obj);
  // Use TextEncoder for byte length (UTF-8)
  const byteLength = new TextEncoder().encode(body).length;
  return `Content-Length: ${byteLength}\r\n\r\n${body}`;
}

// ── Go WASM setup ──

async function loadAndRunTsgo(stdinBuffer: SharedArrayBuffer): Promise<void> {
  // Load wasm_exec.js (provides the Go class)
  // We use importScripts since this is a classic worker context
  try {
    importScripts(WASM_EXEC_URL);
  } catch {
    // If importScripts fails (module worker), try dynamic import
    await import(/* @vite-ignore */ WASM_EXEC_URL);
  }

  // ── Patch process polyfills ──
  // wasm_exec.js sets up globalThis.process with cwd/chdir that throw enosys().
  // tsgo calls os.Getwd() at startup which panics without a working cwd().
  const proc = globalThis.process as unknown as Record<string, unknown>;
  if (proc) {
    proc.cwd = () => "/";
    proc.chdir = () => {};
    if (!proc.env) proc.env = {};
    if (!proc.umask) proc.umask = () => 0o022;
  }

  // @ts-expect-error -- Go class is injected by wasm_exec.js
  const go = new Go();

  // Set up process.argv for LSP mode
  go.argv = ["tsgo", "--lsp", "--stdio"];
  go.env = { TMPDIR: "/tmp" };

  // Set up stdin via SharedArrayBuffer
  const stdinView = new Int32Array(stdinBuffer);
  const stdinData = new Uint8Array(stdinBuffer, 4); // Skip the 4-byte control word
  let stdinReadPos = 0;

  // ── Virtual filesystem + stdin/stdout overrides ──
  const originalFs = (globalThis as Record<string, unknown>).fs as Record<string, unknown> | undefined;
  const fs = { ...(originalFs ?? {}) };

  // Track open file descriptors for virtual files
  let nextFd = 10;
  const openFds = new Map<number, { path: string; content: Uint8Array; pos: number }>();

  // stdout capture: writeSync(1, buf) → parse LSP responses
  fs.writeSync = (fd: number, buf: Uint8Array): number => {
    if (fd === 1) {
      // stdout — LSP response
      const text = new TextDecoder().decode(buf);
      stdoutBuffer += text;

      const { messages, remaining } = parseLspMessages(stdoutBuffer);
      stdoutBuffer = remaining;

      for (const msg of messages) {
        handleLspResponse(msg);
      }

      return buf.length;
    }
    if (fd === 2) {
      // stderr — log for debugging
      const text = new TextDecoder().decode(buf);
      console.warn("[tsgo stderr]", text);
      return buf.length;
    }
    // Virtual file descriptor
    const fdEntry = openFds.get(fd);
    if (fdEntry) return buf.length;
    return 0;
  };

  // stdin: readSync(0, buf) → block via Atomics.wait until data is available
  fs.readSync = (fd: number, buf: Uint8Array): number => {
    if (fd === 0) {
      // Wait for data to be available
      // eslint-disable-next-line no-constant-condition
      while (true) {
        const available = Atomics.load(stdinView, 0);
        if (available > 0) break;
        // Block until notified
        Atomics.wait(stdinView, 0, 0);
      }

      const available = Atomics.load(stdinView, 0);
      const toRead = Math.min(buf.length, available);

      // Copy data from SharedArrayBuffer to the read buffer
      for (let i = 0; i < toRead; i++) {
        buf[i] = stdinData[stdinReadPos + i];
      }

      stdinReadPos += toRead;

      // Update remaining count
      const newAvailable = available - toRead;
      Atomics.store(stdinView, 0, newAvailable);

      if (newAvailable === 0) {
        stdinReadPos = 0;
      }

      return toRead;
    }

    // Virtual file descriptor read
    const fdEntry = openFds.get(fd);
    if (fdEntry) {
      const remaining = fdEntry.content.length - fdEntry.pos;
      if (remaining <= 0) return 0;
      const toRead = Math.min(buf.length, remaining);
      for (let i = 0; i < toRead; i++) {
        buf[i] = fdEntry.content[fdEntry.pos + i];
      }
      fdEntry.pos += toRead;
      return toRead;
    }

    return 0;
  };

  fs.write = (
    fd: number,
    buf: Uint8Array,
    offset: number,
    length: number,
    _position: unknown,
    callback: (err: null, n: number) => void,
  ): void => {
    const slice = buf.slice(offset, offset + length);
    const n = (fs.writeSync as (fd: number, buf: Uint8Array) => number)(fd, slice);
    callback(null, n);
  };

  fs.read = (
    fd: number,
    buf: Uint8Array,
    offset: number,
    length: number,
    _position: unknown,
    callback: (err: null | Error, n: number) => void,
  ): void => {
    const slice = buf.slice(offset, offset + length);
    const n = (fs.readSync as (fd: number, buf: Uint8Array) => number)(fd, slice);
    // Copy back to original buffer
    for (let i = 0; i < n; i++) {
      buf[offset + i] = slice[i];
    }
    callback(null, n);
  };

  // Virtual filesystem operations
  const enosys = () => {
    const err = new Error("not implemented");
    (err as NodeJS.ErrnoException).code = "ENOSYS";
    return err;
  };
  const enoent = (path: string) => {
    const err = new Error(`ENOENT: no such file or directory, open '${path}'`);
    (err as NodeJS.ErrnoException).code = "ENOENT";
    return err;
  };

  fs.open = (path: string, _flags: number, _mode: number, callback: (err: Error | null, fd?: number) => void): void => {
    const content = virtualFiles.get(path);
    if (content != null) {
      const fd = nextFd++;
      openFds.set(fd, { path, content: new TextEncoder().encode(content), pos: 0 });
      callback(null, fd);
    } else {
      callback(enoent(path));
    }
  };

  fs.close = (fd: number, callback: (err: Error | null) => void): void => {
    openFds.delete(fd);
    callback(null);
  };

  fs.stat = (path: string, callback: (err: Error | null, stats?: unknown) => void): void => {
    const content = virtualFiles.get(path);
    if (content != null) {
      const bytes = new TextEncoder().encode(content);
      callback(null, {
        isDirectory: () => false,
        isFile: () => true,
        isSymbolicLink: () => false,
        size: bytes.length,
        mode: 0o644,
        dev: 0,
        ino: 0,
        nlink: 1,
        uid: 0,
        gid: 0,
        rdev: 0,
        blksize: 4096,
        blocks: Math.ceil(bytes.length / 512),
        atimeMs: Date.now(),
        mtimeMs: Date.now(),
        ctimeMs: Date.now(),
        birthtimeMs: Date.now(),
      });
    } else if (path === "/" || path === "/tmp") {
      callback(null, {
        isDirectory: () => true,
        isFile: () => false,
        isSymbolicLink: () => false,
        size: 0,
        mode: 0o755,
        dev: 0,
        ino: 0,
        nlink: 1,
        uid: 0,
        gid: 0,
        rdev: 0,
        blksize: 4096,
        blocks: 0,
        atimeMs: Date.now(),
        mtimeMs: Date.now(),
        ctimeMs: Date.now(),
        birthtimeMs: Date.now(),
      });
    } else {
      callback(enoent(path));
    }
  };

  fs.lstat = fs.stat;

  fs.fstat = (fd: number, callback: (err: Error | null, stats?: unknown) => void): void => {
    const fdEntry = openFds.get(fd);
    if (fdEntry) {
      (fs.stat as (path: string, cb: (err: Error | null, stats?: unknown) => void) => void)(fdEntry.path, callback);
    } else {
      callback(enosys());
    }
  };

  fs.readdir = (path: string, callback: (err: Error | null, files?: string[]) => void): void => {
    // Return virtual files in the given directory
    const entries: string[] = [];
    for (const filePath of virtualFiles.keys()) {
      if (filePath.startsWith(path) && filePath !== path) {
        const relative = filePath.slice(path.endsWith("/") ? path.length : path.length + 1);
        const firstSegment = relative.split("/")[0];
        if (firstSegment && !entries.includes(firstSegment)) {
          entries.push(firstSegment);
        }
      }
    }
    callback(null, entries);
  };

  fs.mkdir = (_path: string, _perm: number, callback: (err: Error | null) => void): void => {
    callback(null); // Silently succeed
  };

  (globalThis as Record<string, unknown>).fs = fs;

  // Fetch and instantiate the WASM module
  self.postMessage({ type: "status", message: "Downloading tsgo WASM..." });

  const wasmResponse = await fetch(TSGO_WASM_URL);
  if (!wasmResponse.ok) {
    throw new Error(`Failed to fetch tsgo WASM: ${wasmResponse.status}`);
  }

  self.postMessage({ type: "status", message: "Loading tsgo WASM..." });

  const wasmModule = await WebAssembly.instantiateStreaming(wasmResponse, go.importObject);

  self.postMessage({ type: "status", message: "Starting tsgo LSP..." });

  // Run the Go main function (non-blocking — it enters the LSP loop)
  go.run(wasmModule.instance).catch((err: Error) => {
    self.postMessage({ type: "error", message: `tsgo exited: ${err.message}` });
  });

  // Give the LSP server a moment to start
  await new Promise((resolve) => setTimeout(resolve, 100));

  lspInitialized = true;
  self.postMessage({ type: "ready" });
}

// ── LSP response handling ──

function handleLspResponse(msg: unknown): void {
  const response = msg as { id?: number; method?: string; params?: unknown };

  if (response.id != null) {
    // Response to a request
    const pending = pendingRequests.get(response.id);
    if (pending) {
      pendingRequests.delete(response.id);
      pending.resolve(response);
    }
  } else if (response.method === "textDocument/publishDiagnostics") {
    // Server-initiated notification
    self.postMessage({ type: "diagnostics", params: response.params });
  }
  // Other notifications can be forwarded as needed
}

// ── Main thread communication ──

let stdinSharedBuffer: SharedArrayBuffer | null = null;

function writeToStdin(data: string): void {
  if (!stdinSharedBuffer) return;

  const stdinView = new Int32Array(stdinSharedBuffer);
  const stdinData = new Uint8Array(stdinSharedBuffer, 4);
  const encoded = new TextEncoder().encode(data);

  // Write data
  for (let i = 0; i < encoded.length; i++) {
    stdinData[i] = encoded[i];
  }

  // Set count and notify
  Atomics.store(stdinView, 0, encoded.length);
  Atomics.notify(stdinView, 0);
}

function sendLspRequest(method: string, params: unknown): Promise<unknown> {
  const id = ++requestIdCounter;
  const message = { jsonrpc: "2.0", id, method, params };

  return new Promise((resolve) => {
    pendingRequests.set(id, { resolve });
    writeToStdin(encodeLspMessage(message));
  });
}

function sendLspNotification(method: string, params: unknown): void {
  const message = { jsonrpc: "2.0", method, params };
  writeToStdin(encodeLspMessage(message));
}

// ── Worker message handler ──

interface WorkerRequest {
  id: number;
  type: string;
  payload?: unknown;
}

self.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  const { id, type, payload } = event.data;
  const respond = (result?: unknown, error?: string) => {
    self.postMessage({ id, result, error });
  };

  try {
    switch (type) {
      case "init": {
        const { sharedBuffer } = payload as { sharedBuffer: SharedArrayBuffer };
        stdinSharedBuffer = sharedBuffer;
        await loadAndRunTsgo(sharedBuffer);

        // Send LSP initialize request
        const initResult = await sendLspRequest("initialize", {
          processId: null,
          capabilities: {
            textDocument: {
              synchronization: { dynamicRegistration: false, willSave: false, didSave: false },
              hover: { contentFormat: ["markdown", "plaintext"] },
              completion: { completionItem: { snippetSupport: false } },
              publishDiagnostics: { relatedInformation: false },
            },
            general: {
              positionEncodings: ["utf-16"],
            },
          },
          rootUri: "file:///",
        });

        // Send initialized notification
        sendLspNotification("initialized", {});

        respond(initResult);
        break;
      }

      case "openFile": {
        const { path, content, languageId } = payload as {
          path: string;
          content: string;
          languageId?: string;
        };
        sendLspNotification("textDocument/didOpen", {
          textDocument: {
            uri: `file://${path}`,
            languageId: languageId ?? "typescriptreact",
            version: 1,
            text: content,
          },
        });
        respond("ok");
        break;
      }

      case "updateFile": {
        const { path, content, version } = payload as {
          path: string;
          content: string;
          version: number;
        };
        sendLspNotification("textDocument/didChange", {
          textDocument: { uri: `file://${path}`, version },
          contentChanges: [{ text: content }],
        });
        respond("ok");
        break;
      }

      case "getHover": {
        const { path, line, character } = payload as {
          path: string;
          line: number;
          character: number;
        };
        const result = await sendLspRequest("textDocument/hover", {
          textDocument: { uri: `file://${path}` },
          position: { line, character },
        });
        respond(result);
        break;
      }

      case "getCompletions": {
        const { path, line, character } = payload as {
          path: string;
          line: number;
          character: number;
        };
        const result = await sendLspRequest("textDocument/completion", {
          textDocument: { uri: `file://${path}` },
          position: { line, character },
        });
        respond(result);
        break;
      }

      case "getDefinition": {
        const { path, line, character } = payload as {
          path: string;
          line: number;
          character: number;
        };
        const result = await sendLspRequest("textDocument/definition", {
          textDocument: { uri: `file://${path}` },
          position: { line, character },
        });
        respond(result);
        break;
      }

      case "getDiagnostics": {
        // tsgo sends diagnostics via publishDiagnostics notifications
        // This is a no-op request — diagnostics arrive asynchronously
        respond([]);
        break;
      }

      default:
        respond(undefined, `Unknown message type: ${type}`);
    }
  } catch (err) {
    respond(undefined, err instanceof Error ? err.message : String(err));
  }
};
