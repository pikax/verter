import { mkdirSync, createWriteStream } from "node:fs";
import { dirname, resolve } from "node:path";
import { spawn, spawnSync } from "node:child_process";

function parseArgs(argv) {
  const out = {
    cwd: process.cwd(),
    timeoutMs: 0,
    log: null,
    errLog: null,
    env: {},
    command: null,
    args: [],
  };

  let index = 0;
  while (index < argv.length) {
    const arg = argv[index];
    if (arg === "--") {
      out.command = argv[index + 1] ?? null;
      out.args = argv.slice(index + 2);
      break;
    }
    if (arg.startsWith("--cwd=")) {
      out.cwd = resolve(arg.slice("--cwd=".length));
      index += 1;
      continue;
    }
    if (arg.startsWith("--timeout-ms=")) {
      out.timeoutMs = Number.parseInt(arg.slice("--timeout-ms=".length), 10);
      index += 1;
      continue;
    }
    if (arg.startsWith("--log=")) {
      out.log = resolve(arg.slice("--log=".length));
      index += 1;
      continue;
    }
    if (arg.startsWith("--err-log=")) {
      out.errLog = resolve(arg.slice("--err-log=".length));
      index += 1;
      continue;
    }
    if (arg.startsWith("--env=")) {
      const entry = arg.slice("--env=".length);
      const equalsIndex = entry.indexOf("=");
      if (equalsIndex <= 0) {
        throw new Error(`Invalid --env entry: ${entry}`);
      }
      out.env[entry.slice(0, equalsIndex)] = entry.slice(equalsIndex + 1);
      index += 1;
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }

  if (!out.command) {
    throw new Error("Missing command after --");
  }
  if (!Number.isFinite(out.timeoutMs) || out.timeoutMs <= 0) {
    throw new Error("--timeout-ms must be a positive integer");
  }
  return out;
}

function killProcessTree(pid) {
  if (!pid) {
    return;
  }
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], {
      stdio: "ignore",
      windowsHide: true,
    });
    return;
  }
  try {
    process.kill(-pid, "SIGKILL");
  } catch {
    try {
      process.kill(pid, "SIGKILL");
    } catch {}
  }
}

function attachPipe(stream, logPath, writer) {
  if (!stream) {
    return;
  }
  stream.on("data", (chunk) => {
    writer.write(chunk);
  });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const stdoutLog = args.log ? createWriteStream(prepareLogPath(args.log)) : null;
  const stderrLog = args.errLog ? createWriteStream(prepareLogPath(args.errLog)) : null;

  const child = spawn(args.command, args.args, {
    cwd: args.cwd,
    shell: process.platform === "win32",
    detached: process.platform !== "win32",
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, FORCE_COLOR: "0", ...args.env },
  });

  attachPipe(child.stdout, args.log, stdoutLog ?? process.stdout);
  attachPipe(child.stderr, args.errLog, stderrLog ?? process.stderr);

  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    killProcessTree(child.pid);
  }, args.timeoutMs);
  timer.unref();

  const exitCode = await new Promise((resolvePromise) => {
    child.once("error", () => resolvePromise(1));
    child.once("close", (code) => resolvePromise(code ?? 1));
  });

  clearTimeout(timer);
  stdoutLog?.end();
  stderrLog?.end();

  if (timedOut) {
    console.error(`TIMEOUT after ${args.timeoutMs}ms`);
    process.exit(124);
  }
  process.exit(exitCode);
}

function prepareLogPath(logPath) {
  mkdirSync(dirname(logPath), { recursive: true });
  return logPath;
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
