// `.mcp.json` writing for Claude Code CLI.
//
// The regression class under test: "Setup Now" used to write a permanent
// `http://localhost:0/mcp` even when the standalone server was ALREADY ready
// (nothing corrected the fresh entry until an unrelated restart), and every
// write used `localhost`, which an IPv6-first resolver sends to `::1` where
// the 127.0.0.1-bound server does not listen.

import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  workspaceRoot: undefined as string | undefined,
  infoMessages: [] as string[],
}));

vi.mock("vscode", () => ({
  window: {
    showInformationMessage: async (message: string) => {
      mocks.infoMessages.push(message);
      return undefined;
    },
    showWarningMessage: async () => undefined,
  },
  workspace: {
    getConfiguration: () => ({ get: (_key: string, fallback?: unknown) => fallback }),
    get workspaceFolders() {
      return mocks.workspaceRoot ? [{ uri: { fsPath: mocks.workspaceRoot } }] : undefined;
    },
  },
  commands: { executeCommand: async () => undefined },
  Uri: { file: (value: string) => ({ fsPath: value }) },
}));

import { mcpJsonUrlForPort, setupMcpForClaudeCode, updateMcpPort } from "./claudeCodeDetection";

const log = {
  info() {},
  warn() {},
  error() {},
  debug() {},
  trace() {},
} as unknown as Parameters<typeof updateMcpPort>[2];

const fakeContext = {} as Parameters<typeof setupMcpForClaudeCode>[0];

let workspaceRoot: string;

beforeEach(() => {
  workspaceRoot = mkdtempSync(path.join(tmpdir(), "verter-mcp-json-"));
  mocks.workspaceRoot = workspaceRoot;
  mocks.infoMessages.length = 0;
});

afterEach(() => {
  rmSync(workspaceRoot, { recursive: true, force: true });
  mocks.workspaceRoot = undefined;
});

function readMcpJson(): { mcpServers?: Record<string, { url?: string }> } {
  return JSON.parse(readFileSync(path.join(workspaceRoot, ".mcp.json"), "utf-8"));
}

describe("mcpJsonUrlForPort", () => {
  it("targets 127.0.0.1 (the bind address), never localhost", () => {
    expect(mcpJsonUrlForPort(54321)).toBe("http://127.0.0.1:54321/mcp");
    expect(mcpJsonUrlForPort(54321)).not.toContain("localhost");
  });
});

describe("setupMcpForClaudeCode", () => {
  it("writes the LIVE endpoint", () => {
    setupMcpForClaudeCode(fakeContext, log, "http://127.0.0.1:43210/mcp");
    expect(readMcpJson().mcpServers?.verter?.url).toBe("http://127.0.0.1:43210/mcp");
    expect(mocks.infoMessages[0]).toContain("running server");
  });

  // The former port-0 placeholder branch is DELETED, not merely untested:
  // `liveUrl` is a required parameter, so the M2 class (persisting a
  // known-dead `http://127.0.0.1:0/mcp`) is unreachable from typed code. The
  // command path resolves a live endpoint first (`resolveMcpEndpointForSetup`,
  // covered in mcpServer.spec.ts) and REFUSES instead of writing.
});

describe("updateMcpPort", () => {
  it("rewrites an existing verter entry to the bound port on 127.0.0.1", () => {
    writeFileSync(
      path.join(workspaceRoot, ".mcp.json"),
      JSON.stringify({ mcpServers: { verter: { url: "http://127.0.0.1:0/mcp" } } }),
    );
    updateMcpPort(workspaceRoot, 43211, log);
    const url = readMcpJson().mcpServers?.verter?.url;
    expect(url).toBe("http://127.0.0.1:43211/mcp");
    expect(url).not.toContain("localhost");
  });

  it("never creates an entry for a user who has not opted in", () => {
    updateMcpPort(workspaceRoot, 43212, log);
    expect(() => readMcpJson()).toThrow(); // no .mcp.json was created
  });
});
